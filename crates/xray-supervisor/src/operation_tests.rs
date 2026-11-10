use super::*;
#[derive(Clone)]
struct FakeProxy;
impl ProxyBackend for FakeProxy {
    fn read(&self) -> Result<ProxySnapshot> {
        Ok(ProxySnapshot::default())
    }
    fn write(&mut self, _: &ProxySnapshot) -> Result<()> {
        Ok(())
    }
}
fn create(name: &str) -> Command {
    Command::CreateProfile {
        profile_id: name.into(),
        json: "{}".into(),
    }
}
async fn call(s: &mut Supervisor<FakeProxy>, command: Command) -> Result<Value> {
    s.handle(Request::new(
        command,
        s.generation.as_ref().map(|g| g.id.clone()),
    ))
    .await
    .result
}

#[tokio::test]
async fn new_runtime_success_clears_error_without_erasing_operation_history() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    let old_success = Request::new(Command::StopCore, None);
    s.handle(old_success.clone()).await.result.unwrap();
    let failed = Request::new(Command::RestartCore, None);
    let error = s.handle(failed.clone()).await.result.unwrap_err();
    assert_eq!(error.code, "NO_ACTIVE_PROFILE");

    for command in [Command::GetStatus, Command::GetLogs, create("unrelated")] {
        call(&mut s, command).await.unwrap();
        assert_eq!(s.status().last_error.as_ref().unwrap().code, error.code);
    }
    // A historical success is not evidence that the current failure was fixed.
    s.handle(old_success).await.result.unwrap();
    assert_eq!(s.status().last_error.as_ref().unwrap().code, error.code);

    call(&mut s, Command::StopCore).await.unwrap();
    assert!(s.status().last_error.is_none());
    let operation = call(
        &mut s,
        Command::GetOperation {
            request_id: failed.request_id,
        },
    )
    .await
    .unwrap();
    assert_eq!(operation["response"]["result"]["Err"]["code"], error.code);
    assert!(s.status().last_error.is_none());
}

#[tokio::test]
async fn successful_safety_stop_keeps_unresolved_recovery_error() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    s.blocked = true;
    s.last_error = Some(Error::new(
        "RECOVERY_REQUIRED",
        "Unresolved recovery fixture",
    ));
    call(&mut s, Command::StopCore).await.unwrap();
    let status = s.status();
    assert!(status.recovery_required);
    assert_eq!(status.last_error.unwrap().code, "RECOVERY_REQUIRED");
}

#[tokio::test]
async fn profile_publication_is_recovered_on_both_sides_of_commit() {
    for delete in [false, true] {
        for fault in [EffectFault::BeforePublish, EffectFault::AfterPublish] {
            let tmp = tempfile::tempdir().unwrap();
            let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
                .await
                .unwrap();
            let command = if delete {
                let value = call(&mut s, create("p")).await.unwrap();
                Command::DeleteProfile {
                    profile_id: "p".into(),
                    expected_revision: value["revision"].as_str().unwrap().into(),
                }
            } else {
                create("p")
            };
            let request = Request::new(command, None);
            s.effect_fault = Some(fault);
            assert!(s.handle(request.clone()).await.result.is_err());
            assert!(s.blocked);
            assert_eq!(
                call(&mut s, create("other")).await.unwrap_err().code,
                "RECOVERY_REQUIRED"
            );
            drop(s);
            let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
                .await
                .unwrap();
            assert!(!s.blocked);
            let result = s.handle(request.clone()).await.result;
            let committed = fault == EffectFault::AfterPublish;
            if committed {
                assert!(result.is_ok());
            } else {
                assert_eq!(result.as_ref().unwrap_err().code, "INTERRUPTED_COMPENSATED");
            }
            assert_eq!(
                s.store.profile("p").is_ok(),
                if delete { !committed } else { committed }
            );
            assert_eq!(s.handle(request).await.result, result);
        }
    }
}
#[tokio::test]
async fn completed_publication_receipt_survives_later_asset_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    let request = Request::new(create("p"), None);
    let op = s.operations.register(&s.caller(), request.clone()).unwrap();
    let original = s.execute(&op).await.unwrap(); // Effect receipt durable, response lost.
    let revision = s.store.profile("p").unwrap().revision;
    call(
        &mut s,
        Command::DeleteProfile {
            profile_id: "p".into(),
            expected_revision: revision,
        },
    )
    .await
    .unwrap();
    drop(s);
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    assert!(!s.blocked);
    assert_eq!(s.handle(request).await.result.unwrap(), original);
    assert!(s.store.profile("p").is_err());
}
#[tokio::test]
async fn external_publication_conflict_remains_pending_until_repair() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    let request = Request::new(create("p"), None);
    s.effect_fault = Some(EffectFault::AfterPublish);
    s.handle(request.clone()).await.result.unwrap_err();
    let path = s.store.profile_path("p").unwrap();
    fs::write(&path, r#"{"external":true}"#).unwrap();
    drop(s);
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    assert!(s.blocked);
    assert_eq!(
        s.handle(request.clone()).await.result.unwrap_err().code,
        "RECOVERY_REQUIRED"
    );
    let op = s
        .operations
        .get(&s.caller(), &request.request_id)
        .unwrap()
        .unwrap();
    assert!(op.response.is_none());
    assert!(op.recovery_required);
    fs::write(&path, "{}").unwrap();
    drop(s);
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    assert!(!s.blocked);
    assert!(s.handle(request).await.result.is_ok());
}
#[tokio::test]
async fn preparation_and_accepted_requests_have_distinct_recovery_outcomes() {
    for started in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
            .await
            .unwrap();
        let request = Request::new(create("p"), None);
        let op = s.operations.register(&s.caller(), request.clone()).unwrap();
        if started {
            s.begin_effect(&op).unwrap();
        }
        drop(s);
        let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
            .await
            .unwrap();
        assert!(!s.blocked);
        assert_eq!(
            s.handle(request).await.result.unwrap_err().code,
            if started {
                "INTERRUPTED_COMPENSATED"
            } else {
                "INTERRUPTED_BEFORE_APPLY"
            }
        );
        assert!(s.store.profile("p").is_err());
    }
}
#[tokio::test]
async fn detected_corruption_blocks_writes_from_all_operation_read_paths() {
    for read_path in 0..3 {
        for syntactically_valid in [false, true] {
            let tmp = tempfile::tempdir().unwrap();
            let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
                .await
                .unwrap();
            let request = Request::new(create("p"), None);
            s.handle(request.clone()).await.result.unwrap();
            let path = fs::read_dir(tmp.path().join("runtime/operations"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let intact = fs::read(&path).unwrap();
            if syntactically_valid {
                let mut value: Value = serde_json::from_slice(&intact).unwrap();
                value["response"]["result"]["Ok"] = json!({"forged":true});
                write_json(&path, &value).unwrap();
            } else {
                fs::write(&path, "{truncated").unwrap();
            }
            let detected = if read_path == 2 {
                call(
                    &mut s,
                    Command::ListHistory {
                        profile_id: "p".into(),
                    },
                )
                .await
            } else if read_path == 1 {
                call(
                    &mut s,
                    Command::GetOperation {
                        request_id: request.request_id.clone(),
                    },
                )
                .await
            } else {
                s.handle(request.clone()).await.result
            };
            assert_eq!(detected.unwrap_err().code, "OPERATION_RECOVERY_REQUIRED");
            assert!(s.blocked);
            assert_eq!(
                call(&mut s, create("next")).await.unwrap_err().code,
                "RECOVERY_REQUIRED"
            );
            // A fresh safety request still works and the corrupt evidence is preserved.
            call(&mut s, Command::StopCore).await.unwrap();
            assert_ne!(fs::read(&path).unwrap(), intact);
            fs::write(&path, intact).unwrap();
            drop(s);
            let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
                .await
                .unwrap();
            assert!(!s.blocked);
            call(&mut s, create("next")).await.unwrap();
        }
    }
}
#[tokio::test]
async fn changed_request_content_is_rejected_without_blocking_the_store() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    let mut request = Request::new(create("p"), None);
    s.handle(request.clone()).await.result.unwrap();
    request.command = create("q");
    assert_eq!(
        s.handle(request).await.result.unwrap_err().code,
        "REQUEST_ID_CONFLICT"
    );
    assert!(!s.blocked);
    call(&mut s, create("q")).await.unwrap();
}

#[tokio::test]
async fn legacy_ambiguous_write_is_preserved_for_reconciliation() {
    let tmp = tempfile::tempdir().unwrap();
    let s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    let request = Request::new(create("p"), None);
    let mut op = s.operations.register(&s.caller(), request.clone()).unwrap();
    op.evidence_version = 0;
    s.operations.save(&op).unwrap();
    s.store.create("p", "{}").unwrap(); // Old writer left no publication receipt.
    drop(s);
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    assert!(s.blocked);
    assert_eq!(
        s.handle(request).await.result.unwrap_err().code,
        "RECOVERY_REQUIRED"
    );
    assert_eq!(s.store.profile("p").unwrap().json, "{}");
    call(&mut s, Command::StopCore).await.unwrap();
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires official Core via XRAY_TEST_CORE; uses fake system proxy"]
async fn official_candidate_and_history_publications_recover_on_windows() {
    let core_path = PathBuf::from(std::env::var_os("XRAY_TEST_CORE").expect("XRAY_TEST_CORE"));
    for history in [false, true] {
        for fault in [
            None,
            Some(EffectFault::BeforePublish),
            Some(EffectFault::AfterPublish),
        ] {
            let tmp = tempfile::tempdir().unwrap();
            let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
                .await
                .unwrap();
            let json = r#"{"outbounds":[{"tag":"direct","protocol":"freedom"}]}"#;
            let p: Profile = serde_json::from_value(
                call(
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
            let validate = Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: p.revision.clone(),
                json: json.into(),
                core_path: core_path.clone(),
                asset_root: tmp.path().into(),
            };
            let command = if history {
                let c: Candidate =
                    serde_json::from_value(call(&mut s, validate).await.unwrap()).unwrap();
                call(
                    &mut s,
                    Command::ApplyProfile {
                        candidate_id: c.id,
                        candidate_digest: c.digest,
                        proxy_mode: ProxyMode::Direct,
                        http_inbound_tag: "http-in".into(),
                    },
                )
                .await
                .unwrap();
                Command::StageHistory {
                    generation_id: s.generation.as_ref().unwrap().id.clone(),
                    expected_revision: p.revision,
                }
            } else {
                validate
            };
            let request = Request::new(command, s.generation.as_ref().map(|g| g.id.clone()));
            let op = s.operations.register(&s.caller(), request.clone()).unwrap();
            s.effect_fault = fault;
            let result = s.execute(&op).await;
            let evidence: Value = read_json(&s.effect_path(&op)).unwrap();
            let count = fs::read_dir(s.store.root.join("candidates"))
                .unwrap()
                .count();
            assert_eq!(result.is_ok(), fault.is_none());
            drop(s);
            let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
                .await
                .unwrap();
            assert!(!s.blocked);
            let outcome = s.handle(request.clone()).await.result;
            if fault == Some(EffectFault::BeforePublish) {
                assert_eq!(
                    outcome.as_ref().unwrap_err().code,
                    "INTERRUPTED_COMPENSATED"
                );
            } else {
                assert_eq!(
                    outcome.as_ref().unwrap(),
                    &evidence["publication"]["success"]
                );
            }
            assert_eq!(s.handle(request).await.result, outcome);
            assert_eq!(
                fs::read_dir(s.store.root.join("candidates"))
                    .unwrap()
                    .count(),
                count
            );
            assert!(s.core.is_none());
        }
    }
}

#[tokio::test]
async fn missing_operation_blocks_replay_and_can_be_repaired_without_recreating_assets() {
    for legacy in [false, true] {
        for restart in [false, true] {
            for query in [false, true] {
                let tmp = tempfile::tempdir().unwrap();
                let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
                    .await
                    .unwrap();
                let request = Request::new(create("p"), None);
                let original = s.handle(request.clone()).await.result.unwrap();
                let op = s
                    .operations
                    .get(&s.caller(), &request.request_id)
                    .unwrap()
                    .unwrap();
                if legacy {
                    let mut old = op.clone();
                    old.evidence_version = 1;
                    s.operations.save(&old).unwrap();
                    fs::remove_file(
                        s.operations
                            .identity_path(&op.caller, &op.request.request_id),
                    )
                    .unwrap();
                }
                call(
                    &mut s,
                    Command::DeleteProfile {
                        profile_id: "p".into(),
                        expected_revision: original["revision"].as_str().unwrap().into(),
                    },
                )
                .await
                .unwrap();
                let path = s.operations.path(&op.caller, &op.request.request_id);
                let evidence = fs::read(&path).unwrap();
                fs::remove_file(&path).unwrap();
                if restart {
                    drop(s);
                    s = Supervisor::with_backend(tmp.path(), FakeProxy)
                        .await
                        .unwrap();
                    assert!(s.blocked);
                }
                let response = if query {
                    call(
                        &mut s,
                        Command::GetOperation {
                            request_id: request.request_id.clone(),
                        },
                    )
                    .await
                } else {
                    s.handle(request.clone()).await.result
                };
                assert_eq!(
                    response.unwrap_err().code,
                    if legacy && restart && !query {
                        "RECOVERY_REQUIRED"
                    } else {
                        "OPERATION_RECOVERY_REQUIRED"
                    }
                );
                assert!(s.blocked);
                assert!(s.store.profile("p").is_err());
                assert_eq!(
                    call(&mut s, create("other")).await.unwrap_err().code,
                    "RECOVERY_REQUIRED"
                );
                call(&mut s, Command::StopCore).await.unwrap();
                assert!(!path.exists()); // Safety stop does not overwrite the damaged request.
                fs::write(&path, evidence).unwrap();
                drop(s);
                let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
                    .await
                    .unwrap();
                assert!(!s.blocked);
                assert_eq!(s.handle(request).await.result.unwrap(), original);
                assert!(s.store.profile("p").is_err());
            }
        }
    }
}

#[tokio::test]
async fn missing_identity_and_orphan_accepted_identity_are_preserved_for_recovery() {
    for remove_primary in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let s = Supervisor::with_backend(tmp.path(), FakeProxy)
            .await
            .unwrap();
        let request = Request::new(create("p"), None);
        let op = s.operations.register(&s.caller(), request.clone()).unwrap();
        let path = if remove_primary {
            s.operations.path(&op.caller, &request.request_id)
        } else {
            s.operations.identity_path(&op.caller, &request.request_id)
        };
        fs::remove_file(&path).unwrap();
        drop(s);
        let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
            .await
            .unwrap();
        assert!(s.blocked);
        assert_eq!(
            s.handle(request).await.result.unwrap_err().code,
            "OPERATION_RECOVERY_REQUIRED"
        );
        call(&mut s, Command::Shutdown).await.unwrap();
        assert!(s.shutdown);
        assert!(s.store.profile("p").is_err());
    }
}

#[tokio::test]
async fn failed_automatic_attempts_share_one_budget_and_manual_stop_remains_available() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    let op = s
        .operations
        .register(&s.caller(), Request::new(Command::RestartCore, None))
        .unwrap();
    let generation = Generation {
        id: id(),
        transaction_id: op.transaction_id,
        request_id: op.request.request_id,
        candidate: Candidate {
            id: id(),
            digest: String::new(),
            profile_id: "p".into(),
            base_revision: String::new(),
            revision: String::new(),
            config_path: tmp.path().join("missing-config"),
            core: CoreIdentity {
                path: tmp.path().join("missing-core"),
                sha256: String::new(),
                version: String::new(),
            },
            resources: Resources {
                id: id(),
                root: tmp.path().join("missing-resources"),
                files: vec![],
                complete: true,
                notes: vec![],
            },
        },
        proxy_mode: ProxyMode::Direct,
        http_inbound_tag: "http-in".into(),
        running: true,
    };
    write_json(&s.manifest(), &generation).unwrap();
    s.generation = Some(generation.clone());
    s.failed_instance = Some("exited-instance".into());
    s.retry = Some(Retry {
        generation: generation.id,
        instance: "exited-instance".into(),
        epoch: s.recovery_epoch,
        due: Instant::now(),
    });
    for attempt in 1..=3 {
        s.retry.as_mut().unwrap().due = Instant::now();
        s.tick().await;
        assert_eq!(s.restart_budget.attempts.len(), attempt);
    }
    assert!(s.retry.is_none());
    assert!(s.core.is_none());
    assert_eq!(s.last_error.as_ref().unwrap().code, "CORE_CRASH_LOOP");
    s.tick().await;
    assert_eq!(s.restart_budget.attempts.len(), 3);
    call(&mut s, Command::StopCore).await.unwrap();
    assert!(!s.generation.as_ref().unwrap().running);
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "requires official Core via XRAY_TEST_CORE; uses fake system proxy"]
async fn official_failed_starts_and_successful_restarts_share_budget_on_windows() {
    let tmp = tempfile::tempdir().unwrap();
    let mut s = Supervisor::with_backend(tmp.path(), FakeProxy)
        .await
        .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = listener.local_addr().unwrap();
    drop(listener);
    let json = json!({"log":{"loglevel":"warning"},"inbounds":[{"tag":"http-in","listen":"127.0.0.1","port":endpoint.port(),"protocol":"http","settings":{}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}).to_string();
    let p: Profile = serde_json::from_value(
        call(
            &mut s,
            Command::CreateProfile {
                profile_id: "p".into(),
                json: json.clone(),
            },
        )
        .await
        .unwrap(),
    )
    .unwrap();
    let c: Candidate = serde_json::from_value(
        call(
            &mut s,
            Command::ValidateDraft {
                profile_id: "p".into(),
                expected_revision: p.revision,
                json,
                core_path: std::env::var_os("XRAY_TEST_CORE")
                    .expect("XRAY_TEST_CORE")
                    .into(),
                asset_root: tmp.path().into(),
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
            proxy_mode: ProxyMode::Direct,
            http_inbound_tag: "http-in".into(),
        },
    )
    .await
    .unwrap();
    s.core.as_mut().unwrap().stop().await.unwrap();
    s.tick().await;
    let blocker = std::net::TcpListener::bind(endpoint).unwrap();
    s.retry.as_mut().unwrap().due = Instant::now();
    s.tick().await;
    assert!(s.core.is_none());
    assert_eq!(s.restart_budget.attempts.len(), 1);
    drop(blocker);
    s.retry.as_mut().unwrap().due = Instant::now();
    s.tick().await;
    assert!(s.core.is_some());
    assert_eq!(s.restart_budget.attempts.len(), 2);
    s.core.as_mut().unwrap().stop().await.unwrap();
    s.tick().await;
    let _blocker = std::net::TcpListener::bind(endpoint).unwrap();
    s.retry.as_mut().unwrap().due = Instant::now();
    s.tick().await;
    assert!(s.core.is_none());
    assert!(s.retry.is_none());
    assert_eq!(s.restart_budget.attempts.len(), 3);
    assert_eq!(s.last_error.as_ref().unwrap().code, "CORE_CRASH_LOOP");
    call(&mut s, Command::StopCore).await.unwrap();
}
