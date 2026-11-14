use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fmt, path::PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Error {
    pub code: String,
    pub message: String,
}
impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::new("IO_ERROR", e.to_string())
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::new("CONFIG_PARSE_FAILED", e.to_string())
    }
}

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn canonical_hash<T: Serialize>(value: &T) -> Result<String> {
    fn sorted(value: Value) -> Value {
        match value {
            Value::Object(map) => {
                let ordered: std::collections::BTreeMap<_, _> =
                    map.into_iter().map(|(k, v)| (k, sorted(v))).collect();
                Value::Object(ordered.into_iter().collect())
            }
            Value::Array(a) => Value::Array(a.into_iter().map(sorted).collect()),
            other => other,
        }
    }
    Ok(hash(&serde_json::to_vec(&sorted(serde_json::to_value(
        value,
    )?))?))
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProxyMode {
    #[default]
    Direct,
    Global,
    Pac,
}

pub const DEFAULT_PAC_PORT: u16 = 18080;
fn default_pac_port() -> u16 {
    DEFAULT_PAC_PORT
}

/// PAC compilation and traffic-splitting strategy.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum PacMode {
    /// Automatically compile from the active config's `/routing/rules` + `geosite.dat`.
    #[default]
    AutoRules,
    /// Selected geosite categories and custom domains go direct; everything else proxies.
    DirectList,
    /// Selected geosite categories and custom domains proxy; everything else goes direct.
    ProxyList,
    /// Proxy everything except local/LAN hosts.
    All,
    /// User-supplied custom JavaScript `FindProxyForURL`.
    Custom,
}

/// PAC editor draft. The script is frozen into an immutable publication only
/// when a PAC generation is applied; drafts never change the running setting.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PacDraft {
    #[serde(default)]
    pub mode: PacMode,
    #[serde(default)]
    pub geosites: Vec<String>,
    #[serde(default)]
    pub domains: Vec<String>,
    #[serde(default)]
    pub script: String,
    #[serde(default = "default_pac_port")]
    pub port: u16,
}

impl Default for PacDraft {
    fn default() -> Self {
        Self {
            mode: PacMode::AutoRules,
            geosites: Vec::new(),
            domains: Vec::new(),
            script: String::new(),
            port: DEFAULT_PAC_PORT,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PacPublication {
    /// Content hash, used as the immutable publication path segment.
    pub revision: String,
    pub sha256: String,
    pub url: String,
    pub port: u16,
    pub path: PathBuf,
}

/// Read-only WinHTTP proxy state. WinHTTP is a separate, machine-wide
/// configuration mechanism and is never modified by this application.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct WinHttpConfig {
    /// WinHTTP access type reported for a default session.
    pub access_type: String,
    pub proxy: String,
    pub bypass: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxySnapshot {
    pub flags: u32,
    pub server: String,
    pub bypass: String,
    pub pac_url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceFile {
    pub source: PathBuf,
    pub relative_path: PathBuf,
    pub sha256: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DynamicResource {
    pub source: PathBuf,
    pub sha256: String,
    #[serde(default)]
    pub observed_at: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Resources {
    pub id: String,
    pub root: PathBuf,
    pub files: Vec<ResourceFile>,
    pub complete: bool,
    pub notes: Vec<String>,
    #[serde(default)]
    pub dynamic: Vec<DynamicResource>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CoreIdentity {
    pub path: PathBuf,
    pub sha256: String,
    pub version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub digest: String,
    pub profile_id: String,
    pub base_revision: String,
    pub revision: String,
    pub config_path: PathBuf,
    pub core: CoreIdentity,
    pub resources: Resources,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Generation {
    pub id: String,
    pub transaction_id: String,
    pub request_id: String,
    pub candidate: Candidate,
    pub proxy_mode: ProxyMode,
    pub http_inbound_tag: String,
    pub running: bool,
    /// Immutable PAC publication associated with this generation.
    #[serde(default)]
    pub pac: Option<PacPublication>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub json: String,
    pub revision: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub id: String,
    pub revision: String,
}
/// Frozen JSON from a specific committed generation, independent of editor drafts.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeConfig {
    pub generation_id: String,
    pub json: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfilePage {
    pub items: Vec<ProfileSummary>,
    pub next_after: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub core_path: PathBuf,
    pub connect_on_start: bool,
    pub start_on_logon: bool,
    /// Restore a Core that was running before the previous component shutdown.
    #[serde(default = "default_true")]
    pub restore_last_state: bool,
}
fn default_true() -> bool {
    true
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            core_path: PathBuf::new(),
            connect_on_start: false,
            start_on_logon: false,
            restore_last_state: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "operation", content = "payload")]
pub enum Command {
    GetStatus,
    GetRuntimeConfig {
        generation_id: String,
    },
    ListProfiles,
    ListProfileSummaries {
        after: Option<String>,
        limit: usize,
    },
    GetProfile {
        profile_id: String,
    },
    ListHistory {
        profile_id: String,
    },
    StageHistory {
        generation_id: String,
        expected_revision: String,
    },
    CreateProfile {
        profile_id: String,
        json: String,
    },
    DeleteProfile {
        profile_id: String,
        expected_revision: String,
    },
    ValidateDraft {
        profile_id: String,
        expected_revision: String,
        json: String,
        core_path: PathBuf,
        asset_root: PathBuf,
        /// Explicitly allow absolute external files as Dynamic inputs. Such
        /// generations cannot auto-connect or auto-recover.
        #[serde(default)]
        allow_dynamic: bool,
    },
    ApplyProfile {
        candidate_id: String,
        candidate_digest: String,
        proxy_mode: ProxyMode,
        http_inbound_tag: String,
        /// Required when `proxy_mode` is Pac; ignored otherwise.
        #[serde(default)]
        pac: Option<PacDraft>,
    },
    StopCore,
    RestartCore,
    /// Explicitly acknowledges an external proxy lease conflict without overwriting Windows settings.
    ResolveProxyConflict,
    SetProxyMode {
        mode: ProxyMode,
        http_inbound_tag: String,
        /// Required when `mode` is Pac; ignored otherwise.
        #[serde(default)]
        pac: Option<PacDraft>,
    },
    /// Read-only PAC `FindProxyForURL` execution test.
    TestPac {
        script: String,
        url: String,
    },
    /// Read-only TCP latency probe to a user-specified host and port.
    TestLatency {
        host: String,
        port: u16,
    },
    SaveSettings {
        settings: Settings,
    },
    GetSettings,
    GetLogs,
    GetOperation {
        request_id: String,
    },
    GetOperationDetails {
        request_id: String,
        expected_hash: String,
        offset: usize,
        limit: usize,
    },
    TestRoute {
        profile_id: String,
        context: RouteContext,
    },
    TestRunningRoute {
        generation_id: String,
        context: RouteContext,
    },
    Shutdown,
}
impl Command {
    pub fn is_write(&self) -> bool {
        !matches!(
            self,
            Self::GetStatus
                | Self::GetRuntimeConfig { .. }
                | Self::ListProfiles
                | Self::ListProfileSummaries { .. }
                | Self::GetProfile { .. }
                | Self::ListHistory { .. }
                | Self::GetSettings
                | Self::GetLogs
                | Self::GetOperation { .. }
                | Self::GetOperationDetails { .. }
                | Self::TestRoute { .. }
                | Self::TestRunningRoute { .. }
                | Self::TestPac { .. }
                | Self::TestLatency { .. }
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Request {
    pub protocol_version: u32,
    pub request_id: String,
    pub expected_generation: Option<String>,
    pub command: Command,
}
impl Request {
    pub fn new(command: Command, expected_generation: Option<String>) -> Self {
        Self {
            protocol_version: 1,
            request_id: id(),
            expected_generation,
            command,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response {
    pub request_id: String,
    pub result: Result<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Status {
    pub generation: Option<Generation>,
    pub core_state: String,
    pub proxy_state: String,
    pub runtime_instance_id: Option<String>,
    pub supervisor_epoch: String,
    /// Latest error; fresh successful runtime work clears it once recovery is healthy.
    /// Status reads and historical operation replay do not acknowledge this error.
    pub last_error: Option<Error>,
    pub recovery_required: bool,
    /// Independent WinHTTP diagnostics; never written.
    #[serde(default)]
    pub winhttp: Option<WinHttpConfig>,
    /// Local HTTP/SOCKS readiness result for the current Core.
    #[serde(default)]
    pub listener_state: String,
    /// PAC service state for the current generation.
    #[serde(default)]
    pub pac_state: String,
    /// Non-fatal startup notice, e.g. automatic connection was skipped.
    #[serde(default)]
    pub auto_connect_notice: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RouteContext {
    pub host: String,
    pub port: Option<u16>,
    pub inbound_tag: Option<String>,
    pub network: Option<String>,
    pub target_ip: Option<String>,
    #[serde(default)]
    pub source_ip: Option<String>,
    #[serde(default)]
    pub source_port: Option<u16>,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub attrs: Option<std::collections::BTreeMap<String, String>>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteResult {
    pub kind: String,
    pub match_state: String,
    pub rule_index: Option<usize>,
    pub expected_outbound: Option<String>,
    pub actual_outbound: String,
    pub reason: String,
}
