use super::*;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

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
async fn send(s: &mut Supervisor<FakeProxy>, command: Command) -> Result<Value> {
    let generation = s.generation.as_ref().map(|g| g.id.clone());
    s.handle(Request::new(command, generation)).await.result
}
async fn setup() -> (
    tempfile::TempDir,
    Supervisor<FakeProxy>,
    FakeProxy,
    Candidate,
) {
    setup_script(include_bytes!("../tests/fixtures/core.py")).await
}
async fn setup_script(
    script: &[u8],
) -> (
    tempfile::TempDir,
    Supervisor<FakeProxy>,
    FakeProxy,
    Candidate,
) {
    use std::os::unix::fs::PermissionsExt;
    let tmp = tempfile::tempdir().unwrap();
    let core = tmp.path().join("fixture core");
    fs::write(&core, script).unwrap();
    fs::set_permissions(&core, fs::Permissions::from_mode(0o700)).unwrap();
    let proxy = FakeProxy(Arc::new(Mutex::new(ProxySnapshot {
        flags: 5,
        pac_url: "https://corp/pac".into(),
        ..Default::default()
    })));
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy.clone())
        .await
        .unwrap();
    let json = r#"{"outbounds":[{"tag":"direct","protocol":"freedom"}]}"#;
    let p: Profile = serde_json::from_value(
        send(
            &mut s,
            Command::CreateProfile {
                profile_id: "p".into(),
                json: json.into(),
            },
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let c: Candidate = serde_json::from_value(
        send(
            &mut s,
            Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: p.revision,
                json: json.into(),
                core_path: core,
                asset_root: tmp.path().into(),
            },
        )
        .await
        .unwrap(),
    )
    .unwrap();
    assert!(c.resources.complete);
    send(&mut s, apply(&c)).await.unwrap();
    (tmp, s, proxy, c)
}
fn apply(c: &Candidate) -> Command {
    Command::ApplyProfile {
        candidate_id: c.id.clone(),
        candidate_digest: c.digest.clone(),
        proxy_mode: ProxyMode::Direct,
        http_inbound_tag: "http-in".into(),
    }
}
async fn crash(s: &mut Supervisor<FakeProxy>) {
    s.core.as_mut().unwrap().process.stop().await.unwrap();
    s.tick().await;
}

#[tokio::test]
async fn stale_retry_cannot_modify_new_generation_or_lease() {
    let (_tmp, mut s, proxy, c) = setup().await;
    crash(&mut s).await;
    let mut stale = s.retry.take().unwrap();
    send(&mut s, apply(&c)).await.unwrap();
    let generation = s.generation.as_ref().unwrap().id.clone();
    let instance = s.core.as_ref().unwrap().instance_id.clone();
    let before = proxy.read().unwrap();
    stale.due = Instant::now();
    s.retry = Some(stale);
    s.tick().await;
    assert_eq!(s.generation.as_ref().unwrap().id, generation);
    assert_eq!(s.core.as_ref().unwrap().instance_id, instance);
    assert_eq!(proxy.read().unwrap(), before);
}

#[tokio::test]
async fn crash_restart_budget_is_bounded_and_stop_cancels_retry() {
    let (_tmp, mut s, _proxy, _c) = setup().await;
    for _ in 0..3 {
        crash(&mut s).await;
        s.retry.as_mut().unwrap().due = Instant::now();
        s.tick().await;
        assert!(s.core.is_some());
    }
    crash(&mut s).await;
    assert!(s.core.is_none());
    assert!(s.retry.is_none());
    assert_eq!(s.last_error.as_ref().unwrap().code, "CORE_CRASH_LOOP");
    send(&mut s, Command::RestartCore).await.unwrap();
    s.restart_budget.attempts.clear();
    crash(&mut s).await;
    assert!(s.retry.is_some());
    send(&mut s, Command::StopCore).await.unwrap();
    assert!(s.retry.is_none());
    assert!(!s.generation.as_ref().unwrap().running);
}

#[tokio::test]
async fn stop_works_with_corrupted_snapshots_and_external_proxy_conflict() {
    let (_tmp, mut s, proxy, c) = setup().await;
    fs::write(c.config_path, "broken").unwrap();
    s.proxy
        .transition(global("127.0.0.1:10809".parse().unwrap()))
        .unwrap();
    let external = ProxySnapshot {
        flags: 3,
        server: "external:80".into(),
        ..Default::default()
    };
    *proxy.0.lock().unwrap() = external.clone();
    s.blocked = true;
    send(&mut s, Command::StopCore).await.unwrap();
    assert!(s.core.is_none());
    assert!(!s.generation.as_ref().unwrap().running);
    assert_eq!(proxy.read().unwrap(), external);
    assert_eq!(s.last_error.as_ref().unwrap().code, "PROXY_LEASE_CONFLICT");
}

#[tokio::test]
async fn broken_lease_evidence_does_not_prevent_stopping_a_live_core() {
    let (tmp, mut s, proxy, _c) = setup().await;
    s.proxy
        .transition(global("127.0.0.1:10809".parse().unwrap()))
        .unwrap();
    let damaged = b"{";
    fs::write(tmp.path().join("data/runtime/proxy-lease.json"), damaged).unwrap();
    s.proxy = ProxyController::open(proxy.clone(), &s.store.root).unwrap();
    s.blocked = true;
    send(&mut s, Command::StopCore).await.unwrap();
    assert!(s.core.is_none());
    assert!(!s.generation.as_ref().unwrap().running);
    assert!(s.blocked);
    assert_eq!(s.proxy.state(), "RecoveryRequired");
    assert_eq!(
        fs::read(tmp.path().join("data/runtime/proxy-lease.json")).unwrap(),
        damaged
    );
}

#[tokio::test]
async fn committed_request_without_result_is_rebuilt_on_restart() {
    let (tmp, mut s, proxy, c) = setup().await;
    let request = Request::new(apply(&c), s.generation.as_ref().map(|g| g.id.clone()));
    let op = s.operations.register(&s.caller(), request.clone()).unwrap();
    let result = s.execute(&op).await.unwrap();
    drop(s);
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy)
        .await
        .unwrap();
    assert_eq!(s.handle(request).await.result.unwrap(), result);
    assert!(s.core.is_none());
}

#[tokio::test]
async fn startup_releases_proxy_even_when_manifest_or_journal_is_corrupt() {
    for target in [
        "runtime/active-manifest.json",
        "runtime/installation.json",
        "runtime/transactions/corrupt/journal.json",
    ] {
        let (tmp, mut s, proxy, _c) = setup().await;
        let root = proxy.read().unwrap();
        s.proxy
            .transition(global("127.0.0.1:10809".parse().unwrap()))
            .unwrap();
        atomic_write(&s.store.root.join(target), b"invalid metadata").unwrap();
        drop(s);
        let s = Supervisor::with_backend(&tmp.path().join("data"), proxy.clone())
            .await
            .unwrap();
        assert!(s.blocked);
        assert_eq!(proxy.read().unwrap(), root);
    }
}

#[tokio::test]
async fn precommit_promoted_asset_is_conditionally_restored_on_startup() {
    let (tmp, mut s, proxy, _c) = setup().await;
    let before = s.store.profile("p").unwrap();
    let mut generation = s.generation.clone().unwrap();
    generation.id = id();
    let op = s
        .operations
        .register(
            &s.caller(),
            Request::new(
                Command::RestartCore,
                s.generation.as_ref().map(|g| g.id.clone()),
            ),
        )
        .unwrap();
    generation.transaction_id = op.transaction_id;
    generation.request_id = op.request.request_id;
    let changed = r#"{"outbounds":[{"tag":"changed","protocol":"freedom"}]}"#;
    generation.candidate.revision = hash(changed.as_bytes());
    let journal = Journal {
        generation: generation.clone(),
        previous: s.generation.clone(),
        previous_instance: s.core.as_ref().map(|c| c.instance_id.clone()),
        before_json: before.json.clone(),
        before_hash: before.revision,
        windows_before: proxy.read().unwrap(),
        lease_before: s.proxy.lease.clone(),
        promoted: true,
        settled: false,
    };
    write_json(&s.journal(&generation.transaction_id), &journal).unwrap();
    atomic_write(&s.store.profile_path("p").unwrap(), changed.as_bytes()).unwrap();
    s.proxy
        .transition(global("127.0.0.1:10809".parse().unwrap()))
        .unwrap();
    drop(s);
    let s = Supervisor::with_backend(&tmp.path().join("data"), proxy)
        .await
        .unwrap();
    assert!(!s.blocked);
    assert_eq!(s.store.profile("p").unwrap().json, before.json);
    assert_ne!(s.generation.as_ref().unwrap().id, generation.id);
}

#[tokio::test]
async fn safety_stop_preserves_an_unrecorded_previous_commit() {
    let (tmp, mut s, proxy, c) = setup().await;
    let json = r#"{"outbounds":[{"tag":"new-commit","protocol":"freedom"}]}"#;
    let revision = s.store.profile("p").unwrap().revision;
    let root = s.store.root.clone();
    let c: Candidate = serde_json::from_value(
        send(
            &mut s,
            Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: revision,
                json: json.into(),
                core_path: c.core.path,
                asset_root: root,
            },
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let request = Request::new(apply(&c), s.generation.as_ref().map(|g| g.id.clone()));
    let op = s.operations.register(&s.caller(), request.clone()).unwrap();
    s.fail_settlement = true;
    let committed = s.execute(&op).await.unwrap();
    assert!(s.blocked); // Manifest committed, but neither settlement nor response was recorded.
    s.fail_settlement = false;
    send(&mut s, Command::StopCore).await.unwrap();
    drop(s);
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy)
        .await
        .unwrap();
    assert!(!s.blocked);
    assert_eq!(s.handle(request).await.result.unwrap(), committed);
    assert!(!s.generation.as_ref().unwrap().running);
    assert_eq!(s.store.profile("p").unwrap().json, json);
}

async fn failing_candidate(s: &mut Supervisor<FakeProxy>) -> Candidate {
    use std::os::unix::fs::PermissionsExt;
    // This tests rollback after an immediate exit, not Python startup under load.
    // A delayed Python interpreter can outlive Direct's 250 ms observation window.
    let core = s.store.root.join("immediate-exit-core");
    fs::write(&core, b"#!/bin/sh\ncase \"$1\" in version) echo 'Xray immediate-exit-fixture'; exit 0;; esac\n[ \"$2\" = '-test' ] && exit 0\nexit 24\n").unwrap();
    fs::set_permissions(&core, fs::Permissions::from_mode(0o700)).unwrap();
    let p = s.store.profile("p").unwrap();
    serde_json::from_value(
        send(
            s,
            Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: p.revision,
                json: r#"{"outbounds":[{"protocol":"freedom"}],"fixtureFailStart":true}"#.into(),
                core_path: core,
                asset_root: s.store.root.clone(),
            },
        )
        .await
        .unwrap(),
    )
    .unwrap()
}
#[tokio::test]
async fn failed_apply_restores_actual_running_or_stopped_state() {
    for running in [true, false] {
        let (tmp, s, proxy, _c) = setup().await;
        let mut s = if running {
            s
        } else {
            drop(s);
            Supervisor::with_backend(&tmp.path().join("data"), proxy)
                .await
                .unwrap()
        };
        assert_eq!(s.core.is_some(), running);
        let c = failing_candidate(&mut s).await;
        assert_eq!(
            send(&mut s, apply(&c)).await.unwrap_err().code,
            "CORE_START_FAILED"
        );
        assert_eq!(s.core.is_some(), running);
        assert!(!s.blocked);
    }
}
#[tokio::test]
async fn unresolved_asset_recovery_remains_pending_until_repair() {
    let (tmp, mut s, proxy, c) = setup().await;
    let before = s.store.profile("p").unwrap();
    let request = Request::new(apply(&c), s.generation.as_ref().map(|g| g.id.clone()));
    let op = s.operations.register(&s.caller(), request.clone()).unwrap();
    let mut generation = s.generation.clone().unwrap();
    generation.id = id();
    generation.transaction_id = op.transaction_id.clone();
    generation.request_id = request.request_id.clone();
    let journal = Journal {
        generation,
        previous: s.generation.clone(),
        previous_instance: s.running_instance().unwrap(),
        before_json: before.json.clone(),
        before_hash: before.revision,
        windows_before: proxy.read().unwrap(),
        lease_before: s.proxy.lease.clone(),
        promoted: true,
        settled: false,
    };
    write_json(&s.journal(&op.transaction_id), &journal).unwrap();
    fs::write(s.store.profile_path("p").unwrap(), r#"{"external":true}"#).unwrap();
    drop(s);
    let root = tmp.path().join("data");
    let mut s = Supervisor::with_backend(&root, proxy.clone())
        .await
        .unwrap();
    assert!(s.blocked);
    let op = s.operations.lookup(&s.caller(), &request).unwrap().unwrap();
    assert!(op.response.is_none());
    assert!(op.recovery_required);
    assert_eq!(
        s.handle(request.clone()).await.result.unwrap_err().code,
        "RECOVERY_REQUIRED"
    );
    fs::write(s.store.profile_path("p").unwrap(), &before.json).unwrap();
    drop(s);
    let mut s = Supervisor::with_backend(&root, proxy).await.unwrap();
    assert!(!s.blocked);
    assert_eq!(
        s.handle(request.clone()).await.result.unwrap_err().code,
        "INTERRUPTED_COMPENSATED"
    );
    assert!(s.core.is_none());
    assert_eq!(
        s.handle(request).await.result.unwrap_err().code,
        "INTERRUPTED_COMPENSATED"
    );
}
#[tokio::test]
async fn failed_rollback_settlement_blocks_new_commits_until_restart() {
    let (tmp, mut s, proxy, good) = setup().await;
    let bad = failing_candidate(&mut s).await;
    s.fail_settlement = true;
    let request = Request::new(apply(&bad), s.generation.as_ref().map(|g| g.id.clone()));
    assert_eq!(
        s.handle(request.clone()).await.result.unwrap_err().code,
        "IO_ERROR"
    );
    assert!(s.blocked);
    assert_eq!(
        send(&mut s, apply(&good)).await.unwrap_err().code,
        "RECOVERY_REQUIRED"
    );
    assert_eq!(
        s.handle(request.clone()).await.result.unwrap_err().code,
        "RECOVERY_REQUIRED"
    );
    let formal = s.store.profile("p").unwrap().json;
    drop(s);
    let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy)
        .await
        .unwrap();
    assert!(!s.blocked);
    assert_eq!(
        s.handle(request).await.result.unwrap_err().code,
        "INTERRUPTED_COMPENSATED"
    );
    send(&mut s, apply(&good)).await.unwrap();
    assert_eq!(s.store.profile("p").unwrap().json, formal);
}

#[derive(Clone, Default)]
struct FakeStartup(Arc<Mutex<(Option<windows_integration::startup::StartupTask>, bool)>>);
impl StartupBackend for FakeStartup {
    fn read(&self, _: &str) -> Result<Option<windows_integration::startup::StartupTask>> {
        Ok(self.0.lock().unwrap().0.clone())
    }
    fn prepare(
        &self,
        owner: &str,
        _: &Path,
        _: &Path,
    ) -> Result<windows_integration::startup::StartupTask> {
        Ok(windows_integration::startup::StartupTask { xml: owner.into() })
    }
    fn replace(
        &mut self,
        _: &str,
        before: &Option<windows_integration::startup::StartupTask>,
        target: &Option<windows_integration::startup::StartupTask>,
    ) -> Result<()> {
        let mut state = self.0.lock().unwrap();
        if state.0 != *before {
            return Err(Error::new("STARTUP_CONFLICT", "external task"));
        }
        if state.1 {
            state.0 = Some(windows_integration::startup::StartupTask {
                xml: "external task".into(),
            });
            return Err(Error::new("STARTUP_CONFLICT", "external task"));
        }
        state.0 = target.clone();
        Ok(())
    }
}
#[tokio::test]
async fn settings_commit_receipt_survives_response_loss_and_later_settings() {
    let (tmp, mut s, proxy, _c) = setup().await;
    let startup = FakeStartup::default();
    s.startup = StartupController::new(
        Box::new(startup.clone()),
        &s.store.root,
        &s.principal,
        &s.installation,
    );
    let request = Request::new(
        Command::SaveSettings {
            settings: Settings {
                start_on_logon: true,
                ..Settings::default()
            },
        },
        s.generation.as_ref().map(|g| g.id.clone()),
    );
    let op = s.operations.register(&s.caller(), request.clone()).unwrap();
    let committed = s.execute(&op).await.unwrap();
    send(
        &mut s,
        Command::SaveSettings {
            settings: Settings::default(),
        },
    )
    .await
    .unwrap();
    drop(s);
    let mut s = Supervisor::with_backends(&tmp.path().join("data"), proxy, Box::new(startup))
        .await
        .unwrap();
    assert!(!s.blocked);
    assert!(!s.settings.start_on_logon);
    assert_eq!(s.handle(request).await.result.unwrap(), committed);
}
#[tokio::test]
async fn unresolved_settings_intent_blocks_changes_and_recovers_after_repair() {
    let (tmp, mut s, proxy, _c) = setup().await;
    let startup = FakeStartup::default();
    startup.0.lock().unwrap().1 = true;
    s.startup = StartupController::new(
        Box::new(startup.clone()),
        &s.store.root,
        &s.principal,
        &s.installation,
    );
    let request = Request::new(
        Command::SaveSettings {
            settings: Settings {
                start_on_logon: true,
                ..Settings::default()
            },
        },
        s.generation.as_ref().map(|g| g.id.clone()),
    );
    assert!(s.handle(request.clone()).await.result.is_err());
    assert!(s.blocked);
    assert_eq!(
        s.handle(request.clone()).await.result.unwrap_err().code,
        "RECOVERY_REQUIRED"
    );
    assert_eq!(
        send(&mut s, Command::RestartCore).await.unwrap_err().code,
        "RECOVERY_REQUIRED"
    );
    drop(s);
    let root = tmp.path().join("data");
    let s = Supervisor::with_backends(&root, proxy.clone(), Box::new(startup.clone()))
        .await
        .unwrap();
    assert!(s.blocked);
    drop(s);
    *startup.0.lock().unwrap() = (None, false);
    let mut s = Supervisor::with_backends(&root, proxy, Box::new(startup))
        .await
        .unwrap();
    assert!(!s.blocked);
    assert_eq!(
        s.handle(request).await.result.unwrap_err().code,
        "INTERRUPTED_COMPENSATED"
    );
}

#[tokio::test]
async fn candidate_and_history_publications_recover_lost_results_without_reexecution() {
    for stage_history in [false, true] {
        for fault in [
            None,
            Some(EffectFault::BeforePublish),
            Some(EffectFault::AfterPublish),
        ] {
            let (tmp, mut s, proxy, c) = setup().await;
            let command = if stage_history {
                Command::StageHistory {
                    generation_id: s.generation.as_ref().unwrap().id.clone(),
                    expected_revision: c.revision.clone(),
                }
            } else {
                Command::ValidateDraft {
                    profile_id: "p".into(),
                    expected_revision: c.revision.clone(),
                    json: s.store.profile("p").unwrap().json,
                    core_path: c.core.path,
                    asset_root: tmp.path().into(),
                }
            };
            let request = Request::new(command, s.generation.as_ref().map(|g| g.id.clone()));
            let op = s.operations.register(&s.caller(), request.clone()).unwrap();
            s.effect_fault = fault;
            let result = s.execute(&op).await;
            let evidence: Value = read_json(&s.effect_path(&op)).unwrap();
            let expected = evidence["publication"]["success"].clone();
            let prepared_count = fs::read_dir(s.store.root.join("candidates"))
                .unwrap()
                .count();
            if fault.is_some() {
                assert!(result.is_err());
            } else {
                assert!(result.is_ok());
            }
            drop(s);
            let mut s = Supervisor::with_backend(&tmp.path().join("data"), proxy)
                .await
                .unwrap();
            assert!(!s.blocked);
            let recovered = s.handle(request.clone()).await.result;
            if fault == Some(EffectFault::BeforePublish) {
                assert_eq!(
                    recovered.as_ref().unwrap_err().code,
                    "INTERRUPTED_COMPENSATED"
                );
            } else {
                assert_eq!(recovered.as_ref().unwrap(), &expected);
            }
            assert_eq!(s.handle(request).await.result, recovered);
            assert_eq!(
                fs::read_dir(s.store.root.join("candidates"))
                    .unwrap()
                    .count(),
                prepared_count
            );
            assert!(s.core.is_none());
        }
    }
}

#[tokio::test]
async fn operation_corruption_still_allows_stopping_a_running_core() {
    for missing in [false, true] {
        let (_tmp, mut s, _proxy, c) = setup().await;
        let request = Request::new(
            Command::CreateProfile {
                profile_id: "unused".into(),
                json: "{}".into(),
            },
            s.generation.as_ref().map(|g| g.id.clone()),
        );
        s.handle(request.clone()).await.result.unwrap();
        let key = hash(format!("{}\0{}", s.caller(), request.request_id).as_bytes());
        let path = s
            .store
            .root
            .join("runtime/operations")
            .join(format!("{key}.json"));
        if missing {
            fs::remove_file(&path).unwrap();
        } else {
            fs::write(&path, "{broken").unwrap();
        }
        assert_eq!(
            s.handle(request).await.result.unwrap_err().code,
            "OPERATION_RECOVERY_REQUIRED"
        );
        assert!(s.blocked);
        assert_eq!(
            send(&mut s, apply(&c)).await.unwrap_err().code,
            "RECOVERY_REQUIRED"
        );
        assert!(s.core.is_some());
        send(&mut s, Command::StopCore).await.unwrap();
        assert!(s.core.is_none());
        assert!(!s.generation.as_ref().unwrap().running);
        if missing {
            assert!(!path.exists());
        } else {
            assert_eq!(fs::read_to_string(path).unwrap(), "{broken");
        }
    }
}

#[tokio::test]
async fn mixed_failed_and_successful_restarts_never_get_a_second_budget() {
    async fn wait_run(counter: &Path, expected: &str) {
        tokio::time::timeout(Duration::from_secs(10), async {
            while fs::read_to_string(counter).ok().as_deref() != Some(expected) {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
    }
    async fn observe_failed_start(s: &mut Supervisor<FakeProxy>) {
        // Direct only promises a startup grace period. Under scheduler pressure the fixture
        // may exit after that period; observe its real exit before checking recovery state.
        if let Some(core) = &mut s.core {
            tokio::time::timeout(Duration::from_secs(10), async {
                while core.process.try_wait().unwrap().is_none() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            s.tick().await;
        }
    }
    let counter_dir = tempfile::tempdir().unwrap();
    let counter = counter_dir.path().join("runs");
    let insertion = format!(
        "import pathlib\np=pathlib.Path({:?})\nn=int(p.read_text())+1 if p.exists() else 1\np.write_text(str(n))\nif n in (2,4): sys.exit(24)\nlisteners = []",
        counter.to_str().unwrap()
    );
    let script = include_str!("../tests/fixtures/core.py").replace("listeners = []", &insertion);
    let (tmp, mut s, _proxy, _c) = setup_script(script.as_bytes()).await;
    wait_run(&counter, "1").await;
    crash(&mut s).await;
    s.retry.as_mut().unwrap().due = Instant::now();
    s.tick().await;
    wait_run(&counter, "2").await;
    observe_failed_start(&mut s).await;
    assert!(s.core.is_none());
    assert_eq!(s.restart_budget.attempts.len(), 1);
    s.retry.as_mut().unwrap().due = Instant::now();
    s.tick().await;
    wait_run(&counter, "3").await;
    assert!(s.core.is_some());
    assert_eq!(s.restart_budget.attempts.len(), 2);
    crash(&mut s).await;
    s.retry.as_mut().unwrap().due = Instant::now();
    s.tick().await;
    wait_run(&counter, "4").await;
    observe_failed_start(&mut s).await;
    assert!(s.core.is_none());
    assert!(s.retry.is_none());
    assert_eq!(fs::read_to_string(&counter).unwrap(), "4");
    assert_eq!(s.restart_budget.attempts.len(), 3);
    assert_eq!(s.last_error.as_ref().unwrap().code, "CORE_CRASH_LOOP");
    // A manual restart remains available but does not reset the automatic budget.
    send(&mut s, Command::RestartCore).await.unwrap();
    wait_run(&counter, "5").await;
    crash(&mut s).await;
    assert!(s.retry.is_none());
    assert_eq!(s.restart_budget.attempts.len(), 3);
    // Expire the sliding window and verify a new crash can schedule recovery.
    for time in &mut s.restart_budget.attempts {
        *time = Instant::now() - Duration::from_secs(61);
    }
    send(&mut s, Command::RestartCore).await.unwrap();
    wait_run(&counter, "6").await;
    crash(&mut s).await;
    assert!(s.retry.is_some());
    send(&mut s, Command::StopCore).await.unwrap();
    assert!(s.retry.is_none());
    drop(s);
    drop(tmp);
}

#[tokio::test]
async fn failed_core_validation_keeps_raw_diagnostics_in_session_logs_only() {
    let (_tmp, mut s, _proxy, good) = setup().await;
    let p = s.store.profile("p").unwrap();
    let request = Request::new(
        Command::ValidateDraft {
            profile_id: "p".into(),
            expected_revision: p.revision,
            json: r#"{"outbounds":[{"protocol":"freedom"}],"fixtureReject":true}"#.into(),
            core_path: good.core.path,
            asset_root: s.store.root.clone(),
        },
        s.generation.as_ref().map(|g| g.id.clone()),
    );
    let result = s.handle(request.clone()).await.result.unwrap_err();
    assert_eq!(result.code, "CORE_VALIDATION_FAILED");
    assert!(!result.message.contains("sensitive-core-diagnostic-marker"));
    let logs = send(&mut s, Command::GetLogs).await.unwrap();
    assert!(
        logs.to_string()
            .contains("sensitive-core-diagnostic-marker")
    );
    let operation = send(
        &mut s,
        Command::GetOperation {
            request_id: request.request_id,
        },
    )
    .await
    .unwrap();
    assert!(
        !operation
            .to_string()
            .contains("sensitive-core-diagnostic-marker")
    );
    for file in std::fs::read_dir(s.store.root.join("runtime/operations")).unwrap() {
        assert!(
            !std::fs::read_to_string(file.unwrap().path())
                .unwrap()
                .contains("sensitive-core-diagnostic-marker")
        );
    }
}
