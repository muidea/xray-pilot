//! GUI metadata is separate from committed native profiles and runtime manifests.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
use xray_model::Result;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum ExitDefault {
    #[default]
    Gui,
    Shutdown,
}

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Workspace {
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    #[serde(default)]
    pub node_names: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(default)]
    pub asset_roots: BTreeMap<String, String>,
    /// Per-profile PAC editor draft. Never applied until an explicit run transaction.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub pac: BTreeMap<String, xray_model::PacDraft>,
    /// Last observed Core running state, used to restore it after a normal exit.
    #[serde(default)]
    pub last_core_running: bool,
    /// Last reminder choice for the single tray exit action.
    #[serde(default)]
    pub exit_default: ExitDefault,
}
impl Workspace {
    pub fn load(root: &Path) -> Result<Self> {
        Ok(xray_config::optional_json(&root.join("config/gui.json"))?.unwrap_or_default())
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        xray_config::write_json(&root.join("config/gui.json"), self)
    }
    pub fn name<'a>(&'a self, id: &'a str) -> &'a str {
        self.names.get(id).map(String::as_str).unwrap_or(id)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct NumberInput {
    pub source: String,
    pub text: String,
}
impl NumberInput {
    pub fn matches(&self, value: &serde_json::Value) -> bool {
        value.as_number().is_some_and(|number| {
            self.source
                .parse::<serde_json::Number>()
                .is_ok_and(|source| *number == source)
        })
    }
}
pub type NumberInputs = BTreeMap<String, NumberInput>;

#[derive(Clone, Deserialize, Serialize)]
pub struct Draft {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub number_inputs: NumberInputs,
    pub base_revision: String,
    pub json: String,
}
impl Draft {
    fn path(root: &Path, id: &str) -> Result<std::path::PathBuf> {
        xray_config::valid_id(id)?;
        Ok(root.join("config/editor-drafts").join(format!("{id}.json")))
    }
    pub fn load(root: &Path, id: &str) -> Result<Option<Self>> {
        let draft: Option<Self> = xray_config::optional_json(&Self::path(root, id)?)?;
        if let Some(draft) = &draft
            && !draft.number_inputs.is_empty()
        {
            let value: serde_json::Value = serde_json::from_str(&draft.json)?;
            for (path, input) in &draft.number_inputs {
                if !value.pointer(path).is_some_and(|n| input.matches(n))
                    || input.text.parse::<serde_json::Number>().is_ok()
                {
                    return Err(xray_model::Error::new(
                        "DRAFT_RECOVERY_REQUIRED",
                        "Unfinished number input does not match its draft; preserve the draft for recovery",
                    ));
                }
            }
        }
        Ok(draft)
    }
    pub fn save(&self, root: &Path, id: &str) -> Result<()> {
        xray_config::write_json(&Self::path(root, id)?, self)
    }
    pub fn remove(root: &Path, id: &str) -> Result<()> {
        match std::fs::remove_file(Self::path(root, id)?) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_and_drafts_never_replace_native_profiles() {
        let root = tempfile::tempdir().unwrap();
        let store = xray_config::Store::open(root.path()).unwrap();
        let profile = store.create("work", "{}").unwrap();
        let mut workspace = Workspace::default();
        workspace.names.insert("work".into(), "工作网络".into());
        workspace.node_names.insert(
            "work".into(),
            BTreeMap::from([("node-1".into(), "香港节点".into())]),
        );
        workspace.last_core_running = true;
        let pac_draft = xray_model::PacDraft {
            generated: false,
            template: true,
            template_mode: xray_model::PacTemplateMode::ProxyList,
            domains: vec!["example.com".into()],
            script: "function FindProxyForURL(){ return 'DIRECT'; }".into(),
            port: xray_model::DEFAULT_PAC_PORT,
        };
        workspace.pac.insert("work".into(), pac_draft.clone());
        workspace.save(root.path()).unwrap();
        let draft = Draft {
            number_inputs: Default::default(),
            base_revision: profile.revision.clone(),
            json: "{\"unapplied\":true}".into(),
        };
        draft.save(root.path(), "work").unwrap();
        assert_eq!(store.profile("work").unwrap().json, "{}");
        assert_eq!(
            Workspace::load(root.path()).unwrap().name("work"),
            "工作网络"
        );
        assert_eq!(
            Draft::load(root.path(), "work")
                .unwrap()
                .unwrap()
                .base_revision,
            profile.revision
        );
        assert_eq!(
            Workspace::load(root.path()).unwrap().node_names["work"]["node-1"],
            "香港节点"
        );
        assert!(Workspace::load(root.path()).unwrap().last_core_running);
        assert_eq!(Workspace::load(root.path()).unwrap().pac["work"], pac_draft);
        assert!(Draft::load(root.path(), "../work").is_err());
        Draft::remove(root.path(), "work").unwrap();
        assert!(Draft::load(root.path(), "work").unwrap().is_none());
    }
}
