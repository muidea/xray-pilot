use super::*;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use windows_integration::ProxyBackend;
use xray_model::{Command, ProxySnapshot};

#[derive(Clone)]
struct Fake(Arc<Mutex<ProxySnapshot>>);
impl ProxyBackend for Fake {
    fn read(&self) -> Result<ProxySnapshot> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn write(&mut self, value: &ProxySnapshot) -> Result<()> {
        *self.0.lock().unwrap() = value.clone();
        Ok(())
    }
}
#[cfg(not(windows))]
type Stream = tokio::net::UnixStream;
#[cfg(windows)]
type Stream = tokio::net::windows::named_pipe::NamedPipeClient;
async fn open(root: &Path) -> Stream {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            #[cfg(not(windows))]
            let result = Stream::connect(endpoint(root)).await;
            #[cfg(windows)]
            let result = tokio::net::windows::named_pipe::ClientOptions::new().open(endpoint(root));
            if let Ok(stream) = result {
                return stream;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}
async fn ping(root: &Path) -> serde_json::Value {
    tokio::time::timeout(
        Duration::from_secs(2),
        call(root, &Request::new(Command::GetStatus, None)),
    )
    .await
    .unwrap()
    .unwrap()
    .result
    .unwrap()
}
async fn shutdown(root: &Path, task: tokio::task::JoinHandle<Result<()>>) {
    let status = ping(root).await;
    call(
        root,
        &Request::new(
            Command::Shutdown,
            status["generation"]["id"].as_str().map(str::to_owned),
        ),
    )
    .await
    .unwrap()
    .result
    .unwrap();
    tokio::time::timeout(Duration::from_secs(7), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn idle_truncated_slow_read_and_unacknowledged_clients_do_not_block_status() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Supervisor::with_backend(
        tmp.path(),
        Fake(Arc::new(Mutex::new(ProxySnapshot::default()))),
    )
    .await
    .unwrap();
    let root = s.store.root.clone();
    xray_runtime::log(&s.logs, "x".repeat(4 * 1024 * 1024));
    let task = tokio::spawn(serve_supervisor(s));
    let idle = open(&root).await;
    let mut truncated = open(&root).await;
    truncated.write_u32_le(100).await.unwrap();
    truncated.write_all(b"{").await.unwrap();
    let mut unacknowledged = open(&root).await;
    send(&mut unacknowledged, &Request::new(Command::GetStatus, None))
        .await
        .unwrap();
    let _: Response = receive(&mut unacknowledged).await.unwrap();
    let mut slow = open(&root).await;
    send(&mut slow, &Request::new(Command::GetLogs, None))
        .await
        .unwrap();
    for _ in 0..3 {
        assert_eq!(ping(&root).await["core_state"], "Stopped");
    }
    drop((idle, truncated, unacknowledged, slow));
    shutdown(&root, task).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connection_admission_is_bounded_and_recovers_after_clients_leave() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Supervisor::with_backend(
        tmp.path(),
        Fake(Arc::new(Mutex::new(ProxySnapshot::default()))),
    )
    .await
    .unwrap();
    let root = s.store.root.clone();
    let task = tokio::spawn(serve_supervisor(s));
    let mut idle = vec![];
    for _ in 0..MAX_CONNECTIONS {
        idle.push(open(&root).await);
    }
    tokio::time::sleep(Duration::from_millis(100)).await;
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        call(&root, &Request::new(Command::GetStatus, None)),
    )
    .await
    .unwrap();
    assert!(
        result.is_err(),
        "full admission must reject, not accumulate connection tasks"
    );
    drop(idle);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(ping(&root).await["core_state"], "Stopped");
    shutdown(&root, task).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn core_exit_releases_global_proxy_while_an_idle_client_is_connected() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let marker = tmp.path().join("exit-core");
    let core = tmp.path().join("fixture-core");
    let script = include_str!("../tests/fixtures/core.py").replace(
        "while True:",
        &format!(
            "while not __import__('os').path.exists({:?}):",
            marker.to_str().unwrap()
        ),
    );
    std::fs::write(&core, script).unwrap();
    std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o700)).unwrap();
    let original = ProxySnapshot {
        flags: 5,
        pac_url: "https://enterprise.invalid/pac".into(),
        ..Default::default()
    };
    let backend = Fake(Arc::new(Mutex::new(original.clone())));
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), backend.clone())
        .await
        .unwrap();
    let root = s.store.root.clone();
    let p = s
        .handle(Request::new(
            Command::CreateProfile {
                profile_id: "p".into(),
                json: "{}".into(),
            },
            None,
        ))
        .await
        .result
        .unwrap();
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reserved.local_addr().unwrap().port();
    drop(reserved);
    let config = serde_json::json!({"inbounds":[{"tag":"http-in","protocol":"http","listen":"127.0.0.1","port":port,"settings":{}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}).to_string();
    let c: xray_model::Candidate = serde_json::from_value(
        s.handle(Request::new(
            Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: p["revision"].as_str().unwrap().into(),
                json: config,
                core_path: core,
                asset_root: tmp.path().into(),

                allow_dynamic: false,
            },
            None,
        ))
        .await
        .result
        .unwrap(),
    )
    .unwrap();
    s.handle(Request::new(
        Command::ApplyProfile {
            candidate_id: c.id,
            candidate_digest: c.digest,
            proxy_mode: xray_model::ProxyMode::Global,
            http_inbound_tag: "http-in".into(),

            pac: None,
        },
        None,
    ))
    .await
    .result
    .unwrap();
    // Keep this test about exit detection and cleanup, independently of restart timing.
    for _ in 0..3 {
        assert!(s.restart_budget.charge(std::time::Instant::now()));
    }
    assert_ne!(backend.read().unwrap(), original);
    let task = tokio::spawn(serve_supervisor(s));
    let idle = open(&root).await;
    std::fs::write(marker, "exit").unwrap();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let status = ping(&root).await;
            if status["core_state"] == "Stopped" {
                assert!(status["runtime_instance_id"].is_null());
                assert_eq!(status["proxy_state"], "Released");
                assert_eq!(backend.read().unwrap(), original);
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .unwrap();
    drop(idle);
    shutdown(&root, task).await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serve_accepts_status_while_auto_connect_recovery_is_in_progress() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let core = tmp.path().join("fixture-core");
    let script = include_str!("../tests/fixtures/core.py").replace(
        "if \"-test\" in sys.argv:\n    sys.exit(0)",
        "if \"-test\" in sys.argv:\n    __import__('time').sleep(0.35)\n    sys.exit(0)",
    );
    std::fs::write(&core, script).unwrap();
    std::fs::set_permissions(&core, std::fs::Permissions::from_mode(0o700)).unwrap();
    let data = tmp.path().join("data");
    let backend = Fake(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = Supervisor::with_backend(&data, backend.clone())
        .await
        .unwrap();
    let p = s
        .handle(Request::new(
            Command::CreateProfile {
                profile_id: "p".into(),
                json: "{}".into(),
            },
            None,
        ))
        .await
        .result
        .unwrap();
    let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reserved.local_addr().unwrap().port();
    drop(reserved);
    let config = serde_json::json!({"inbounds":[{"tag":"http-in","protocol":"http","listen":"127.0.0.1","port":port,"settings":{}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}).to_string();
    let c: xray_model::Candidate = serde_json::from_value(
        s.handle(Request::new(
            Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: p["revision"].as_str().unwrap().into(),
                json: config,
                core_path: core,
                asset_root: tmp.path().into(),
                allow_dynamic: false,
            },
            None,
        ))
        .await
        .result
        .unwrap(),
    )
    .unwrap();
    s.handle(Request::new(
        Command::ApplyProfile {
            candidate_id: c.id,
            candidate_digest: c.digest,
            proxy_mode: xray_model::ProxyMode::Direct,
            http_inbound_tag: "http-in".into(),
            pac: None,
        },
        None,
    ))
    .await
    .result
    .unwrap();
    s.handle(Request::new(
        Command::SaveSettings {
            settings: xray_model::Settings {
                connect_on_start: true,
                ..Default::default()
            },
        },
        s.generation.as_ref().map(|g| g.id.clone()),
    ))
    .await
    .result
    .unwrap();
    drop(s);

    let (prepared, needs_recover) = Supervisor::prepare_with_backends(
        &data,
        backend,
        Box::new(windows_integration::SystemStartup),
    )
    .unwrap();
    assert!(needs_recover);
    let root = prepared.store.root.clone();
    let task = tokio::spawn(serve_prepared(prepared, needs_recover));

    // Give the spawned task a brief tick to bind the socket, well shorter than the 350ms -test delay.
    tokio::time::sleep(Duration::from_millis(50)).await;
    let status = ping(&root).await;
    assert_eq!(status["core_state"], "Running");
    shutdown(&root, task).await;
}
