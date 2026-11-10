mod coverage;
pub mod resources;
pub mod routing;
pub mod security;

use fs2::FileExt;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};
use xray_model::{Error, Profile, Result, hash, id};

/// Explicitly unlock before closing: a concurrent Unix fork can briefly inherit the open
/// file description before exec, so close alone need not release its flock immediately.
pub struct StoreLock(File);
impl Drop for StoreLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::new("INVALID_PATH", "Missing parent directory"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{}.tmp", id()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        #[cfg(not(windows))]
        fs::rename(&temporary, path)?;
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            };
            let source = security::wide(&temporary);
            let target = security::wide(path);
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
            loop {
                if MoveFileExW(
                    source.as_ptr(),
                    target.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                ) != 0
                {
                    break;
                }
                let error = std::io::Error::last_os_error();
                // Short-lived scanner/indexer handles can deny replacement. Keep atomic rename
                // semantics and a fixed deadline; persistent locks and permission failures still fail.
                if !matches!(error.raw_os_error(), Some(5 | 32 | 33))
                    || std::time::Instant::now() >= deadline
                {
                    return Err(error.into());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        }
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}
pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(value)?)
}
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
pub fn optional_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn valid_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 80
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err(Error::new(
            "INVALID_ID",
            "Use 1–80 ASCII letters, digits, '-' or '_'",
        ));
    }
    Ok(())
}
pub fn parse_config(bytes: &[u8]) -> Result<Value> {
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(Error::new(
            "CONFIG_TOO_LARGE",
            "Configuration exceeds 4 MiB",
        ));
    }
    let value: Value = serde_json::from_slice(bytes)?;
    if !value.is_object() {
        return Err(Error::new(
            "CONFIG_PARSE_FAILED",
            "Configuration must be a JSON object",
        ));
    }
    for name in ["inbounds", "outbounds"] {
        if let Some(items) = value.get(name) {
            let items = items.as_array().ok_or_else(|| {
                Error::new(
                    "STATIC_VALIDATION_FAILED",
                    format!("{name} must be an array"),
                )
            })?;
            let mut tags = std::collections::HashSet::new();
            for item in items {
                if !item.is_object() {
                    return Err(Error::new(
                        "STATIC_VALIDATION_FAILED",
                        format!("{name} entries must be objects"),
                    ));
                }
                if let Some(tag) = item.get("tag").and_then(Value::as_str)
                    && !tags.insert(tag)
                {
                    return Err(Error::new(
                        "STATIC_VALIDATION_FAILED",
                        format!("Duplicate {name} tag: {tag}"),
                    ));
                }
                if name == "inbounds" && item.get("protocol").and_then(Value::as_str) == Some("tun")
                {
                    return Err(Error::new(
                        "UNSUPPORTED_CAPABILITY",
                        "TUN requires the Phase 5 privileged helper",
                    ));
                }
            }
        }
    }
    if let Some(rules) = value.pointer("/routing/rules").and_then(Value::as_array) {
        let tags: std::collections::HashSet<_> = value
            .get("outbounds")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|v| v.get("tag").and_then(Value::as_str))
            .collect();
        for rule in rules {
            if let Some(tag) = rule.get("outboundTag").and_then(Value::as_str)
                && !tags.contains(tag)
            {
                return Err(Error::new(
                    "STATIC_VALIDATION_FAILED",
                    format!("Unknown outboundTag: {tag}"),
                ));
            }
        }
    }
    Ok(value)
}

pub struct Store {
    pub root: PathBuf,
}
impl Store {
    pub fn open(root: impl AsRef<Path>) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        security::protect_directory(&root)?;
        for dir in [
            "config/profiles",
            "runtime/transactions",
            "runtime/operations",
            "runtime/commits",
            "runtime/recovery",
            "candidates",
            "resources",
            "cores",
            "snapshots",
        ] {
            fs::create_dir_all(root.join(dir))?;
        }
        Ok(Self { root })
    }
    pub fn lock(&self) -> Result<StoreLock> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join("runtime/supervisor.lock"))?;
        file.try_lock_exclusive().map_err(|_| {
            Error::new(
                "SUPERVISOR_ALREADY_RUNNING",
                "Another supervisor owns this data directory",
            )
        })?;
        Ok(StoreLock(file))
    }
    pub fn profile_path(&self, profile: &str) -> Result<PathBuf> {
        valid_id(profile)?;
        Ok(self
            .root
            .join("config/profiles")
            .join(profile)
            .join("config.json"))
    }
    pub fn profile(&self, profile: &str) -> Result<Profile> {
        let bytes = fs::read(self.profile_path(profile)?)?;
        Ok(Profile {
            id: profile.into(),
            revision: hash(&bytes),
            json: String::from_utf8(bytes)
                .map_err(|_| Error::new("CONFIG_PARSE_FAILED", "JSON must be UTF-8"))?,
        })
    }
    pub fn create(&self, profile: &str, json: &str) -> Result<Profile> {
        parse_config(json.as_bytes())?;
        let path = self.profile_path(profile)?;
        if path.exists() {
            return Err(Error::new("PROFILE_EXISTS", "Profile already exists"));
        }
        atomic_write(&path, json.as_bytes())?;
        self.profile(profile)
    }
    pub fn check_revision(&self, profile: &str, expected: &str) -> Result<Profile> {
        let current = self.profile(profile)?;
        if current.revision != expected {
            return Err(Error::new(
                "ASSET_REVISION_CONFLICT",
                "Profile changed; reload before applying",
            ));
        }
        Ok(current)
    }
    pub fn profiles(&self) -> Result<Vec<Profile>> {
        let mut profiles = vec![];
        for dir in fs::read_dir(self.root.join("config/profiles"))? {
            let dir = dir?;
            if dir.file_type()?.is_dir() && dir.path().join("config.json").is_file() {
                profiles.push(self.profile(&dir.file_name().to_string_lossy())?);
            }
        }
        profiles.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(profiles)
    }
    pub fn profile_page(
        &self,
        after: Option<&str>,
        limit: usize,
    ) -> Result<xray_model::ProfilePage> {
        if !(1..=100).contains(&limit) {
            return Err(Error::new(
                "INVALID_PAGE",
                "Profile page limit must be 1–100",
            ));
        }
        if let Some(after) = after {
            valid_id(after)?;
        }
        let mut names = vec![];
        for entry in fs::read_dir(self.root.join("config/profiles"))? {
            let entry = entry?;
            if entry.file_type()?.is_dir() && entry.path().join("config.json").is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                valid_id(&name)?;
                if after.is_none_or(|after| name.as_str() > after) {
                    names.push(name);
                }
            }
        }
        names.sort();
        let more = names.len() > limit;
        let items: Vec<_> = names
            .into_iter()
            .take(limit)
            .map(|name| {
                let bytes = fs::read(self.profile_path(&name)?)?;
                Ok(xray_model::ProfileSummary {
                    id: name,
                    revision: hash(&bytes),
                })
            })
            .collect::<Result<_>>()?;
        let next_after = more.then(|| items.last().unwrap().id.clone());
        Ok(xray_model::ProfilePage { items, next_after })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lock_is_released_even_if_an_open_description_is_temporarily_duplicated() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).unwrap();
        let lock = store.lock().unwrap();
        let duplicate = lock.0.try_clone().unwrap();
        assert_eq!(
            store.lock().err().unwrap().code,
            "SUPERVISOR_ALREADY_RUNNING"
        );
        drop(lock);
        let next = store.lock().unwrap();
        drop(duplicate);
        // Closing the old duplicated description must not release the new owner's lock.
        assert_eq!(
            store.lock().err().unwrap().code,
            "SUPERVISOR_ALREADY_RUNNING"
        );
        drop(next);
        assert!(store.lock().is_ok());
    }
    #[test]
    fn native_fields_and_external_revisions_are_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::open(temp.path()).unwrap();
        let raw = r#"{"outbounds":[{"tag":"a","protocol":"freedom","future":{"x":1}}],"vendor":{"unknown":true}}"#;
        let initial = store.create("a", raw).unwrap();
        assert_eq!(store.profile("a").unwrap().json, raw);
        atomic_write(&store.profile_path("a").unwrap(), b"{}").unwrap();
        assert_eq!(
            store
                .check_revision("a", &initial.revision)
                .unwrap_err()
                .code,
            "ASSET_REVISION_CONFLICT"
        );
        assert!(store.profile_path("../outside").is_err());
    }
}
