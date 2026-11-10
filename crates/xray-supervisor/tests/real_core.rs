//! Run with XRAY_TEST_CORE=/absolute/path/to/xray cargo test -p xray-supervisor --test real_core -- --ignored.
use std::{
    path::PathBuf,
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
    fn write(&mut self, value: &ProxySnapshot) -> Result<()> {
        *self.0.lock().unwrap() = value.clone();
        Ok(())
    }
}
async fn send(s: &mut Supervisor<FakeProxy>, command: Command) -> serde_json::Value {
    let generation = s.generation.as_ref().map(|g| g.id.clone());
    s.handle(Request::new(command, generation))
        .await
        .result
        .unwrap()
}
#[tokio::test]
#[ignore = "requires an explicitly selected official Xray executable via XRAY_TEST_CORE"]
async fn official_core_validation_apply_dedup_release_and_stop() {
    let core = PathBuf::from(std::env::var_os("XRAY_TEST_CORE").expect("XRAY_TEST_CORE"));
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("data with spaces 测试");
    let original = ProxySnapshot {
        flags: 5,
        pac_url: "https://corp.invalid/proxy.pac".into(),
        ..Default::default()
    };
    let proxy = FakeProxy(Arc::new(Mutex::new(original.clone())));
    let mut s = Supervisor::with_backend(&root, proxy.clone())
        .await
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let json = serde_json::json!({"log":{"loglevel":"warning"},"inbounds":[{"tag":"http-in","protocol":"http","listen":"127.0.0.1","port":port,"settings":{}}],"outbounds":[{"tag":"direct","protocol":"freedom"},{"tag":"blocked","protocol":"blackhole"}],"routing":{"rules":[{"type":"field","domain":["full:blocked.invalid"],"outboundTag":"blocked"}]}}).to_string();
    let p: Profile = serde_json::from_value(
        send(
            &mut s,
            Command::CreateProfile {
                profile_id: "p".into(),
                json: json.clone(),
            },
        )
        .await,
    )
    .unwrap();
    let candidate: Candidate = serde_json::from_value(
        send(
            &mut s,
            Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: p.revision,
                json,
                core_path: core,
                asset_root: root.clone(),
            },
        )
        .await,
    )
    .unwrap();
    assert!(
        candidate.resources.complete,
        "{:?}",
        candidate.resources.notes
    );
    // Official Core accepts unknown metadata; resource enumeration must not invent dependencies.
    let metadata: Candidate = serde_json::from_value(send(&mut s, Command::ValidateDraft {
        profile_id: "p".into(), expected_revision: candidate.base_revision.clone(),
        json: r#"{"outbounds":[{"tag":"direct","protocol":"freedom"}],"metadata":{"note":"geosite:just-a-label"}}"#.into(),
        core_path: candidate.core.path.clone(), asset_root: root.clone(),
    }).await).unwrap();
    assert!(metadata.resources.files.is_empty());
    assert!(!metadata.resources.complete);
    // Official diagnostic output is available in session logs, not the persistent error.
    let rejected = Request::new(
        Command::ValidateDraft {
            profile_id: "p".into(),
            expected_revision: candidate.base_revision.clone(),
            json: r#"{"outbounds":[{"tag":"direct","protocol":"invalid-validation-protocol"}]}"#
                .into(),
            core_path: candidate.core.path.clone(),
            asset_root: root.clone(),
        },
        None,
    );
    let error = s.handle(rejected.clone()).await.result.unwrap_err();
    assert_eq!(error.code, "CORE_VALIDATION_FAILED");
    assert!(!error.message.contains("unknown config id"));
    let logs = send(&mut s, Command::GetLogs).await;
    assert!(
        logs.as_array()
            .unwrap()
            .iter()
            .any(|line| line.as_str().unwrap().contains("ERROR Core -test"))
    );
    // A legal Core configuration may still be ineligible for system proxy takeover.
    let mut wildcard: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&candidate.config_path).unwrap()).unwrap();
    wildcard["inbounds"][0]["listen"] = "0.0.0.0".into();
    let invalid: Candidate = serde_json::from_value(
        send(
            &mut s,
            Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: candidate.base_revision.clone(),
                json: wildcard.to_string(),
                core_path: candidate.core.path.clone(),
                asset_root: root.clone(),
            },
        )
        .await,
    )
    .unwrap();
    let failed = Request::new(
        Command::ApplyProfile {
            candidate_id: invalid.id,
            candidate_digest: invalid.digest,
            proxy_mode: ProxyMode::Global,
            http_inbound_tag: "http-in".into(),
        },
        None,
    );
    assert_eq!(
        s.handle(failed.clone()).await.result.unwrap_err().code,
        "PROXY_ENDPOINT_INVALID"
    );
    assert_eq!(
        send(&mut s, Command::GetStatus).await["last_error"]["code"],
        "PROXY_ENDPOINT_INVALID"
    );
    assert_eq!(proxy.read().unwrap(), original);
    let request = Request::new(
        Command::ApplyProfile {
            candidate_id: candidate.id,
            candidate_digest: candidate.digest,
            proxy_mode: ProxyMode::Global,
            http_inbound_tag: "http-in".into(),
        },
        None,
    );
    let response = s.handle(request.clone()).await.result.unwrap();
    assert!(send(&mut s, Command::GetStatus).await["last_error"].is_null());
    let failure_record = send(
        &mut s,
        Command::GetOperation {
            request_id: failed.request_id,
        },
    )
    .await;
    assert_eq!(
        failure_record["response"]["result"]["Err"]["code"],
        "PROXY_ENDPOINT_INVALID"
    );
    assert!(send(&mut s, Command::GetStatus).await["last_error"].is_null());
    assert_ne!(proxy.read().unwrap(), original);
    assert_eq!(s.handle(request).await.result.unwrap(), response);
    let context = RouteContext {
        host: "blocked.invalid".into(),
        ..Default::default()
    };
    let route = send(
        &mut s,
        Command::TestRoute {
            profile_id: "p".into(),
            context,
        },
    )
    .await;
    assert_eq!(route["expected_outbound"], "blocked");
    assert_eq!(route["actual_outbound"], "Unverified");
    send(
        &mut s,
        Command::SetProxyMode {
            mode: ProxyMode::Direct,
            http_inbound_tag: "http-in".into(),
        },
    )
    .await;
    assert_eq!(proxy.read().unwrap(), original);
    send(&mut s, Command::StopCore).await;
    assert_eq!(
        send(&mut s, Command::GetStatus).await["core_state"],
        "Stopped"
    );
    drop(s);
    let mut s = Supervisor::with_backend(&root, proxy).await.unwrap();
    assert_eq!(
        send(&mut s, Command::GetStatus).await["core_state"],
        "Stopped"
    );
}
