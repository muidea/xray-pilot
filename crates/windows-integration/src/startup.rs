//! Login task ownership and recoverable manager-settings transactions.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use xray_config::{optional_json, read_json, write_json};
use xray_model::{Error, Result, Settings, canonical_hash, hash};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StartupTask {
    /// Task Scheduler XML, normalized by the backend (excluding its generated URI).
    pub xml: String,
}
pub trait StartupBackend: Send {
    fn read(&self, name: &str) -> Result<Option<StartupTask>>;
    fn prepare(&self, owner: &str, exe: &Path, root: &Path) -> Result<StartupTask>;
    /// Recheck before changing anything; never unconditionally overwrite a task.
    fn replace(
        &mut self,
        name: &str,
        before: &Option<StartupTask>,
        target: &Option<StartupTask>,
    ) -> Result<()>;
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SettingsOutcome {
    Committed,
    Compensated,
}
#[derive(Serialize, Deserialize)]
struct SettingsJournal {
    before: Option<Settings>,
    target: Settings,
    task_before: Option<StartupTask>,
    task_target: Option<StartupTask>,
    outcome: Option<SettingsOutcome>,
}
#[derive(Serialize, Deserialize)]
struct Ownership {
    expected: Option<StartupTask>,
}

pub struct StartupController {
    backend: Box<dyn StartupBackend>,
    root: PathBuf,
    pub name: String,
    owner: String,
    #[cfg(test)]
    fail_settings_write: bool,
}
impl StartupController {
    pub fn new(
        backend: Box<dyn StartupBackend>,
        root: &Path,
        principal: &str,
        installation: &str,
    ) -> Self {
        let owner = format!(
            "XrayPilot:{principal}:{installation}:{}",
            hash(root.to_string_lossy().as_bytes())
        );
        Self {
            backend,
            root: root.into(),
            name: format!("XrayPilot-{}", &hash(owner.as_bytes())[..32]),
            owner,
            #[cfg(test)]
            fail_settings_write: false,
        }
    }
    fn ownership(&self) -> PathBuf {
        self.root.join("runtime/startup-owner.json")
    }
    fn journal(&self, tx: &str) -> PathBuf {
        self.root
            .join("runtime/transactions")
            .join(tx)
            .join("settings.json")
    }
    pub fn outcome(&self, tx: &str) -> Result<Option<SettingsOutcome>> {
        Ok(optional_json::<SettingsJournal>(&self.journal(tx))?.and_then(|j| j.outcome))
    }
    pub fn pending(&self, tx: &str) -> Result<bool> {
        Ok(optional_json::<SettingsJournal>(&self.journal(tx))?
            .is_some_and(|j| j.outcome.is_none()))
    }
    fn matches<T: Serialize>(a: &T, b: &T) -> Result<bool> {
        Ok(canonical_hash(a)? == canonical_hash(b)?)
    }
    fn finish(&mut self, path: &Path, journal: &mut SettingsJournal) -> Result<SettingsOutcome> {
        let settings = optional_json::<Settings>(&self.root.join("config/manager.json"))?;
        let committed = Self::matches(&settings, &Some(journal.target.clone()))?;
        let target = if committed {
            &journal.task_target
        } else {
            if !Self::matches(&settings, &journal.before)? {
                return Err(Error::new(
                    "SETTINGS_CONFLICT",
                    "External manager settings preserved; repair pending transaction",
                ));
            }
            &journal.task_before
        };
        let current = self.backend.read(&self.name)?;
        if current != *target {
            // Only either recorded endpoint is ours to compensate. A third state belongs to others.
            if current != journal.task_before && current != journal.task_target {
                return Err(Error::new(
                    "STARTUP_CONFLICT",
                    "Externally changed login task preserved",
                ));
            }
            self.backend.replace(&self.name, &current, target)?;
        }
        if self.backend.read(&self.name)? != *target {
            return Err(Error::new("STARTUP_FAILED", "Login task readback failed"));
        }
        write_json(
            &self.ownership(),
            &Ownership {
                expected: target.clone(),
            },
        )?;
        let outcome = if committed {
            SettingsOutcome::Committed
        } else {
            SettingsOutcome::Compensated
        };
        journal.outcome = Some(outcome);
        write_json(path, journal)?;
        Ok(outcome)
    }
    pub fn save(&mut self, tx: &str, settings: &Settings, exe: &Path) -> Result<()> {
        let before = optional_json::<Settings>(&self.root.join("config/manager.json"))?;
        let task_before = self.backend.read(&self.name)?;
        let owned = optional_json::<Ownership>(&self.ownership())?.and_then(|o| o.expected);
        if task_before != owned {
            return Err(Error::new(
                "STARTUP_CONFLICT",
                "Login task ownership/action changed; external task preserved",
            ));
        }
        let task_target = if settings.start_on_logon {
            Some(self.backend.prepare(&self.owner, exe, &self.root)?)
        } else {
            None
        };
        let mut journal = SettingsJournal {
            before,
            target: settings.clone(),
            task_before,
            task_target,
            outcome: None,
        };
        let path = self.journal(tx);
        write_json(&path, &journal)?;
        let effect = (|| {
            if journal.task_before != journal.task_target {
                self.backend
                    .replace(&self.name, &journal.task_before, &journal.task_target)?;
            }
            if self.backend.read(&self.name)? != journal.task_target {
                return Err(Error::new(
                    "STARTUP_FAILED",
                    "Login task readback differs from target",
                ));
            }
            if !Self::matches(
                &optional_json::<Settings>(&self.root.join("config/manager.json"))?,
                &journal.before,
            )? {
                return Err(Error::new(
                    "SETTINGS_CONFLICT",
                    "Manager settings changed during task update",
                ));
            }
            #[cfg(test)]
            if self.fail_settings_write {
                return Err(Error::new(
                    "IO_ERROR",
                    "Injected settings publication failure",
                ));
            }
            write_json(&self.root.join("config/manager.json"), settings)
        })();
        match self.finish(&path, &mut journal)? {
            SettingsOutcome::Committed => Ok(()),
            SettingsOutcome::Compensated => Err(effect.err().unwrap_or_else(|| {
                Error::new("SETTINGS_SAVE_FAILED", "Settings transaction compensated")
            })),
        }
    }
    pub fn recover(&mut self) -> Result<()> {
        for entry in fs::read_dir(self.root.join("runtime/transactions"))? {
            let path = entry?.path().join("settings.json");
            if path.exists() {
                let mut journal: SettingsJournal = read_json(&path)?;
                if journal.outcome.is_none() {
                    self.finish(&path, &mut journal)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
