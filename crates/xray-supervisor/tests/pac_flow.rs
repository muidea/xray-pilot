#![cfg(unix)]
use std::{
    fs,
    io::{Read, Write},
    net::TcpStream,
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
    time::Duration,
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
fn port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
async fn http_get(url: &str) -> String {
    let url = url.to_owned();
    tokio::task::spawn_blocking(move || http_get_blocking(&url))
        .await
        .unwrap()
}
fn http_get_blocking(url: &str) -> String {
    let rest = url.strip_prefix("http://").unwrap();
    let (authority, path) = rest.split_once('/').unwrap();
    let mut stream = TcpStream::connect(authority).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(
        stream,
        "GET /{path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}
async fn send(s: &mut Supervisor<FakeProxy>, command: Command) -> Result<serde_json::Value> {
    let generation = s.generation.as_ref().map(|g| g.id.clone());
    s.handle(Request::new(command, generation)).await.result
}
fn config(http_port: u16) -> String {
    format!(
        r#"{{"inbounds":[{{"tag":"http-in","listen":"127.0.0.1","port":{http_port},"protocol":"http"}}],"outbounds":[{{"tag":"proxy","protocol":"vless"}},{{"tag":"direct","protocol":"freedom"}},{{"tag":"blocked","protocol":"blackhole"}}],"routing":{{"rules":[{{"type":"field","domain":["full:direct.example"],"outboundTag":"direct"}}]}}}}"#
    )
}
async fn setup(tmp: &tempfile::TempDir, proxy: FakeProxy) -> Supervisor<FakeProxy> {
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy)
        .await
        .unwrap();
    send(
        &mut s,
        Command::CreateProfile {
            profile_id: "p".into(),
            json: config(port()),
        },
    )
    .await
    .unwrap();
    s
}
async fn candidate(s: &mut Supervisor<FakeProxy>, core: &std::path::Path, json: &str) -> Candidate {
    let profile = s.store.profile("p").unwrap();
    serde_json::from_value(
        send(
            s,
            Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: profile.revision,
                json: json.into(),
                core_path: core.into(),
                asset_root: s.store.root.clone(),

                allow_dynamic: false,
            },
        )
        .await
        .unwrap(),
    )
    .unwrap()
}

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
async fn serial() -> tokio::sync::MutexGuard<'static, ()> {
    SERIAL.lock().await
}

#[tokio::test]
async fn pac_apply_publishes_immutable_script_sets_proxy_and_releases_cleanly() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot {
        flags: 3,
        server: "corp:8080".into(),
        bypass: "<local>".into(),
        pac_url: String::new(),
    })));
    let mut s = setup(&tmp, proxy.clone()).await;
    let core = fixture(tmp.path());
    let json = config(port());
    let c = candidate(&mut s, &core, &json).await;
    let pac_port = port();
    let script = r#"function FindProxyForURL(url, host) {
        if (host === "direct.example") return "DIRECT";
        return "PROXY 127.0.0.1:10809";
    }"#;
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id.clone(),
            candidate_digest: c.digest.clone(),
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: script.into(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let generation = s.generation.as_ref().unwrap();
    let pac = generation.pac.as_ref().unwrap();
    assert_eq!(pac.port, pac_port);
    assert_eq!(pac.revision.len(), 64);
    let snapshot = proxy.read().unwrap();
    assert_eq!(snapshot.flags, 1 | 4);
    assert!(snapshot.server.is_empty());
    assert_eq!(snapshot.pac_url, pac.url);
    let response = http_get(&pac.url).await;
    assert!(response.starts_with("HTTP/1.1 200"));
    assert!(response.contains("Content-Type: application/x-ns-proxy-autoconfig"));
    assert!(response.contains("Cache-Control: no-cache"));
    assert!(response.ends_with(script));

    send(&mut s, Command::StopCore).await.unwrap();
    assert_eq!(proxy.read().unwrap().server, "corp:8080");
    assert!(
        TcpStream::connect(("127.0.0.1", pac_port)).is_err(),
        "PAC listener must stop after conditional release"
    );
}

#[tokio::test]
async fn empty_pac_script_auto_generates_from_native_domain_rules() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy).await;
    let core = fixture(tmp.path());
    let json = config(port());
    let c = candidate(&mut s, &core, &json).await;
    let pac_port = port();
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id,
            candidate_digest: c.digest,
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: String::new(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let url = s
        .generation
        .as_ref()
        .unwrap()
        .pac
        .as_ref()
        .unwrap()
        .url
        .clone();
    let body = http_get(&url).await;
    let script = body.split("\r\n\r\n").nth(1).unwrap_or_default();
    assert!(script.contains("host === \"direct.example\""));
    assert!(script.contains("return \"PROXY 127.0.0.1:"));
    send(&mut s, Command::StopCore).await.unwrap();
}

#[tokio::test]
async fn invalid_pac_never_changes_the_proxy_and_port_conflicts_fail_closed() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy.clone()).await;
    let core = fixture(tmp.path());
    let json = config(port());
    let c = candidate(&mut s, &core, &json).await;
    let error = send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id.clone(),
            candidate_digest: c.digest.clone(),
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: "not a pac".into(),
                port: port(),
            }),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "PAC_SCRIPT_INVALID");
    assert_eq!(proxy.read().unwrap(), ProxySnapshot::default());

    let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let occupied_port = occupied.local_addr().unwrap().port();
    let error = send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id,
            candidate_digest: c.digest,
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: "function FindProxyForURL(){return 'DIRECT';}".into(),
                port: occupied_port,
            }),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "PAC_PORT_CONFLICT");
    assert_eq!(proxy.read().unwrap(), ProxySnapshot::default());
}

#[tokio::test]
async fn pac_startup_without_auto_connect_releases_even_if_the_service_is_rebuilt() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy.clone()).await;
    let core = fixture(tmp.path());
    let json = config(port());
    let c = candidate(&mut s, &core, &json).await;
    let pac_port = port();
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id,
            candidate_digest: c.digest,
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: "function FindProxyForURL(){return 'DIRECT';}".into(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let owned = proxy.read().unwrap().clone();
    assert!(!owned.pac_url.is_empty());
    drop(s);
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy.clone())
        .await
        .unwrap();
    let status: Status =
        serde_json::from_value(send(&mut s, Command::GetStatus).await.unwrap()).unwrap();
    assert!(!status.recovery_required, "{:?}", status.last_error);
    assert_eq!(proxy.read().unwrap(), ProxySnapshot::default());
    assert!(TcpStream::connect(("127.0.0.1", pac_port)).is_err());
    assert!(s.generation.as_ref().unwrap().pac.is_some());
}

#[tokio::test]
async fn pac_port_change_keeps_old_listener_through_commit_then_moves_service() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy.clone()).await;
    let core = fixture(tmp.path());
    let json = config(port());
    let c = candidate(&mut s, &core, &json).await;
    let first_port = port();
    let first_script = "function FindProxyForURL(){ return 'PROXY 127.0.0.1:10809'; }";
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id.clone(),
            candidate_digest: c.digest.clone(),
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: first_script.into(),
                port: first_port,
            }),
        },
    )
    .await
    .unwrap();
    let first_url = s
        .generation
        .as_ref()
        .unwrap()
        .pac
        .as_ref()
        .unwrap()
        .url
        .clone();
    let second_port = port();
    send(
        &mut s,
        Command::SetProxyMode {
            mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: "function FindProxyForURL(){ return 'PROXY 127.0.0.1:20809'; }".into(),
                port: second_port,
            }),
        },
    )
    .await
    .unwrap();
    let second_url = s
        .generation
        .as_ref()
        .unwrap()
        .pac
        .as_ref()
        .unwrap()
        .url
        .clone();
    assert_ne!(first_url, second_url);
    assert_eq!(proxy.read().unwrap().pac_url, second_url);
    assert!(TcpStream::connect(("127.0.0.1", first_port)).is_err());
    assert!(http_get(&second_url).await.starts_with("HTTP/1.1 200"));
    send(&mut s, Command::StopCore).await.unwrap();
}

#[tokio::test]
async fn same_port_keeps_previous_revision_available_until_release() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy).await;
    let core = fixture(tmp.path());
    let json = config(port());
    let c = candidate(&mut s, &core, &json).await;
    let pac_port = port();
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id.clone(),
            candidate_digest: c.digest.clone(),
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: "function FindProxyForURL(){ return 'DIRECT'; }".into(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let old_url = s
        .generation
        .as_ref()
        .unwrap()
        .pac
        .as_ref()
        .unwrap()
        .url
        .clone();
    send(
        &mut s,
        Command::SetProxyMode {
            mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: "function FindProxyForURL(){ return 'PROXY 127.0.0.1:10809'; }".into(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    assert!(http_get(&old_url).await.starts_with("HTTP/1.1 200"));
    send(&mut s, Command::StopCore).await.unwrap();
    assert!(TcpStream::connect(("127.0.0.1", pac_port)).is_err());
}

#[tokio::test]
async fn failed_pac_candidate_restores_previous_publication_and_proxy() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy.clone()).await;
    let core = fixture(tmp.path());
    let good_json = config(port());
    let c = candidate(&mut s, &core, &good_json).await;
    let pac_port = port();
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id.clone(),
            candidate_digest: c.digest.clone(),
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: "function FindProxyForURL(){ return 'DIRECT'; }".into(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let old_url = proxy.read().unwrap().pac_url;
    let failing = format!(
        r#"{{"inbounds":[{{"tag":"http-in","listen":"127.0.0.1","port":{},"protocol":"http"}}],"outbounds":[{{"protocol":"freedom"}}],"fixtureFailStart":true}}"#,
        port()
    );
    let failing = candidate(&mut s, &core, &failing).await;
    let new_port = port();
    let error = send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: failing.id,
            candidate_digest: failing.digest,
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: "function FindProxyForURL(){ return 'PROXY 127.0.0.1:10809'; }".into(),
                port: new_port,
            }),
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.code, "CORE_START_FAILED");
    assert_eq!(proxy.read().unwrap().pac_url, old_url);
    assert!(TcpStream::connect(("127.0.0.1", new_port)).is_err());
    assert!(http_get(&old_url).await.starts_with("HTTP/1.1 200"));
    send(&mut s, Command::StopCore).await.unwrap();
}

#[tokio::test]
async fn pac_auto_connect_reuses_committed_publication() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy.clone()).await;
    send(
        &mut s,
        Command::SaveSettings {
            settings: Settings {
                core_path: std::path::PathBuf::new(),
                connect_on_start: true,
                start_on_logon: false,
                restore_last_state: true,
            },
        },
    )
    .await
    .unwrap();
    let core = fixture(tmp.path());
    let json = config(port());
    let c = candidate(&mut s, &core, &json).await;
    let pac_port = port();
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id,
            candidate_digest: c.digest,
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: "function FindProxyForURL(){ return 'DIRECT'; }".into(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let url = proxy.read().unwrap().pac_url;
    drop(s);
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy.clone())
        .await
        .unwrap();
    assert_eq!(proxy.read().unwrap().pac_url, url);
    assert!(http_get(&url).await.starts_with("HTTP/1.1 200"));
    assert!(s.generation.as_ref().unwrap().running);
    send(&mut s, Command::StopCore).await.unwrap();
}

#[tokio::test]
async fn generated_pac_follows_http_inbound_port_changes() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy).await;
    let core = fixture(tmp.path());
    let first_http = port();
    let first_json = config(first_http);
    let first = candidate(&mut s, &core, &first_json).await;
    let pac_port = port();
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: first.id,
            candidate_digest: first.digest,
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: true,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: String::new(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let old_body = http_get(&s.generation.as_ref().unwrap().pac.as_ref().unwrap().url).await;
    assert!(old_body.contains(&format!("PROXY 127.0.0.1:{first_http}")));

    let second_http = port();
    let second = candidate(&mut s, &core, &config(second_http)).await;
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: second.id,
            candidate_digest: second.digest,
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: true,
                template: false,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: String::new(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let new_body = http_get(&s.generation.as_ref().unwrap().pac.as_ref().unwrap().url).await;
    assert!(new_body.contains(&format!("PROXY 127.0.0.1:{second_http}")));
    assert_ne!(old_body, new_body);
    send(&mut s, Command::StopCore).await.unwrap();
}

#[tokio::test]
async fn builtin_template_is_used_without_manual_script_editing() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy).await;
    let core = fixture(tmp.path());
    let http_port = port();
    let json = config(http_port);
    let c = candidate(&mut s, &core, &json).await;
    let pac_port = port();
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id,
            candidate_digest: c.digest,
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: true,
                template_mode: Default::default(),
                domains: Vec::new(),
                script: String::new(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let url = s
        .generation
        .as_ref()
        .unwrap()
        .pac
        .as_ref()
        .unwrap()
        .url
        .clone();
    let body = http_get(&url).await;
    let script = body.split("\r\n\r\n").nth(1).unwrap_or_default();
    assert!(script.contains("isPlainHostName(host)"));
    assert!(script.contains(&format!("PROXY 127.0.0.1:{http_port}")));
    assert_eq!(
        xray_pac::evaluate(script, "http://printer.lan/")
            .unwrap()
            .kind,
        "DIRECT"
    );
    assert_eq!(
        xray_pac::evaluate(script, "https://example.com/")
            .unwrap()
            .kind,
        "PROXY"
    );
    send(&mut s, Command::StopCore).await.unwrap();
}

#[tokio::test]
async fn builtin_template_domain_list_limits_which_hosts_use_the_proxy() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy).await;
    let core = fixture(tmp.path());
    let http_port = port();
    let json = config(http_port);
    let c = candidate(&mut s, &core, &json).await;
    let pac_port = port();
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id,
            candidate_digest: c.digest,
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: true,
                template_mode: xray_model::PacTemplateMode::ProxyList,
                domains: vec!["example.com".into()],
                script: String::new(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let url = s
        .generation
        .as_ref()
        .unwrap()
        .pac
        .as_ref()
        .unwrap()
        .url
        .clone();
    let body = http_get(&url).await;
    let script = body.split("\r\n\r\n").nth(1).unwrap_or_default();
    assert_eq!(
        xray_pac::evaluate(script, "https://example.com/")
            .unwrap()
            .kind,
        "PROXY"
    );
    assert_eq!(
        xray_pac::evaluate(script, "https://unrelated.test/")
            .unwrap()
            .kind,
        "DIRECT"
    );
    send(&mut s, Command::StopCore).await.unwrap();
}

#[tokio::test]
async fn empty_proxy_list_directs_all_regular_hosts() {
    let _guard = serial().await;
    let tmp = tempfile::tempdir().unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot::default())));
    let mut s = setup(&tmp, proxy).await;
    let core = fixture(tmp.path());
    let http_port = port();
    let json = config(http_port);
    let c = candidate(&mut s, &core, &json).await;
    let pac_port = port();
    send(
        &mut s,
        Command::ApplyProfile {
            candidate_id: c.id,
            candidate_digest: c.digest,
            proxy_mode: ProxyMode::Pac,
            http_inbound_tag: "http-in".into(),
            pac: Some(PacDraft {
                generated: false,
                template: true,
                template_mode: xray_model::PacTemplateMode::ProxyList,
                domains: Vec::new(),
                script: String::new(),
                port: pac_port,
            }),
        },
    )
    .await
    .unwrap();
    let url = s
        .generation
        .as_ref()
        .unwrap()
        .pac
        .as_ref()
        .unwrap()
        .url
        .clone();
    let body = http_get(&url).await;
    let script = body.split("\r\n\r\n").nth(1).unwrap_or_default();
    assert_eq!(
        xray_pac::evaluate(script, "https://example.com/")
            .unwrap()
            .kind,
        "DIRECT"
    );
    assert_eq!(
        xray_pac::evaluate(script, "https://anything.test/")
            .unwrap()
            .kind,
        "DIRECT"
    );
    send(&mut s, Command::StopCore).await.unwrap();
}
