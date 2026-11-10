use eframe::egui;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
use xray_model::*;

const MAX_PENDING: usize = 32;
#[derive(Clone, PartialEq, Eq)]
struct EditorContext {
    epoch: u64,
    selected: String,
    profile: String,
    revision: String,
    json_hash: String,
    core: String,
    assets: String,
    mode: ProxyMode,
    http_tag: String,
}
impl EditorContext {
    fn same_draft(&self, other: &Self) -> bool {
        self.epoch == other.epoch
            && self.selected == other.selected
            && self.profile == other.profile
            && self.revision == other.revision
            && self.json_hash == other.json_hash
    }
}
#[derive(Clone, PartialEq, Eq)]
struct SettingsContext {
    epoch: u64,
    edit_epoch: u64,
    core: String,
    start_on_logon: bool,
    connect_on_start: bool,
}
struct Pending {
    context: EditorContext,
    settings_context: Option<SettingsContext>,
    poll: Option<&'static str>,
}
struct DependencyJob {
    response: mpsc::Receiver<Result<crate::dependencies::Report>>,
    context: SettingsContext,
    asset_root: String,
    select: bool,
    version: String,
}

pub struct App {
    root: PathBuf,
    tx: mpsc::SyncSender<Request>,
    rx: mpsc::Receiver<(Command, Response)>,
    pending: HashMap<String, Pending>,
    view_epoch: u64,
    selected_profile: String,
    candidate_context: Option<EditorContext>,
    status: Option<Status>,
    profiles: Vec<ProfileSummary>,
    saved_json: String,
    profile_id: String,
    editor: String,
    revision: String,
    candidate: Option<Candidate>,
    core_path: String,
    asset_root: String,
    mode: ProxyMode,
    http_tag: String,
    message: String,
    logs: Vec<String>,
    busy: bool,
    last_poll: Instant,
    tab: usize,
    new_profile: String,
    settings: Settings,
    settings_epoch: u64,
    settings_edit_epoch: u64,
    dependency_job: Option<DependencyJob>,
    dependency_report: crate::dependencies::Report,
    dependency_version: String,
    dependency_message: String,
    dependency_repaint: Option<egui::Context>,
    appearance: crate::preferences::Appearance,
    route_host: String,
    route_inbound: String,
    route_network: String,
    route_result: String,
    confirm_apply: bool,
    exit: bool,
    history: Vec<Generation>,
    native_section: String,
    raw_json: bool,
    failed_request: Option<String>,
    #[cfg(windows)]
    tray: Option<crate::tray::Tray>,
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, root: PathBuf) -> Self {
        let (tx, requests) = mpsc::sync_channel::<Request>(MAX_PENDING);
        let (responses, rx) = mpsc::channel();
        let worker_root = root.clone();
        let context = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            let ping = Request::new(Command::GetStatus, None);
            if runtime
                .block_on(xray_supervisor::ipc::call(&worker_root, &ping))
                .is_err()
            {
                let exe = std::env::current_exe().unwrap();
                let mut cmd = std::process::Command::new(exe);
                cmd.arg("--supervisor").arg("--data-dir").arg(&worker_root);
                #[cfg(windows)]
                {
                    use std::os::windows::process::CommandExt;
                    cmd.creation_flags(0x08000000);
                }
                let _ = cmd.spawn();
                for _ in 0..40 {
                    if runtime
                        .block_on(xray_supervisor::ipc::call(&worker_root, &ping))
                        .is_ok()
                    {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
            while let Ok(request) = requests.recv() {
                let command = request.command.clone();
                let response =
                    match runtime.block_on(xray_supervisor::ipc::call(&worker_root, &request)) {
                        Ok(r) => r,
                        Err(e) => {
                            if matches!(command, Command::GetStatus)
                                && e.code == "IO_ERROR"
                                && let Ok(exe) = std::env::current_exe()
                            {
                                let mut cmd = std::process::Command::new(exe);
                                cmd.arg("--supervisor").arg("--data-dir").arg(&worker_root);
                                #[cfg(windows)]
                                {
                                    use std::os::windows::process::CommandExt;
                                    cmd.creation_flags(0x08000000);
                                }
                                let _ = cmd.spawn();
                            }
                            Response {
                                request_id: request.request_id,
                                result: Err(e),
                            }
                        }
                    };
                if responses.send((command, response)).is_err() {
                    break;
                }
                context.request_repaint();
            }
        });
        let mut app = Self::with_channels(root, tx, rx);
        app.dependency_repaint = Some(cc.egui_ctx.clone());
        match crate::preferences::Appearance::load(&app.root) {
            Ok(appearance) => app.appearance = appearance,
            Err(error) => app.message = format!("Appearance could not be loaded: {error}"),
        }
        app.appearance.apply(&cc.egui_ctx);
        #[cfg(windows)]
        {
            app.tray = crate::tray::Tray::new().ok();
        }
        #[cfg(windows)]
        if app.tray.is_none() {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }
        app.send(Command::GetStatus);
        app.refresh_profiles();
        app.send(Command::GetSettings);
        app
    }
    fn with_channels(
        root: PathBuf,
        tx: mpsc::SyncSender<Request>,
        rx: mpsc::Receiver<(Command, Response)>,
    ) -> Self {
        Self {
            root: root.clone(),
            tx,
            rx,
            pending: HashMap::new(),
            view_epoch: 0,
            selected_profile: String::new(),
            candidate_context: None,
            status: None,
            profiles: vec![],
            saved_json: String::new(),
            profile_id: String::new(),
            editor: String::new(),
            revision: String::new(),
            candidate: None,
            core_path: String::new(),
            asset_root: root.display().to_string(),
            mode: ProxyMode::Direct,
            http_tag: "http-in".into(),
            message: "Connecting to Supervisor…".into(),
            logs: vec![],
            busy: false,
            last_poll: Instant::now(),
            tab: 0,
            new_profile: "default".into(),
            settings: Settings::default(),
            settings_epoch: 0,
            settings_edit_epoch: 0,
            dependency_job: None,
            dependency_report: crate::dependencies::Report::default(),
            dependency_version: crate::dependencies::DEFAULT_VERSION.into(),
            dependency_message: String::new(),
            dependency_repaint: None,
            appearance: crate::preferences::Appearance::default(),
            route_host: "example.com".into(),
            route_inbound: "http-in".into(),
            route_network: "tcp".into(),
            route_result: String::new(),
            confirm_apply: false,
            exit: false,
            history: vec![],
            native_section: "inbounds".into(),
            raw_json: true,
            failed_request: None,
            #[cfg(windows)]
            tray: None,
        }
    }
    fn context(&self) -> EditorContext {
        EditorContext {
            epoch: self.view_epoch,
            selected: self.selected_profile.clone(),
            profile: self.profile_id.clone(),
            revision: self.revision.clone(),
            json_hash: hash(self.editor.as_bytes()),
            core: self.core_path.clone(),
            assets: self.asset_root.clone(),
            mode: self.mode,
            http_tag: self.http_tag.clone(),
        }
    }
    fn invalidate_candidate(&mut self) {
        self.candidate = None;
        self.candidate_context = None;
        self.confirm_apply = false;
    }
    fn settings_context(&self) -> SettingsContext {
        SettingsContext {
            epoch: self.settings_epoch,
            edit_epoch: self.settings_edit_epoch,
            core: self.core_path.clone(),
            start_on_logon: self.settings.start_on_logon,
            connect_on_start: self.settings.connect_on_start,
        }
    }
    fn settings_edited(&mut self) {
        self.settings_edit_epoch = self.settings_edit_epoch.wrapping_add(1);
    }
    fn candidate_matches(&self) -> bool {
        self.candidate.as_ref().is_some_and(|c| {
            c.profile_id == self.profile_id
                && c.base_revision == self.revision
                && c.revision == hash(self.editor.as_bytes())
        }) && self
            .candidate_context
            .as_ref()
            .is_some_and(|context| *context == self.context())
    }
    fn accept_candidate(&mut self, c: Candidate) {
        if c.profile_id == self.profile_id
            && c.base_revision == self.revision
            && c.revision == hash(self.editor.as_bytes())
        {
            self.candidate_context = Some(self.context());
            self.candidate = Some(c);
            self.confirm_apply = true;
        } else {
            self.invalidate_candidate();
            self.message = "Candidate does not match the current draft; validate again".into();
        }
    }
    fn refresh_profiles(&mut self) {
        self.send(Command::ListProfileSummaries {
            after: None,
            limit: 100,
        });
    }
    fn send(&mut self, command: Command) {
        if command.is_write() && self.busy {
            self.message = "Wait for the current operation to finish".into();
            return;
        }
        if let Command::ApplyProfile {
            candidate_id,
            candidate_digest,
            proxy_mode,
            http_inbound_tag,
        } = &command
        {
            let matches = self.candidate_matches()
                && self
                    .candidate
                    .as_ref()
                    .is_some_and(|c| c.id == *candidate_id && c.digest == *candidate_digest)
                && *proxy_mode == self.mode
                && *http_inbound_tag == self.http_tag;
            if !matches {
                self.invalidate_candidate();
                self.message = "Draft or validation context changed; validate again".into();
                return;
            }
        }
        if matches!(command, Command::ValidateDraft { .. })
            && self.selected_profile != self.profile_id
        {
            self.message = "Wait for the selected profile to load".into();
            return;
        }
        let poll = match command {
            Command::GetStatus => Some("status"),
            Command::GetLogs => Some("logs"),
            _ => None,
        };
        if poll.is_some() && self.pending.values().any(|p| p.poll == poll) {
            return;
        }
        if self.pending.len() >= MAX_PENDING {
            self.message = "Request queue is full; wait for pending operations".into();
            return;
        }
        if let Command::GetProfile { profile_id } = &command {
            self.selected_profile = profile_id.clone();
            self.view_epoch = self.view_epoch.wrapping_add(1);
            self.invalidate_candidate();
        }
        let context = self.context();
        let write = command.is_write();
        let settings_read = matches!(command, Command::GetSettings);
        let settings_write = matches!(command, Command::SaveSettings { .. });
        let request = Request::new(
            command,
            self.status
                .as_ref()
                .and_then(|s| s.generation.as_ref().map(|g| g.id.clone())),
        );
        let request_id = request.request_id.clone();
        match self.tx.try_send(request) {
            Ok(()) => {
                if settings_read {
                    self.settings_epoch = self.settings_epoch.wrapping_add(1);
                }
                if settings_write {
                    self.settings_edited();
                }
                let settings_context = settings_read.then(|| self.settings_context());
                self.pending.insert(
                    request_id,
                    Pending {
                        context,
                        settings_context,
                        poll,
                    },
                );
                if write {
                    self.busy = true;
                }
            }
            Err(error) => self.message = format!("Request was not sent: {error}"),
        }
    }
    fn receive(&mut self) {
        while let Ok((command, response)) = self.rx.try_recv() {
            let Some(pending) = self.pending.remove(&response.request_id) else {
                continue;
            };
            if command.is_write() {
                self.busy = false;
            }
            if pending
                .settings_context
                .as_ref()
                .is_some_and(|context| *context != self.settings_context())
            {
                continue;
            }
            let context_matches = match command {
                // Reading native JSON/history does not depend on the Core selection.
                Command::GetProfile { .. } | Command::ListHistory { .. } => {
                    pending.context.same_draft(&self.context())
                }
                Command::ValidateDraft { .. } | Command::StageHistory { .. } => {
                    pending.context == self.context()
                }
                _ => true,
            };
            if !context_matches {
                if matches!(command, Command::GetProfile { .. })
                    && pending.context.epoch == self.view_epoch
                {
                    // The latest load was superseded by editing. Keep the visible selection
                    // attached to the draft that was preserved, rather than a discarded load.
                    self.selected_profile = self.profile_id.clone();
                    self.message =
                        "Draft changed while loading; current edits were preserved".into();
                }
                if command.is_write() {
                    self.message =
                        "Draft or selection changed; validate the current draft again".into();
                }
                continue;
            }
            match response.result {
                Err(error) => {
                    self.message = format!("{} (request ID: {})", error, response.request_id);
                    if command.is_write() {
                        self.failed_request = Some(response.request_id);
                        self.send(Command::GetStatus);
                    }
                    if matches!(command, Command::GetStatus) {
                        self.status = None;
                    }
                }
                Ok(value) => match command {
                    Command::GetStatus => {
                        if let Ok(status) = serde_json::from_value::<Status>(value) {
                            self.status = Some(status);
                        }
                    }
                    Command::ListProfileSummaries { after, .. } => {
                        if let Ok(page) = serde_json::from_value::<ProfilePage>(value) {
                            if after.is_none() {
                                self.profiles.clear();
                            }
                            self.profiles.extend(page.items);
                            if after.is_none()
                                && self.profile_id.is_empty()
                                && let Some(p) = self.profiles.first()
                            {
                                self.send(Command::GetProfile {
                                    profile_id: p.id.clone(),
                                });
                            }
                            if let Some(after) = page.next_after {
                                self.send(Command::ListProfileSummaries {
                                    after: Some(after),
                                    limit: 100,
                                });
                            }
                        }
                    }
                    Command::GetProfile { .. } => {
                        if let Ok(p) = serde_json::from_value(value) {
                            self.load(p);
                        }
                    }
                    Command::ListHistory { .. } => {
                        self.history = serde_json::from_value(value).unwrap_or_default()
                    }
                    Command::StageHistory { .. } => {
                        if let (Some(json), Some(c)) = (
                            value.get("json").and_then(|v| v.as_str()),
                            value.get("candidate").cloned(),
                        ) {
                            self.editor = json.into();
                            if let Ok(c) = serde_json::from_value(c) {
                                self.accept_candidate(c);
                            }
                            self.message =
                                "Historical snapshot staged; review the diff before applying"
                                    .into();
                        }
                    }
                    Command::GetSettings => {
                        if let Ok(s) = serde_json::from_value::<Settings>(value) {
                            self.core_path = s.core_path.display().to_string();
                            self.settings = s;
                            if self.core_path.is_empty() && self.editor.is_empty() {
                                self.tab = 1;
                            }
                            self.start_dependencies("Status", "", None);
                        }
                    }
                    Command::ValidateDraft { .. } => {
                        if let Ok(c) = serde_json::from_value::<Candidate>(value) {
                            self.message = format!("Core validation passed: {}", c.core.version);
                            self.accept_candidate(c);
                        }
                    }
                    Command::GetOperation { .. } => {
                        self.message = serde_json::to_string_pretty(&value).unwrap_or_default();
                    }
                    Command::DeleteProfile { .. } => {
                        self.profile_id.clear();
                        self.editor.clear();
                        self.saved_json.clear();
                        self.revision.clear();
                        self.selected_profile.clear();
                        self.view_epoch = self.view_epoch.wrapping_add(1);
                        self.invalidate_candidate();
                        self.refresh_profiles();
                    }
                    Command::GetLogs => {
                        self.logs = serde_json::from_value(value).unwrap_or_default()
                    }
                    Command::TestRoute { .. } => {
                        self.route_result = serde_json::to_string_pretty(&value).unwrap_or_default()
                    }
                    Command::Shutdown => self.exit = true,
                    Command::CreateProfile { .. } => {
                        if let Ok(profile) = serde_json::from_value(value) {
                            self.load(profile);
                        }
                        self.message = "Profile created".into();
                        self.refresh_profiles();
                    }
                    Command::ApplyProfile { .. } => {
                        self.message = "Configuration applied".into();
                        self.invalidate_candidate();
                        self.send(Command::GetStatus);
                        self.refresh_profiles();
                        self.send(Command::GetProfile {
                            profile_id: self.profile_id.clone(),
                        });
                    }
                    _ => {
                        self.message = "Operation completed".into();
                        self.send(Command::GetStatus);
                    }
                },
            }
        }
    }
    fn load(&mut self, p: Profile) {
        self.view_epoch = self.view_epoch.wrapping_add(1);
        self.selected_profile = p.id.clone();
        self.profile_id = p.id;
        self.saved_json = p.json.clone();
        self.editor = p.json;
        self.revision = p.revision;
        self.invalidate_candidate();
        self.history.clear();
        self.send(Command::ListHistory {
            profile_id: self.profile_id.clone(),
        });
    }
    fn editor(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Profile");
            let mut selected = self.selected_profile.clone();
            egui::ComboBox::from_id_salt("profiles")
                .selected_text(&selected)
                .show_ui(ui, |ui| {
                    for p in &self.profiles {
                        ui.selectable_value(&mut selected, p.id.clone(), &p.id);
                    }
                });
            if selected != self.selected_profile {
                self.send(Command::GetProfile {
                    profile_id: selected,
                });
            }
            if ui.button("Reload").clicked() && !self.profile_id.is_empty() {
                self.send(Command::GetProfile {
                    profile_id: self.profile_id.clone(),
                });
            }
            if ui.button("Export JSON…").clicked() {
                #[cfg(windows)]
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("config.json")
                    .save_file()
                {
                    self.message = match std::fs::write(path, &self.editor) {
                        Ok(_) => "Exported; file contains credentials".into(),
                        Err(e) => e.to_string(),
                    };
                }
            }
        });
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.new_profile);
            if ui.button("Create").clicked() {
                self.send(Command::CreateProfile {
                    profile_id: self.new_profile.clone(),
                    json: default_config(),
                });
            }
            if ui.button("Import JSON…").clicked() {
                #[cfg(windows)]
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .pick_file()
                {
                    match std::fs::read_to_string(&path) {
                        Ok(json) => {
                            self.asset_root =
                                path.parent().unwrap_or(&self.root).display().to_string();
                            self.send(Command::CreateProfile {
                                profile_id: self.new_profile.clone(),
                                json,
                            });
                        }
                        Err(e) => self.message = e.to_string(),
                    }
                }
            }
            if ui.button("Duplicate").clicked() {
                self.send(Command::CreateProfile {
                    profile_id: self.new_profile.clone(),
                    json: self.editor.clone(),
                });
            }
            if ui.button("Delete").clicked() && !self.profile_id.is_empty() {
                self.send(Command::DeleteProfile {
                    profile_id: self.profile_id.clone(),
                    expected_revision: self.revision.clone(),
                });
            }
        });
        ui.horizontal(|ui| {
            ui.label("HTTP inbound tag");
            ui.text_edit_singleline(&mut self.http_tag);
            egui::ComboBox::from_label("Proxy mode")
                .selected_text(format!("{:?}", self.mode))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.mode,
                        ProxyMode::Direct,
                        "Direct (release ownership)",
                    );
                    ui.selectable_value(&mut self.mode, ProxyMode::Global, "Global");
                });
        });
        ui.label("Native JSON — unknown fields and rule order are preserved. Changes require validation and explicit apply.");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.raw_json, true, "Raw JSON");
            ui.selectable_value(&mut self.raw_json, false, "Structured fields");
            if !self.raw_json {
                for section in ["inbounds", "outbounds", "rules"] {
                    ui.selectable_value(&mut self.native_section, section.into(), section);
                }
            }
        });
        let changed = egui::ScrollArea::vertical()
            .max_height(350.0)
            .show(ui, |ui| {
                if self.raw_json {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.editor)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(20),
                    )
                    .changed()
                } else if let Ok(mut value) = serde_json::from_str(&self.editor) {
                    let changed = crate::native_editor::edit(ui, &mut value, &self.native_section);
                    if changed {
                        self.editor = serde_json::to_string_pretty(&value).unwrap();
                    }
                    changed
                } else {
                    ui.label("Repair JSON syntax in the raw editor first.");
                    false
                }
            })
            .inner;
        if changed {
            self.invalidate_candidate();
        }
        if ui
            .add_enabled(
                !self.busy && !self.profile_id.is_empty(),
                egui::Button::new("Validate candidate / preview changes"),
            )
            .clicked()
        {
            self.send(Command::ValidateDraft {
                profile_id: self.profile_id.clone(),
                expected_revision: self.revision.clone(),
                json: self.editor.clone(),
                core_path: self.core_path.clone().into(),
                asset_root: self.asset_root.clone().into(),
            });
        }
        if let Some(candidate) = self.candidate.clone() {
            ui.label(format!(
                "Validated with {} · resource files: {}",
                candidate.core.version,
                candidate.resources.files.len()
            ));
            for note in &candidate.resources.notes {
                ui.colored_label(ui.visuals().warn_fg_color, note);
            }
            egui::CollapsingHeader::new("Changes from saved JSON").show(ui, |ui| {
                let old = self.saved_json.as_str();
                for line in diff_lines(old, &self.editor) {
                    ui.monospace(line);
                }
            });
            if self.confirm_apply
                && self.candidate_matches()
                && ui
                    .add_enabled(
                        !self.busy,
                        egui::Button::new("Apply validated configuration"),
                    )
                    .clicked()
            {
                self.send(Command::ApplyProfile {
                    candidate_id: candidate.id,
                    candidate_digest: candidate.digest,
                    proxy_mode: self.mode,
                    http_inbound_tag: self.http_tag.clone(),
                });
            }
        }
        egui::CollapsingHeader::new("Committed snapshots / rollback").show(ui, |ui| {
            for g in self.history.clone() {
                ui.horizontal(|ui| {
                    ui.monospace(format!("{} · {}", &g.id[..8], &g.candidate.revision[..12]));
                    if ui.button("Preview this snapshot").clicked() {
                        self.send(Command::StageHistory {
                            generation_id: g.id,
                            expected_revision: self.revision.clone(),
                        });
                    }
                });
            }
        });
    }
    fn settings(&mut self, ui: &mut egui::Ui) {
        let before = self.settings_context();
        let previous_appearance = self.appearance;
        egui::ComboBox::from_label("Appearance")
            .selected_text(match self.appearance {
                crate::preferences::Appearance::System => "Follow system",
                crate::preferences::Appearance::Light => "Light",
                crate::preferences::Appearance::Dark => "Dark",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut self.appearance,
                    crate::preferences::Appearance::System,
                    "Follow system",
                );
                ui.selectable_value(
                    &mut self.appearance,
                    crate::preferences::Appearance::Light,
                    "Light",
                );
                ui.selectable_value(
                    &mut self.appearance,
                    crate::preferences::Appearance::Dark,
                    "Dark",
                );
            });
        if self.appearance != previous_appearance {
            self.appearance.apply(ui.ctx());
            if let Err(error) = self.appearance.save(&self.root) {
                self.message = format!("Appearance changed but could not be saved: {error}");
            }
        }
        ui.separator();
        ui.heading("Managed Xray dependencies");
        ui.label("Download official Xray Core, geoip.dat and geosite.dat. Existing running configurations keep their frozen dependencies.");
        if !cfg!(windows) {
            ui.label("Managed installation is available on Windows x64. Select an official Core manually on this platform.");
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Official version");
            ui.text_edit_singleline(&mut self.dependency_version);
            if ui
                .add_enabled(
                    cfg!(windows) && self.dependency_job.is_none(),
                    egui::Button::new("Download / repair official Core"),
                )
                .clicked()
            {
                self.start_dependencies("Install", "", Some(ui.ctx().clone()));
            }
            if ui
                .add_enabled(
                    cfg!(windows) && self.dependency_job.is_none(),
                    egui::Button::new("Check installed dependencies"),
                )
                .clicked()
            {
                self.start_dependencies("Status", "", Some(ui.ctx().clone()));
            }
        });
        if self.dependency_job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Checking or downloading dependencies; this may take a few minutes…");
            });
        }
        ui.label(&self.dependency_message);
        for installation in self.dependency_report.installations.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} · {}",
                    installation.version,
                    if installation.valid {
                        "Verified"
                    } else {
                        "Needs repair"
                    }
                ));
                if let Some(error) = &installation.error {
                    ui.label(error);
                }
                if ui
                    .add_enabled(
                        cfg!(windows) && installation.valid && self.dependency_job.is_none(),
                        egui::Button::new("Use this version"),
                    )
                    .clicked()
                {
                    self.dependency_version = installation.version;
                    self.start_dependencies(
                        "Select",
                        &installation.install_id,
                        Some(ui.ctx().clone()),
                    );
                }
            });
        }
        ui.separator();
        ui.label("Official Xray Core executable (absolute path)");
        ui.text_edit_singleline(&mut self.core_path);
        if ui.button("Choose Core…").clicked() {
            #[cfg(windows)]
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Executable", &["exe"])
                .pick_file()
            {
                self.core_path = path.display().to_string();
                if let Some(parent) = path.parent() {
                    self.asset_root = parent.display().to_string();
                }
            }
        }
        ui.label("Resource directory for relative certificate paths and geo data");
        ui.text_edit_singleline(&mut self.asset_root);
        ui.checkbox(&mut self.settings.start_on_logon, "Start on Windows login");
        ui.checkbox(
            &mut self.settings.connect_on_start,
            "Connect on Supervisor startup",
        );
        if before != self.settings_context() {
            self.settings_edited();
        }
        if ui.button("Save settings").clicked() {
            self.settings.core_path = self.core_path.clone().into();
            self.send(Command::SaveSettings {
                settings: self.settings.clone(),
            });
        }
        ui.label(format!("Data directory: {}", self.root.display()));
        ui.label("Phase 0–2: native profiles, runtime transactions and Windows system proxy. PAC and TUN are not enabled in this release.");
    }
    fn start_dependencies(
        &mut self,
        action: &'static str,
        install_id: &str,
        repaint: Option<egui::Context>,
    ) {
        if !cfg!(windows) || self.dependency_job.is_some() {
            return;
        }
        let Some(repaint) = repaint.or_else(|| self.dependency_repaint.clone()) else {
            return;
        };
        let select = action != "Status" || self.core_path.is_empty();
        let response = crate::dependencies::spawn(
            crate::dependencies::Job::new(action, &self.root, &self.dependency_version, install_id),
            repaint,
        );
        self.dependency_job = Some(DependencyJob {
            response,
            context: self.settings_context(),
            asset_root: self.asset_root.clone(),
            select,
            version: self.dependency_version.clone(),
        });
        self.dependency_message = String::new();
    }
    fn receive_dependencies(&mut self) {
        let Some(job) = &self.dependency_job else {
            return;
        };
        let result = match job.response.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err(Error::new(
                "DEPENDENCY_INSTALL_FAILED",
                "Dependency worker disconnected",
            )),
        };
        let job = self.dependency_job.take().unwrap();
        match result {
            Ok(report) => {
                let matches = job.context == self.settings_context()
                    && job.asset_root == self.asset_root
                    && job.version == self.dependency_version;
                if let Some(bundle) = &report.selected {
                    if job.select && matches {
                        self.core_path = bundle.core_path.display().to_string();
                        self.asset_root = bundle.asset_root.display().to_string();
                        self.settings_edited();
                        self.invalidate_candidate();
                        self.dependency_message = format!(
                            "{} verified and selected. Save settings, then validate your configuration.",
                            bundle.version
                        );
                    } else {
                        self.dependency_message = format!(
                            "{} verified. Current settings were preserved; use a verified version when ready.",
                            bundle.version
                        );
                    }
                } else {
                    self.dependency_message = "No verified current bundle. Download / repair official Core to prepare dependencies.".into();
                }
                self.dependency_report = report;
            }
            Err(error) => self.dependency_message = error.to_string(),
        }
    }
}
impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive();
        self.receive_dependencies();
        ctx.request_repaint_after(Duration::from_millis(300));
        if self.exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        #[cfg(windows)]
        {
            let actions = self.tray.as_ref().map(|t| t.actions()).unwrap_or_default();
            for action in actions {
                match action.as_str() {
                    "open" => {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    }
                    "stop" => self.send(Command::StopCore),
                    "start" => self.send(Command::RestartCore),
                    "direct" => self.send(Command::SetProxyMode {
                        mode: ProxyMode::Direct,
                        http_inbound_tag: self.http_tag.clone(),
                    }),
                    "global" => self.send(Command::SetProxyMode {
                        mode: ProxyMode::Global,
                        http_inbound_tag: self.http_tag.clone(),
                    }),
                    "exit-gui" => self.exit = true,
                    "quit" => self.send(Command::Shutdown),
                    _ => {}
                }
            }
            if ctx.input(|i| i.viewport().close_requested()) && self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        if self.last_poll.elapsed() > Duration::from_secs(2) && !self.busy {
            self.send(Command::GetStatus);
            if self.tab == 3 {
                self.send(Command::GetLogs);
            }
            self.last_poll = Instant::now();
        }
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Xray Pilot");
                if let Some(status) = &self.status {
                    ui.label(format!(
                        "Core: {} · System proxy: {}",
                        status.core_state, status.proxy_state
                    ));
                    if status.recovery_required {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            "Recovery required — inspect the last error",
                        );
                    }
                }
                if self.busy {
                    ui.spinner();
                }
            });
            ui.horizontal(|ui| {
                for (i, title) in ["Profiles / JSON", "Settings", "Routing diagnostics", "Logs"]
                    .iter()
                    .enumerate()
                {
                    ui.selectable_value(&mut self.tab, i, *title);
                }
                if self.status.is_none() && ui.button("Reconnect Supervisor").clicked() {
                    self.send(Command::GetStatus);
                }
                if let Some(request_id) = self.failed_request.clone()
                    && ui.button("Query failed request").clicked()
                {
                    self.send(Command::GetOperation { request_id });
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Stop Core"))
                    .clicked()
                {
                    self.send(Command::StopCore);
                }
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Start / restart"))
                    .clicked()
                {
                    self.send(Command::RestartCore);
                }
            });
        });
        egui::TopBottomPanel::bottom("result").show(ctx, |ui| {
            ui.label(&self.message);
            if let Some(e) = self.status.as_ref().and_then(|s| s.last_error.as_ref()) {
                ui.colored_label(ui.visuals().warn_fg_color, e.to_string());
            }
        });
        egui::CentralPanel::default().show(ctx,|ui|{ui.add_enabled_ui(!self.busy,|ui|match self.tab{
            0=>self.editor(ui),1=>{egui::ScrollArea::vertical().id_salt("settings-scroll").show(ui,|ui|self.settings(ui));},2=>{
                ui.label("Static prediction only; missing context or unsupported earlier rules return Unknown.");
                ui.horizontal(|ui|{ui.label("Host");ui.text_edit_singleline(&mut self.route_host);});ui.horizontal(|ui|{ui.label("Inbound tag");ui.text_edit_singleline(&mut self.route_inbound);ui.label("Network");ui.text_edit_singleline(&mut self.route_network);});
                if ui.button("Simulate saved profile").clicked(){self.send(Command::TestRoute{profile_id:self.profile_id.clone(),context:RouteContext{host:self.route_host.clone(),port:Some(443),inbound_tag:Some(self.route_inbound.clone()),network:Some(self.route_network.clone()),target_ip:None}});}
                ui.monospace(&self.route_result);
            },_=>{ui.label("Core logs may include destinations or credentials. Logs are kept in memory, bounded to 1000 chunks.");egui::ScrollArea::vertical().stick_to_bottom(true).show(ui,|ui|{for line in &self.logs{ui.monospace(line);}});}
        });});
    }
}
fn default_config() -> String {
    serde_json::to_string_pretty(&serde_json::json!({"log":{"loglevel":"warning"},"inbounds":[{"tag":"socks-in","listen":"127.0.0.1","port":10808,"protocol":"socks","settings":{"udp":true}},{"tag":"http-in","listen":"127.0.0.1","port":10809,"protocol":"http","settings":{}}],"outbounds":[{"tag":"direct","protocol":"freedom"}]})).unwrap()
}
fn diff_lines(old: &str, new: &str) -> Vec<String> {
    let old: Vec<_> = old.lines().collect();
    let new: Vec<_> = new.lines().collect();
    let mut lines = vec![];
    for i in 0..old.len().max(new.len()) {
        if old.get(i) != new.get(i) {
            if let Some(v) = old.get(i) {
                lines.push(format!("- {v}"));
            }
            if let Some(v) = new.get(i) {
                lines.push(format!("+ {v}"));
            }
        }
    }
    lines
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
