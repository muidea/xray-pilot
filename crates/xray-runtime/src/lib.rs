mod process;
mod readiness;
pub use process::Process;
pub use readiness::listener_plan;

use std::{
    borrow::Cow,
    collections::VecDeque,
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::Command,
    time::timeout,
};
use xray_model::{Candidate, CoreIdentity, Error, Result, hash, id};

const LOG_LINES: usize = 2000;
const LOG_MAX_BYTES: u64 = 4 * 1024 * 1024;
const LOG_BACKUP_MAX_AGE: Duration = Duration::from_secs(7 * 86400);

struct LogInner {
    lines: VecDeque<String>,
    file: Option<std::fs::File>,
    path: Option<PathBuf>,
    bytes: u64,
    context: Option<String>,
}
/// Bounded session log with optional rotating file persistence.
#[derive(Clone)]
pub struct Logs {
    inner: Arc<Mutex<LogInner>>,
}
pub fn logs() -> Logs {
    Logs {
        inner: Arc::new(Mutex::new(LogInner {
            lines: VecDeque::new(),
            file: None,
            path: None,
            bytes: 0,
            context: None,
        })),
    }
}
/// Open a session log file. One rotated backup is kept; the backup is removed
/// after seven days. Failure to open the file falls back to memory-only logging.
pub fn logs_with_file(path: impl AsRef<Path>) -> crate::Result<Logs> {
    let path = path.as_ref().to_path_buf();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let backup = path.with_extension("log.1");
    if backup
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| SystemTime::now().duration_since(modified).ok())
        .is_some_and(|age| age > LOG_BACKUP_MAX_AGE)
    {
        let _ = std::fs::remove_file(&backup);
    }
    if path.metadata().map(|m| m.len()).unwrap_or(0) > LOG_MAX_BYTES {
        let _ = std::fs::remove_file(&backup);
        let _ = std::fs::rename(&path, &backup);
    }
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let bytes = file.metadata()?.len();
    let mut lines = VecDeque::new();
    if let Ok(text) = std::fs::read_to_string(&path) {
        for line in text
            .lines()
            .rev()
            .take(LOG_LINES)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            lines.push_back(line.to_owned());
        }
    }
    Ok(Logs {
        inner: Arc::new(Mutex::new(LogInner {
            lines,
            file: Some(file),
            path: Some(path),
            bytes,
            context: None,
        })),
    })
}
impl Logs {
    pub fn set_context(&self, context: Option<String>) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.context = context;
        }
    }
    pub fn lines(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|inner| inner.lines.iter().cloned().collect())
            .unwrap_or_default()
    }
    fn rotate(inner: &mut LogInner) {
        inner.file = None;
        let Some(path) = inner.path.clone() else {
            return;
        };
        let backup = path.with_extension("log.1");
        let _ = std::fs::remove_file(&backup);
        let _ = std::fs::rename(&path, &backup);
        if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
            inner.file = Some(file);
            inner.bytes = 0;
        }
    }
}
const LOCAL_STAMP_FORMAT: &[time::format_description::FormatItem<'_>] = time::macros::format_description!(
    "[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3] [offset_hour sign:mandatory]:[offset_minute]"
);

static FALLBACK_LOCAL_OFFSET: OnceLock<time::UtcOffset> = OnceLock::new();

/// Captures the process's initial local UTC offset before worker threads start.
/// Needed on Unix targets where `time` refuses multi-threaded `localtime_r` lookups.
pub fn init_local_offset() {
    if let Ok(offset) = time::UtcOffset::current_local_offset() {
        let _ = FALLBACK_LOCAL_OFFSET.set(offset);
    }
}

pub fn format_timestamp(dt: time::OffsetDateTime) -> String {
    dt.format(LOCAL_STAMP_FORMAT)
        .unwrap_or_else(|_| "unknown-time".into())
}

fn local_offset_for(utc: time::OffsetDateTime) -> time::UtcOffset {
    time::UtcOffset::local_offset_at(utc)
        .ok()
        .or_else(|| FALLBACK_LOCAL_OFFSET.get().copied())
        .unwrap_or(time::UtcOffset::UTC)
}

pub fn local_timestamp() -> String {
    let utc = time::OffsetDateTime::now_utc();
    let dt = match time::OffsetDateTime::now_local() {
        Ok(local) => local,
        Err(_) => utc.to_offset(local_offset_for(utc)),
    };
    format_timestamp(dt)
}

/// Converts a leading `[<RFC3339>]` log timestamp prefix into the host's local timezone.
pub fn localize_log_line(line: &str) -> Cow<'_, str> {
    localize_log_line_with_offset(line, None)
}

pub fn localize_log_line_with_offset(line: &str, offset: Option<time::UtcOffset>) -> Cow<'_, str> {
    if let Some(rest) = line.strip_prefix('[')
        && let Some(end) = rest.find(']')
        && end <= 48
        && let Ok(parsed) = time::OffsetDateTime::parse(
            &rest[..end],
            &time::format_description::well_known::Rfc3339,
        )
    {
        let target = match offset {
            Some(off) => parsed.to_offset(off),
            None => parsed.to_offset(local_offset_for(parsed)),
        };
        return Cow::Owned(format!(
            "[{}]{}",
            format_timestamp(target),
            &rest[end + 1..]
        ));
    }
    Cow::Borrowed(line)
}

pub fn log(logs: &Logs, text: impl Into<String>) {
    let stamp = local_timestamp();
    let text = redact(&text.into());
    let Ok(mut inner) = logs.inner.lock() else {
        return;
    };
    let prefix = inner
        .context
        .as_ref()
        .map(|c| format!("{c} "))
        .unwrap_or_default();
    let line = format!("[{stamp}] {prefix}{text}");
    inner.lines.push_back(line.clone());
    while inner.lines.len() > LOG_LINES {
        inner.lines.pop_front();
    }
    if let Some(file) = &mut inner.file
        && writeln!(file, "{line}").is_ok()
    {
        inner.bytes = inner.bytes.saturating_add(line.len() as u64 + 1);
        if inner.bytes > LOG_MAX_BYTES {
            Logs::rotate(&mut inner);
        }
    }
}

/// Mask values associated with common credential keys and Authorization headers.
/// This is deliberately conservative; free-form text that does not look like a
/// key/value pair is preserved for diagnosis.
pub fn redact(text: &str) -> String {
    const KEYS: &[&str] = &[
        "password",
        "passwd",
        "pass",
        "privatekey",
        "publickey",
        "shortid",
        "uuid",
        "user",
        "id",
        "authorization",
    ];
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < text.len() {
        let Some(rest) = text[index..].strip_prefix('"') else {
            let ch = text[index..].chars().next().unwrap();
            out.push(ch);
            index += ch.len_utf8();
            continue;
        };
        let Some(end) = rest.find('"') else {
            out.push('"');
            index += 1;
            continue;
        };
        let key = &rest[..end];
        let after_key = index + 1 + end + 1;
        out.push('"');
        out.push_str(key);
        out.push('"');
        if KEYS.contains(&key.to_ascii_lowercase().as_str()) {
            let tail = &text[after_key..];
            let trimmed = tail.trim_start();
            if let Some(rest) = trimmed.strip_prefix(':') {
                let value = rest.trim_start();
                if let Some(after_quote) = value.strip_prefix('"')
                    && let Some(value_end) = after_quote.find('"')
                {
                    out.push_str(&tail[..tail.len() - trimmed.len()]);
                    out.push(':');
                    out.push_str(&rest[..rest.len() - value.len()]);
                    out.push_str("\"***\"");
                    index = after_key
                        + (tail.len() - trimmed.len())
                        + 1
                        + (rest.len() - value.len())
                        + 1
                        + value_end
                        + 1;
                    continue;
                }
            }
        }
        index = after_key;
    }

    // Bare or quoted key followed by ':'/'=' and a quoted value. This covers
    // forms such as `password = "..."` and JSON keys with escaped values.
    let lower = out.to_ascii_lowercase();
    let bytes = out.as_bytes();
    let mut bare = String::with_capacity(out.len());
    let mut index = 0;
    while index < bytes.len() {
        let word_boundary =
            index == 0 || !(bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == b'_');
        if word_boundary
            && let Some(key) = KEYS.iter().find(|key| lower[index..].starts_with(**key))
        {
            let mut cursor = index + key.len();
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && matches!(bytes[cursor], b':' | b'=') {
                cursor += 1;
                while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                    cursor += 1;
                }
                if cursor < bytes.len() && bytes[cursor] == b'"' {
                    let start = cursor + 1;
                    let mut end = start;
                    let mut escaped = false;
                    while end < bytes.len() {
                        if escaped {
                            escaped = false;
                        } else if bytes[end] == b'\\' {
                            escaped = true;
                        } else if bytes[end] == b'"' {
                            break;
                        }
                        end += 1;
                    }
                    if end < bytes.len() {
                        bare.push_str(&out[index..start]);
                        bare.push_str("***\"");
                        index = end + 1;
                        continue;
                    }
                }
            }
        }
        let ch = out[index..].chars().next().unwrap();
        bare.push(ch);
        index += ch.len_utf8();
    }
    let out = bare;

    // Unquoted key=value pairs (common in Core diagnostics).
    let mut masked = String::with_capacity(out.len());
    for chunk in out.split_inclusive(char::is_whitespace) {
        let trailing = chunk.len() - chunk.trim_end_matches(char::is_whitespace).len();
        let core = &chunk[..chunk.len() - trailing];
        if let Some((key, value)) = core.split_once('=')
            && !value.is_empty()
            && KEYS.contains(&key.trim_matches('"').to_ascii_lowercase().as_str())
        {
            masked.push_str(key);
            masked.push_str("=***");
            masked.push_str(&chunk[chunk.len() - trailing..]);
        } else {
            masked.push_str(chunk);
        }
    }

    // Basic Authorization headers.
    let mut result = String::with_capacity(masked.len());
    for line in masked.split_inclusive('\n') {
        if let Some(pos) = line.to_ascii_lowercase().find("authorization:") {
            let end = line[pos..].find('\r').unwrap_or(line.len() - pos);
            result.push_str(&line[..pos]);
            result.push_str("Authorization: ***");
            if let Some(tail) = line[pos + end..].strip_prefix('\r') {
                result.push('\r');
                result.push_str(tail);
            } else {
                result.push('\n');
            }
        } else {
            result.push_str(line);
        }
    }
    result
}
pub(crate) fn core_environment(root: &Path) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    // Do not let inherited Core-specific variables inject extra config directories or change geo inputs.
    let mut environment: Vec<_> = std::env::vars_os()
        .filter(|(key, _)| {
            let key = key.to_string_lossy().to_ascii_lowercase();
            !["xray.", "xray_", "v2ray.", "v2ray_"]
                .iter()
                .any(|prefix| key.starts_with(prefix))
        })
        .collect();
    environment.push(("XRAY_LOCATION_ASSET".into(), root.as_os_str().to_owned()));
    environment
}
fn command(path: &Path) -> Command {
    let mut cmd = Command::new(path);
    cmd.kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000);
    }
    cmd
}
async fn output(mut cmd: Command) -> Result<std::process::Output> {
    timeout(Duration::from_secs(15), cmd.output())
        .await
        .map_err(|_| Error::new("CORE_TIMEOUT", "Core command timed out"))?
        .map_err(Into::into)
}
pub async fn version(path: &Path) -> Result<String> {
    if !path.is_absolute() {
        return Err(Error::new(
            "INVALID_CORE_PATH",
            "Core path must be absolute",
        ));
    }
    let mut cmd = command(path);
    cmd.arg("version");
    let result = output(cmd).await?;
    if !result.status.success() {
        return Err(Error::new("INVALID_CORE", "Core version command failed"));
    }
    let line = String::from_utf8_lossy(&result.stdout)
        .lines()
        .next()
        .unwrap_or_default()
        .to_owned();
    if !line.to_ascii_lowercase().starts_with("xray ") {
        return Err(Error::new(
            "INVALID_CORE",
            "Selected binary does not identify as Xray",
        ));
    }
    Ok(line.chars().take(200).collect())
}
pub async fn validate(candidate: &Candidate) -> Result<()> {
    validate_inner(candidate, None).await
}
/// Diagnostics remain in bounded session logs; persistent operation errors stay generic.
pub async fn validate_with_logs(candidate: &Candidate, logs: &Logs) -> Result<()> {
    validate_inner(candidate, Some(logs)).await
}
async fn validate_inner(candidate: &Candidate, logs: Option<&Logs>) -> Result<()> {
    verify_core(&candidate.core)?;
    let mut cmd = command(&candidate.core.path);
    cmd.args(["run", "-test", "-config"])
        .arg(&candidate.config_path)
        .current_dir(&candidate.resources.root)
        .env_clear()
        .envs(core_environment(&candidate.resources.root));
    let result = output(cmd).await?;
    if !result.status.success() {
        // Core diagnostics may contain native configuration secrets: keep raw output in memory only.
        if let Some(logs) = logs {
            for (source, bytes) in [("stdout", &result.stdout), ("stderr", &result.stderr)] {
                // Redact the complete bounded block before it is stored. Chunking the
                // raw bytes first could split a quoted credential across two records.
                let bounded = &bytes[..bytes.len().min(16 * 1024)];
                log(
                    logs,
                    format!(
                        "ERROR Core -test {source}: {}",
                        String::from_utf8_lossy(bounded)
                    ),
                );
                if bytes.len() > 16 * 1024 {
                    log(logs, format!("ERROR Core -test {source}: [诊断已截断]"));
                }
            }
        }
        return Err(Error::new(
            "CORE_VALIDATION_FAILED",
            format!(
                "Core rejected candidate (exit {:?}); see session logs for diagnostics and check the JSON and target Core version",
                result.status.code()
            ),
        ));
    }
    Ok(())
}
pub fn verify_core(core: &CoreIdentity) -> Result<()> {
    if hash(&std::fs::read(&core.path)?) != core.sha256 {
        return Err(Error::new(
            "CORE_INTEGRITY_FAILED",
            "Frozen Core binary changed",
        ));
    }
    Ok(())
}

pub struct ManagedCore {
    pub process: Process,
    pub instance_id: String,
    pub generation_id: String,
}
impl ManagedCore {
    pub async fn start(candidate: &Candidate, generation: &str, logs: Logs) -> Result<Self> {
        verify_core(&candidate.core)?;
        let process = Process::spawn(candidate, logs)?;
        let mut managed = Self {
            process,
            instance_id: id(),
            generation_id: generation.into(),
        };
        tokio::time::sleep(Duration::from_millis(250)).await;
        if let Some(code) = managed.process.try_wait()? {
            return Err(Error::new(
                "CORE_START_FAILED",
                format!("Core exited during startup: {code}"),
            ));
        }
        Ok(managed)
    }
    /// Dropping this future cancels probing; the caller still owns the process and its transaction.
    pub async fn wait_http(&mut self, endpoint: std::net::SocketAddr) -> Result<()> {
        tokio::select! {
            result = probe_http(endpoint) => result,
            result = async {
                loop {
                    if let Some(code) = self.process.try_wait()? {
                        return Err(Error::new("CORE_START_FAILED", format!("Core exited before HTTP readiness: {code}")));
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            } => result,
        }
    }
    pub async fn wait_listeners(&mut self, config: &serde_json::Value, logs: &Logs) -> Result<()> {
        let (listeners, notes) = readiness::listener_plan(config)?;
        for note in notes {
            log(logs, note);
        }
        tokio::select! {
            result = readiness::wait_all(listeners) => result,
            result = async {
                loop {
                    if let Some(code) = self.process.try_wait()? { return Err(Error::new("CORE_START_FAILED",format!("Core exited before listener readiness: {code}"))); }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            } => result,
        }
    }
    pub async fn stop(&mut self) -> Result<()> {
        self.process.stop().await
    }
}

pub fn http_endpoint(config: &serde_json::Value, tag: &str) -> Result<std::net::SocketAddr> {
    let inbound = config
        .get("inbounds")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .find(|v| v.get("tag").and_then(|v| v.as_str()) == Some(tag))
        .ok_or_else(|| {
            Error::new(
                "PROXY_ENDPOINT_INVALID",
                "Select an existing HTTP inbound tag",
            )
        })?;
    if !matches!(
        inbound.get("protocol").and_then(|v| v.as_str()),
        Some("http" | "mixed")
    ) || inbound
        .pointer("/settings/accounts")
        .and_then(|v| v.as_array())
        .is_some_and(|v| !v.is_empty())
    {
        return Err(Error::new(
            "PROXY_ENDPOINT_INVALID",
            "System proxy requires an HTTP inbound without authentication",
        ));
    }
    let listen = inbound
        .get("listen")
        .and_then(|v| v.as_str())
        .unwrap_or("0.0.0.0");
    let ip: std::net::IpAddr = listen.parse().map_err(|_| {
        Error::new(
            "PROXY_ENDPOINT_INVALID",
            "HTTP listen must be a loopback IP address",
        )
    })?;
    if !ip.is_loopback() {
        return Err(Error::new(
            "PROXY_ENDPOINT_INVALID",
            "System proxy requires a loopback listener",
        ));
    }
    let port = inbound
        .get("port")
        .and_then(|v| v.as_u64())
        .filter(|p| *p > 0 && *p <= 65535)
        .ok_or_else(|| {
            Error::new(
                "PROXY_ENDPOINT_INVALID",
                "HTTP inbound requires one numeric port",
            )
        })? as u16;
    Ok(std::net::SocketAddr::new(ip, port))
}
pub async fn probe_http(endpoint: std::net::SocketAddr) -> Result<()> {
    timeout(Duration::from_secs(3), async {
        loop {
            match probe_once(endpoint).await {
                Ok(()) => return Ok(()),
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::ConnectionRefused
                            | std::io::ErrorKind::ConnectionReset
                            | std::io::ErrorKind::ConnectionAborted
                            | std::io::ErrorKind::NotConnected
                            | std::io::ErrorKind::UnexpectedEof
                            | std::io::ErrorKind::Interrupted
                            | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(error) => return Err(Error::new("LOCAL_PROXY_NOT_READY", error.to_string())),
            }
        }
    })
    .await
    .map_err(|_| {
        Error::new(
            "LOCAL_PROXY_NOT_READY",
            "HTTP proxy readiness probe timed out after 3 seconds",
        )
    })?
}
async fn probe_once(endpoint: std::net::SocketAddr) -> std::io::Result<()> {
    let mut stream = TcpStream::connect(endpoint).await?;
    // Xray acknowledges CONNECT before routing. Port zero cannot contact an external service.
    stream
        .write_all(b"CONNECT 127.0.0.1:0 HTTP/1.1\r\nHost: 127.0.0.1:0\r\n\r\n")
        .await?;
    let mut bytes = [0; 13];
    stream.read_exact(&mut bytes).await?;
    if &bytes != b"HTTP/1.1 200 " && &bytes != b"HTTP/1.0 200 " {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Listener did not respond as an HTTP proxy",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod readiness_tests {
    use super::*;
    #[tokio::test]
    async fn refused_connections_are_retried_until_listener_is_ready() {
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let endpoint = reserved.local_addr().unwrap();
        drop(reserved);
        let server = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(700)).await;
            let listener = tokio::net::TcpListener::bind(endpoint).await.unwrap();
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.unwrap());
            }
            stream.write_all(b"HTTP/1.1 200 ").await.unwrap();
        });
        let started = std::time::Instant::now();
        probe_http(endpoint).await.unwrap();
        assert!(started.elapsed() >= Duration::from_millis(650));
        server.await.unwrap();
    }
    #[tokio::test]
    async fn cancelled_probe_closes_its_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = listener.local_addr().unwrap();
        let worker = tokio::spawn(probe_http(endpoint));
        let (mut peer, _) = listener.accept().await.unwrap();
        let mut byte = [0];
        peer.read_exact(&mut byte).await.unwrap();
        worker.abort();
        assert!(worker.await.unwrap_err().is_cancelled());
        let mut rest = Vec::new();
        timeout(Duration::from_secs(1), peer.read_to_end(&mut rest))
            .await
            .unwrap()
            .unwrap();
    }
}

#[cfg(test)]
mod log_tests {
    use super::*;
    #[test]
    fn credential_values_are_masked_but_diagnostics_remain() {
        let text =
            r#"{"password":"secret","id":"11111111-1111-1111-1111-111111111111","note":"keep-me"}"#;
        let masked = redact(text);
        assert!(!masked.contains("secret"));
        assert!(!masked.contains("11111111"));
        assert!(masked.contains("keep-me"));
        assert!(masked.contains("***"));
        assert!(redact("Authorization: Basic abc").contains("Authorization: ***"));
        assert!(!redact("password = \"spaced-secret\"").contains("spaced-secret"));
        assert!(!redact(r#"{"Authorization":"Basic json-secret"}"#).contains("json-secret"));
        assert!(!redact("password = \"escaped\\\"secret\"").contains("escaped"));
    }
    #[test]
    fn session_log_persists_timestamp_context_and_rotates_one_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("logs/supervisor.log");
        let logs = logs_with_file(&path).unwrap();
        logs.set_context(Some("revision=abc".into()));
        log(&logs, "hello password=secret");
        let lines = logs.lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("revision=abc"));
        assert!(lines[0].contains("***"));
        let disk = std::fs::read_to_string(&path).unwrap();
        assert!(disk.contains("revision=abc"));
        assert!(!disk.contains("secret"));
        assert!(disk.starts_with('['));
        let mut inner = logs.inner.lock().unwrap();
        Logs::rotate(&mut inner);
        drop(inner);
        assert!(path.exists());
        assert!(tmp.path().join("logs/supervisor.log.1").exists());
    }
    #[test]
    fn timestamps_and_legacy_utc_prefixes_format_with_timezone_offset() {
        let utc = time::OffsetDateTime::parse(
            "2026-11-14T04:04:50.5837401Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let plus_8 = time::UtcOffset::from_hms(8, 0, 0).unwrap();
        assert_eq!(
            format_timestamp(utc.to_offset(plus_8)),
            "2026-11-14 12:04:50.583 +08:00"
        );
        let minus_5 = time::UtcOffset::from_hms(-5, 0, 0).unwrap();
        assert_eq!(
            format_timestamp(utc.to_offset(minus_5)),
            "2026-11-13 23:04:50.583 -05:00"
        );

        let converted = localize_log_line_with_offset(
            "[2026-11-14T04:04:50.5837401Z] profile=p 2026/11/14 12:04:50 accepted",
            Some(plus_8),
        );
        assert_eq!(
            converted,
            "[2026-11-14 12:04:50.583 +08:00] profile=p 2026/11/14 12:04:50 accepted"
        );
        assert!(matches!(
            localize_log_line_with_offset("2026-11-13 [Info] core ready", Some(plus_8)),
            Cow::Borrowed("2026-11-13 [Info] core ready")
        ));
        assert!(matches!(
            localize_log_line_with_offset("[2026-11-14 12:04:50.583 +08:00] already", Some(plus_8)),
            Cow::Borrowed("[2026-11-14 12:04:50.583 +08:00] already")
        ));

        let logs = logs();
        log(&logs, "check stamp");
        let lines = logs.lines();
        assert!(!lines[0].contains("Z]"));
        assert!(lines[0].contains(':'));
    }
}
