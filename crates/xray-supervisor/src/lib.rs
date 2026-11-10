mod effects;
pub mod ipc;
mod operations;
mod restart_budget;
mod views;
#[cfg(test)]
use effects::EffectFault;

use operations::{Operation, OperationStore};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Instant,
};
use windows_integration::startup::{SettingsOutcome, StartupBackend, StartupController};
use windows_integration::{
    Lease, ProxyBackend, ProxyController, SystemProxy, SystemStartup, global,
};
use xray_config::{
    Store, atomic_write, optional_json, parse_config, parse_document, read_json, resources,
    valid_id, write_json,
};
use xray_model::*;
use xray_runtime::{Logs, ManagedCore};

#[derive(Clone, Serialize, Deserialize)]
struct Journal {
    generation: Generation,
    previous: Option<Generation>,
    // Missing in older journals: never infer a live process from desired state.
    #[serde(default)]
    previous_instance: Option<String>,
    before_json: String,
    before_hash: String,
    windows_before: ProxySnapshot,
    lease_before: Option<Lease>,
    promoted: bool,
    settled: bool,
}
struct Retry {
    generation: String,
    instance: String,
    epoch: u64,
    due: Instant,
}
pub struct Supervisor<B: ProxyBackend> {
    pub store: Store,
    _lock: xray_config::StoreLock,
    operations: OperationStore,
    proxy: ProxyController<B>,
    startup: StartupController,
    pub generation: Option<Generation>,
    core: Option<ManagedCore>,
    logs: Logs,
    epoch: String,
    recovery_epoch: u64,
    retry: Option<Retry>,
    failed_instance: Option<String>,
    last_error: Option<Error>,
    blocked: bool,
    pub shutdown: bool,
    settings: Settings,
    principal: String,
    installation: String,
    restart_budget: restart_budget::RestartBudget,
    #[cfg(test)]
    fail_settlement: bool,
    #[cfg(test)]
    effect_fault: Option<EffectFault>,
}
impl Supervisor<SystemProxy> {
    pub async fn open(root: &Path) -> Result<Self> {
        Self::with_backend(root, SystemProxy).await
    }
}
impl<B: ProxyBackend> Supervisor<B> {
    pub async fn with_backend(root: &Path, backend: B) -> Result<Self> {
        Self::with_backends(root, backend, Box::new(SystemStartup)).await
    }
    pub async fn with_backends(
        root: &Path,
        backend: B,
        startup_backend: Box<dyn StartupBackend>,
    ) -> Result<Self> {
        let store = Store::open(root)?;
        let lock = store.lock()?;
        let mut proxy = ProxyController::open(backend, &store.root)?;
        let installation_path = store.root.join("runtime/installation.json");
        let installation_result: Result<String> = (|| {
            if let Some(v) = optional_json::<String>(&installation_path)? {
                return Ok(v);
            }
            let has_evidence = [
                "operations",
                "request-identities",
                "transactions",
                "commits",
            ]
            .into_iter()
            .try_fold(false, |found, dir| -> Result<bool> {
                let path = store.root.join("runtime").join(dir);
                Ok(found || (path.try_exists()? && fs::read_dir(path)?.next().is_some()))
            })?;
            if has_evidence
                || store
                    .root
                    .join("runtime/active-manifest.json")
                    .try_exists()?
            {
                return Err(Error::new(
                    "INSTALLATION_ID_MISSING",
                    "Restore installation.json to preserve request deduplication",
                ));
            }
            let v = id();
            write_json(&installation_path, &v)?;
            Ok(v)
        })();
        let settings_result = optional_json::<Settings>(&store.root.join("config/manager.json"));
        let generation_result =
            optional_json::<Generation>(&store.root.join("runtime/active-manifest.json"));
        let lease_check = Self::check_legacy_proxy_evidence(
            &store,
            &mut proxy,
            generation_result.as_ref().ok().and_then(|g| g.as_ref()),
        );
        let initial_error = settings_result
            .as_ref()
            .err()
            .or(generation_result.as_ref().err())
            .or(installation_result.as_ref().err())
            .or(lease_check.as_ref().err())
            .or(proxy.recovery_error())
            .cloned();
        let installation = installation_result.unwrap_or_else(|_| "recovery-only".into());
        let settings = settings_result.unwrap_or_default().unwrap_or_default();
        let generation = generation_result.unwrap_or_default();
        let operations = OperationStore::new(&store.root);
        let principal = xray_config::security::principal()?;
        let startup =
            StartupController::new(startup_backend, &store.root, &principal, &installation);
        let mut supervisor = Self {
            store,
            _lock: lock,
            operations,
            proxy,
            startup,
            generation,
            core: None,
            logs: xray_runtime::logs(),
            epoch: id(),
            recovery_epoch: 0,
            retry: None,
            failed_instance: None,
            last_error: None,
            blocked: false,
            shutdown: false,
            settings,
            principal,
            installation,
            restart_budget: restart_budget::RestartBudget::default(),
            #[cfg(test)]
            fail_settlement: false,
            #[cfg(test)]
            effect_fault: None,
        };
        if let Some(e) = initial_error {
            // Invalid manager metadata must not prevent cleanup of an independently recorded lease.
            let cleanup = supervisor.proxy.release();
            supervisor.blocked = true;
            supervisor.last_error = Some(cleanup.err().unwrap_or(e));
        } else if let Err(e) = supervisor.recover().await {
            supervisor.blocked = true;
            supervisor.last_error = Some(e);
        }
        Ok(supervisor)
    }
    fn check_legacy_proxy_evidence(
        store: &Store,
        proxy: &mut ProxyController<B>,
        generation: Option<&Generation>,
    ) -> Result<()> {
        if proxy.lease.is_some() || proxy.recovery_error().is_some() {
            return Ok(());
        }
        if generation.is_some_and(|g| g.running && g.proxy_mode == ProxyMode::Global) {
            proxy.require_lease();
            return Ok(());
        }
        for entry in fs::read_dir(store.root.join("runtime/transactions"))? {
            let journal = optional_json::<Journal>(&entry?.path().join("journal.json"))?;
            if let Some(journal) = journal
                && !journal.settled
            {
                let previous_owned = journal
                    .lease_before
                    .as_ref()
                    .is_some_and(|l| l.state != windows_integration::LeaseState::Released);
                let possible_acquisition = journal.generation.proxy_mode == ProxyMode::Global
                    && proxy.snapshot()? != journal.windows_before;
                if previous_owned || possible_acquisition {
                    proxy.require_lease();
                    break;
                }
            }
        }
        Ok(())
    }
    fn manifest(&self) -> PathBuf {
        self.store.root.join("runtime/active-manifest.json")
    }
    fn publish(&mut self, generation: &Generation) -> Result<()> {
        let result = (|| {
            // Keep commit evidence before replacing the manifest, including during safety stops.
            let disk = optional_json::<Generation>(&self.manifest())?;
            if canonical_hash(&disk)? != canonical_hash(&self.generation)? {
                return Err(Error::new(
                    "GENERATION_COMMIT_CONFLICT",
                    "Commit point changed outside this supervisor; preserve it for recovery",
                ));
            }
            if let Some(previous) = &self.generation {
                write_json(
                    &self
                        .store
                        .root
                        .join("runtime/commits")
                        .join(format!("{}.json", previous.transaction_id)),
                    previous,
                )?;
            }
            write_json(&self.manifest(), generation)
        })();
        if let Err(e) = &result {
            self.blocked = true;
            self.last_error = Some(e.clone());
        }
        result
    }
    fn journal(&self, tx: &str) -> PathBuf {
        self.store
            .root
            .join("runtime/transactions")
            .join(tx)
            .join("journal.json")
    }
    fn settle(&mut self, path: &Path, journal: &mut Journal) -> Result<()> {
        journal.settled = true;
        #[cfg(test)]
        if self.fail_settlement {
            self.blocked = true;
            return Err(Error::new("IO_ERROR", "Injected settlement failure"));
        }
        let result = write_json(path, journal);
        if result.is_err() {
            self.blocked = true;
        }
        result
    }
    fn caller(&self) -> String {
        format!("{}:{}", self.principal, self.installation)
    }
    fn invalidate_retry(&mut self) {
        self.recovery_epoch = self.recovery_epoch.wrapping_add(1);
        self.retry = None;
        self.failed_instance = None;
    }
    fn status(&self) -> Status {
        Status {
            generation: self.generation.clone(),
            core_state: if self.core.is_some() {
                "Running"
            } else if self.retry.is_some() {
                "Degraded"
            } else {
                "Stopped"
            }
            .into(),
            proxy_state: self.proxy.state(),
            runtime_instance_id: self.core.as_ref().map(|c| c.instance_id.clone()),
            supervisor_epoch: self.epoch.clone(),
            last_error: self.last_error.clone(),
            recovery_required: self.blocked,
        }
    }
    fn history(&self, profile: &str) -> Result<Vec<Generation>> {
        valid_id(profile)?;
        let mut generations = std::collections::BTreeMap::new();
        for op in self.operations.all()? {
            if let Some(Response {
                result: Ok(value), ..
            }) = op.response
                && let Ok(g) = serde_json::from_value::<Generation>(value)
                && g.candidate.profile_id == profile
            {
                generations.insert(g.id.clone(), g);
            }
        }
        if let Some(g) = self
            .generation
            .as_ref()
            .filter(|g| g.candidate.profile_id == profile)
        {
            generations.insert(g.id.clone(), g.clone());
        }
        Ok(generations.into_values().collect())
    }
    fn runtime_config(&self, generation_id: &str) -> Result<RuntimeConfig> {
        let generation = self
            .generation
            .as_ref()
            .filter(|g| g.id == generation_id)
            .ok_or_else(|| {
                Error::new(
                    "GENERATION_CONFLICT",
                    "Refresh the runtime generation before reading its configuration",
                )
            })?;
        let json = fs::read_to_string(&generation.candidate.config_path)?;
        if hash(json.as_bytes()) != generation.candidate.revision {
            return Err(Error::new(
                "CONFIG_HASH_MISMATCH",
                "Frozen runtime configuration changed",
            ));
        }
        parse_config(json.as_bytes())?;
        Ok(RuntimeConfig {
            generation_id: generation.id.clone(),
            json,
        })
    }
    pub async fn handle(&mut self, request: Request) -> Response {
        let request_id = request.request_id.clone();
        let result = self.dispatch(request).await;
        if let Err(e) = &result {
            self.last_error = Some(e.clone());
        }
        Response { request_id, result }
    }
    fn operation_read<T>(&mut self, result: Result<T>) -> Result<T> {
        if let Err(error) = &result
            && error.code == "OPERATION_RECOVERY_REQUIRED"
        {
            self.blocked = true;
            self.last_error = Some(error.clone());
            self.invalidate_retry();
        }
        result
    }
    async fn dispatch(&mut self, request: Request) -> Result<Value> {
        if request.protocol_version != 1 {
            return Err(Error::new(
                "PROTOCOL_VERSION_UNSUPPORTED",
                "Only protocol version 1 is supported",
            ));
        }
        if request.request_id.len() > 160 || request.request_id.is_empty() {
            return Err(Error::new("INVALID_REQUEST", "Invalid request ID"));
        }
        let caller = self.caller();
        match &request.command {
            Command::GetStatus => return Ok(serde_json::to_value(self.status())?),
            Command::GetRuntimeConfig { generation_id } => {
                return Ok(serde_json::to_value(self.runtime_config(generation_id)?)?);
            }
            Command::ListProfiles => return Ok(serde_json::to_value(self.store.profiles()?)?),
            Command::ListProfileSummaries { after, limit } => {
                return Ok(serde_json::to_value(
                    self.store.profile_page(after.as_deref(), *limit)?,
                )?);
            }
            Command::GetProfile { profile_id } => {
                return Ok(serde_json::to_value(self.store.profile(profile_id)?)?);
            }
            Command::ListHistory { profile_id } => {
                let result = self.history(profile_id);
                let history = self.operation_read(result)?;
                return Ok(serde_json::to_value(history)?);
            }
            Command::GetSettings => return Ok(serde_json::to_value(&self.settings)?),
            Command::GetLogs => {
                return Ok(json!(
                    self.logs
                        .lock()
                        .map(|l| l.iter().cloned().collect::<Vec<_>>())
                        .unwrap_or_default()
                ));
            }
            Command::GetOperation { request_id } => {
                let result = self.operations.get(&caller, request_id);
                let operation = self.operation_read(result)?;
                if operation.is_none() {
                    let audit = self.operations.all();
                    self.operation_read(audit)?;
                }
                return operation
                    .map(|op| op.view(self.blocked))
                    .transpose()
                    .map(|v| v.unwrap_or(Value::Null));
            }
            Command::GetOperationDetails {
                request_id,
                expected_hash,
                offset,
                limit,
            } => {
                let result = self.operations.get(&caller, request_id);
                let op = self
                    .operation_read(result)?
                    .ok_or_else(|| Error::new("OPERATION_NOT_FOUND", "Unknown operation"))?;
                return op.details(expected_hash, *offset, *limit);
            }
            Command::TestRoute {
                profile_id,
                context,
            } => {
                let profile = self.store.profile(profile_id)?;
                return Ok(serde_json::to_value(xray_config::routing::simulate(
                    &parse_config(profile.json.as_bytes())?,
                    context,
                ))?);
            }
            Command::TestRunningRoute {
                generation_id,
                context,
            } => {
                if self.core.is_none() {
                    return Err(Error::new(
                        "CORE_NOT_RUNNING",
                        "Start Core before testing the running configuration",
                    ));
                }
                let config = self.runtime_config(generation_id)?;
                return Ok(serde_json::to_value(xray_config::routing::simulate(
                    &parse_config(config.json.as_bytes())?,
                    context,
                ))?);
            }
            _ => {}
        }
        // Deduplication deliberately precedes generation/revision checks, including after restart.
        let lookup = self.operations.lookup(&caller, &request);
        let existing = self.operation_read(lookup)?;
        if let Some(existing) = existing {
            return existing.response.map(|r| r.result).unwrap_or_else(|| {
                Err(Error::new(
                    if existing.recovery_required || self.blocked {
                        "RECOVERY_REQUIRED"
                    } else {
                        "OPERATION_IN_PROGRESS"
                    },
                    "Query the original operation; it will not be executed again",
                ))
            });
        }
        if !self.blocked && !matches!(request.command, Command::StopCore | Command::Shutdown) {
            // New identities also require an intact evidence set, including legacy orphan journals.
            let audit = self.operations.all();
            self.operation_read(audit)?;
        }
        if self.blocked && !matches!(request.command, Command::StopCore | Command::Shutdown) {
            return Err(Error::new(
                "RECOVERY_REQUIRED",
                "Unresolved recovery; inspect status and repair before writing",
            ));
        }
        let mut op = match self.operations.register(&caller, request.clone()) {
            Ok(op) => op,
            Err(e) => {
                self.blocked = true;
                return Err(e);
            }
        };
        let result =
            if request.expected_generation != self.generation.as_ref().map(|g| g.id.clone()) {
                Err(Error::new(
                    "GENERATION_CONFLICT",
                    "Runtime changed; refresh status",
                ))
            } else {
                self.execute(&op).await
            };
        // An unresolved effect intent is not a terminal failure. Keep it replay-safe until recovery.
        let pending =
            result.is_err() && self.blocked && self.has_pending_intent(&op).unwrap_or(true);
        op.recovery_required = pending;
        if !pending {
            op.response = Some(Response {
                request_id: request.request_id,
                result: result.clone(),
            });
        }
        if let Err(e) = self.operations.save(&op) {
            self.blocked = true;
            self.last_error = Some(e.clone());
            return Err(Error::new(
                "OPERATION_RECOVERY_REQUIRED",
                "Operation result could not be persisted; no further changes allowed",
            ));
        }
        result
    }
    async fn execute(&mut self, op: &Operation) -> Result<Value> {
        let tracked = Self::tracks_effect(&op.request.command);
        if tracked {
            self.begin_effect(op)?;
        }
        let result = self.execute_inner(op).await;
        let result = self.operation_read(result);
        // A publication error may be ambiguous. Preserve its intent until startup recovery.
        if tracked
            && !self.blocked
            && let Some(outcome) = self.finish_effect(op, result.as_ref().err().cloned())?
        {
            return outcome;
        }
        result
    }
    async fn execute_inner(&mut self, op: &Operation) -> Result<Value> {
        match &op.request.command {
            Command::StageHistory {
                generation_id,
                expected_revision,
            } => {
                let mut old = None;
                for profile in self.store.profiles()? {
                    for g in self.history(&profile.id)? {
                        if g.id == *generation_id {
                            old = Some(g);
                        }
                    }
                }
                let old = old.ok_or_else(|| {
                    Error::new("SNAPSHOT_NOT_FOUND", "Unknown historical generation")
                })?;
                self.store
                    .check_revision(&old.candidate.profile_id, expected_revision)?;
                Self::verify_candidate(&old.candidate)?;
                let mut candidate = old.candidate;
                candidate.id = id();
                candidate.base_revision = expected_revision.clone();
                candidate.digest.clear();
                candidate.digest = canonical_hash(&candidate)?;
                let path = self.store.root.join("candidates").join(&candidate.id);
                xray_runtime::validate(&candidate).await?;
                let success = json!({"json":fs::read_to_string(&candidate.config_path)?,"candidate":candidate});
                self.publish_effect(
                    op,
                    path.join("candidate.json"),
                    None,
                    Some(serde_json::to_string_pretty(&candidate)?),
                    success,
                )
            }
            Command::CreateProfile { profile_id, json } => {
                parse_document(json.as_bytes())?;
                let path = self.store.profile_path(profile_id)?;
                if path.exists() {
                    return Err(Error::new("PROFILE_EXISTS", "Profile already exists"));
                }
                let success = serde_json::to_value(Profile {
                    id: profile_id.clone(),
                    json: json.clone(),
                    revision: hash(json.as_bytes()),
                })?;
                self.publish_effect(op, path, None, Some(json.clone()), success)
            }
            Command::DeleteProfile {
                profile_id,
                expected_revision,
            } => {
                if self
                    .generation
                    .as_ref()
                    .is_some_and(|g| g.candidate.profile_id == *profile_id)
                {
                    return Err(Error::new(
                        "PROFILE_IN_USE",
                        "Committed profile cannot be deleted",
                    ));
                }
                self.store.check_revision(profile_id, expected_revision)?;
                self.publish_effect(
                    op,
                    self.store.profile_path(profile_id)?,
                    Some(expected_revision.clone()),
                    None,
                    json!({"deleted":true}),
                )
            }
            Command::ValidateDraft {
                profile_id,
                expected_revision,
                json,
                core_path,
                asset_root,
            } => {
                self.store.check_revision(profile_id, expected_revision)?;
                let config = parse_config(json.as_bytes())?;
                let core_path = fs::canonicalize(core_path)?;
                let core_bytes = fs::read(&core_path)?;
                let core_hash = hash(&core_bytes);
                let directory = self.store.root.join("cores").join(&core_hash);
                fs::create_dir_all(&directory)?;
                let frozen = directory.join(if cfg!(windows) { "xray.exe" } else { "xray" });
                if !frozen.exists() {
                    atomic_write(&frozen, &core_bytes)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&frozen, fs::Permissions::from_mode(0o700))?;
                    }
                }
                let version = xray_runtime::version(&frozen).await?;
                let resources =
                    resources::freeze(&config, asset_root, &self.store.root.join("resources"))?;
                let candidate_id = id();
                let path = self.store.root.join("candidates").join(&candidate_id);
                fs::create_dir_all(&path)?;
                let config_path = path.join("config.json");
                atomic_write(&config_path, json.as_bytes())?;
                let mut candidate = Candidate {
                    id: candidate_id,
                    digest: String::new(),
                    profile_id: profile_id.clone(),
                    base_revision: expected_revision.clone(),
                    revision: hash(json.as_bytes()),
                    config_path,
                    core: CoreIdentity {
                        path: frozen,
                        sha256: core_hash,
                        version,
                    },
                    resources,
                };
                candidate.digest = canonical_hash(&candidate)?;
                resources::verify(&candidate.resources)?;
                xray_runtime::validate(&candidate).await?;
                self.publish_effect(
                    op,
                    path.join("candidate.json"),
                    None,
                    Some(serde_json::to_string_pretty(&candidate)?),
                    serde_json::to_value(&candidate)?,
                )
            }
            Command::ApplyProfile {
                candidate_id,
                candidate_digest,
                proxy_mode,
                http_inbound_tag,
            } => {
                valid_id(candidate_id)?;
                let candidate: Candidate = read_json(
                    &self
                        .store
                        .root
                        .join("candidates")
                        .join(candidate_id)
                        .join("candidate.json"),
                )?;
                if &candidate.digest != candidate_digest {
                    return Err(Error::new(
                        "CANDIDATE_CONFLICT",
                        "Candidate digest does not match request",
                    ));
                }
                self.invalidate_retry();
                self.apply(op, candidate, *proxy_mode, http_inbound_tag.clone(), true)
                    .await
            }
            Command::StopCore | Command::Shutdown => {
                self.invalidate_retry();
                if let Some(g) = self.generation.clone() {
                    let value = self
                        .apply(
                            op,
                            g.candidate,
                            ProxyMode::Direct,
                            g.http_inbound_tag,
                            false,
                        )
                        .await?;
                    if matches!(op.request.command, Command::Shutdown) {
                        self.shutdown = true;
                    }
                    Ok(value)
                } else {
                    if let Err(error) = self.proxy.release() {
                        self.blocked = true;
                        self.last_error = Some(error);
                    }
                    if matches!(op.request.command, Command::Shutdown) {
                        self.shutdown = true;
                    }
                    Ok(json!({"stopped":true}))
                }
            }
            Command::RestartCore | Command::SetProxyMode { .. } => {
                let old = self.generation.clone().ok_or_else(|| {
                    Error::new("NO_ACTIVE_PROFILE", "Apply a validated profile first")
                })?;
                let (mode, tag) = match &op.request.command {
                    Command::SetProxyMode {
                        mode,
                        http_inbound_tag,
                    } => (*mode, http_inbound_tag.clone()),
                    _ => (old.proxy_mode, old.http_inbound_tag),
                };
                self.invalidate_retry();
                self.apply(op, old.candidate, mode, tag, true).await
            }
            Command::SaveSettings { settings } => {
                let result =
                    self.startup
                        .save(&op.transaction_id, settings, &std::env::current_exe()?);
                if let Err(e) = result {
                    if self.startup.pending(&op.transaction_id).unwrap_or(true) {
                        self.blocked = true;
                    }
                    return Err(e);
                }
                self.settings = settings.clone();
                Ok(json!({"saved":true}))
            }
            _ => Err(Error::new(
                "INVALID_OPERATION",
                "Unexpected write operation",
            )),
        }
    }
    fn verify_candidate(candidate: &Candidate) -> Result<()> {
        let mut check = candidate.clone();
        check.digest.clear();
        if canonical_hash(&check)? != candidate.digest
            || hash(&fs::read(&candidate.config_path)?) != candidate.revision
        {
            return Err(Error::new("CANDIDATE_CONFLICT", "Frozen candidate changed"));
        }
        resources::verify(&candidate.resources)?;
        xray_runtime::verify_core(&candidate.core)
    }
    async fn start(&mut self, g: &Generation) -> Result<()> {
        Self::verify_candidate(&g.candidate)?;
        self.core = Some(ManagedCore::start(&g.candidate, &g.id, self.logs.clone()).await?);
        if g.proxy_mode == ProxyMode::Global {
            let config = parse_config(&fs::read(&g.candidate.config_path)?)?;
            let endpoint = xray_runtime::http_endpoint(&config, &g.http_inbound_tag)?;
            self.core.as_mut().unwrap().wait_http(endpoint).await?;
        }
        Ok(())
    }
    fn running_instance(&mut self) -> Result<Option<String>> {
        self.core
            .as_mut()
            .map(|c| {
                c.process
                    .try_wait()
                    .map(|exit| exit.is_none().then(|| c.instance_id.clone()))
            })
            .transpose()
            .map(Option::flatten)
    }
    async fn stop_process(&mut self) -> Result<()> {
        if let Some(mut core) = self.core.take() {
            core.stop().await?;
        }
        Ok(())
    }
    async fn apply(
        &mut self,
        op: &Operation,
        candidate: Candidate,
        mode: ProxyMode,
        tag: String,
        running: bool,
    ) -> Result<Value> {
        if !running {
            return self.stop_generation(op, candidate, tag).await;
        }
        Self::verify_candidate(&candidate)?;
        let old_profile = self.store.profile(&candidate.profile_id)?;
        let reuse = self
            .generation
            .as_ref()
            .is_some_and(|g| g.candidate.id == candidate.id);
        if old_profile.revision != candidate.base_revision
            && !(reuse && old_profile.revision == candidate.revision)
        {
            return Err(Error::new(
                "ASSET_REVISION_CONFLICT",
                "Profile changed after validation",
            ));
        }
        if running {
            xray_runtime::validate(&candidate).await?;
        }
        let target = if running && mode == ProxyMode::Global {
            Some(global(xray_runtime::http_endpoint(
                &parse_config(&fs::read(&candidate.config_path)?)?,
                &tag,
            )?))
        } else {
            None
        };
        let generation = Generation {
            id: id(),
            transaction_id: op.transaction_id.clone(),
            request_id: op.request.request_id.clone(),
            candidate,
            proxy_mode: mode,
            http_inbound_tag: tag,
            running,
        };
        let mut journal = Journal {
            generation: generation.clone(),
            previous: self.generation.clone(),
            previous_instance: self.running_instance()?,
            before_hash: old_profile.revision,
            before_json: old_profile.json,
            windows_before: self.proxy.snapshot()?,
            lease_before: self.proxy.lease.clone(),
            promoted: false,
            settled: false,
        };
        let journal_path = self.journal(&op.transaction_id);
        if let Err(e) = write_json(&journal_path, &journal) {
            self.blocked = true;
            return Err(e);
        }
        let result: Result<()> = async {
            if !running {
                self.proxy.release()?;
            }
            self.stop_process().await?;
            if running {
                self.start(&generation).await?;
            }
            if let Some(target) = target {
                self.proxy.transition(target)?;
            } else {
                self.proxy.release()?;
            }
            self.store
                .check_revision(&generation.candidate.profile_id, &journal.before_hash)?;
            // Persist promotion intent before replacing the formal native asset.
            journal.promoted = true;
            write_json(&journal_path, &journal)?;
            atomic_write(
                &self.store.profile_path(&generation.candidate.profile_id)?,
                &fs::read(&generation.candidate.config_path)?,
            )?;
            self.publish(&generation)?;
            Ok(())
        }
        .await;
        // Re-read the commit point even if publication returned an ambiguous error.
        let committed = match optional_json::<Generation>(&self.manifest()) {
            Ok(value) => value,
            Err(_) => {
                self.blocked = true;
                let _ = self.proxy.release();
                let _ = self.stop_process().await;
                return Err(Error::new(
                    "GENERATION_COMMIT_UNCERTAIN",
                    "Cannot read commit point; runtime stopped and recovery evidence preserved",
                ));
            }
        };
        if committed.as_ref().is_some_and(|g| g.id == generation.id) {
            self.generation = Some(generation.clone());
            if let Err(e) = self.proxy.finalize() {
                self.blocked = true;
                self.last_error = Some(e);
            }
            if let Err(e) = self.settle(&journal_path, &mut journal) {
                self.blocked = true;
                self.last_error = Some(e);
            }
            return Ok(serde_json::to_value(generation)?);
        }
        if let Err(error) = result {
            if let Err(rollback) = self.compensate(&journal, true).await {
                self.blocked = true;
                return Err(Error::new(
                    "ROLLBACK_FAILED",
                    format!("{}; {}", error.code, rollback.code),
                ));
            }
            self.settle(&journal_path, &mut journal)?;
            return Err(error);
        }
        self.blocked = true;
        Err(Error::new(
            "GENERATION_COMMIT_FAILED",
            "Manifest does not match committed generation",
        ))
    }
    // Stopping must remain available when the frozen Core/config/resources are damaged.
    async fn stop_generation(
        &mut self,
        op: &Operation,
        candidate: Candidate,
        tag: String,
    ) -> Result<Value> {
        let generation = Generation {
            id: id(),
            transaction_id: op.transaction_id.clone(),
            request_id: op.request.request_id.clone(),
            candidate,
            proxy_mode: ProxyMode::Direct,
            http_inbound_tag: tag,
            running: false,
        };
        let mut journal = Journal {
            generation: generation.clone(),
            previous: self.generation.clone(),
            previous_instance: self.running_instance()?,
            before_json: String::new(),
            before_hash: String::new(),
            windows_before: self.proxy.snapshot().unwrap_or_default(),
            lease_before: self.proxy.lease.clone(),
            promoted: false,
            settled: false,
        };
        let path = self.journal(&op.transaction_id);
        if let Err(e) = write_json(&path, &journal) {
            self.blocked = true;
            return Err(e);
        }
        let proxy_result = self.proxy.release();
        if let Err(e) = self.stop_process().await {
            self.blocked = true;
            return Err(e);
        }
        if let Err(e) = proxy_result {
            self.blocked = true;
            self.last_error = Some(e);
        }
        if let Err(e) = self.publish(&generation)
            && optional_json::<Generation>(&self.manifest())
                .ok()
                .flatten()
                .is_none_or(|g| g.id != generation.id)
        {
            self.blocked = true;
            return Err(e);
        }
        self.generation = Some(generation.clone());
        if let Err(e) = self.settle(&path, &mut journal) {
            self.blocked = true;
            self.last_error = Some(e);
        }
        Ok(serde_json::to_value(generation)?)
    }
    fn restore_asset(&self, journal: &Journal) -> Result<()> {
        if !journal.promoted {
            return Ok(());
        }
        let current = self
            .store
            .profile(&journal.generation.candidate.profile_id)?;
        if current.revision == journal.before_hash {
            return Ok(());
        }
        if current.revision != journal.generation.candidate.revision {
            return Err(Error::new(
                "ASSET_REVISION_CONFLICT",
                "External file change preserved during recovery",
            ));
        }
        atomic_write(
            &self.store.profile_path(&current.id)?,
            journal.before_json.as_bytes(),
        )
    }
    async fn compensate(&mut self, journal: &Journal, restart: bool) -> Result<()> {
        self.stop_process().await?;
        if let Err(e) = self.restore_asset(journal) {
            let _ = self.proxy.release();
            return Err(e);
        }
        if restart {
            if let Some(g) = &journal.previous
                && journal.previous_instance.is_some()
            {
                if !g.candidate.resources.complete {
                    self.proxy.release()?;
                    return Err(Error::new(
                        "RESOURCE_REVALIDATION_REQUIRED",
                        "Previous generation contains unclassified dependencies",
                    ));
                }
                if let Err(e) = self.start(g).await {
                    let _ = self.stop_process().await;
                    let _ = self.proxy.release();
                    return Err(e);
                }
            }
            self.proxy
                .compensate(journal.lease_before.clone(), journal.windows_before.clone())?;
        } else {
            self.proxy.release()?;
        }
        Ok(())
    }
    fn has_pending_intent(&self, op: &Operation) -> Result<bool> {
        Ok(
            optional_json::<Journal>(&self.journal(&op.transaction_id))?
                .is_some_and(|j| !j.settled)
                || self.startup.pending(&op.transaction_id)?
                || self.pending_effect(op)?,
        )
    }
    async fn recover(&mut self) -> Result<()> {
        let mut recovery_error = None;
        let journal_recovery: Result<()> = (|| {
            for entry in fs::read_dir(self.store.root.join("runtime/transactions"))? {
                let path = entry?.path().join("journal.json");
                if !path.exists() {
                    continue;
                }
                let mut journal: Journal = read_json(&path)?;
                if journal.settled {
                    continue;
                }
                if self
                    .generation
                    .as_ref()
                    .is_some_and(|g| g.id == journal.generation.id)
                    || optional_json::<Generation>(
                        &self
                            .store
                            .root
                            .join("runtime/commits")
                            .join(format!("{}.json", journal.generation.transaction_id)),
                    )?
                    .is_some_and(|g| g.id == journal.generation.id)
                {
                    self.proxy.finalize()?;
                } else {
                    self.restore_asset(&journal)?;
                }
                self.proxy.release()?;
                self.settle(&path, &mut journal)?;
            }
            Ok(())
        })();
        if let Err(e) = journal_recovery {
            recovery_error = Some(e);
        }
        // Always conditionally release stale ownership, even when journal/asset recovery failed.
        if let Err(e) = self.proxy.release() {
            recovery_error = Some(e);
        }
        if let Err(e) = self.startup.recover() {
            recovery_error = Some(e);
        } else {
            self.settings =
                optional_json::<Settings>(&self.store.root.join("config/manager.json"))?
                    .unwrap_or_default();
        }
        for mut op in self.operations.all()? {
            if op.response.is_some() {
                continue;
            }
            let committed = self
                .generation
                .as_ref()
                .filter(|g| g.transaction_id == op.transaction_id)
                .cloned()
                .or(optional_json::<Generation>(
                    &self
                        .store
                        .root
                        .join("runtime/commits")
                        .join(format!("{}.json", op.transaction_id)),
                )?);
            let publication = match self.finish_effect(&op, None) {
                Ok(value) => value,
                Err(e) => {
                    recovery_error = Some(e);
                    op.recovery_required = true;
                    self.operations.save(&op)?;
                    continue;
                }
            };
            let result = if let Some(outcome) = publication {
                outcome
            } else if op.evidence_version == 0 && Self::tracks_effect(&op.request.command) {
                recovery_error = Some(Error::new(
                    "OPERATION_RECOVERY_REQUIRED",
                    "Legacy operation lacks publication evidence; manual reconciliation required",
                ));
                op.recovery_required = true;
                self.operations.save(&op)?;
                continue;
            } else if let Some(g) = committed {
                Ok(serde_json::to_value(g)?)
            } else if self.startup.outcome(&op.transaction_id)? == Some(SettingsOutcome::Committed)
            {
                Ok(json!({"saved":true}))
            } else if recovery_error.is_some() || self.has_pending_intent(&op)? {
                op.recovery_required = true;
                self.operations.save(&op)?;
                continue;
            } else if self.journal(&op.transaction_id).exists()
                || self.startup.outcome(&op.transaction_id)? == Some(SettingsOutcome::Compensated)
            {
                Err(Error::new(
                    "INTERRUPTED_COMPENSATED",
                    "Executed intent was compensated; no generation committed",
                ))
            } else {
                Err(Error::new(
                    "INTERRUPTED_BEFORE_APPLY",
                    "No durable execution intent or commit found",
                ))
            };
            op.recovery_required = false;
            op.response = Some(Response {
                request_id: op.request.request_id.clone(),
                result,
            });
            self.operations.save(&op)?;
        }
        if let Some(e) = recovery_error {
            return Err(e);
        }
        if self.settings.connect_on_start
            && let Some(old) = self.generation.clone().filter(|g| g.running)
        {
            if !old.candidate.resources.complete {
                return Err(Error::new(
                    "RESOURCE_REVALIDATION_REQUIRED",
                    "Automatic connection requires complete frozen dependencies",
                ));
            }
            let request = Request::new(
                Command::ApplyProfile {
                    candidate_id: old.candidate.id,
                    candidate_digest: old.candidate.digest,
                    proxy_mode: old.proxy_mode,
                    http_inbound_tag: old.http_inbound_tag,
                },
                Some(old.id),
            );
            let response = self.handle(request).await;
            response.result?;
        }
        Ok(())
    }
    pub async fn tick(&mut self) {
        if let Some(core) = self.core.as_mut() {
            match core.process.try_wait() {
                Ok(Some(code)) => {
                    let core = self.core.take().unwrap();
                    let instance = core.instance_id;
                    xray_runtime::log(&self.logs, format!("Core exited: {code}"));
                    if let Err(e) = self.proxy.release() {
                        self.blocked = true;
                        self.last_error = Some(e);
                    }
                    self.failed_instance = Some(instance.clone());
                    if !self.restart_budget.available(Instant::now()) {
                        self.last_error = Some(Self::budget_exhausted());
                    } else if !self.blocked
                        && self
                            .generation
                            .as_ref()
                            .is_some_and(|g| g.running && g.candidate.resources.complete)
                    {
                        self.retry = Some(Retry {
                            generation: core.generation_id,
                            instance,
                            epoch: self.recovery_epoch,
                            due: Instant::now() + self.restart_budget.delay(),
                        });
                    }
                }
                Err(e) => {
                    self.last_error = Some(e);
                }
                _ => {}
            }
        }
        if self.retry.as_ref().is_some_and(|r| r.due <= Instant::now()) {
            let mut ticket = self.retry.take().unwrap();
            if self.blocked
                || ticket.epoch != self.recovery_epoch
                || self.failed_instance.as_ref() != Some(&ticket.instance)
                || self
                    .generation
                    .as_ref()
                    .is_none_or(|g| g.id != ticket.generation || !g.running)
            {
                return;
            }
            // Charge once per automatic recovery attempt, including validation/startup failure.
            if !self.restart_budget.charge(Instant::now()) {
                self.last_error = Some(Self::budget_exhausted());
                return;
            }
            let old = self.generation.clone().unwrap();
            let request = Request::new(
                Command::ApplyProfile {
                    candidate_id: old.candidate.id,
                    candidate_digest: old.candidate.digest,
                    proxy_mode: old.proxy_mode,
                    http_inbound_tag: old.http_inbound_tag,
                },
                Some(old.id),
            );
            let response = self.handle(request).await;
            if let Err(e) = response.result {
                self.last_error = Some(e);
                if !self.blocked
                    && self.core.is_none()
                    && self.restart_budget.available(Instant::now())
                {
                    ticket.epoch = self.recovery_epoch;
                    self.failed_instance = Some(ticket.instance.clone());
                    ticket.due = Instant::now() + self.restart_budget.delay();
                    self.retry = Some(ticket);
                } else if !self.restart_budget.available(Instant::now()) {
                    xray_runtime::log(
                        &self.logs,
                        "Automatic restart failed; retry budget exhausted",
                    );
                    self.last_error = Some(Self::budget_exhausted());
                }
            }
        }
    }
    fn budget_exhausted() -> Error {
        Error::new(
            "CORE_CRASH_LOOP",
            "Restart budget exhausted (3 attempts per minute); apply or start manually",
        )
    }
}

pub fn default_root() -> Result<PathBuf> {
    #[cfg(windows)]
    let root = std::env::current_exe()?
        .parent()
        .ok_or_else(|| Error::new("INVALID_DATA_DIR", "Executable directory is missing"))?
        .join("data");
    #[cfg(not(windows))]
    let root = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        })
        .join("xray-pilot");
    Ok(root)
}

#[cfg(all(test, unix))]
mod recovery_tests;

#[cfg(test)]
mod operation_tests;

#[cfg(test)]
mod lease_recovery_tests;
