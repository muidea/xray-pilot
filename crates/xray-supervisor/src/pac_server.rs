//! Loopback PAC HTTP service with immutable revision paths.
//!
//! Only `/pac/<revision>/proxy.pac` is served; there is no filesystem or
//! arbitrary path access. The listener runs on a small dedicated thread so
//! stopping it (proxy release) closes the port before the caller proceeds.
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream as AsyncTcpStream,
    time::timeout,
};
use xray_model::{Error, Result};
use xray_runtime::Logs;

const MAX_REQUEST: usize = 16 * 1024;
const MAX_CONNECTIONS: usize = 32;
const IO_TIMEOUT: Duration = Duration::from_secs(5);

pub struct PacServer {
    port: u16,
    revisions: Arc<Mutex<BTreeMap<String, Arc<Vec<u8>>>>>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl PacServer {
    pub fn start(port: u16, logs: Logs) -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).map_err(|e| {
            Error::new(
                "PAC_PORT_CONFLICT",
                format!("Cannot bind PAC service on 127.0.0.1:{port}: {e}"),
            )
        })?;
        listener.set_nonblocking(true)?;
        let revisions = Arc::new(Mutex::new(BTreeMap::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_revisions = revisions.clone();
        let thread_stop = stop.clone();
        let thread_logs = logs.clone();
        let thread = std::thread::Builder::new()
            .name(format!("pac-http-{port}"))
            .spawn(move || {
                let active = Arc::new(AtomicUsize::new(0));
                while !thread_stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if active.fetch_add(1, Ordering::Relaxed) >= MAX_CONNECTIONS {
                                active.fetch_sub(1, Ordering::Relaxed);
                                continue;
                            }
                            let revisions = thread_revisions.clone();
                            let logs = thread_logs.clone();
                            let active = active.clone();
                            std::thread::spawn(move || {
                                let _ = serve(stream, revisions, logs);
                                active.fetch_sub(1, Ordering::Relaxed);
                            });
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(error) => {
                            xray_runtime::log(&thread_logs, format!("PAC listener error: {error}"));
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
                // Dropping the listener before the join returns releases the port.
                drop(listener);
            })
            .map_err(|e| Error::new("PAC_SERVICE_UNAVAILABLE", e.to_string()))?;
        Ok(Self {
            port,
            revisions,
            stop,
            thread: Some(thread),
        })
    }
    pub fn port(&self) -> u16 {
        self.port
    }
    pub fn publish(&self, revision: &str, script: Vec<u8>) {
        if let Ok(mut revisions) = self.revisions.lock() {
            revisions.insert(revision.to_owned(), Arc::new(script));
        }
    }
    /// Fetch the published URL and verify status, headers and exact bytes.
    pub async fn probe(&self, revision: &str, expected: &[u8]) -> Result<()> {
        let mut stream = AsyncTcpStream::connect(("127.0.0.1", self.port))
            .await
            .map_err(|e| Error::new("PAC_SERVICE_UNAVAILABLE", e.to_string()))?;
        let request = format!(
            "GET /pac/{revision}/proxy.pac HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            self.port
        );
        stream.write_all(request.as_bytes()).await?;
        let mut response = Vec::new();
        timeout(IO_TIMEOUT, stream.read_to_end(&mut response))
            .await
            .map_err(|_| Error::new("PAC_SERVICE_UNAVAILABLE", "PAC probe timed out"))??;
        let split = response
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(|| Error::new("PAC_SERVICE_UNAVAILABLE", "Malformed PAC response"))?;
        let head = String::from_utf8_lossy(&response[..split]).to_ascii_lowercase();
        if !head.starts_with("http/1.1 200") {
            return Err(Error::new(
                "PAC_SERVICE_UNAVAILABLE",
                "PAC service did not return 200",
            ));
        }
        if !head.contains("content-type: application/x-ns-proxy-autoconfig")
            || !head.contains("cache-control: no-cache")
        {
            return Err(Error::new(
                "PAC_SERVICE_UNAVAILABLE",
                "PAC response headers are incomplete",
            ));
        }
        if &response[split + 4..] != expected {
            return Err(Error::new(
                "PAC_SERVICE_UNAVAILABLE",
                "PAC response body differs from the immutable revision",
            ));
        }
        Ok(())
    }
    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
    pub fn stop(mut self) {
        self.shutdown();
    }
}
impl Drop for PacServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn serve(
    mut stream: TcpStream,
    revisions: Arc<Mutex<BTreeMap<String, Arc<Vec<u8>>>>>,
    logs: Logs,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|w| w == b"\r\n\r\n") || request.len() > MAX_REQUEST {
            break;
        }
    }
    let text = String::from_utf8_lossy(&request);
    let first = text.lines().next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let (status, content_type, body, cache) = match (method, target) {
        ("GET" | "HEAD", path) if path.starts_with("/pac/") => {
            let revision = path
                .strip_prefix("/pac/")
                .and_then(|p| p.strip_suffix("/proxy.pac"))
                .filter(|r| !r.is_empty() && r.chars().all(|c| c.is_ascii_hexdigit()));
            match revision.and_then(|r| revisions.lock().ok().and_then(|m| m.get(r).cloned())) {
                Some(body) => (
                    "200 OK",
                    "application/x-ns-proxy-autoconfig",
                    body,
                    "no-cache",
                ),
                None => (
                    "404 Not Found",
                    "text/plain",
                    Arc::new(b"Not found".to_vec()),
                    "no-store",
                ),
            }
        }
        ("GET" | "HEAD", _) => (
            "404 Not Found",
            "text/plain",
            Arc::new(b"Not found".to_vec()),
            "no-store",
        ),
        _ => (
            "405 Method Not Allowed",
            "text/plain",
            Arc::new(b"Method not allowed".to_vec()),
            "no-store",
        ),
    };
    xray_runtime::log(&logs, format!("PAC {method} {target} -> {status}"));
    let header = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nCache-Control: {cache}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    if method != "HEAD" {
        stream.write_all(&body)?;
    }
    stream.flush()?;
    Ok(())
}
