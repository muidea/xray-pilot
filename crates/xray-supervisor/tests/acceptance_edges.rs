//! Isolated acceptance checks using the explicitly selected official Core.
//! No real system proxy, login task, DNS lookup, or remote upstream is used.
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use windows_integration::ProxyBackend;
use xray_model::*;
use xray_supervisor::Supervisor;

#[derive(Clone, Default)]
struct FakeProxy(Arc<Mutex<ProxySnapshot>>);
impl ProxyBackend for FakeProxy {
    fn read(&self) -> Result<ProxySnapshot> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn write(&mut self, value: &ProxySnapshot) -> Result<()> {
        *self.0.lock().unwrap() = value.clone();
        Ok(())
    }
}
async fn send(s: &mut Supervisor<FakeProxy>, command: Command) -> serde_json::Value {
    let expected = s.generation.as_ref().map(|g| g.id.clone());
    s.handle(Request::new(command, expected))
        .await
        .result
        .unwrap()
}
async fn candidate(s: &mut Supervisor<FakeProxy>, json: String) -> Candidate {
    let core = PathBuf::from(std::env::var_os("XRAY_TEST_CORE").expect("XRAY_TEST_CORE"));
    let profile = if let Ok(p) = s.store.profile("p") {
        p
    } else {
        serde_json::from_value(
            send(
                s,
                Command::CreateProfile {
                    profile_id: "p".into(),
                    json: json.clone(),
                },
            )
            .await,
        )
        .unwrap()
    };
    let assets = s.store.root.clone();
    serde_json::from_value(
        send(
            s,
            Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: profile.revision,
                json,
                core_path: core,
                asset_root: assets,
            },
        )
        .await,
    )
    .unwrap()
}
fn apply(c: &Candidate, mode: ProxyMode) -> Command {
    Command::ApplyProfile {
        candidate_id: c.id.clone(),
        candidate_digest: c.digest.clone(),
        proxy_mode: mode,
        http_inbound_tag: "http-in".into(),
    }
}
fn unused_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
struct Upstream {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
    failing: Arc<AtomicBool>,
    worker: tokio::task::JoinHandle<()>,
}
impl Drop for Upstream {
    fn drop(&mut self) {
        self.worker.abort();
    }
}
impl Upstream {
    async fn open(marker: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let requests = Arc::new(Mutex::new(vec![]));
        let failing = Arc::new(AtomicBool::new(false));
        let received = requests.clone();
        let fail = failing.clone();
        let worker = tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let received = received.clone();
                let fail = fail.clone();
                tokio::spawn(async move {
                    if let Err(error) = reply_socks(stream, received, fail, marker).await {
                        eprintln!("SOCKS fixture: {error}");
                    }
                });
            }
        });
        Self {
            port,
            requests,
            failing,
            worker,
        }
    }
}
async fn reply_socks(
    mut stream: TcpStream,
    received: Arc<Mutex<Vec<String>>>,
    failing: Arc<AtomicBool>,
    marker: &str,
) -> std::io::Result<()> {
    let mut hello = [0; 2];
    stream.read_exact(&mut hello).await?;
    let mut methods = vec![0; hello[1] as usize];
    stream.read_exact(&mut methods).await?;
    stream.write_all(&[5, 0]).await?;
    let mut head = [0; 4];
    stream.read_exact(&mut head).await?;
    if head[3] != 3 {
        return Err(std::io::Error::other("Expected SOCKS domain address"));
    }
    let len = stream.read_u8().await? as usize;
    let mut host = vec![0; len];
    stream.read_exact(&mut host).await?;
    let _port = stream.read_u16().await?;
    received
        .lock()
        .unwrap()
        .push(String::from_utf8_lossy(&host).into_owned());
    if failing.load(Ordering::SeqCst) {
        stream.write_all(&[5, 5, 0, 1, 127, 0, 0, 1, 0, 0]).await?;
        return Ok(());
    }
    stream.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).await?;
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") && header.len() < 8192 {
        header.push(stream.read_u8().await?);
    }
    let body = format!("upstream-{marker}");
    stream
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await?;
    stream.shutdown().await
}
async fn tunnel(port: u16, host: &str) -> std::io::Result<String> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await?;
        stream
            .write_all(format!("CONNECT {host}:80 HTTP/1.1\r\nHost: {host}:80\r\n\r\n").as_bytes())
            .await?;
        let mut header = Vec::new();
        while !header.ends_with(b"\r\n\r\n") && header.len() < 8192 {
            header.push(stream.read_u8().await?)
        }
        if !header.starts_with(b"HTTP/1.1 200 ") {
            return Err(std::io::Error::other("HTTP CONNECT rejected"));
        }
        stream
            .write_all(
                format!("GET / HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes(),
            )
            .await?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await?;
        Ok(String::from_utf8_lossy(&response).into_owned())
    })
    .await
    .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "HTTP tunnel timeout"))?
}
#[tokio::test]
#[ignore = "requires XRAY_TEST_CORE; uses only controlled loopback SOCKS upstreams"]
async fn official_core_routes_domains_to_b_and_never_falls_back_to_a() {
    let a = Upstream::open("A").await;
    let mut b = Upstream::open("B").await;
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy::default())
        .await
        .unwrap();
    let port = unused_port();
    // Same first-outbound default and domain rule as examples/two-upstreams.json.
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../../../examples/two-upstreams.json")).unwrap();
    value["inbounds"] = serde_json::json!([{"tag":"http-in","protocol":"http","listen":"127.0.0.1","port":port,"settings":{}}]);
    value["outbounds"][0]["settings"]["servers"][0]["port"] = a.port.into();
    value["outbounds"][1]["settings"]["servers"][0]["port"] = b.port.into();
    let c = candidate(&mut s, value.to_string()).await;
    send(&mut s, apply(&c, ProxyMode::Direct)).await;
    assert!(
        tunnel(port, "example.com")
            .await
            .unwrap()
            .ends_with("upstream-B")
    );
    assert!(
        tunnel(port, "www.example.com")
            .await
            .unwrap()
            .ends_with("upstream-B")
    );
    assert!(
        tunnel(port, "other.invalid")
            .await
            .unwrap()
            .ends_with("upstream-A")
    );
    assert_eq!(a.requests.lock().unwrap().as_slice(), ["other.invalid"]);
    assert_eq!(
        b.requests.lock().unwrap().as_slice(),
        ["example.com", "www.example.com"]
    );
    b.failing.store(true, Ordering::SeqCst);
    let failed = tunnel(port, "example.com").await;
    assert!(
        !failed.as_ref().is_ok_and(|body| body.contains("upstream-")),
        "Failed B produced a successful upstream response: {failed:?}"
    );
    assert_eq!(
        a.requests.lock().unwrap().as_slice(),
        ["other.invalid"],
        "B failure must not connect to A"
    );
    assert!(b.requests.lock().unwrap().len() >= 3);
    if let Err(error) = failed {
        assert_ne!(error.kind(), std::io::ErrorKind::TimedOut);
    }
    b.worker.abort();
    let _ = (&mut b.worker).await;
    let down = tunnel(port, "example.com").await;
    assert!(!down.as_ref().is_ok_and(|body| body.contains("upstream-")));
    if let Err(error) = down {
        assert_ne!(error.kind(), std::io::ErrorKind::TimedOut);
    }
    assert_eq!(a.requests.lock().unwrap().as_slice(), ["other.invalid"]);
    send(&mut s, Command::StopCore).await;
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires XRAY_TEST_CORE; denies replacement of an isolated profile file"]
async fn locked_profile_keeps_old_asset_generation_core_and_proxy() {
    use std::os::windows::fs::OpenOptionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy::default();
    let mut s = Supervisor::with_backend(tmp.path(), proxy.clone())
        .await
        .unwrap();
    let p1 = unused_port();
    let p2 = unused_port();
    let config = |port| {
        serde_json::json!({"inbounds":[{"tag":"http-in","protocol":"http","listen":"127.0.0.1","port":port,"settings":{}}],"outbounds":[{"protocol":"freedom"}]}).to_string()
    };
    let before = config(p1);
    let c1 = candidate(&mut s, before.clone()).await;
    let old = send(&mut s, apply(&c1, ProxyMode::Global)).await;
    let old_proxy = proxy.read().unwrap();
    let c2 = candidate(&mut s, config(p2)).await;
    // FILE_SHARE_READ permits reads for verification but denies the atomic replacement/delete.
    let locked = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(s.store.profile_path("p").unwrap())
        .unwrap();
    let response = s
        .handle(Request::new(
            apply(&c2, ProxyMode::Global),
            s.generation.as_ref().map(|g| g.id.clone()),
        ))
        .await;
    assert!(response.result.is_err());
    let status = send(&mut s, Command::GetStatus).await;
    assert_eq!(status["core_state"], "Running");
    assert_eq!(status["generation"]["id"], old["id"]);
    assert_eq!(status["recovery_required"], false);
    assert_eq!(s.store.profile("p").unwrap().json, before);
    assert_eq!(proxy.read().unwrap(), old_proxy);
    xray_runtime::probe_http((std::net::Ipv4Addr::LOCALHOST, p1).into())
        .await
        .unwrap();
    drop(locked);
    send(&mut s, Command::StopCore).await;
}
