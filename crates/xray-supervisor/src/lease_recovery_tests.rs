use super::*;
use std::sync::{Arc, Mutex};

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
async fn call(s: &mut Supervisor<Fake>, command: Command) -> Result<Value> {
    s.handle(Request::new(
        command,
        s.generation.as_ref().map(|g| g.id.clone()),
    ))
    .await
    .result
}

#[tokio::test]
async fn unavailable_proxy_evidence_blocks_writes_but_keeps_status_and_safety_stop() {
    for fault in [
        "missing",
        "truncated",
        "null",
        "modified",
        "missing-witness",
        "bad-witness",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let original = ProxySnapshot {
            flags: 5,
            pac_url: "https://enterprise.invalid/pac".into(),
            ..Default::default()
        };
        let backend = Fake(Arc::new(Mutex::new(original.clone())));
        let mut s = Supervisor::with_backend(tmp.path(), backend.clone())
            .await
            .unwrap();
        s.proxy
            .transition(global("127.0.0.1:10809".parse().unwrap()))
            .unwrap();
        s.proxy.finalize().unwrap();
        let owned = backend.read().unwrap();
        let primary = tmp.path().join("runtime/proxy-lease.json");
        let witness = tmp.path().join("runtime/proxy-lease-evidence.json");
        let saved_primary = fs::read(&primary).unwrap();
        let saved_witness = fs::read(&witness).unwrap();
        drop(s);
        match fault {
            "missing" => fs::remove_file(&primary).unwrap(),
            "truncated" => fs::write(&primary, "{").unwrap(),
            "null" => fs::write(&primary, "null").unwrap(),
            "modified" => {
                let mut lease: Lease = read_json(&primary).unwrap();
                lease.root.server = "untrusted-root:80".into();
                write_json(&primary, &lease).unwrap();
            }
            "missing-witness" => fs::remove_file(&witness).unwrap(),
            _ => fs::write(&witness, "{}").unwrap(),
        }
        let damaged_primary = fs::read(&primary).ok();
        let damaged_witness = fs::read(&witness).ok();
        let mut s = Supervisor::with_backend(tmp.path(), backend.clone())
            .await
            .unwrap();
        let status = call(&mut s, Command::GetStatus).await.unwrap();
        assert_eq!(status["recovery_required"], true, "{fault}");
        assert_eq!(status["proxy_state"], "RecoveryRequired", "{fault}");
        assert_eq!(status["last_error"]["code"], "PROXY_RECOVERY_REQUIRED");
        assert_eq!(backend.read().unwrap(), owned);
        assert_eq!(
            call(
                &mut s,
                Command::CreateProfile {
                    profile_id: "blocked".into(),
                    json: "{}".into()
                }
            )
            .await
            .unwrap_err()
            .code,
            "RECOVERY_REQUIRED"
        );
        call(&mut s, Command::StopCore).await.unwrap();
        call(&mut s, Command::Shutdown).await.unwrap();
        assert!(s.shutdown);
        assert_eq!(fs::read(&primary).ok(), damaged_primary);
        assert_eq!(fs::read(&witness).ok(), damaged_witness);
        drop(s);
        // Restoring original, mutually consistent evidence permits conditional cleanup.
        fs::write(&primary, saved_primary).unwrap();
        fs::write(&witness, saved_witness).unwrap();
        let mut s = Supervisor::with_backend(tmp.path(), backend.clone())
            .await
            .unwrap();
        assert!(!s.blocked, "{fault}");
        assert_eq!(backend.read().unwrap(), original);
        assert_eq!(
            call(&mut s, Command::GetStatus).await.unwrap()["proxy_state"],
            "Released"
        );
    }
}

#[tokio::test]
async fn missing_legacy_global_lease_stays_blocked_after_safety_stop_and_restart() {
    for phase in ["committed", "pending", "before-write"] {
        let tmp = tempfile::tempdir().unwrap();
        let backend = Fake(Arc::new(Mutex::new(global(
            "127.0.0.1:10809".parse().unwrap(),
        ))));
        let mut s = Supervisor::with_backend(tmp.path(), backend.clone())
            .await
            .unwrap();
        let request = Request::new(Command::RestartCore, None);
        let mut op = s.operations.register(&s.caller(), request).unwrap();
        let c = Candidate {
            id: id(),
            digest: "legacy-digest".into(),
            profile_id: "p".into(),
            base_revision: hash(b"{}"),
            revision: hash(b"{}"),
            config_path: tmp.path().join("missing-config"),
            core: CoreIdentity {
                path: tmp.path().join("missing-core"),
                sha256: "legacy".into(),
                version: "Xray test".into(),
            },
            resources: Resources {
                id: id(),
                root: tmp.path().into(),
                files: vec![],
                complete: true,
                notes: vec![],

                dynamic: vec![],
            },
        };
        let g = Generation {
            id: id(),
            transaction_id: op.transaction_id.clone(),
            request_id: op.request.request_id.clone(),
            candidate: c,
            proxy_mode: ProxyMode::Global,
            http_inbound_tag: "http-in".into(),
            running: true,

            pac: None,
        };
        if phase == "committed" {
            s.publish(&g).unwrap();
            op.response = Some(Response {
                request_id: op.request.request_id.clone(),
                result: Ok(serde_json::to_value(&g).unwrap()),
            });
            s.operations.save(&op).unwrap();
        } else {
            let before = ProxySnapshot::default();
            if phase == "before-write" {
                *backend.0.lock().unwrap() = before.clone();
            }
            let journal = Journal {
                generation: g,
                previous: None,
                previous_instance: None,
                before_json: "{}".into(),
                before_hash: hash(b"{}"),
                windows_before: before,
                lease_before: None,
                promoted: false,
                settled: false,
            };
            write_json(&s.journal(&op.transaction_id), &journal).unwrap();
        }
        drop(s);
        let mut s = Supervisor::with_backend(tmp.path(), backend.clone())
            .await
            .unwrap();
        if phase == "before-write" {
            assert!(!s.blocked);
            assert_eq!(backend.read().unwrap(), ProxySnapshot::default());
            continue;
        }
        assert!(s.blocked);
        call(&mut s, Command::StopCore).await.unwrap();
        assert!(s.generation.as_ref().is_none_or(|g| !g.running));
        drop(s);
        let s = Supervisor::with_backend(tmp.path(), backend.clone())
            .await
            .unwrap();
        assert!(s.blocked);
        assert_eq!(s.proxy.state(), "RecoveryRequired");
        assert_eq!(
            backend.read().unwrap(),
            global("127.0.0.1:10809".parse().unwrap())
        );
    }
}

#[tokio::test]
#[ignore = "requires XRAY_TEST_CORE; uses a fake proxy and isolated official Core"]
async fn official_global_lease_loss_keeps_diagnostics_and_can_be_repaired() {
    let core = PathBuf::from(std::env::var_os("XRAY_TEST_CORE").expect("XRAY_TEST_CORE"));
    for truncate in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let original = ProxySnapshot {
            flags: 5,
            pac_url: "https://enterprise.invalid/original.pac".into(),
            ..Default::default()
        };
        let backend = Fake(Arc::new(Mutex::new(original.clone())));
        let mut s = Supervisor::with_backend(tmp.path(), backend.clone())
            .await
            .unwrap();
        let p = call(
            &mut s,
            Command::CreateProfile {
                profile_id: "p".into(),
                json: "{}".into(),
            },
        )
        .await
        .unwrap();
        let reserved = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reserved.local_addr().unwrap().port();
        drop(reserved);
        let json = json!({"inbounds":[{"tag":"http-in","protocol":"http","listen":"127.0.0.1","port":port,"settings":{}}],"outbounds":[{"protocol":"freedom","tag":"direct"}]}).to_string();
        let c: Candidate = serde_json::from_value(
            call(
                &mut s,
                Command::ValidateDraft {
                    profile_id: "p".into(),
                    expected_revision: p["revision"].as_str().unwrap().into(),
                    json,
                    core_path: core.clone(),
                    asset_root: tmp.path().into(),

                    allow_dynamic: false,
                },
            )
            .await
            .unwrap(),
        )
        .unwrap();
        call(
            &mut s,
            Command::ApplyProfile {
                candidate_id: c.id,
                candidate_digest: c.digest,
                proxy_mode: ProxyMode::Global,
                http_inbound_tag: "http-in".into(),

                pac: None,
            },
        )
        .await
        .unwrap();
        assert!(s.core.is_some());
        let owned = backend.read().unwrap();
        let path = tmp.path().join("runtime/proxy-lease.json");
        let bytes = fs::read(&path).unwrap();
        drop(s);
        if truncate {
            fs::write(&path, "{").unwrap();
        } else {
            fs::remove_file(&path).unwrap();
        }
        let mut s = Supervisor::with_backend(tmp.path(), backend.clone())
            .await
            .unwrap();
        let status = call(&mut s, Command::GetStatus).await.unwrap();
        assert_eq!(status["core_state"], "Stopped");
        assert_eq!(status["proxy_state"], "RecoveryRequired");
        assert_eq!(status["recovery_required"], true);
        assert_eq!(backend.read().unwrap(), owned);
        assert_eq!(
            call(&mut s, Command::RestartCore).await.unwrap_err().code,
            "RECOVERY_REQUIRED"
        );
        call(&mut s, Command::StopCore).await.unwrap();
        call(&mut s, Command::Shutdown).await.unwrap();
        drop(s);
        fs::write(&path, bytes).unwrap();
        let mut s = Supervisor::with_backend(tmp.path(), backend.clone())
            .await
            .unwrap();
        assert!(!s.blocked);
        assert_eq!(backend.read().unwrap(), original);
        assert_eq!(
            call(&mut s, Command::GetStatus).await.unwrap()["proxy_state"],
            "Released"
        );
    }
}
