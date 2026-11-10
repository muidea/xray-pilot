use super::*;
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct FakeState {
    task: Option<StartupTask>,
    writes: usize,
    fail_write: Option<usize>,
}
#[derive(Clone, Default)]
struct Fake(Arc<Mutex<FakeState>>);
impl StartupBackend for Fake {
    fn read(&self, _: &str) -> Result<Option<StartupTask>> {
        Ok(self.0.lock().unwrap().task.clone())
    }
    fn prepare(&self, owner: &str, exe: &Path, root: &Path) -> Result<StartupTask> {
        Ok(StartupTask {
            xml: format!("{owner}:{}:{}", exe.display(), root.display()),
        })
    }
    fn replace(
        &mut self,
        _: &str,
        before: &Option<StartupTask>,
        target: &Option<StartupTask>,
    ) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.task != *before {
            return Err(Error::new("STARTUP_CONFLICT", "external change"));
        }
        state.writes += 1;
        if state.fail_write == Some(state.writes) {
            return Err(Error::new(
                "STARTUP_FAILED",
                "Injected compensation failure",
            ));
        }
        state.task = target.clone();
        Ok(())
    }
}
fn controller(root: &Path, fake: &Fake, installation: &str) -> StartupController {
    fs::create_dir_all(root.join("runtime/transactions")).unwrap();
    StartupController::new(Box::new(fake.clone()), root, "sid", installation)
}
#[test]
fn task_changes_are_compensated_when_settings_publication_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let mut c = controller(tmp.path(), &fake, "install");
    let enabled = Settings {
        start_on_logon: true,
        ..Settings::default()
    };
    c.fail_settings_write = true;
    assert_eq!(
        c.save("enable-failed", &enabled, Path::new("manager.exe"))
            .unwrap_err()
            .code,
        "IO_ERROR"
    );
    assert!(fake.read(&c.name).unwrap().is_none());
    assert_eq!(
        c.outcome("enable-failed").unwrap(),
        Some(SettingsOutcome::Compensated)
    );
    c.fail_settings_write = false;
    c.save("enabled", &enabled, Path::new("manager.exe"))
        .unwrap();
    let owned = fake.read(&c.name).unwrap();
    c.fail_settings_write = true;
    assert!(
        c.save(
            "disable-failed",
            &Settings::default(),
            Path::new("manager.exe")
        )
        .is_err()
    );
    assert_eq!(fake.read(&c.name).unwrap(), owned);
    c.fail_settings_write = false;
    c.save("disabled", &Settings::default(), Path::new("manager.exe"))
        .unwrap();
    c.save(
        "disabled-again",
        &Settings::default(),
        Path::new("manager.exe"),
    )
    .unwrap();
    assert!(fake.read(&c.name).unwrap().is_none());
}
#[test]
fn interrupted_intents_and_lost_success_are_recovered() {
    for committed in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let mut c = controller(tmp.path(), &fake, "install");
        let target = Settings {
            start_on_logon: true,
            ..Settings::default()
        };
        let task = fake
            .prepare("owner", Path::new("manager.exe"), tmp.path())
            .unwrap();
        let journal = SettingsJournal {
            before: None,
            target: target.clone(),
            task_before: None,
            task_target: Some(task.clone()),
            outcome: None,
        };
        write_json(&c.journal("interrupted"), &journal).unwrap();
        fake.0.lock().unwrap().task = Some(task);
        if committed {
            write_json(&tmp.path().join("config/manager.json"), &target).unwrap();
        }
        c.recover().unwrap();
        assert_eq!(fake.read(&c.name).unwrap().is_some(), committed);
        assert_eq!(
            c.outcome("interrupted").unwrap(),
            Some(if committed {
                SettingsOutcome::Committed
            } else {
                SettingsOutcome::Compensated
            })
        );
        c.recover().unwrap();
    }
}
#[test]
fn external_task_is_preserved_and_pending_recovery_can_resume() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    let mut c = controller(tmp.path(), &fake, "install");
    let target = Settings {
        start_on_logon: true,
        ..Settings::default()
    };
    let owned = StartupTask {
        xml: "owned".into(),
    };
    let external = StartupTask {
        xml: "external".into(),
    };
    write_json(
        &c.journal("pending"),
        &SettingsJournal {
            before: None,
            target,
            task_before: None,
            task_target: Some(owned.clone()),
            outcome: None,
        },
    )
    .unwrap();
    fake.0.lock().unwrap().task = Some(external.clone());
    assert_eq!(c.recover().unwrap_err().code, "STARTUP_CONFLICT");
    assert!(c.pending("pending").unwrap());
    assert_eq!(fake.read(&c.name).unwrap(), Some(external));
    fake.0.lock().unwrap().task = Some(owned);
    c.recover().unwrap();
    assert!(!c.pending("pending").unwrap());
    let other = controller(tmp.path(), &fake, "another-installation");
    assert_ne!(c.name, other.name);
    let other_root = controller(&tmp.path().join("other-root"), &fake, "install");
    assert_ne!(c.name, other_root.name);
    c.save(
        "enabled",
        &Settings {
            start_on_logon: true,
            ..Settings::default()
        },
        Path::new("manager.exe"),
    )
    .unwrap();
    fake.0.lock().unwrap().task = Some(StartupTask {
        xml: "external-edit".into(),
    });
    assert_eq!(
        c.save(
            "disable-external",
            &Settings::default(),
            Path::new("manager.exe")
        )
        .unwrap_err()
        .code,
        "STARTUP_CONFLICT"
    );
    assert_eq!(fake.read(&c.name).unwrap().unwrap().xml, "external-edit");
}

#[cfg(windows)]
#[test]
#[ignore = "Creates and removes an isolated current-user login task"]
fn native_login_task_roundtrip() {
    use crate::SystemStartup;
    struct Cleanup {
        name: String,
        owner: String,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Ok(Some(task)) = SystemStartup.read(&self.name)
                && task.xml.contains(&self.owner)
            {
                let _ = SystemStartup.replace(&self.name, &Some(task), &None);
            }
        }
    }
    let tmp = tempfile::Builder::new()
        .prefix("xray pilot 自启 ")
        .tempdir()
        .unwrap();
    let mut c = StartupController::new(
        Box::new(SystemStartup),
        tmp.path(),
        &xray_config::security::principal().unwrap(),
        &xray_model::id(),
    );
    let _cleanup = Cleanup {
        name: c.name.clone(),
        owner: c.owner.clone(),
    };
    fs::create_dir_all(tmp.path().join("runtime/transactions")).unwrap();
    let enabled = Settings {
        start_on_logon: true,
        ..Settings::default()
    };
    let exe = std::env::current_exe().unwrap();
    c.save("enabled", &enabled, &exe).unwrap();
    let original = c.backend.read(&c.name).unwrap().unwrap();
    let external = StartupTask {
        xml: original.xml.replace(
            &format!("<Description>{}</Description>", c.owner),
            &format!("<Description>{} external edit</Description>", c.owner),
        ),
    };
    c.backend
        .replace(&c.name, &Some(original.clone()), &Some(external.clone()))
        .unwrap();
    assert_eq!(
        c.save("disable-external", &Settings::default(), &exe)
            .unwrap_err()
            .code,
        "STARTUP_CONFLICT"
    );
    assert_eq!(c.backend.read(&c.name).unwrap(), Some(external.clone()));
    c.backend
        .replace(&c.name, &Some(external), &Some(original))
        .unwrap();
    c.save("disabled", &Settings::default(), &exe).unwrap();
    c.save("disabled-again", &Settings::default(), &exe)
        .unwrap();
    assert!(c.backend.read(&c.name).unwrap().is_none());
}

#[test]
fn failed_compensation_preserves_intent_and_is_retried_after_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let fake = Fake::default();
    fake.0.lock().unwrap().fail_write = Some(2);
    let mut c = controller(tmp.path(), &fake, "install");
    c.fail_settings_write = true;
    assert_eq!(
        c.save(
            "pending",
            &Settings {
                start_on_logon: true,
                ..Settings::default()
            },
            Path::new("manager.exe")
        )
        .unwrap_err()
        .code,
        "STARTUP_FAILED"
    );
    assert!(c.pending("pending").unwrap());
    assert!(fake.read(&c.name).unwrap().is_some());
    drop(c);
    let mut c = controller(tmp.path(), &fake, "install");
    c.recover().unwrap();
    assert_eq!(
        c.outcome("pending").unwrap(),
        Some(SettingsOutcome::Compensated)
    );
    assert!(fake.read(&c.name).unwrap().is_none());
}
