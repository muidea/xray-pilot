#![cfg(unix)]
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
};
use windows_integration::ProxyBackend;
use xray_model::*;
use xray_supervisor::Supervisor;

#[derive(Clone)]
struct FakeProxy(Arc<Mutex<ProxySnapshot>>);
impl ProxyBackend for FakeProxy {
    fn read(&self) -> Result<ProxySnapshot> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn write(&mut self, s: &ProxySnapshot) -> Result<()> {
        *self.0.lock().unwrap() = s.clone();
        Ok(())
    }
}
fn fixture(root: &std::path::Path) -> std::path::PathBuf {
    let path = root.join("core with spaces");
    fs::write(&path, include_bytes!("fixtures/core.py")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
async fn send(s: &mut Supervisor<FakeProxy>, command: Command) -> Result<serde_json::Value> {
    let generation = s.generation.as_ref().map(|g| g.id.clone());
    s.handle(Request::new(command, generation)).await.result
}
async fn candidate(s: &mut Supervisor<FakeProxy>, core: &std::path::Path, json: &str) -> Candidate {
    let profile = s.store.profile("default").unwrap();
    let root = s.store.root.clone();
    serde_json::from_value(
        send(
            s,
            Command::ValidateDraft {
                profile_id: "default".into(),
                expected_revision: profile.revision,
                json: json.into(),
                core_path: core.into(),
                asset_root: root,

                allow_dynamic: false,
            },
        )
        .await
        .unwrap(),
    )
    .unwrap()
}
fn apply(c: &Candidate, mode: ProxyMode) -> Command {
    Command::ApplyProfile {
        candidate_id: c.id.clone(),
        candidate_digest: c.digest.clone(),
        proxy_mode: mode,
        http_inbound_tag: "http-in".into(),
        pac: None,
    }
}
fn port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[tokio::test]
async fn runtime_diagnostics_read_frozen_json_and_reject_stale_generations() {
    let tmp = tempfile::tempdir().unwrap();
    let core = fixture(tmp.path());
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy)
        .await
        .unwrap();
    let json = r#"{"outbounds":[{"tag":"running-direct","protocol":"freedom"}]}"#;
    send(
        &mut s,
        Command::CreateProfile {
            profile_id: "default".into(),
            json: json.into(),
        },
    )
    .await
    .unwrap();
    let c = candidate(&mut s, &core, json).await;
    send(&mut s, apply(&c, ProxyMode::Direct)).await.unwrap();
    let generation_id = s.generation.as_ref().unwrap().id.clone();
    // A mutable saved profile is no longer the authority for the running version.
    std::fs::write(
        s.store.profile_path("default").unwrap(),
        r#"{"outbounds":[{"tag":"saved-other","protocol":"freedom"}]}"#,
    )
    .unwrap();
    let config: RuntimeConfig = serde_json::from_value(
        send(
            &mut s,
            Command::GetRuntimeConfig {
                generation_id: generation_id.clone(),
            },
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(config.json, json);
    let context = RouteContext {
        host: "example.com".into(),
        ..Default::default()
    };
    let result: RouteResult = serde_json::from_value(
        send(
            &mut s,
            Command::TestRunningRoute {
                generation_id: generation_id.clone(),
                context: context.clone(),
            },
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert_eq!(result.expected_outbound.as_deref(), Some("running-direct"));
    assert_eq!(result.actual_outbound, "Unverified");
    let error = send(
        &mut s,
        Command::TestRunningRoute {
            generation_id: "stale".into(),
            context,
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "GENERATION_CONFLICT");
    std::fs::write(&c.config_path, b"{}").unwrap();
    assert_eq!(
        send(
            &mut s,
            Command::GetRuntimeConfig {
                generation_id: generation_id.clone()
            }
        )
        .await
        .unwrap_err()
        .code,
        "CONFIG_HASH_MISMATCH"
    );
    // Put immutable evidence back before using the ordinary stop transaction.
    std::fs::write(&c.config_path, json).unwrap();
    std::fs::write(s.store.profile_path("default").unwrap(), json).unwrap();
    send(&mut s, Command::StopCore).await.unwrap();
    assert_eq!(
        send(
            &mut s,
            Command::TestRunningRoute {
                generation_id,
                context: RouteContext::default()
            }
        )
        .await
        .unwrap_err()
        .code,
        "CORE_NOT_RUNNING"
    );
}

#[tokio::test]
async fn committed_request_survives_restart_and_stale_proxy_is_released() {
    let tmp = tempfile::tempdir().unwrap();
    let core = fixture(tmp.path());
    let original = ProxySnapshot {
        flags: 5,
        pac_url: "https://enterprise/pac".into(),
        ..Default::default()
    };
    let proxy = FakeProxy(Arc::new(Mutex::new(original.clone())));
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy.clone())
        .await
        .unwrap();
    send(
        &mut s,
        Command::CreateProfile {
            profile_id: "default".into(),
            json: "{}".into(),
        },
    )
    .await
    .unwrap();
    let json=serde_json::json!({"inbounds":[{"tag":"http-in","protocol":"http","listen":"127.0.0.1","port":port(),"settings":{}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}).to_string();
    let c = candidate(&mut s, &core, &json).await;
    let request = Request::new(apply(&c, ProxyMode::Global), None);
    let response = s.handle(request.clone()).await;
    response.result.clone().unwrap();
    assert_ne!(proxy.read().unwrap(), original);
    assert_eq!(s.store.profile("default").unwrap().json, json);
    let g = s.generation.as_ref().unwrap().id.clone();
    drop(s);
    let mut restarted = Supervisor::with_backend(&tmp.path().join("data"), proxy.clone())
        .await
        .unwrap();
    assert_eq!(proxy.read().unwrap(), original);
    let response = restarted.handle(request).await;
    assert_eq!(response.result.unwrap()["id"], g);
    let status = send(&mut restarted, Command::GetStatus).await.unwrap();
    assert_eq!(status["core_state"], "Stopped");
    assert_eq!(status["proxy_state"], "Released");
}

#[tokio::test]
async fn failed_candidate_rolls_back_process_formal_json_and_proxy() {
    let tmp = tempfile::tempdir().unwrap();
    let core = fixture(tmp.path());
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy.clone())
        .await
        .unwrap();
    send(
        &mut s,
        Command::CreateProfile {
            profile_id: "default".into(),
            json: "{}".into(),
        },
    )
    .await
    .unwrap();
    let json=serde_json::json!({"inbounds":[{"tag":"http-in","protocol":"http","listen":"127.0.0.1","port":port()}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}).to_string();
    let c = candidate(&mut s, &core, &json).await;
    send(&mut s, apply(&c, ProxyMode::Global)).await.unwrap();
    let old = s.generation.as_ref().unwrap().id.clone();
    let old_proxy = proxy.read().unwrap();
    let mut broken: serde_json::Value = serde_json::from_str(&json).unwrap();
    broken["fixtureFailStart"] = true.into();
    let broken = candidate(&mut s, &core, &broken.to_string()).await;
    assert_eq!(
        send(&mut s, apply(&broken, ProxyMode::Global))
            .await
            .unwrap_err()
            .code,
        "CORE_START_FAILED"
    );
    assert_eq!(s.generation.as_ref().unwrap().id, old);
    assert_eq!(s.store.profile("default").unwrap().json, json);
    assert_eq!(proxy.read().unwrap(), old_proxy);
    send(&mut s, Command::StopCore).await.unwrap();
}

#[tokio::test]
async fn foreign_profile_revision_is_not_overwritten() {
    let tmp = tempfile::tempdir().unwrap();
    let core = fixture(tmp.path());
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy)
        .await
        .unwrap();
    send(
        &mut s,
        Command::CreateProfile {
            profile_id: "default".into(),
            json: "{}".into(),
        },
    )
    .await
    .unwrap();
    let c = candidate(
        &mut s,
        &core,
        r#"{"outbounds":[{"tag":"direct","protocol":"freedom"}]}"#,
    )
    .await;
    fs::write(
        s.store.profile_path("default").unwrap(),
        "{\"external\":true}",
    )
    .unwrap();
    assert_eq!(
        send(&mut s, apply(&c, ProxyMode::Direct))
            .await
            .unwrap_err()
            .code,
        "ASSET_REVISION_CONFLICT"
    );
    assert!(s.generation.is_none());
}

#[tokio::test]
async fn connect_on_start_and_history_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("data");
    let core = fixture(tmp.path());
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = Supervisor::with_backend(&root, proxy.clone())
        .await
        .unwrap();
    send(
        &mut s,
        Command::CreateProfile {
            profile_id: "default".into(),
            json: "{}".into(),
        },
    )
    .await
    .unwrap();
    let first_json = r#"{"outbounds":[{"tag":"first","protocol":"freedom"}]}"#;
    let first = candidate(&mut s, &core, first_json).await;
    let g1: Generation = serde_json::from_value(
        send(&mut s, apply(&first, ProxyMode::Direct))
            .await
            .unwrap(),
    )
    .unwrap();
    send(
        &mut s,
        Command::SaveSettings {
            settings: Settings {
                connect_on_start: true,
                ..Settings::default()
            },
        },
    )
    .await
    .unwrap();
    drop(s);
    let mut s = Supervisor::with_backend(&root, proxy.clone())
        .await
        .unwrap();
    let status = send(&mut s, Command::GetStatus).await.unwrap();
    assert_eq!(status["core_state"], "Running");
    assert_ne!(status["generation"]["id"], g1.id);
    let second = candidate(
        &mut s,
        &core,
        r#"{"outbounds":[{"tag":"second","protocol":"freedom"}]}"#,
    )
    .await;
    send(&mut s, apply(&second, ProxyMode::Direct))
        .await
        .unwrap();
    let revision = s.store.profile("default").unwrap().revision;
    let staged = send(
        &mut s,
        Command::StageHistory {
            generation_id: g1.id,
            expected_revision: revision,
        },
    )
    .await
    .unwrap();
    assert_eq!(staged["json"], first_json);
    let old: Candidate = serde_json::from_value(staged["candidate"].clone()).unwrap();
    send(&mut s, apply(&old, ProxyMode::Direct)).await.unwrap();
    assert_eq!(s.store.profile("default").unwrap().json, first_json);
    send(&mut s, Command::StopCore).await.unwrap();
}
#[tokio::test]
async fn missing_frozen_resource_blocks_auto_connect() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("data");
    let core = fixture(tmp.path());
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = Supervisor::with_backend(&root, proxy.clone())
        .await
        .unwrap();
    send(
        &mut s,
        Command::CreateProfile {
            profile_id: "default".into(),
            json: "{}".into(),
        },
    )
    .await
    .unwrap();
    fs::write(root.join("geoip.dat"), "fixture data").unwrap();
    let c=candidate(&mut s,&core,r#"{"outbounds":[{"tag":"direct","protocol":"freedom"}],"routing":{"rules":[{"ip":["geoip:private"],"outboundTag":"direct"}]}}"#).await;
    send(&mut s, apply(&c, ProxyMode::Direct)).await.unwrap();
    send(
        &mut s,
        Command::SaveSettings {
            settings: Settings {
                connect_on_start: true,
                ..Settings::default()
            },
        },
    )
    .await
    .unwrap();
    drop(s);
    fs::remove_file(c.resources.root.join("geoip.dat")).unwrap();
    let mut s = Supervisor::with_backend(&root, proxy).await.unwrap();
    let status = send(&mut s, Command::GetStatus).await.unwrap();
    assert_eq!(status["core_state"], "Stopped");
    assert_eq!(status["recovery_required"], true);
    assert_eq!(status["last_error"]["code"], "RESOURCE_INTEGRITY_FAILED");
}

#[tokio::test]
async fn global_waits_for_slow_listener_and_restores_old_core_on_timeout_or_exit() {
    for (delay, exit_before_listen, expected_error) in [
        (0.7, false, None),
        (5.0, false, Some("LOCAL_PROXY_NOT_READY")),
        (0.7, true, Some("CORE_START_FAILED")),
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let core = fixture(tmp.path());
        let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
        let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy.clone())
            .await
            .unwrap();
        send(
            &mut s,
            Command::CreateProfile {
                profile_id: "default".into(),
                json: "{}".into(),
            },
        )
        .await
        .unwrap();
        let old_port = port();
        let config = |port| {
            serde_json::json!({"inbounds":[{"tag":"http-in","protocol":"http","listen":"127.0.0.1","port":port,"settings":{}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}).to_string()
        };
        let old_json = config(old_port);
        let good = candidate(&mut s, &core, &old_json).await;
        let previous = send(&mut s, apply(&good, ProxyMode::Global)).await.unwrap();
        let previous_proxy = proxy.read().unwrap();
        let source = fs::read_to_string(&core).unwrap().replace(
            "listeners = []",
            &format!(
                "import time\ntime.sleep({delay})\n{}\nlisteners = []",
                if exit_before_listen {
                    "sys.exit(25)"
                } else {
                    ""
                }
            ),
        );
        fs::write(&core, source).unwrap();
        let new_port = port();
        let slow = candidate(&mut s, &core, &config(new_port)).await;
        assert!(slow.resources.complete);
        let begin = std::time::Instant::now();
        let response = send(&mut s, apply(&slow, ProxyMode::Global)).await;
        if let Some(code) = expected_error {
            assert_eq!(response.unwrap_err().code, code);
            let status = send(&mut s, Command::GetStatus).await.unwrap();
            assert_eq!(status["core_state"], "Running");
            assert_eq!(status["generation"]["id"], previous["id"]);
            assert_eq!(status["recovery_required"], false);
            assert_eq!(proxy.read().unwrap(), previous_proxy);
            assert_eq!(s.store.profile("default").unwrap().json, old_json);
            xray_runtime::probe_http(([127, 0, 0, 1], old_port).into())
                .await
                .unwrap();
            if exit_before_listen {
                assert!(begin.elapsed() < std::time::Duration::from_secs(3));
            }
        } else {
            assert!(response.is_ok());
            assert!(begin.elapsed() >= std::time::Duration::from_millis(650));
            assert_ne!(proxy.read().unwrap(), previous_proxy);
            xray_runtime::probe_http(([127, 0, 0, 1], new_port).into())
                .await
                .unwrap();
        }
        send(&mut s, Command::StopCore).await.unwrap();
    }
}

#[tokio::test]
async fn dynamic_external_inputs_are_opt_in_and_revalidated_before_restart() {
    let tmp = tempfile::tempdir().unwrap();
    let core = fixture(tmp.path());
    let external = tmp.path().join("external-cert.pem");
    fs::write(&external, "certificate-v1").unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy)
        .await
        .unwrap();
    let json = serde_json::json!({
        "inbounds": [{"tag":"http-in","listen":"127.0.0.1","port":port(),"protocol":"http"}],
        "outbounds": [{"tag":"direct","protocol":"freedom","streamSettings":{"tlsSettings":{"certificates":[{"certificateFile": external.to_string_lossy()}]}}}]
    })
    .to_string();
    send(
        &mut s,
        Command::CreateProfile {
            profile_id: "default".into(),
            json: json.clone(),
        },
    )
    .await
    .unwrap();
    let profile = s.store.profile("default").unwrap();
    let root = s.store.root.clone();
    let c: Candidate = serde_json::from_value(
        send(
            &mut s,
            Command::ValidateDraft {
                profile_id: "default".into(),
                expected_revision: profile.revision,
                json,
                core_path: core,
                asset_root: root,
                allow_dynamic: true,
            },
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert!(!c.resources.complete);
    assert_eq!(c.resources.dynamic.len(), 1);
    send(&mut s, apply(&c, ProxyMode::Direct)).await.unwrap();
    assert!(s.generation.as_ref().unwrap().running);
    fs::write(&external, "certificate-v2").unwrap();
    let error = send(&mut s, Command::RestartCore).await.unwrap_err();
    assert_eq!(error.code, "RESOURCE_REVALIDATION_REQUIRED");
    assert!(s.generation.as_ref().unwrap().running);
    send(&mut s, Command::StopCore).await.unwrap();
}

#[tokio::test]
async fn tcp_latency_probe_measures_a_local_listener_and_rejects_invalid_input() {
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let _ = listener.accept().await;
    });
    let result = send(
        &mut s,
        Command::TestLatency {
            host: "127.0.0.1".into(),
            port: address.port(),
        },
    )
    .await
    .unwrap();
    assert_eq!(result["port"], address.port());
    assert!(result["ms"].is_u64());
    server.await.unwrap();
    assert_eq!(
        send(
            &mut s,
            Command::TestLatency {
                host: "  ".into(),
                port: 443
            }
        )
        .await
        .unwrap_err()
        .code,
        "LATENCY_HOST_INVALID"
    );
    assert_eq!(
        send(
            &mut s,
            Command::TestLatency {
                host: "127.0.0.1".into(),
                port: 0
            }
        )
        .await
        .unwrap_err()
        .code,
        "LATENCY_PORT_INVALID"
    );
}

#[tokio::test]
async fn incomplete_frozen_resources_attempt_auto_connect_without_blocking() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("data");
    let core = fixture(tmp.path());
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = Supervisor::with_backend(&root, proxy.clone())
        .await
        .unwrap();
    send(
        &mut s,
        Command::CreateProfile {
            profile_id: "default".into(),
            json: "{}".into(),
        },
    )
    .await
    .unwrap();
    let c = candidate(
        &mut s,
        &core,
        r#"{"unknown":{"extension":true},"outbounds":[{"tag":"direct","protocol":"freedom"}]}"#,
    )
    .await;
    assert!(!c.resources.complete);
    send(&mut s, apply(&c, ProxyMode::Direct)).await.unwrap();
    send(
        &mut s,
        Command::SaveSettings {
            settings: Settings {
                connect_on_start: true,
                ..Settings::default()
            },
        },
    )
    .await
    .unwrap();
    drop(s);

    let mut s = Supervisor::with_backend(&root, proxy).await.unwrap();
    let status = send(&mut s, Command::GetStatus).await.unwrap();
    assert_eq!(status["core_state"], "Running");
    assert_eq!(status["recovery_required"], false);
    assert!(
        status["auto_connect_notice"]
            .as_str()
            .is_some_and(|notice| notice.contains("未分类依赖"))
    );
    send(&mut s, Command::StopCore).await.unwrap();
}
