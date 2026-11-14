//! Offline, conservative maintenance. Request identities and history never expire.
use crate::{Journal, operations::OperationStore};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use xray_config::{Store, atomic_write, optional_json, read_json, valid_id};
use xray_model::{Error, Result};

#[derive(Clone, Debug)]
pub struct Options {
    pub apply: bool,
    pub retain_days: u32,
    /// Explicit opt-in: older binaries cannot read compressed operation records.
    pub compress_operations: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            apply: false,
            retain_days: 7,
            compress_operations: false,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct AssetReport {
    pub path: PathBuf,
    pub bytes: u64,
    pub disposition: &'static str,
}
#[derive(Debug, Serialize)]
pub struct CompressionReport {
    pub path: PathBuf,
    pub before_bytes: u64,
    pub after_bytes: u64,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub applied: bool,
    pub retain_days: u32,
    pub assets: Vec<AssetReport>,
    pub operations: Vec<CompressionReport>,
    pub reclaimable_bytes: u64,
}
#[derive(PartialEq, Eq)]
struct Entry {
    path: PathBuf,
    bytes: u64,
    modified: SystemTime,
    directory: bool,
}
fn blocked(message: &str) -> Error {
    Error::new("MAINTENANCE_BLOCKED", message)
}
fn scan(path: &Path, depth: usize, entries: &mut Vec<Entry>) -> Result<()> {
    if depth > 128 {
        return Err(blocked("Directory nesting exceeds maintenance limit"));
    }
    let meta = fs::symlink_metadata(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(blocked("Reparse points are not eligible for maintenance"));
        }
    }
    if meta.is_symlink() || (!meta.is_dir() && !meta.is_file()) {
        return Err(blocked(
            "Links and special files are not eligible for maintenance",
        ));
    }
    entries.push(Entry {
        path: path.into(),
        bytes: if meta.is_file() { meta.len() } else { 0 },
        modified: meta.modified()?,
        directory: meta.is_dir(),
    });
    if meta.is_dir() {
        let mut children = fs::read_dir(path)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        children.sort();
        for child in children {
            scan(&child, depth + 1, entries)?;
        }
    }
    Ok(())
}
fn snapshot(root: &Path) -> Result<Vec<Entry>> {
    let mut entries = vec![];
    scan(root, 0, &mut entries)?;
    Ok(entries)
}
struct Asset {
    id: String,
    path: PathBuf,
    bytes: u64,
    newest: SystemTime,
}
// Parse embedded JSON too: effect.after and draft.json may contain escaped reference strings.
fn mark(value: &Value, assets: &[Asset], live: &mut HashSet<usize>, depth: usize) -> Result<()> {
    if depth > 128 {
        return Err(blocked("Reference nesting exceeds maintenance limit"));
    }
    match value {
        Value::String(text) => {
            let normalized = text.to_ascii_lowercase();
            for (index, asset) in assets.iter().enumerate() {
                if normalized.contains(&asset.id.to_ascii_lowercase()) {
                    live.insert(index);
                }
            }
            if let Ok(nested) = serde_json::from_str::<Value>(text)
                && nested != *value
            {
                mark(&nested, assets, live, depth + 1)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                mark(value, assets, live, depth + 1)?;
            }
        }
        Value::Object(map) => {
            for (key, value) in map {
                mark(&Value::String(key.clone()), assets, live, depth + 1)?;
                mark(value, assets, live, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn terminal_outcome(value: &Value) -> bool {
    value.as_object().is_some_and(|object| {
        object.len() == 1 && (object.contains_key("Ok") || object.contains_key("Err"))
    })
}
fn terminal_settings_evidence(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    // SettingsJournal is intentionally checked as a closed shape here.  A
    // terminal outcome alone is not enough to prove that recovery can replay
    // the recorded settings/task transition.
    if object.len() != 5
        || !["before", "target", "task_before", "task_target", "outcome"]
            .iter()
            .all(|key| object.contains_key(*key))
    {
        return false;
    }
    let valid_settings = |value: &Value| value.is_null() || value.is_object();
    let valid_task = |value: &Value| {
        value.is_null()
            || value.as_object().is_some_and(|task| {
                task.len() == 1 && task.get("xml").is_some_and(Value::is_string)
            })
    };
    valid_settings(&object["before"])
        && object["target"].is_object()
        && valid_task(&object["task_before"])
        && valid_task(&object["task_target"])
        && matches!(
            object["outcome"].as_str(),
            Some("Committed") | Some("Compensated")
        )
}
fn settled(path: &Path, transaction_id: &str, operation_digest: Option<&str>) -> Result<()> {
    match path.file_name().and_then(|n| n.to_str()) {
        Some("journal.json") => {
            let journal: Journal = read_json(path)?;
            if !journal.settled {
                return Err(blocked(
                    "An unsettled runtime transaction requires recovery",
                ));
            }
        }
        Some("effect.json") => {
            let value: Value = read_json(path)?;
            if value.get("transaction_id").and_then(Value::as_str) != Some(transaction_id)
                || operation_digest.is_some_and(|digest| {
                    value.get("request_digest").and_then(Value::as_str) != Some(digest)
                })
                || value.get("publication").is_none()
                || value
                    .get("outcome")
                    .is_none_or(|outcome| !terminal_outcome(outcome))
            {
                return Err(blocked("An unfinished publication requires recovery"));
            }
        }
        Some("settings.json") => {
            let value: Value = read_json(path)?;
            if !terminal_settings_evidence(&value) {
                return Err(blocked(
                    "An unfinished settings transaction requires recovery",
                ));
            }
        }
        _ => {}
    }
    Ok(())
}
/// Holds the same exclusive lock as Supervisor throughout planning and mutation.
/// Never constructs a proxy/startup backend and never contacts IPC or the network.
pub fn run(root: &Path, options: &Options) -> Result<Report> {
    run_at(root, options, SystemTime::now())
}
fn run_at(root: &Path, options: &Options, now: SystemTime) -> Result<Report> {
    if !(1..=36500).contains(&options.retain_days) {
        return Err(blocked("Retention must be 1–36500 days"));
    }
    let root = fs::canonicalize(root)?;
    // Reject links before opening any lock file. Do not initialize a new store here.
    snapshot(&root)?;
    let store = Store { root: root.clone() };
    let _lock = store.lock()?;
    let before = snapshot(&root)?;
    let operations = OperationStore::new(&root);
    let records = operations.all()?;
    if records
        .iter()
        .any(|op| op.response.is_none() || op.recovery_required)
    {
        return Err(blocked(
            "Unfinished operations must be recovered before maintenance",
        ));
    }
    if fs::read_dir(root.join("runtime/recovery"))?
        .next()
        .is_some()
    {
        return Err(blocked(
            "Recovery records must be resolved before maintenance",
        ));
    }
    windows_integration::check_offline_proxy_evidence(&root)?;
    // Missing Global lease evidence must not be interpreted as a clean Direct installation.
    if let Some(g) =
        optional_json::<xray_model::Generation>(&root.join("runtime/active-manifest.json"))?
        && matches!(
            g.proxy_mode,
            xray_model::ProxyMode::Global | xray_model::ProxyMode::Pac
        )
        && !root.join("runtime/proxy-lease.json").try_exists()?
    {
        return Err(blocked("A Global generation has no proxy lease evidence"));
    }
    let mut assets = vec![];
    for kind in ["candidates", "resources", "cores", "runtime/pac"] {
        let directory = root.join(kind);
        if !directory.try_exists()? {
            continue;
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let id = entry
                .file_name()
                .into_string()
                .map_err(|_| blocked("Non-UTF8 asset name"))?;
            valid_id(&id)?;
            let managed_name = if kind == "cores" || kind == "runtime/pac" {
                id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())
            } else {
                id.len() == 36
                    && id.bytes().enumerate().all(|(index, byte)| {
                        if [8, 13, 18, 23].contains(&index) {
                            byte == b'-'
                        } else {
                            byte.is_ascii_hexdigit()
                        }
                    })
            };
            if !managed_name {
                return Err(blocked(
                    "Unrecognized asset name; preserve it for manual inspection",
                ));
            }
            if !entry.file_type()?.is_dir() {
                return Err(blocked("Unexpected file in asset directory"));
            }
            let path = entry.path();
            let tree: Vec<_> = before
                .iter()
                .filter(|e| e.path.starts_with(&path))
                .collect();
            assets.push(Asset {
                id,
                path,
                bytes: tree.iter().map(|e| e.bytes).sum(),
                newest: tree
                    .iter()
                    .filter(|e| !e.directory)
                    .map(|e| e.modified)
                    .max()
                    .or_else(|| tree.iter().map(|e| e.modified).max())
                    .ok_or_else(|| blocked("Missing asset metadata"))?,
            });
        }
    }
    assets.sort_by(|a, b| a.path.cmp(&b.path));
    let mut live = HashSet::new();
    for op in &records {
        mark(&serde_json::to_value(op)?, &assets, &mut live, 0)?;
    }
    let operation_digests: std::collections::HashMap<_, _> = records
        .iter()
        .map(|operation| (operation.transaction_id.as_str(), operation.digest.as_str()))
        .collect();
    for entry in &before {
        let path = &entry.path;
        if path.extension().is_some_and(|e| e == "json")
            && !entry.directory
            && ["config", "runtime", "snapshots"]
                .iter()
                .any(|d| path.starts_with(root.join(d)))
            && !path.starts_with(root.join("runtime/operations"))
        {
            if path.starts_with(root.join("runtime/transactions")) {
                let transaction_id = path
                    .parent()
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| blocked("Invalid transaction evidence path"))?;
                settled(
                    path,
                    transaction_id,
                    operation_digests.get(transaction_id).copied(),
                )?;
            }
            mark(&read_json::<Value>(path)?, &assets, &mut live, 0)?;
        }
    }
    // Follow only reachable artifact metadata. Unreachable manifests do not root themselves.
    let mut visited = HashSet::new();
    loop {
        let pending: Vec<_> = live.difference(&visited).copied().collect();
        if pending.is_empty() {
            break;
        }
        for index in pending {
            visited.insert(index);
            for entry in &before {
                if entry.path.starts_with(&assets[index].path)
                    && !entry.directory
                    && entry.path.extension().is_some_and(|e| e == "json")
                {
                    mark(&read_json::<Value>(&entry.path)?, &assets, &mut live, 0)?;
                }
            }
        }
    }
    let retention = Duration::from_secs(u64::from(options.retain_days) * 86400);
    let mut report = Report {
        applied: options.apply,
        retain_days: options.retain_days,
        assets: vec![],
        operations: vec![],
        reclaimable_bytes: 0,
    };
    let mut remove = vec![];
    for (index, asset) in assets.iter().enumerate() {
        let disposition = if live.contains(&index) {
            "referenced"
        } else if now
            .duration_since(asset.newest)
            .is_ok_and(|age| age >= retention)
        {
            remove.push(asset.path.clone());
            report.reclaimable_bytes += asset.bytes;
            "orphan"
        } else {
            "recent"
        };
        report.assets.push(AssetReport {
            path: asset.path.strip_prefix(&root).unwrap().into(),
            bytes: asset.bytes,
            disposition,
        });
    }
    let mut archives = vec![];
    if options.compress_operations {
        for op in records {
            let path = operations.path(&op.caller, &op.request.request_id);
            let meta = fs::metadata(&path)?;
            if now
                .duration_since(meta.modified()?)
                .is_ok_and(|age| age >= retention)
            {
                let bytes = op.archive()?;
                if bytes.len() as u64 >= meta.len() {
                    continue;
                }
                report.reclaimable_bytes += meta.len() - bytes.len() as u64;
                report.operations.push(CompressionReport {
                    path: path.strip_prefix(&root).unwrap().into(),
                    before_bytes: meta.len(),
                    after_bytes: bytes.len() as u64,
                });
                archives.push((path, bytes));
            }
        }
    }
    if options.apply {
        if snapshot(&root)? != before {
            return Err(blocked(
                "Data changed while planning maintenance; retry offline",
            ));
        }
        // Each replacement is atomic. Interruption leaves either an ordinary record or a full archive.
        for (path, bytes) in archives {
            atomic_write(&path, &bytes)?;
        }
        for path in remove {
            fs::remove_dir_all(path)?;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use xray_config::write_json;
    use xray_model::{Command, Request, Response, hash};
    fn artifact(root: &Path, kind: &str, id: &str, value: Value) -> PathBuf {
        let path = root.join(kind).join(id);
        fs::create_dir_all(&path).unwrap();
        write_json(&path.join("manifest.json"), &value).unwrap();
        path
    }
    fn later() -> SystemTime {
        SystemTime::now() + Duration::from_secs(8 * 86400)
    }
    fn completed(root: &Path, response: Value) -> (crate::operations::Operation, Request) {
        let operations = OperationStore::new(root);
        let request = Request::new(
            Command::CreateProfile {
                profile_id: "p".into(),
                json: format!("{{\"padding\":\"{}\"}}", "x".repeat(12000)),
            },
            None,
        );
        let mut op = operations.register("sid", request.clone()).unwrap();
        op.response = Some(Response {
            request_id: request.request_id.clone(),
            result: Ok(response),
        });
        operations.save(&op).unwrap();
        (
            operations.get("sid", &request.request_id).unwrap().unwrap(),
            request,
        )
    }
    #[test]
    fn preview_and_apply_protect_transitive_history_settings_and_recent_descendants() {
        let tmp = tempfile::tempdir().unwrap();
        Store::open(tmp.path()).unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let core = "1".repeat(64);
        let resource = xray_model::id();
        let candidate = xray_model::id();
        artifact(
            &root,
            "cores",
            &core,
            serde_json::json!({"binary":"present"}),
        );
        artifact(
            &root,
            "resources",
            &resource,
            serde_json::json!({"id":resource}),
        );
        artifact(
            &root,
            "candidates",
            &candidate,
            serde_json::json!({"resources":resource,"core":core}),
        );
        let (op, request) = completed(&root, serde_json::json!({"candidate":candidate}));
        let settings_core = "2".repeat(64);
        artifact(&root, "cores", &settings_core, serde_json::json!({}));
        write_json(
            &root.join("config/manager.json"),
            &serde_json::json!({"core_path":root.join("cores").join(&settings_core).join("xray")}),
        )
        .unwrap();
        let history = xray_model::id();
        artifact(&root, "candidates", &history, serde_json::json!({}));
        write_json(
            &root.join("snapshots/old.json"),
            &serde_json::json!({"candidate":history}),
        )
        .unwrap();
        let orphan = xray_model::id();
        let orphan_path = artifact(
            &root,
            "resources",
            &orphan,
            serde_json::json!({"id":orphan}),
        );
        let recent = xray_model::id();
        let recent_path = artifact(&root, "candidates", &recent, serde_json::json!({}));
        let now = later();
        fs::OpenOptions::new()
            .write(true)
            .open(recent_path.join("manifest.json"))
            .unwrap()
            .set_modified(now)
            .unwrap();
        let identity = OperationStore::new(&root).identity_path("sid", &request.request_id);
        let identity_hash = hash(&fs::read(&identity).unwrap());
        let options = Options {
            compress_operations: true,
            ..Default::default()
        };
        let preview = run_at(&root, &options, now).unwrap();
        assert!(!preview.applied);
        assert_eq!(
            preview
                .assets
                .iter()
                .filter(|a| a.disposition == "orphan")
                .count(),
            1
        );
        assert_eq!(
            preview
                .assets
                .iter()
                .filter(|a| a.disposition == "recent")
                .count(),
            1
        );
        assert_eq!(preview.operations.len(), 1);
        assert!(orphan_path.exists());
        let before_details = serde_json::to_vec(&op).unwrap();
        run_at(
            &root,
            &Options {
                apply: true,
                ..options
            },
            now,
        )
        .unwrap();
        assert!(!orphan_path.exists());
        assert!(recent_path.exists());
        assert_eq!(hash(&fs::read(&identity).unwrap()), identity_hash);
        let reopened = OperationStore::new(&root);
        assert_eq!(
            serde_json::to_vec(&reopened.lookup("sid", &request).unwrap().unwrap()).unwrap(),
            before_details
        );
        assert_eq!(reopened.all().unwrap().len(), 1);
        // Repeated maintenance does not alter the request or its full result.
        let next = run_at(
            &root,
            &Options {
                apply: true,
                compress_operations: true,
                ..Default::default()
            },
            now,
        )
        .unwrap();
        assert!(next.operations.is_empty());
    }
    #[test]
    fn active_owner_pending_operation_unsettled_and_corrupt_evidence_block_all_mutations() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).unwrap();
        let orphan = artifact(
            &store.root,
            "candidates",
            &xray_model::id(),
            serde_json::json!({}),
        );
        let options = Options {
            apply: true,
            ..Default::default()
        };
        let lock = store.lock().unwrap();
        assert_eq!(
            run_at(&store.root, &options, later()).unwrap_err().code,
            "SUPERVISOR_ALREADY_RUNNING"
        );
        drop(lock);
        let operations = OperationStore::new(&store.root);
        let request = Request::new(Command::StopCore, None);
        let mut op = operations.register("sid", request.clone()).unwrap();
        assert_eq!(
            run_at(&store.root, &options, later()).unwrap_err().code,
            "MAINTENANCE_BLOCKED"
        );
        op.response = Some(Response {
            request_id: request.request_id,
            result: Ok(serde_json::json!({})),
        });
        operations.save(&op).unwrap();
        let journal = store
            .root
            .join("runtime/transactions")
            .join(&op.transaction_id)
            .join("effect.json");
        write_json(&journal, &serde_json::json!({"outcome":null})).unwrap();
        assert_eq!(
            run_at(&store.root, &options, later()).unwrap_err().code,
            "MAINTENANCE_BLOCKED"
        );
        fs::remove_file(journal).unwrap();
        fs::write(
            store.root.join("runtime/proxy-lease-evidence.json"),
            b"broken",
        )
        .unwrap();
        assert!(run_at(&store.root, &options, later()).is_err());
        assert!(orphan.exists());
    }

    #[test]
    fn malformed_terminal_evidence_is_not_treated_as_settled() {
        let tmp = tempfile::tempdir().unwrap();
        let effect = tmp.path().join("effect.json");
        write_json(
            &effect,
            &serde_json::json!({
                "transaction_id": "tx",
                "request_digest": "digest",
                "publication": {},
                "outcome": false
            }),
        )
        .unwrap();
        assert_eq!(
            settled(&effect, "tx", Some("digest")).unwrap_err().code,
            "MAINTENANCE_BLOCKED"
        );
        let settings = tmp.path().join("settings.json");
        for malformed in [
            serde_json::json!({"outcome":"Committed"}),
            serde_json::json!({
                "before": null,
                "target": [],
                "task_before": null,
                "task_target": null,
                "outcome": "Committed"
            }),
            serde_json::json!({
                "before": null,
                "target": {},
                "task_before": {"xml": 1},
                "task_target": null,
                "outcome": "Committed"
            }),
        ] {
            write_json(&settings, &malformed).unwrap();
            assert_eq!(
                settled(&settings, "tx", Some("digest")).unwrap_err().code,
                "MAINTENANCE_BLOCKED"
            );
        }
        write_json(
            &settings,
            &serde_json::json!({
                "before": null,
                "target": {},
                "task_before": null,
                "task_target": {"xml": "<Task/>"},
                "outcome": "Committed"
            }),
        )
        .unwrap();
        assert!(settled(&settings, "tx", Some("digest")).is_ok());
        write_json(
            &effect,
            &serde_json::json!({
                "transaction_id": "other",
                "request_digest": "digest",
                "publication": {},
                "outcome": {"Ok": {}}
            }),
        )
        .unwrap();
        assert_eq!(
            settled(&effect, "tx", Some("digest")).unwrap_err().code,
            "MAINTENANCE_BLOCKED"
        );
    }
    #[test]
    fn damaged_archives_missing_identity_and_unpublished_temporary_files_never_enable_replay() {
        for fault in [
            "checksum",
            "truncated",
            "length",
            "version",
            "missing-identity",
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let store = Store::open(tmp.path()).unwrap();
            let (op, request) = completed(&store.root, serde_json::json!({"result":"original"}));
            let operations = OperationStore::new(&store.root);
            let path = operations.path("sid", &request.request_id);
            // A crash before atomic replacement only leaves an ignored unpublished temporary.
            fs::write(path.with_file_name(".interrupted.tmp"), b"broken").unwrap();
            assert!(operations.lookup("sid", &request).unwrap().is_some());
            let mut archive: Value = serde_json::from_slice(&op.archive().unwrap()).unwrap();
            match fault {
                "checksum" => archive["sha256"] = serde_json::json!("wrong"),
                "truncated" => {
                    archive["gzip"].as_array_mut().unwrap().truncate(10);
                }
                "length" => archive["uncompressed_bytes"] = serde_json::json!(2),
                "version" => archive["operation_archive"] = serde_json::json!(99),
                _ => fs::remove_file(operations.identity_path("sid", &request.request_id)).unwrap(),
            }
            write_json(&path, &archive).unwrap();
            assert_eq!(
                operations.lookup("sid", &request).unwrap_err().code,
                "OPERATION_RECOVERY_REQUIRED",
                "{fault}"
            );
            assert!(
                run_at(
                    &store.root,
                    &Options {
                        apply: true,
                        ..Default::default()
                    },
                    later()
                )
                .is_err()
            );
        }
    }
    #[tokio::test]
    async fn compressed_original_request_replays_without_recreating_a_deleted_profile() {
        struct Proxy(xray_model::ProxySnapshot);
        impl windows_integration::ProxyBackend for Proxy {
            fn read(&self) -> Result<xray_model::ProxySnapshot> {
                Ok(self.0.clone())
            }
            fn write(&mut self, value: &xray_model::ProxySnapshot) -> Result<()> {
                self.0 = value.clone();
                Ok(())
            }
        }
        let tmp = tempfile::tempdir().unwrap();
        let mut supervisor = crate::Supervisor::with_backend(tmp.path(), Proxy(Default::default()))
            .await
            .unwrap();
        let json = serde_json::json!({"padding":"x".repeat(16000)}).to_string();
        let request = Request::new(
            Command::CreateProfile {
                profile_id: "p".into(),
                json: json.clone(),
            },
            None,
        );
        let original = supervisor.handle(request.clone()).await.result.unwrap();
        supervisor
            .handle(Request::new(
                Command::DeleteProfile {
                    profile_id: "p".into(),
                    expected_revision: hash(json.as_bytes()),
                },
                None,
            ))
            .await
            .result
            .unwrap();
        drop(supervisor);
        let report = run_at(
            tmp.path(),
            &Options {
                apply: true,
                compress_operations: true,
                ..Default::default()
            },
            later(),
        )
        .unwrap();
        assert!(!report.operations.is_empty());
        let mut reopened = crate::Supervisor::with_backend(tmp.path(), Proxy(Default::default()))
            .await
            .unwrap();
        assert_eq!(reopened.handle(request).await.result.unwrap(), original);
        assert!(!reopened.store.profile_path("p").unwrap().exists());
        assert!(
            !reopened
                .handle(Request::new(Command::GetStatus, None))
                .await
                .result
                .unwrap()["recovery_required"]
                .as_bool()
                .unwrap()
        );
    }
    #[cfg(windows)]
    #[test]
    fn windows_junction_is_rejected_and_its_target_is_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("data")).unwrap();
        let outside = tmp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"keep").unwrap();
        let junction = store.root.join("resources").join(xray_model::id());
        let output = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&junction)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            run_at(
                &store.root,
                &Options {
                    apply: true,
                    ..Default::default()
                },
                later()
            )
            .unwrap_err()
            .code,
            "MAINTENANCE_BLOCKED"
        );
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"keep");
        fs::remove_dir(junction).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn symlinks_in_any_asset_or_evidence_tree_are_rejected_without_touching_the_target() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path().join("data")).unwrap();
        let outside = tmp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("keep"), b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, store.root.join("resources/link")).unwrap();
        assert_eq!(
            run_at(
                &store.root,
                &Options {
                    apply: true,
                    ..Default::default()
                },
                later()
            )
            .unwrap_err()
            .code,
            "MAINTENANCE_BLOCKED"
        );
        assert_eq!(fs::read(outside.join("keep")).unwrap(), b"keep");
    }
}

#[cfg(test)]
mod pac_cleanup_tests {
    use super::*;
    use xray_config::write_json;
    #[test]
    fn pac_publications_follow_the_same_reference_and_retention_rules() {
        let tmp = tempfile::tempdir().unwrap();
        Store::open(tmp.path()).unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let referenced = "a".repeat(64);
        let orphan = "b".repeat(64);
        for revision in [&referenced, &orphan] {
            let dir = root.join("runtime/pac").join(revision);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("proxy.pac"), "script").unwrap();
            let old = SystemTime::now() - Duration::from_secs(10 * 86400);
            fs::OpenOptions::new()
                .write(true)
                .open(dir.join("proxy.pac"))
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        let operations = OperationStore::new(&root);
        let request = xray_model::Request::new(xray_model::Command::StopCore, None);
        let mut op = operations.register("sid", request.clone()).unwrap();
        op.response = Some(xray_model::Response {
            request_id: request.request_id.clone(),
            result: Ok(serde_json::json!({"stopped": true})),
        });
        operations.save(&op).unwrap();
        let generation = xray_model::Generation {
            id: xray_model::id(),
            transaction_id: op.transaction_id.clone(),
            request_id: request.request_id.clone(),
            candidate: xray_model::Candidate {
                id: xray_model::id(),
                digest: "digest".into(),
                profile_id: "p".into(),
                base_revision: "base".into(),
                revision: "rev".into(),
                config_path: root.join("config/profiles/p/config.json"),
                core: xray_model::CoreIdentity {
                    path: root.join("cores").join("1".repeat(64)),
                    sha256: "1".repeat(64),
                    version: "test".into(),
                },
                resources: xray_model::Resources {
                    id: xray_model::id(),
                    root: root.join("resources").join(xray_model::id()),
                    files: vec![],
                    complete: true,
                    notes: vec![],
                    dynamic: vec![],
                },
            },
            proxy_mode: xray_model::ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            running: false,
            pac: Some(xray_model::PacPublication {
                revision: referenced.clone(),
                sha256: referenced.clone(),
                url: "http://127.0.0.1:18080/pac/a/proxy.pac".into(),
                port: 18080,
                path: root.join("runtime/pac").join(&referenced).join("proxy.pac"),
            }),
        };
        write_json(
            &root
                .join("runtime/commits")
                .join(format!("{}.json", op.transaction_id)),
            &generation,
        )
        .unwrap();
        let report = run_at(
            &root,
            &Options {
                apply: true,
                retain_days: 7,
                compress_operations: false,
            },
            SystemTime::now() + Duration::from_secs(8 * 86400),
        )
        .unwrap();
        assert!(root.join("runtime/pac").join(&referenced).exists());
        assert!(!root.join("runtime/pac").join(&orphan).exists());
        assert!(report.assets.iter().any(|a| a.disposition == "referenced"));
        assert!(report.assets.iter().any(|a| a.disposition == "orphan"));
    }
}
