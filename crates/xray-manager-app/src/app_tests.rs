use super::*;

struct Harness {
    app: App,
    requests: mpsc::Receiver<Request>,
    responses: mpsc::Sender<(Command, Response)>,
}
impl Harness {
    fn empty() -> Self {
        let (tx, requests) = mpsc::sync_channel(MAX_PENDING);
        let (responses, rx) = mpsc::channel();
        let mut app = App::with_channels(PathBuf::from("isolated-gui-test"), tx, rx);
        app.ui.connected = true;
        Self {
            app,
            requests,
            responses,
        }
    }
    fn new() -> Self {
        let mut h = Self::empty();
        h.app.load(profile("p", "{}"));
        h
    }
    fn send(&mut self, command: Command) -> Request {
        self.app.send(command.clone());
        self.requests
            .try_iter()
            .find(|r| std::mem::discriminant(&r.command) == std::mem::discriminant(&command))
            .unwrap()
    }
    fn reply(&mut self, request: Request, value: impl serde::Serialize) {
        self.responses
            .send((
                request.command,
                Response {
                    request_id: request.request_id,
                    result: Ok(serde_json::to_value(value).unwrap()),
                },
            ))
            .unwrap();
        self.app.receive();
    }
    fn validate(&mut self) -> Request {
        self.send(Command::ValidateDraft {
            profile_id: self.app.profile_id.clone(),
            expected_revision: self.app.revision.clone(),
            json: self.app.editor.clone(),
            core_path: self.app.core_path.clone().into(),
            asset_root: self.app.asset_root.clone().into(),

            allow_dynamic: false,
        })
    }
}
fn profile(id: &str, json: &str) -> Profile {
    Profile {
        id: id.into(),
        json: json.into(),
        revision: hash(json.as_bytes()),
    }
}
fn candidate(app: &App, json: &str) -> Candidate {
    Candidate {
        id: id(),
        digest: "test-digest".into(),
        profile_id: app.profile_id.clone(),
        base_revision: app.revision.clone(),
        revision: hash(json.as_bytes()),
        config_path: "frozen/config.json".into(),
        core: CoreIdentity {
            path: "frozen/core".into(),
            sha256: "core-hash".into(),
            version: "Xray test".into(),
        },
        resources: Resources {
            id: id(),
            root: "frozen/resources".into(),
            files: vec![],
            complete: true,
            notes: vec![],

            dynamic: vec![],
        },
    }
}
fn apply(c: &Candidate) -> Command {
    Command::ApplyProfile {
        candidate_id: c.id.clone(),
        candidate_digest: c.digest.clone(),
        proxy_mode: ProxyMode::Direct,
        http_inbound_tag: "http-in".into(),

        pac: None,
    }
}

#[test]
fn delayed_reload_cannot_replace_the_draft_being_validated() {
    let mut h = Harness::new();
    let read = h.send(Command::GetProfile {
        profile_id: "p".into(),
    });
    h.app.editor = "{\"outbounds\":[]}".into();
    let validate = h.validate();
    let c = candidate(&h.app, &h.app.editor);
    h.reply(read, profile("p", "{}"));
    assert_eq!(h.app.editor, "{\"outbounds\":[]}");
    h.reply(validate, &c);
    assert!(h.app.candidate_matches());
    assert!(!diff_lines(&h.app.saved_json, &h.app.editor).is_empty());
    let request = h.send(apply(&c));
    assert!(
        matches!(request.command, Command::ApplyProfile { candidate_id, .. } if candidate_id == c.id)
    );
}

#[test]
fn latest_selection_wins_and_old_validation_is_discarded() {
    let mut h = Harness::new();
    let validate = h.validate();
    let c = candidate(&h.app, "{}");
    let b = h.send(Command::GetProfile {
        profile_id: "b".into(),
    });
    let d = h.send(Command::GetProfile {
        profile_id: "d".into(),
    });
    h.reply(b, profile("b", "{\"b\":1}"));
    assert_eq!(h.app.profile_id, "p");
    h.reply(validate, c);
    assert!(!h.app.busy);
    assert!(!h.app.confirm_apply);
    h.reply(d, profile("d", "{\"d\":1}"));
    assert_eq!(h.app.profile_id, "d");
    assert_eq!(h.app.selected_profile, "d");
}

#[test]
fn validation_and_apply_are_bound_to_the_draft_and_context() {
    for change in ["json", "core", "assets", "mode", "tag", "revision"] {
        let mut h = Harness::new();
        let request = h.validate();
        let c = candidate(&h.app, "{}");
        h.reply(request, &c);
        assert!(h.app.candidate_matches());
        match change {
            "json" => h.app.editor.push(' '),
            "core" => h.app.core_path = "other-core".into(),
            "assets" => h.app.asset_root = "other-assets".into(),
            "mode" => h.app.mode = ProxyMode::Global,
            "tag" => h.app.http_tag = "other-inbound".into(),
            _ => h.app.revision = "other-revision".into(),
        }
        h.app.send(apply(&c));
        assert!(!h.app.confirm_apply, "{change}");
        assert!(
            !h.requests.try_iter().any(|r| r.command.is_write()),
            "{change}"
        );
    }
    let mut h = Harness::new();
    let request = h.validate();
    let c = candidate(&h.app, "{}");
    h.app.core_path = "changed-during-validation".into();
    h.reply(request, c);
    assert!(!h.app.confirm_apply);
    assert!(h.app.candidate.is_none());
}

#[test]
fn history_preview_is_bound_to_selection_and_stages_matching_json() {
    let mut h = Harness::new();
    let request = h.send(Command::StageHistory {
        generation_id: "history".into(),
        expected_revision: h.app.revision.clone(),
    });
    let c = candidate(&h.app, "{\"historical\":true}");
    let before = h.app.editor.clone();
    h.reply(
        request,
        serde_json::json!({"json":"{\"historical\":true}","candidate":c}),
    );
    assert_eq!(h.app.editor, before);
    assert!(matches!(h.app.ui.dialog, Some(ui::Dialog::HistoryDiff)));
    assert_eq!(
        h.app.ui.history_preview.as_ref().unwrap().json,
        "{\"historical\":true}"
    );
    assert!(h.app.load_history_preview());
    assert_eq!(h.app.editor, "{\"historical\":true}");
    assert!(!h.app.candidate_matches());
    let request = h.send(Command::StageHistory {
        generation_id: "history".into(),
        expected_revision: h.app.revision.clone(),
    });
    let c = candidate(&h.app, "{}");
    h.app.send(Command::GetProfile {
        profile_id: "different".into(),
    });
    h.reply(request, serde_json::json!({"json":"{}","candidate":c}));
    assert_eq!(h.app.editor, "{\"historical\":true}");
    assert!(!h.app.confirm_apply);
}

#[test]
fn polling_is_coalesced_and_unsent_writes_do_not_set_busy() {
    let mut h = Harness::new();
    for _ in 0..100 {
        h.app.send(Command::GetStatus);
        h.app.send(Command::GetLogs);
    }
    assert_eq!(
        h.requests
            .try_iter()
            .filter(|r| matches!(r.command, Command::GetStatus | Command::GetLogs))
            .count(),
        2
    );
    while h.app.pending.len() < MAX_PENDING {
        h.app.send(Command::GetProfile { profile_id: id() });
    }
    h.app.send(Command::CreateProfile {
        profile_id: "not-sent".into(),
        json: "{}".into(),
    });
    assert!(!h.app.busy);
    assert_eq!(h.app.pending.len(), MAX_PENDING);
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
}

fn stored_settings() -> Settings {
    Settings {
        core_path: std::env::temp_dir().join("saved-xray.exe"),
        start_on_logon: true,
        connect_on_start: true,
        restore_last_state: true,
    }
}
fn assert_settings_and_save(h: &mut Harness, expected: &Settings) {
    assert_eq!(h.app.core_path, expected.core_path.display().to_string());
    assert_eq!(h.app.settings.start_on_logon, expected.start_on_logon);
    assert_eq!(h.app.settings.connect_on_start, expected.connect_on_start);
    assert_eq!(
        h.app.settings.restore_last_state,
        expected.restore_last_state
    );
    h.app.settings.core_path = h.app.core_path.clone().into();
    let request = h.send(Command::SaveSettings {
        settings: h.app.settings.clone(),
    });
    let Command::SaveSettings { settings } = request.command else {
        panic!("Expected SaveSettings");
    };
    assert_eq!(settings.core_path, expected.core_path);
    assert_eq!(settings.start_on_logon, expected.start_on_logon);
    assert_eq!(settings.connect_on_start, expected.connect_on_start);
    assert_eq!(settings.restore_last_state, expected.restore_last_state);
}

#[test]
fn legacy_settings_default_to_restoring_the_previous_core_state() {
    let settings: Settings =
        serde_json::from_str(r#"{"core_path":"","connect_on_start":false,"start_on_logon":false}"#)
            .unwrap();
    assert!(settings.restore_last_state);
}

#[test]
fn startup_loads_profile_and_saved_settings_in_either_response_order() {
    for settings_first in [true, false] {
        let mut h = Harness::empty();
        h.send(Command::GetStatus);
        let list = h.send(Command::ListProfileSummaries {
            after: None,
            limit: 100,
        });
        let settings = h.send(Command::GetSettings);
        h.reply(
            list,
            ProfilePage {
                items: vec![ProfileSummary {
                    id: "p".into(),
                    revision: hash(b"{}"),
                }],
                next_after: None,
            },
        );
        let load = h
            .requests
            .try_iter()
            .find(|r| matches!(r.command, Command::GetProfile { .. }))
            .unwrap();
        let saved = stored_settings();
        if settings_first {
            h.reply(settings, &saved);
            h.reply(load, profile("p", "{}"));
        } else {
            h.reply(load, profile("p", "{}"));
            h.reply(settings, &saved);
        }
        assert_eq!(h.app.profile_id, "p");
        assert_eq!(h.app.selected_profile, "p");
        assert_eq!(h.app.editor, "{}");
        assert_settings_and_save(&mut h, &saved);
    }
}

#[test]
fn startup_without_profiles_loads_saved_settings() {
    let mut h = Harness::empty();
    let list = h.send(Command::ListProfileSummaries {
        after: None,
        limit: 100,
    });
    let settings = h.send(Command::GetSettings);
    h.reply(
        list,
        ProfilePage {
            items: vec![],
            next_after: None,
        },
    );
    let saved = stored_settings();
    h.reply(settings, &saved);
    assert!(h.app.profile_id.is_empty());
    assert_settings_and_save(&mut h, &saved);
}

#[test]
fn profile_selection_and_json_edits_do_not_discard_settings() {
    let mut h = Harness::new();
    let settings = h.send(Command::GetSettings);
    let load = h.send(Command::GetProfile {
        profile_id: "q".into(),
    });
    h.reply(load, profile("q", "{}"));
    h.app.editor = "{\"edited\":true}".into();
    let saved = stored_settings();
    h.reply(settings, &saved);
    assert_eq!(h.app.profile_id, "q");
    assert_eq!(h.app.editor, "{\"edited\":true}");
    assert_settings_and_save(&mut h, &saved);
}

#[test]
fn late_settings_do_not_replace_user_edits_or_reverted_edits() {
    for change in ["core", "login", "connect", "reverted"] {
        let mut h = Harness::new();
        let request = h.send(Command::GetSettings);
        match change {
            "core" => h.app.core_path = "chosen-core".into(),
            "login" => h.app.settings.start_on_logon = true,
            "connect" => h.app.settings.connect_on_start = true,
            _ => {
                h.app.settings.start_on_logon = true;
                h.app.settings_edited();
                h.app.settings.start_on_logon = false;
            }
        }
        h.app.settings_edited();
        let expected = h.app.settings_context();
        h.reply(request, stored_settings());
        assert!(h.app.settings_context() == expected, "{change}");
    }
}

#[test]
fn latest_settings_read_wins_and_save_supersedes_pending_reads() {
    let mut h = Harness::new();
    let old = h.send(Command::GetSettings);
    let latest = h.send(Command::GetSettings);
    h.reply(latest, stored_settings());
    h.reply(old, Settings::default());
    assert_settings_and_save(&mut h, &stored_settings());

    let mut h = Harness::new();
    let read = h.send(Command::GetSettings);
    let save = h.send(Command::SaveSettings {
        settings: h.app.settings.clone(),
    });
    h.reply(read, stored_settings());
    assert!(h.app.core_path.is_empty());
    assert!(!h.app.settings.start_on_logon && !h.app.settings.connect_on_start);
    assert!(h.app.busy);
    h.reply(save, serde_json::json!({"saved":true}));
    assert!(!h.app.busy);
}

fn dependency_job(
    h: &mut Harness,
    select: bool,
) -> mpsc::Sender<Result<crate::dependencies::Report>> {
    let (tx, rx) = mpsc::channel();
    h.app.dependency_job = Some(DependencyJob {
        response: rx,
        context: h.app.settings_context(),
        asset_root: h.app.asset_root.clone(),
        select,
        version: h.app.dependency_version.clone(),
    });
    tx
}
fn managed_report() -> crate::dependencies::Report {
    serde_json::from_value(serde_json::json!({
        "selected": {"version":"v26.11.9","core_path":"managed/xray.exe","asset_root":"managed"},
        "installations":[{"version":"v26.11.9","install_id":"test","valid":true,"error":null}]
    }))
    .unwrap()
}

#[test]
fn managed_bundle_populates_settings_and_requires_fresh_validation() {
    let mut h = Harness::new();
    let c = candidate(&h.app, "{}");
    h.app.accept_candidate(c);
    let tx = dependency_job(&mut h, true);
    tx.send(Ok(managed_report())).unwrap();
    h.app.receive_dependencies();
    assert_eq!(h.app.core_path, "managed/xray.exe");
    assert_eq!(h.app.asset_root, "managed");
    assert!(!h.app.candidate_matches());
    assert!(!h.app.busy);
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
    let expected = Settings {
        core_path: "managed/xray.exe".into(),
        ..Settings::default()
    };
    assert_settings_and_save(&mut h, &expected);
}

#[test]
fn dependency_completion_preserves_new_settings_and_installation_intent() {
    for change in ["core", "assets", "login", "version", "save"] {
        let mut h = Harness::new();
        let tx = dependency_job(&mut h, true);
        match change {
            "core" => h.app.core_path = "user-core".into(),
            "assets" => h.app.asset_root = "user-assets".into(),
            "login" => h.app.settings.start_on_logon = true,
            "version" => h.app.dependency_version = "v26.3.27".into(),
            _ => {
                h.send(Command::SaveSettings {
                    settings: h.app.settings.clone(),
                });
            }
        }
        let expected = h.app.settings_context();
        let assets = h.app.asset_root.clone();
        tx.send(Ok(managed_report())).unwrap();
        h.app.receive_dependencies();
        assert!(h.app.settings_context() == expected, "{change}");
        assert_eq!(h.app.asset_root, assets, "{change}");
        assert_eq!(h.app.busy, change == "save");
        assert_eq!(h.app.dependency_report.installations.len(), 1);
    }
}

#[test]
fn dependency_checks_and_failures_do_not_replace_a_selected_core() {
    let mut h = Harness::new();
    h.app.core_path = "user-core".into();
    let tx = dependency_job(&mut h, false);
    tx.send(Ok(managed_report())).unwrap();
    h.app.receive_dependencies();
    assert_eq!(h.app.core_path, "user-core");
    let tx = dependency_job(&mut h, true);
    tx.send(Err(Error::new(
        "DEPENDENCY_INSTALL_FAILED",
        "checksum failed",
    )))
    .unwrap();
    h.app.receive_dependencies();
    assert!(h.app.dependency_message.contains("checksum failed"));
    assert_eq!(h.app.core_path, "user-core");
    let tx = dependency_job(&mut h, true);
    drop(tx);
    h.app.receive_dependencies();
    assert!(h.app.dependency_job.is_none());
    assert!(h.app.dependency_message.contains("disconnected"));
    assert_eq!(h.app.core_path, "user-core");
}

#[test]
fn tray_gui_exit_keeps_the_supervisor_and_core_untouched() {
    let mut h = Harness::new();
    h.app.tray_action("exit-gui", &egui::Context::default());
    assert!(h.app.exit);
    assert!(!h.app.shutdown_requested);
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
}

#[test]
fn tray_quit_waits_for_an_inflight_write_and_exits_after_shutdown_succeeds() {
    let mut h = Harness::new();
    let save = h.send(Command::SaveSettings {
        settings: h.app.settings.clone(),
    });
    let context = egui::Context::default();
    h.app.tray_action("quit", &context);
    h.app.advance_shutdown();
    assert!(h.app.shutdown_requested);
    assert!(!h.app.exit);
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
    h.reply(save, serde_json::json!({"saved":true}));
    h.app.advance_shutdown();
    let shutdown = h
        .requests
        .try_iter()
        .find(|r| matches!(r.command, Command::Shutdown))
        .unwrap();
    h.app.tray_action("quit", &context);
    h.app.tray_action("start", &context);
    h.app.advance_shutdown();
    assert!(!h.app.exit);
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
    h.reply(shutdown, serde_json::json!({"stopped":true}));
    assert!(h.app.exit);
}

#[test]
fn tray_shutdown_failure_keeps_the_gui_and_request_identity_without_retrying() {
    let mut h = Harness::new();
    h.app.tray_action("quit", &egui::Context::default());
    h.app.advance_shutdown();
    let request = h
        .requests
        .try_iter()
        .find(|r| matches!(r.command, Command::Shutdown))
        .unwrap();
    let request_id = request.request_id;
    h.responses
        .send((
            request.command,
            Response {
                request_id: request_id.clone(),
                result: Err(Error::new("IPC_TIMEOUT", "Query the shutdown result")),
            },
        ))
        .unwrap();
    h.app.receive();
    h.app.advance_shutdown();
    assert!(!h.app.exit);
    assert!(!h.app.shutdown_requested);
    assert_eq!(h.app.failed_request, Some(request_id));
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
}

fn status(generation: Option<Generation>) -> Status {
    Status {
        generation,
        core_state: "Running".into(),
        proxy_state: "Unmanaged".into(),
        runtime_instance_id: Some("core-instance".into()),
        supervisor_epoch: "supervisor-1".into(),
        last_error: None,
        recovery_required: false,
        winhttp: None,
        listener_state: "Ready".into(),
        pac_state: "Stopped".into(),
        auto_connect_notice: None,
    }
}
fn generation(app: &App) -> Generation {
    Generation {
        id: "generation-1".into(),
        transaction_id: "transaction-1".into(),
        request_id: "apply-1".into(),
        candidate: candidate(app, &app.editor),
        proxy_mode: ProxyMode::Direct,
        http_inbound_tag: "http-in".into(),
        running: true,

        pac: None,
    }
}
fn fail_request(h: &mut Harness, request: Request, code: &str) {
    h.responses
        .send((
            request.command,
            Response {
                request_id: request.request_id,
                result: Err(Error::new(code, "test failure")),
            },
        ))
        .unwrap();
    h.app.receive();
}

#[test]
fn successful_runtime_operations_refresh_request_errors_and_keep_event_history() {
    for action in ["apply", "proxy", "restart", "stop"] {
        let mut h = Harness::new();
        h.app.ui.settings_loaded = true;
        h.app.ui.profiles_loaded = true;
        let g = generation(&h.app);
        h.app.status = Some(status(Some(g.clone())));
        let failed = h.send(Command::SetProxyMode {
            mode: ProxyMode::Global,
            http_inbound_tag: "http-in".into(),

            pac: None,
        });
        fail_request(&mut h, failed, "PROXY_ENDPOINT_INVALID");
        let poll = h.send(Command::GetStatus);
        let mut before = status(Some(g.clone()));
        before.last_error = Some(Error::new("PROXY_ENDPOINT_INVALID", "Old endpoint"));
        h.reply(poll, before);
        assert!(h.app.ui.failure.is_some());
        let logs = h.send(Command::GetLogs);
        h.reply(logs, Vec::<String>::new());
        assert!(
            h.app.ui.failure.is_some(),
            "reads must not dismiss the error"
        );

        let command = match action {
            "apply" => {
                let c = candidate(&h.app, &h.app.editor);
                h.app.accept_candidate(c.clone());
                apply(&c)
            }
            "proxy" => Command::SetProxyMode {
                mode: ProxyMode::Global,
                http_inbound_tag: "http-in".into(),

                pac: None,
            },
            "restart" => Command::RestartCore,
            _ => Command::StopCore,
        };
        let request = h.send(command);
        h.reply(request, &g);
        assert!(h.app.ui.failure.is_none(), "{action}");
        assert!(h.app.failed_request.is_none(), "{action}");
        let poll = h.send(Command::GetStatus);
        h.reply(poll, status(Some(g)));
        assert!(h.app.status.as_ref().unwrap().last_error.is_none());
        assert!(h.app.log_rows().iter().any(|(level, message)| {
            level == "错误" && message.contains("PROXY_ENDPOINT_INVALID")
        }));
    }
}

#[test]
fn successful_runtime_request_preserves_local_persistence_failure() {
    for action in ["apply", "proxy"] {
        let mut h = Harness::new();
        let g = generation(&h.app);
        h.app.status = Some(status(Some(g.clone())));
        let command = if action == "apply" {
            let c = candidate(&h.app, &h.app.editor);
            h.app.accept_candidate(c.clone());
            apply(&c)
        } else {
            Command::SetProxyMode {
                mode: ProxyMode::Global,
                http_inbound_tag: "http-in".into(),

                pac: None,
            }
        };
        let request = h.send(command);
        h.app.fail(
            Error::new("IO_ERROR", "Local draft write failed"),
            String::new(),
        );
        h.reply(request, &g);
        assert!(h.app.ui.failure.is_some(), "{action}");
    }
}

#[test]
fn profile_drafts_survive_switching_without_changing_the_runtime() {
    let mut h = Harness::new();
    let g = generation(&h.app);
    h.app.status = Some(status(Some(g.clone())));
    h.app.editor = "{\"draft\":1}".into();
    let other = h.send(Command::GetProfile {
        profile_id: "other".into(),
    });
    h.reply(other, profile("other", "{}"));
    h.app.editor = "{\"other\":2}".into();
    let first = h.send(Command::GetProfile {
        profile_id: "p".into(),
    });
    h.reply(first, profile("p", "{}"));
    assert_eq!(h.app.editor, "{\"draft\":1}");
    assert_eq!(h.app.saved_json, "{}");
    assert_eq!(
        h.app
            .status
            .as_ref()
            .unwrap()
            .generation
            .as_ref()
            .unwrap()
            .id,
        g.id
    );
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
}

#[test]
fn persisted_draft_revision_conflict_requires_an_explicit_decision() {
    let root = tempfile::tempdir().unwrap();
    crate::workspace::Draft {
        number_inputs: Default::default(),
        base_revision: hash(b"old"),
        json: "{\"recovered\":true}".into(),
    }
    .save(root.path(), "p")
    .unwrap();
    let mut h = Harness::empty();
    h.app.root = root.path().into();
    h.app.ui.persistence = true;
    h.app.load(profile("p", "{}"));
    assert_eq!(h.app.editor, "{\"recovered\":true}");
    assert!(h.app.has_draft_conflict());
    h.app.validate_draft();
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
    h.app.stash_draft();
    assert_eq!(
        crate::workspace::Draft::load(root.path(), "p")
            .unwrap()
            .unwrap()
            .base_revision,
        hash(b"old")
    );
}

#[test]
fn editing_and_reverting_invalidates_an_inflight_validation() {
    let mut h = Harness::new();
    let request = h.validate();
    let c = candidate(&h.app, "{}");
    h.app.editor = "{\"temporary\":true}".into();
    h.app.draft_edited();
    h.app.editor = "{}".into();
    h.app.draft_edited();
    h.reply(request, c);
    assert!(!h.app.candidate_matches());
    assert!(!h.app.confirm_apply);
}

#[test]
fn disconnected_backend_preserves_last_status_and_blocks_mutations() {
    let mut h = Harness::new();
    let g = generation(&h.app);
    h.app.status = Some(status(Some(g.clone())));
    let poll = h.send(Command::GetStatus);
    fail_request(&mut h, poll, "CONNECTION_LOST");
    assert!(!h.app.ui.connected);
    assert_eq!(
        h.app
            .status
            .as_ref()
            .unwrap()
            .generation
            .as_ref()
            .unwrap()
            .id,
        g.id
    );
    h.app.send(Command::StopCore);
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
    let poll = h.send(Command::GetStatus);
    h.reply(poll, status(Some(g)));
    assert!(h.app.ui.connected);
    assert!(h.app.message.is_empty());
    assert!(h.app.ui.failure.is_none());
}

#[test]
fn ambiguous_apply_is_queried_by_identity_and_never_resubmitted() {
    let mut h = Harness::new();
    h.app.editor = "{\"applied\":true}".into();
    let c = candidate(&h.app, &h.app.editor);
    h.app.accept_candidate(c.clone());
    let request = h.send(apply(&c));
    let request_id = request.request_id.clone();
    let original_command = request.command.clone();
    fail_request(&mut h, request, "IPC_TIMEOUT");
    assert!(h.app.ui.uncertain.is_some());
    h.app.send(Command::RestartCore);
    h.app.send(Command::StopCore);
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
    let query = h.send(Command::GetOperation {
        request_id: request_id.clone(),
    });
    h.reply(query, serde_json::Value::Null);
    assert!(h.app.ui.uncertain.is_some());
    let query = h.send(Command::GetOperation {
        request_id: request_id.clone(),
    });
    h.reply(query,serde_json::json!({"request":{"request_id":request_id,"command":original_command},"response":Response{request_id:request_id.clone(),result:Ok(serde_json::to_value(generation(&h.app)).unwrap())}}));
    assert!(h.app.ui.uncertain.is_none());
    assert_eq!(h.app.saved_json, "{\"applied\":true}");
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
}

#[test]
fn apply_acknowledgement_preserves_a_new_editor_selection() {
    let mut h = Harness::new();
    h.app.editor = "{\"new\":true}".into();
    let c = candidate(&h.app, &h.app.editor);
    h.app.accept_candidate(c.clone());
    let apply = h.send(apply(&c));
    let read = h.send(Command::GetProfile {
        profile_id: "other".into(),
    });
    let g = generation(&h.app);
    h.reply(apply, g);
    h.reply(read, profile("other", "{}"));
    assert_eq!(h.app.profile_id, "other");
    assert_eq!(h.app.selected_profile, "other");
    assert!(
        !h.requests
            .try_iter()
            .any(|r| matches!(r.command,Command::GetProfile{profile_id} if profile_id=="p"))
    );
}

#[test]
fn runtime_reads_and_route_results_cannot_cross_generation_boundaries() {
    let mut h = Harness::new();
    let g = generation(&h.app);
    h.app.status = Some(status(Some(g.clone())));
    let read = h.send(Command::GetRuntimeConfig {
        generation_id: g.id.clone(),
    });
    let route = h.send(Command::TestRunningRoute {
        generation_id: g.id.clone(),
        context: RouteContext {
            host: "example.com".into(),
            ..Default::default()
        },
    });
    let mut new = g.clone();
    new.id = "generation-2".into();
    h.app.status = Some(status(Some(new)));
    h.reply(
        read,
        RuntimeConfig {
            generation_id: g.id,
            json: "{}".into(),
        },
    );
    h.reply(route, serde_json::json!({"expected_outbound":"wrong"}));
    assert!(h.app.ui.runtime.is_none());
    assert!(h.app.route_result.is_empty());
}

#[test]
fn recovery_allows_safe_stop_but_blocks_normal_writes() {
    let mut h = Harness::new();
    let mut s = status(None);
    s.recovery_required = true;
    h.app.status = Some(s);
    h.app.send(Command::RestartCore);
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
    let request = h.send(Command::StopCore);
    assert!(matches!(request.command, Command::StopCore));
}

#[test]
fn proxy_conflict_recovery_allows_resolve_and_save_settings_while_blocking_core_writes() {
    let mut h = Harness::new();
    let mut s = status(None);
    s.recovery_required = true;
    s.proxy_state = "Conflict".into();
    s.last_error = Some(Error::new("PROXY_LEASE_CONFLICT", "External change"));
    h.app.status = Some(s);

    h.app.send(Command::RestartCore);
    h.app.send(Command::SetProxyMode {
        mode: ProxyMode::Global,
        http_inbound_tag: "http-in".into(),
        pac: None,
    });
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));

    let resolve = h.send(Command::ResolveProxyConflict);
    assert!(matches!(resolve.command, Command::ResolveProxyConflict));
    h.app.busy = false;

    let mut settings = h.app.settings.clone();
    settings.start_on_logon = false;
    let save = h.send(Command::SaveSettings { settings });
    assert!(matches!(save.command, Command::SaveSettings { .. }));
    h.app.busy = false;

    if let Some(status) = h.app.status.as_mut() {
        status.last_error = Some(Error::new("STARTUP_CONFLICT", "External task changed"));
    }
    h.app.send(Command::SaveSettings {
        settings: h.app.settings.clone(),
    });
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
}

#[test]
fn deleting_a_profile_does_not_clear_a_different_editors_draft() {
    let mut h = Harness::new();
    let deletion = h.send(Command::DeleteProfile {
        profile_id: "p".into(),
        expected_revision: h.app.revision.clone(),
    });
    let other = h.send(Command::GetProfile {
        profile_id: "other".into(),
    });
    h.reply(other, profile("other", "{}"));
    h.app.editor = "{\"preserved\":true}".into();
    h.reply(deletion, serde_json::json!({"deleted":true}));
    assert_eq!(h.app.profile_id, "other");
    assert_eq!(h.app.editor, "{\"preserved\":true}");
}

#[test]
fn corrupted_draft_and_workspace_files_are_not_overwritten_implicitly() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    std::fs::create_dir_all(config.join("editor-drafts")).unwrap();
    std::fs::write(config.join("gui.json"), "broken-metadata").unwrap();
    std::fs::write(config.join("editor-drafts/p.json"), "broken-draft").unwrap();
    let mut h = Harness::empty();
    h.app.root = root.path().into();
    h.app.ui.persistence = true;
    h.app.ui.workspace_valid = false;
    h.app.load(profile("p", "{}"));
    h.app.editor = "{\"edited\":true}".into();
    h.app.stash_draft();
    h.app.save_workspace();
    assert_eq!(
        std::fs::read_to_string(config.join("gui.json")).unwrap(),
        "broken-metadata"
    );
    assert_eq!(
        std::fs::read_to_string(config.join("editor-drafts/p.json")).unwrap(),
        "broken-draft"
    );
}

#[test]
fn incomplete_import_opens_field_advice_without_replacing_runtime_or_source() {
    let mut h = Harness::new();
    let g = generation(&h.app);
    h.app.status = Some(status(Some(g.clone())));
    h.app.fail(
        Error::new("STATIC_VALIDATION_FAILED", "old import error"),
        String::new(),
    );
    let json = "{\n \"outbounds\": [{\"tag\":\"pending\",\"extra\":true}]\n}";
    let request = h.send(Command::CreateProfile {
        profile_id: "partial".into(),
        json: json.into(),
    });
    h.reply(request, profile("partial", json));
    assert_eq!(h.app.editor, json);
    assert_eq!(h.app.saved_json, json);
    assert!(h.app.ui.page == ui::Page::Profiles);
    assert!(h.app.ui.failure.is_none());
    assert_eq!(
        h.app
            .status
            .as_ref()
            .unwrap()
            .generation
            .as_ref()
            .unwrap()
            .id,
        g.id
    );
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
}

#[test]
fn initial_settings_read_and_status_retry_preserve_the_user_draft() {
    for change in ["core", "assets", "login", "connect", "reverted"] {
        let mut h = Harness::new();
        let read = h.send(Command::GetSettings);
        match change {
            "core" => h.app.core_path = "chosen-core".into(),
            "assets" => h.app.asset_root = "chosen-assets".into(),
            "login" => h.app.settings.start_on_logon = true,
            "connect" => h.app.settings.connect_on_start = true,
            _ => {
                h.app.settings.start_on_logon = true;
                h.app.settings_edited();
                h.app.settings.start_on_logon = false;
            }
        }
        h.app.settings_edited();
        let expected = h.app.settings_context();
        h.reply(read, stored_settings());
        assert!(h.app.ui.settings_loaded, "{change}");
        assert_eq!(
            h.app.ui.saved_settings.as_ref().unwrap().core_path,
            stored_settings().core_path
        );
        let poll = h.send(Command::GetStatus);
        h.reply(poll, status(None));
        assert!(
            !h.requests
                .try_iter()
                .any(|r| matches!(r.command, Command::GetSettings))
        );
        assert!(h.app.settings_context() == expected, "{change}");
        // Even an explicit later read must preserve the unsaved draft.
        let retry = h.send(Command::GetSettings);
        let expected = h.app.settings_context();
        h.reply(retry, stored_settings());
        assert!(h.app.settings_context() == expected, "{change}");
        h.app.settings.core_path = h.app.core_path.clone().into();
        let save = h.send(Command::SaveSettings {
            settings: h.app.settings.clone(),
        });
        if change == "assets" {
            let saved_request = save.clone();
            fail_request(&mut h, save, "IPC_TIMEOUT");
            let query = h.send(Command::GetOperation {
                request_id: saved_request.request_id.clone(),
            });
            h.reply(
                query,
                serde_json::json!({"request": saved_request, "response": {
                    "request_id": saved_request.request_id, "result": {"Ok": {"saved":true}}
                }}),
            );
        } else {
            h.reply(save, serde_json::json!({"saved":true}));
        }
        assert!(!h.app.ui.settings_dirty);
    }
}

#[test]
fn automatic_settings_retry_after_failure_preserves_edits_and_save_protects_new_edits() {
    let mut h = Harness::new();
    let read = h.send(Command::GetSettings);
    h.responses
        .send((
            read.command,
            Response {
                request_id: read.request_id,
                result: Err(Error::new("CONNECTION_LOST", "disconnected")),
            },
        ))
        .unwrap();
    h.app.receive();
    h.app.core_path = "chosen-core".into();
    h.app.settings_edited();
    let poll = h.send(Command::GetStatus);
    h.reply(poll, status(None));
    let retry = h
        .requests
        .try_iter()
        .find(|r| matches!(r.command, Command::GetSettings))
        .unwrap();
    h.reply(retry, stored_settings());
    assert_eq!(h.app.core_path, "chosen-core");
    assert!(h.app.ui.settings_loaded);
    h.app.settings.core_path = h.app.core_path.clone().into();
    let save = h.send(Command::SaveSettings {
        settings: h.app.settings.clone(),
    });
    h.app.core_path = "newer-core".into();
    h.app.settings_edited();
    h.reply(save, serde_json::json!({"saved":true}));
    assert!(h.app.ui.settings_dirty);
    let read = h.send(Command::GetSettings);
    h.reply(read, stored_settings());
    assert_eq!(h.app.core_path, "newer-core");
}

#[test]
fn background_apply_persists_the_new_draft_base_for_response_and_result_query() {
    for query in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut h = Harness::new();
        h.app.root = root.path().into();
        h.app.ui.persistence = true;
        h.app.editor = "{\"s\":1}".into();
        let applied_json = h.app.editor.clone();
        let c = candidate(&h.app, &applied_json);
        h.app.accept_candidate(c.clone());
        let g = generation(&h.app);
        let applying = h.send(apply(&c));
        h.app.editor = "{\"s\":2}".into();
        h.app.draft_edited();
        let switching = h.send(Command::GetProfile {
            profile_id: "b".into(),
        });
        h.reply(switching, profile("b", "{}"));
        if query {
            h.responses
                .send((
                    applying.command.clone(),
                    Response {
                        request_id: applying.request_id.clone(),
                        result: Err(Error::new("IPC_TIMEOUT", "query original request")),
                    },
                ))
                .unwrap();
            h.app.receive();
            let lookup = h.send(Command::GetOperation {
                request_id: applying.request_id.clone(),
            });
            h.reply(
                lookup,
                serde_json::json!({"request": applying, "response": {
                    "request_id": applying.request_id, "result": {"Ok": g}
                }}),
            );
        } else {
            h.reply(applying, g);
        }
        let disk = crate::workspace::Draft::load(root.path(), "p")
            .unwrap()
            .unwrap();
        assert_eq!(
            disk.base_revision,
            hash(applied_json.as_bytes()),
            "query={query}"
        );
        assert_eq!(disk.json, "{\"s\":2}");
        let mut reopened = Harness::empty();
        reopened.app.root = root.path().into();
        reopened.app.ui.persistence = true;
        reopened.app.load(profile("p", &applied_json));
        assert_eq!(reopened.app.editor, "{\"s\":2}");
        assert!(!reopened.app.has_draft_conflict(), "query={query}");
    }
}

#[test]
fn unfinished_numbers_survive_failed_save_switch_restart_and_background_apply() {
    use crate::workspace::{Draft, NumberInput};
    let root = tempfile::tempdir().unwrap();
    let mut h = Harness::empty();
    h.app.root = root.path().into();
    h.app.ui.persistence = true;
    let json = r#"{"outbounds":[{"settings":{"port":0}}]}"#;
    h.app.load(profile("p", json));
    let path = "/outbounds/0/settings/port";
    h.app.ui.number_inputs.insert(
        path.into(),
        NumberInput {
            source: "0".into(),
            text: "-".into(),
        },
    );
    h.app.draft_edited();
    assert!(h.app.has_draft_edits());
    h.app.validate_draft();
    assert!(!h.requests.try_iter().any(|r| r.command.is_write()));
    std::fs::create_dir_all(root.path().join("config")).unwrap();
    std::fs::write(root.path().join("config/editor-drafts"), b"blocked").unwrap();
    h.app.request_exit(crate::app::ui::ExitKind::Gui);
    assert!(!h.app.exit);
    std::fs::remove_file(root.path().join("config/editor-drafts")).unwrap();
    assert!(h.app.stash_draft());
    h.app.load(profile("b", "{}"));
    // Applying the same JSON in the background must not remove a pending number draft.
    h.app.applied_draft("p", Some(json));
    let disk = Draft::load(root.path(), "p").unwrap().unwrap();
    assert_eq!(disk.number_inputs[path].text, "-");
    let mut reopened = Harness::empty();
    reopened.app.root = root.path().into();
    reopened.app.ui.persistence = true;
    reopened.app.load(profile("p", json));
    assert_eq!(reopened.app.ui.number_inputs[path].text, "-");
    assert!(!reopened.app.has_draft_conflict());
    reopened.app.editor = json.replace("port\":0", "port\":-2");
    reopened.app.draft_edited();
    assert!(reopened.app.ui.number_inputs.is_empty());
    assert!(reopened.app.stash_draft());
    assert!(
        Draft::load(root.path(), "p")
            .unwrap()
            .unwrap()
            .number_inputs
            .is_empty()
    );
}

#[test]
fn tray_profile_selection_opens_editor_without_applying_and_preserves_draft_confirmation() {
    let mut h = Harness::new();
    h.app.profiles = vec![
        xray_model::ProfileSummary {
            id: "p".into(),
            revision: hash(b"{}"),
        },
        xray_model::ProfileSummary {
            id: "b".into(),
            revision: hash(b"{}"),
        },
    ];
    let ctx = eframe::egui::Context::default();
    h.app.open_profile_from_tray("b", &ctx);
    let requests: Vec<_> = h.requests.try_iter().collect();
    assert!(
        requests
            .iter()
            .any(|r| matches!(&r.command, Command::GetProfile{profile_id} if profile_id=="b"))
    );
    assert!(!requests.iter().any(|r| r.command.is_write()));
    let mut h = Harness::new();
    h.app.profiles = vec![xray_model::ProfileSummary {
        id: "b".into(),
        revision: hash(b"{}"),
    }];
    h.app.editor = "{\"draft\":true}".into();
    h.app.draft_edited();
    h.app.open_profile_from_tray("b", &ctx);
    assert!(
        !h.requests
            .try_iter()
            .any(|r| matches!(r.command, Command::GetProfile { .. }) || r.command.is_write())
    );
    assert_eq!(h.app.editor, "{\"draft\":true}");
}

#[test]
fn background_hide_waits_for_first_eframe_show_then_hides_and_uncloaks() {
    use eframe::egui::{ViewportCommand, ViewportId};

    let mut h = Harness::new();
    h.app.ui.background_hide = crate::app::ui::BackgroundHide::HideAfterFirstFrame;
    let ctx = eframe::egui::Context::default();

    let first = ctx.run(Default::default(), |ctx| {
        assert_eq!(ctx.cumulative_pass_nr(), 0);
        h.app.advance_background_hide(ctx);
    });
    assert_eq!(
        h.app.ui.background_hide,
        crate::app::ui::BackgroundHide::HideAfterFirstFrame
    );
    assert!(
        !first.viewport_output[&ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::Visible(false))
    );

    let second = ctx.run(Default::default(), |ctx| {
        assert_eq!(ctx.cumulative_pass_nr(), 1);
        h.app.advance_background_hide(ctx);
    });
    assert_eq!(
        h.app.ui.background_hide,
        crate::app::ui::BackgroundHide::UncloakAfterHide
    );
    assert!(
        second.viewport_output[&ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::Visible(false))
    );

    let third = ctx.run(Default::default(), |ctx| {
        h.app.advance_background_hide(ctx);
    });
    assert_eq!(
        h.app.ui.background_hide,
        crate::app::ui::BackgroundHide::None
    );
    assert!(
        !third.viewport_output[&ViewportId::ROOT]
            .commands
            .contains(&ViewportCommand::Visible(true))
    );
}

#[test]
fn tray_open_cancels_pending_background_hide_and_reveals_window() {
    use eframe::egui::{ViewportCommand, ViewportId};

    let mut h = Harness::new();
    let ctx = eframe::egui::Context::default();
    let _ = ctx.run(Default::default(), |_| {});
    h.app.ui.background_hide = crate::app::ui::BackgroundHide::HideAfterFirstFrame;

    let output = ctx.run(Default::default(), |ctx| {
        h.app.tray_action("open", ctx);
        h.app.advance_background_hide(ctx);
    });
    assert_eq!(
        h.app.ui.background_hide,
        crate::app::ui::BackgroundHide::None
    );
    let commands = &output.viewport_output[&ViewportId::ROOT].commands;
    assert!(commands.contains(&ViewportCommand::Visible(true)));
    assert!(commands.contains(&ViewportCommand::Focus));
    assert!(!commands.contains(&ViewportCommand::Visible(false)));
}

#[test]
fn ensure_supervisor_ready_waits_for_live_supervisor_and_fails_fast_on_exit() {
    struct MockSpawn {
        exited: bool,
    }
    impl SpawnHandle for MockSpawn {
        fn has_exited(&mut self) -> bool {
            self.exited
        }
    }

    let mut probes = 0usize;
    let mut spawns = 0usize;
    let ready = ensure_supervisor_ready(
        || {
            probes += 1;
            probes >= 4
        },
        || {
            spawns += 1;
            Some(Box::new(MockSpawn { exited: false }))
        },
        Duration::from_millis(500),
        Duration::from_millis(50),
        Duration::from_millis(5),
    );
    assert!(ready);
    assert_eq!(spawns, 1);

    let failed = ensure_supervisor_ready(
        || false,
        || Some(Box::new(MockSpawn { exited: true })),
        Duration::from_secs(5),
        Duration::from_millis(20),
        Duration::from_millis(5),
    );
    assert!(!failed);
}

#[test]
fn connection_lost_advice_distinguishes_read_startup_polls_from_writes() {
    let mut h = Harness::new();
    h.app.fail_command(
        &Command::GetStatus,
        Error::new("CONNECTION_LOST", "os error 2"),
        "req-read".into(),
    );
    assert_eq!(
        h.app.message,
        "后台服务尚未就绪或连接已断开，请稍候或点击刷新状态。"
    );

    h.app.fail_command(
        &Command::RestartCore,
        Error::new("CONNECTION_LOST", "os error 2"),
        "req-write".into(),
    );
    assert_eq!(
        h.app.message,
        "后台响应中断，请先查询原操作结果，再进行其他变更。"
    );
}
