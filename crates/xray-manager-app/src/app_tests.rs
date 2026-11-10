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
        let app = App::with_channels(PathBuf::from("isolated-gui-test"), tx, rx);
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
        },
    }
}
fn apply(c: &Candidate) -> Command {
    Command::ApplyProfile {
        candidate_id: c.id.clone(),
        candidate_digest: c.digest.clone(),
        proxy_mode: ProxyMode::Direct,
        http_inbound_tag: "http-in".into(),
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
    h.reply(
        request,
        serde_json::json!({"json":"{\"historical\":true}","candidate":c}),
    );
    assert_eq!(h.app.editor, "{\"historical\":true}");
    assert!(h.app.candidate_matches());
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
    }
}
fn assert_settings_and_save(h: &mut Harness, expected: &Settings) {
    assert_eq!(h.app.core_path, expected.core_path.display().to_string());
    assert_eq!(h.app.settings.start_on_logon, expected.start_on_logon);
    assert_eq!(h.app.settings.connect_on_start, expected.connect_on_start);
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
