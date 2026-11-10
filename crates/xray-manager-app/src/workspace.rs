//! GUI metadata is separate from committed native profiles and runtime manifests.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};
use xray_model::Result;

#[derive(Clone, Default, Deserialize, Serialize)]
pub struct Workspace {
    #[serde(default)]
    pub names: BTreeMap<String, String>,
    #[serde(default)]
    pub asset_roots: BTreeMap<String, String>,
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

#[derive(Clone, Deserialize, Serialize)]
pub struct Draft {
    pub base_revision: String,
    pub json: String,
}
impl Draft {
    fn path(root: &Path, id: &str) -> Result<std::path::PathBuf> {
        xray_config::valid_id(id)?;
        Ok(root.join("config/editor-drafts").join(format!("{id}.json")))
    }
    pub fn load(root: &Path, id: &str) -> Result<Option<Self>> {
        xray_config::optional_json(&Self::path(root, id)?)
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
        workspace.save(root.path()).unwrap();
        let draft = Draft {
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
        assert!(Draft::load(root.path(), "../work").is_err());
        Draft::remove(root.path(), "work").unwrap();
        assert!(Draft::load(root.path(), "work").unwrap().is_none());
    }
}
