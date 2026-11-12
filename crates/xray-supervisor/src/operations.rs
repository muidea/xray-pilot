use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use xray_config::{optional_json, read_json, valid_id, write_json};
use xray_model::{Error, Request, Response, Result, canonical_hash, hash, id};

const MAX_RECORD_BYTES: usize = 64 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    operation_archive: u32,
    uncompressed_bytes: usize,
    sha256: String,
    gzip: Vec<u8>,
}
impl Operation {
    pub(super) fn archive(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(OperationStore::corrupt());
        }
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&bytes)?;
        Ok(serde_json::to_vec(&Archive {
            operation_archive: 1,
            uncompressed_bytes: bytes.len(),
            sha256: hash(&bytes),
            gzip: encoder.finish()?,
        })?)
    }
}

fn read_operation(path: &Path) -> Result<Option<Operation>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_RECORD_BYTES {
        return Err(OperationStore::corrupt());
    }
    let value: serde_json::Value = serde_json::from_slice(&bytes)?;
    if value.get("operation_archive").is_some() {
        let archive: Archive = serde_json::from_value(value)?;
        if archive.operation_archive != 1 || archive.uncompressed_bytes > MAX_RECORD_BYTES {
            return Err(OperationStore::corrupt());
        }
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(archive.gzip.as_slice())
            .take(archive.uncompressed_bytes as u64 + 1)
            .read_to_end(&mut decoded)?;
        if decoded.len() != archive.uncompressed_bytes || hash(&decoded) != archive.sha256 {
            return Err(OperationStore::corrupt());
        }
        Ok(Some(serde_json::from_slice(&decoded)?))
    } else {
        Ok(Some(serde_json::from_value(value)?))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Operation {
    pub caller: String,
    pub request: Request,
    pub digest: String,
    pub transaction_id: String,
    pub response: Option<Response>,
    #[serde(default)]
    pub recovery_required: bool,
    #[serde(default)]
    pub evidence_version: u32,
    #[serde(default)]
    pub record_hash: String,
}
#[derive(Serialize, Deserialize)]
struct Identity {
    caller: String,
    request_id: String,
    digest: String,
    transaction_id: String,
    checksum: String,
}
impl Identity {
    fn from_operation(op: &Operation) -> Result<Self> {
        let mut value = Self {
            caller: op.caller.clone(),
            request_id: op.request.request_id.clone(),
            digest: op.digest.clone(),
            transaction_id: op.transaction_id.clone(),
            checksum: String::new(),
        };
        value.checksum = canonical_hash(&value)?;
        Ok(value)
    }
}
pub struct OperationStore {
    root: PathBuf,
    runtime: PathBuf,
    identities: PathBuf,
    // Also remembers legacy records seen during this process, even if their files disappear later.
    known: Mutex<HashSet<PathBuf>>,
}
impl OperationStore {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.join("runtime/operations"),
            runtime: root.join("runtime"),
            identities: root.join("runtime/request-identities"),
            known: Mutex::new(HashSet::new()),
        }
    }
    pub(super) fn path(&self, caller: &str, request_id: &str) -> PathBuf {
        self.root.join(format!(
            "{}.json",
            hash(format!("{caller}\0{request_id}").as_bytes())
        ))
    }
    pub fn lookup(&self, caller: &str, request: &Request) -> Result<Option<Operation>> {
        let existing = self.get(caller, &request.request_id)?;
        if let Some(op) = &existing
            && op.digest != canonical_hash(request)?
        {
            return Err(Error::new(
                "REQUEST_ID_CONFLICT",
                "Request ID already belongs to different content",
            ));
        }
        Ok(existing)
    }
    pub fn get(&self, caller: &str, id: &str) -> Result<Option<Operation>> {
        let path = self.path(caller, id);
        let result = (|| {
            let op = read_operation(&path)?;
            if let Some(op) = &op {
                self.verify(&path, op)?;
                if op.caller != caller || op.request.request_id != id {
                    return Err(Self::corrupt());
                }
                self.known
                    .lock()
                    .map_err(|_| Self::corrupt())?
                    .insert(path.clone());
            } else if self.identity_path(caller, id).try_exists()?
                || self
                    .known
                    .lock()
                    .map_err(|_| Self::corrupt())?
                    .contains(&path)
            {
                return Err(Self::corrupt());
            }
            Ok(op)
        })();
        result.map_err(|_| Self::corrupt())
    }
    pub fn register(&self, caller: &str, request: Request) -> Result<Operation> {
        if let Some(existing) = self.lookup(caller, &request)? {
            return Ok(existing);
        }
        let op = Operation {
            caller: caller.into(),
            digest: canonical_hash(&request)?,
            request,
            transaction_id: id(),
            response: None,
            recovery_required: false,
            evidence_version: 2,
            record_hash: String::new(),
        };
        // Keep an independent identity before the primary record or any side effect.
        write_json(
            &self.identity_path(caller, &op.request.request_id),
            &Identity::from_operation(&op)?,
        )?;
        self.save(&op)?;
        self.known
            .lock()
            .map_err(|_| Self::corrupt())?
            .insert(self.path(caller, &op.request.request_id));
        Ok(op)
    }
    pub fn save(&self, op: &Operation) -> Result<()> {
        let mut record = op.clone();
        record.record_hash.clear();
        record.record_hash = canonical_hash(&record)?;
        write_json(&self.path(&op.caller, &op.request.request_id), &record)
    }
    fn corrupt() -> Error {
        Error::new(
            "OPERATION_RECOVERY_REQUIRED",
            "Operation evidence is unreadable or inconsistent; preserve it for recovery",
        )
    }
    fn verify(&self, path: &Path, op: &Operation) -> Result<()> {
        let mut record = op.clone();
        record.record_hash.clear();
        if path != self.path(&op.caller, &op.request.request_id)
            || op.request.request_id.is_empty()
            || op.request.request_id.len() > 160
            || valid_id(&op.transaction_id).is_err()
            || op.evidence_version > 2
            || op.digest != canonical_hash(&op.request)?
            || op
                .response
                .as_ref()
                .is_some_and(|r| r.request_id != op.request.request_id)
            || (op.evidence_version > 0 && op.record_hash.is_empty())
            || (!op.record_hash.is_empty() && op.record_hash != canonical_hash(&record)?)
        {
            return Err(Self::corrupt());
        }
        if let Some(identity) =
            optional_json::<Identity>(&self.identity_path(&op.caller, &op.request.request_id))?
        {
            if canonical_hash(&identity)? != canonical_hash(&Identity::from_operation(op)?)? {
                return Err(Self::corrupt());
            }
        } else if op.evidence_version >= 2 {
            return Err(Self::corrupt());
        }
        Ok(())
    }
    pub(super) fn identity_path(&self, caller: &str, request: &str) -> PathBuf {
        self.identities
            .join(self.path(caller, request).file_name().unwrap())
    }
    pub fn all(&self) -> Result<Vec<Operation>> {
        self.read_all().map_err(|_| Self::corrupt())
    }
    fn read_all(&self) -> Result<Vec<Operation>> {
        let mut records = vec![];
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            // Crash leftovers from atomic_write are not published operation records.
            if path.extension().is_some_and(|ext| ext == "json") {
                let op = read_operation(&path)?.ok_or_else(Self::corrupt)?;
                self.verify(&path, &op)?;
                records.push(op);
            }
        }
        let present: HashSet<_> = records
            .iter()
            .map(|op| self.path(&op.caller, &op.request.request_id))
            .collect();
        if !self
            .known
            .lock()
            .map_err(|_| Self::corrupt())?
            .is_subset(&present)
        {
            return Err(Self::corrupt());
        }
        let by_tx: HashMap<_, _> = records
            .iter()
            .map(|op| (op.transaction_id.clone(), op))
            .collect();
        if by_tx.len() != records.len() {
            return Err(Self::corrupt());
        }
        if self.identities.try_exists()? {
            for entry in fs::read_dir(&self.identities)? {
                let path = entry?.path();
                if path.extension().is_some_and(|ext| ext == "json") {
                    let identity: Identity = read_json(&path)?;
                    let op = by_tx
                        .get(&identity.transaction_id)
                        .ok_or_else(Self::corrupt)?;
                    if path != self.identity_path(&identity.caller, &identity.request_id)
                        || canonical_hash(&identity)?
                            != canonical_hash(&Identity::from_operation(op)?)?
                    {
                        return Err(Self::corrupt());
                    }
                }
            }
        }
        // Older versions have no independent identity. Their orphaned evidence must still block replay.
        for entry in fs::read_dir(self.runtime.join("transactions"))? {
            let path = entry?.path();
            let has_evidence = ["effect.json", "settings.json", "journal.json"]
                .into_iter()
                .try_fold(false, |found, name| -> Result<bool> {
                    Ok(path.join(name).try_exists()? || found)
                })?;
            if has_evidence
                && !by_tx.contains_key(&path.file_name().unwrap().to_string_lossy().into_owned())
            {
                return Err(Self::corrupt());
            }
        }
        for entry in fs::read_dir(self.runtime.join("commits"))? {
            let path = entry?.path();
            if path.extension().is_some_and(|ext| ext == "json") {
                let generation: xray_model::Generation = read_json(&path)?;
                if !by_tx.contains_key(&generation.transaction_id) {
                    return Err(Self::corrupt());
                }
            }
        }
        if let Some(generation) =
            optional_json::<xray_model::Generation>(&self.runtime.join("active-manifest.json"))?
            && !by_tx.contains_key(&generation.transaction_id)
        {
            return Err(Self::corrupt());
        }
        self.known.lock().map_err(|_| Self::corrupt())?.extend(
            records
                .iter()
                .map(|op| self.path(&op.caller, &op.request.request_id)),
        );
        Ok(records)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xray_model::Command;
    #[test]
    fn dedup_survives_restart_and_rejects_changed_content() {
        let tmp = tempfile::tempdir().unwrap();
        let _ = xray_config::Store::open(tmp.path()).unwrap();
        let store = OperationStore::new(tmp.path());
        let request = Request::new(Command::StopCore, Some("g1".into()));
        let mut op = store.register("sid", request.clone()).unwrap();
        op.response = Some(Response {
            request_id: request.request_id.clone(),
            result: Ok(serde_json::json!({"generation":"g2"})),
        });
        store.save(&op).unwrap();
        fs::write(store.root.join(".orphan.tmp"), b"{unfinished write").unwrap();
        assert_eq!(store.all().unwrap().len(), 1);
        let restarted = OperationStore::new(tmp.path());
        assert!(
            restarted
                .lookup("sid", &request)
                .unwrap()
                .unwrap()
                .response
                .unwrap()
                .result
                .is_ok()
        );
        let mut changed = request;
        changed.expected_generation = Some("g2".into());
        assert_eq!(
            restarted.lookup("sid", &changed).unwrap_err().code,
            "REQUEST_ID_CONFLICT"
        );
    }
}
