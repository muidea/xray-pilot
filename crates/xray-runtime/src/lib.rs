mod process;
pub use process::Process;

use std::{
    collections::VecDeque,
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    process::Command,
    time::timeout,
};
use xray_model::{Candidate, CoreIdentity, Error, Result, hash, id};

pub type Logs = Arc<Mutex<VecDeque<String>>>;
pub fn logs() -> Logs {
    Arc::new(Mutex::new(VecDeque::new()))
}
pub fn log(logs: &Logs, text: impl Into<String>) {
    if let Ok(mut lines) = logs.lock() {
        lines.push_back(text.into());
        while lines.len() > 1000 {
            lines.pop_front();
        }
    }
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
                for chunk in bytes[..bytes.len().min(16 * 1024)].chunks(1024) {
                    log(
                        logs,
                        format!(
                            "ERROR Core -test {source}: {}",
                            String::from_utf8_lossy(chunk)
                        ),
                    );
                }
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
    if inbound.get("protocol").and_then(|v| v.as_str()) != Some("http")
        || inbound
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
