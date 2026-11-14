use eframe::egui;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
use xray_model::*;

#[path = "ui.rs"]
mod ui;
use ui::{Page, UiState};

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
    validation_epoch: u64,
    runtime_generation: Option<String>,
    supervisor_epoch: Option<String>,
}
impl EditorContext {
    fn same_draft(&self, other: &Self) -> bool {
        self.epoch == other.epoch
            && self.selected == other.selected
            && self.profile == other.profile
            && self.json_hash == other.json_hash
    }
}
#[derive(Clone, PartialEq, Eq)]
struct SettingsContext {
    epoch: u64,
    edit_epoch: u64,
    core: String,
    assets: String,
    start_on_logon: bool,
    connect_on_start: bool,
    restore_last_state: bool,
}
struct Pending {
    context: EditorContext,
    settings_context: Option<SettingsContext>,
    poll: Option<&'static str>,
    submitted_json: Option<String>,
    asset_root: String,
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
    ui: UiState,
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
    shutdown_requested: bool,
    wakeup: Option<crate::wakeup::Wakeup>,
    history: Vec<Generation>,
    failed_request: Option<String>,
    #[cfg(windows)]
    tray: Option<crate::tray::Tray>,
}
impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, root: PathBuf, background: bool) -> Self {
        let (tx, requests) = mpsc::sync_channel::<Request>(MAX_PENDING);
        let (responses, rx) = mpsc::channel();
        let worker_root = root.clone();
        #[cfg(windows)]
        crate::icon::configure_taskbar(cc);
        let wakeup = crate::wakeup::Wakeup::new(cc);
        let worker_wakeup = wakeup.clone();
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
                                result: Err(Error::new("CONNECTION_LOST", e.to_string())),
                            }
                        }
                    };
                if responses.send((command, response)).is_err() {
                    break;
                }
                worker_wakeup.request();
            }
        });
        let mut app = Self::with_channels(root, tx, rx);
        app.wakeup = Some(wakeup.clone());
        app.ui.persistence = true;
        app.dependency_repaint = Some(cc.egui_ctx.clone());
        match crate::preferences::Appearance::load(&app.root) {
            Ok(appearance) => app.appearance = appearance,
            Err(error) => app.message = format!("无法读取外观设置：{error}"),
        }
        app.appearance.apply(&cc.egui_ctx);
        ui::configure(&cc.egui_ctx);
        match crate::workspace::Workspace::load(&app.root) {
            Ok(workspace) => app.ui.workspace = workspace,
            Err(error) => {
                app.ui.workspace_valid = false;
                app.message = format!("界面资料无法读取，原文件已保留：{error}");
            }
        }
        #[cfg(windows)]
        {
            app.tray = crate::tray::Tray::new(wakeup).ok();
            if background && app.tray.is_some() {
                // `eframe` always reveals the viewport after rendering frame 1; cloak via DWM until frame 2 hides it.
                app.ui.background_hide = ui::BackgroundHide::HideAfterFirstFrame;
                if let Some(wakeup) = &app.wakeup {
                    wakeup.set_cloaked(true);
                }
            } else if app.tray.is_none() {
                cc.egui_ctx
                    .send_viewport_cmd(egui::ViewportCommand::Visible(true));
            }
        }
        #[cfg(not(windows))]
        let _ = background;
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
            message: "正在连接后台…".into(),
            logs: vec![],
            busy: false,
            last_poll: Instant::now(),
            ui: UiState::default(),
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
            shutdown_requested: false,
            wakeup: None,
            history: vec![],
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
            validation_epoch: self.ui.validation_epoch,
            runtime_generation: self
                .status
                .as_ref()
                .and_then(|s| s.generation.as_ref().map(|g| g.id.clone())),
            supervisor_epoch: self.status.as_ref().map(|s| s.supervisor_epoch.clone()),
        }
    }
    fn invalidate_candidate(&mut self) {
        self.ui.validation_epoch = self.ui.validation_epoch.wrapping_add(1);
        self.ui.preview_open = false;
        self.candidate = None;
        self.candidate_context = None;
        self.confirm_apply = false;
    }
    fn settings_context(&self) -> SettingsContext {
        SettingsContext {
            epoch: self.settings_epoch,
            edit_epoch: self.settings_edit_epoch,
            core: self.core_path.clone(),
            assets: self.asset_root.clone(),
            start_on_logon: self.settings.start_on_logon,
            connect_on_start: self.settings.connect_on_start,
            restore_last_state: self.settings.restore_last_state,
        }
    }
    fn settings_edited(&mut self) {
        self.settings_edit_epoch = self.settings_edit_epoch.wrapping_add(1);
        self.ui.settings_dirty = true;
        self.invalidate_candidate();
    }
    fn candidate_matches(&self) -> bool {
        self.ui.number_inputs.is_empty()
            && self.candidate.as_ref().is_some_and(|c| {
                c.profile_id == self.profile_id
                    && c.base_revision == self.revision
                    && c.revision == hash(self.editor.as_bytes())
            })
            && self
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
            self.ui.preview_open = true;
        } else {
            self.invalidate_candidate();
            self.message = "候选与当前草稿不一致，请重新检查".into();
        }
    }
    fn refresh_profiles(&mut self) {
        self.send(Command::ListProfileSummaries {
            after: None,
            limit: 100,
        });
    }
    #[cfg_attr(not(windows), allow(dead_code))]
    fn tray_status(&self) -> String {
        let Some(status) = &self.status else {
            return "未读取".into();
        };
        let core = match status.core_state.as_str() {
            "Running" => "运行中",
            "Degraded" => "等待恢复",
            _ => "已停止",
        };
        let proxy = match status.proxy_state.as_str() {
            "Owned" => "系统代理已接管",
            "Released" => "系统代理已释放",
            "Conflict" => "代理冲突",
            "RecoveryRequired" => "代理恢复受限",
            _ => "未接管系统代理",
        };
        format!("{core} · {proxy}")
    }
    pub(in crate::app) fn pac_draft(&self) -> PacDraft {
        self.pac_draft_for(&self.profile_id)
    }
    fn pac_draft_for(&self, profile: &str) -> PacDraft {
        self.ui
            .workspace
            .pac
            .get(profile)
            .cloned()
            .unwrap_or_default()
    }
    /// Mode switches apply to the committed generation, not the profile being edited.
    pub(in crate::app) fn running_pac_draft(&self) -> PacDraft {
        self.committed_id()
            .map(|profile| self.pac_draft_for(profile))
            .unwrap_or_default()
    }
    pub(in crate::app) fn pac_endpoint(
        &mut self,
    ) -> std::result::Result<std::net::SocketAddr, String> {
        if self.editor.trim().is_empty() {
            return Err("尚未选择配置；请先在配置页添加或打开配置。".into());
        }
        let value = serde_json::from_str::<serde_json::Value>(&self.editor)
            .map_err(|error| format!("当前 JSON 无法解析：{error}"))?;
        let candidates: Vec<String> = value
            .get("inbounds")
            .and_then(|v| v.as_array())
            .into_iter()
            .flatten()
            .filter(|v| matches!(v["protocol"].as_str(), Some("http" | "mixed")))
            .filter_map(|v| v["tag"].as_str())
            .map(str::to_owned)
            .collect();
        if candidates.is_empty() {
            return Err(
                "当前配置没有 HTTP 或 Mixed 入站；请先在配置的本地入口中添加回环、无认证的入口。"
                    .into(),
            );
        }
        // Prefer the current selection, but recover automatically when the
        // user added/renamed an inbound or made the previous one ineligible.
        let mut order = Vec::with_capacity(candidates.len());
        if candidates.contains(&self.http_tag) {
            order.push(self.http_tag.clone());
        }
        order.extend(
            candidates
                .iter()
                .filter(|tag| **tag != self.http_tag)
                .cloned(),
        );
        let mut last_error = String::new();
        for tag in order {
            match xray_runtime::http_endpoint(&value, &tag) {
                Ok(endpoint) => {
                    self.http_tag = tag;
                    return Ok(endpoint);
                }
                Err(error) => last_error = error.message,
            }
        }
        Err(format!(
            "当前 HTTP/Mixed 入站不能用于系统代理：{last_error}"
        ))
    }
    pub(in crate::app) fn save_pac_draft(&mut self, draft: PacDraft) {
        self.ui.workspace.pac.insert(self.profile_id.clone(), draft);
        self.save_workspace();
    }
    fn note_core_running(&mut self, running: bool) {
        if running && !self.ui.workspace.last_core_running {
            self.ui.workspace.last_core_running = true;
            self.save_workspace();
        }
    }
    fn maybe_restore_core(&mut self) {
        if !self.ui.settings_loaded
            || !self.settings.restore_last_state
            || self.ui.restore_attempted
            || self.busy
            || self.shutdown_requested
            || self.ui.uncertain.is_some()
            || self.write_blocker().is_some()
            || !self.ui.workspace.last_core_running
        {
            return;
        }
        let Some(status) = self.status.as_ref() else {
            return;
        };
        if status.core_state != "Stopped" || status.recovery_required || status.generation.is_none()
        {
            return;
        }
        self.ui.restore_attempted = true;
        self.message = "正在恢复上次 Core 运行状态…".into();
        self.send(Command::RestartCore);
    }
    fn send(&mut self, command: Command) {
        if matches!(command, Command::StopCore) && self.ui.workspace.last_core_running {
            self.ui.workspace.last_core_running = false;
            self.save_workspace();
        }
        if command.is_write() && self.busy {
            self.message = "请等待当前操作完成".into();
            return;
        }
        if command.is_write() {
            let safe_recovery = matches!(
                command,
                Command::StopCore | Command::Shutdown | Command::ResolveProxyConflict
            ) || (matches!(command, Command::SaveSettings { .. })
                && self.can_save_settings());
            let shutdown_safe = matches!(command, Command::StopCore | Command::Shutdown);
            if !self.ui.connected
                || self.ui.uncertain.is_some()
                || (!shutdown_safe && self.shutdown_requested)
                || (!safe_recovery && self.status.as_ref().is_some_and(|s| s.recovery_required))
            {
                self.message = self.write_blocker().unwrap_or("当前状态暂不能变更").into();
                return;
            }
        }
        if let Command::ApplyProfile {
            candidate_id,
            candidate_digest,
            proxy_mode,
            http_inbound_tag,
            ..
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
                self.message = "草稿或检查上下文已变化，请重新检查".into();
                return;
            }
        }
        if matches!(command, Command::ValidateDraft { .. })
            && (self.selected_profile != self.profile_id || self.has_draft_conflict())
        {
            self.message = "请等待读取配置，或先处理草稿冲突".into();
            return;
        }
        if matches!(command, Command::SetProxyMode { .. }) && !self.is_running() {
            self.message = "请先启动 Core，再切换系统代理接管方式。".into();
            return;
        }
        let poll = match command {
            Command::GetStatus => Some("status"),
            Command::GetLogs => Some("logs"),
            Command::ListProfileSummaries { .. } => Some("profiles"),
            Command::GetRuntimeConfig { .. } => Some("runtime"),
            _ => None,
        };
        if poll.is_some() && self.pending.values().any(|p| p.poll == poll) {
            return;
        }
        if self.pending.len() >= MAX_PENDING {
            self.message = "请求队列已满，请等待当前操作完成".into();
            return;
        }
        if let Command::GetProfile { profile_id } = &command {
            self.stash_draft();
            self.selected_profile = profile_id.clone();
            self.view_epoch = self.view_epoch.wrapping_add(1);
            self.invalidate_candidate();
        }
        let context = self.context();
        let submitted_json =
            matches!(command, Command::ApplyProfile { .. }).then(|| self.editor.clone());
        let write = command.is_write();
        let operation_label = ui::command_name(&command);
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
                if settings_read || settings_write {
                    self.settings_epoch = self.settings_epoch.wrapping_add(1);
                }
                if settings_write {
                    self.settings_edited();
                }
                let settings_context = settings_read.then(|| self.settings_context());
                if write {
                    self.add_event(
                        "信息",
                        format!("已提交{operation_label}"),
                        request_id.clone(),
                    );
                }
                self.pending.insert(
                    request_id,
                    Pending {
                        context,
                        settings_context,
                        poll,
                        submitted_json,
                        asset_root: self.asset_root.clone(),
                    },
                );
                if write {
                    self.busy = true;
                }
            }
            Err(error) => self.message = format!("请求未发送：{error}"),
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
                .is_some_and(|context| context.epoch != self.settings_epoch)
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
                    self.message = "载入期间草稿已变化，已保留当前编辑".into();
                }
                if command.is_write() {
                    self.message = "草稿或编辑对象已变化，请重新检查当前草稿".into();
                }
                continue;
            }
            match response.result {
                Err(error) => {
                    if command.is_write()
                        && matches!(
                            error.code.as_str(),
                            "CONNECTION_LOST"
                                | "IPC_TIMEOUT"
                                | "IPC_MESSAGE_TOO_LARGE"
                                | "OPERATION_IN_PROGRESS"
                                | "OPERATION_RECOVERY_REQUIRED"
                        )
                    {
                        self.ui.uncertain = Some(ui::Uncertain {
                            request_id: response.request_id.clone(),
                            command: command.clone(),
                            json: pending.submitted_json.clone(),
                            profile: pending.context.profile.clone(),
                            assets: pending.asset_root.clone(),
                        });
                    }
                    if error.code == "CONNECTION_LOST" {
                        self.ui.connected = false;
                    }
                    self.fail(error.clone(), response.request_id.clone());
                    if matches!(command, Command::GetProfile { .. })
                        && pending.context.epoch == self.view_epoch
                    {
                        self.selected_profile = self.profile_id.clone();
                    }
                    if matches!(command, Command::Shutdown) {
                        self.shutdown_requested = false;
                        if let Some(wakeup) = &self.wakeup {
                            wakeup.reveal();
                        }
                    }
                    self.message = format!("{} 请求：{}", self.message, response.request_id);
                    if command.is_write() {
                        self.failed_request = Some(response.request_id);
                        self.send(Command::GetStatus);
                    }
                    if matches!(command, Command::GetStatus) {
                        self.ui.connected = false;
                    }
                }
                Ok(value) => match command {
                    Command::GetStatus => {
                        if let Ok(status) = serde_json::from_value::<Status>(value) {
                            let old_identity = self.status.as_ref().map(|s| {
                                (&s.supervisor_epoch, s.generation.as_ref().map(|g| &g.id))
                            });
                            let new_identity = Some((
                                &status.supervisor_epoch,
                                status.generation.as_ref().map(|g| &g.id),
                            ));
                            if old_identity != new_identity {
                                self.invalidate_candidate();
                                self.ui.route_tested = None;
                                self.route_result.clear();
                            }
                            self.connection_restored();
                            if !self.ui.settings_loaded
                                && !self.pending.values().any(|p| p.settings_context.is_some())
                            {
                                self.send(Command::GetSettings);
                            }
                            if !self.ui.profiles_loaded
                                && !self.pending.values().any(|p| p.poll == Some("profiles"))
                            {
                                self.refresh_profiles();
                            }
                            let generation = status.generation.as_ref().map(|g| g.id.clone());
                            let running = status.core_state == "Running";
                            self.status = Some(status);
                            self.note_core_running(running);
                            self.maybe_restore_core();
                            if let Some(generation_id) = generation {
                                if self
                                    .ui
                                    .runtime
                                    .as_ref()
                                    .is_none_or(|r| r.generation_id != generation_id)
                                {
                                    self.send(Command::GetRuntimeConfig { generation_id });
                                }
                            } else {
                                self.ui.runtime = None;
                            }
                        }
                    }
                    Command::GetRuntimeConfig { generation_id } => {
                        if self
                            .status
                            .as_ref()
                            .and_then(|s| s.generation.as_ref())
                            .is_some_and(|g| g.id == generation_id)
                            && let Ok(config) = serde_json::from_value::<RuntimeConfig>(value)
                            && config.generation_id == generation_id
                        {
                            self.ui.runtime = Some(config);
                        }
                    }
                    Command::ListProfileSummaries { after, .. } => {
                        if let Ok(page) = serde_json::from_value::<ProfilePage>(value) {
                            if after.is_none() {
                                self.profiles.clear();
                            }
                            self.profiles.extend(page.items);
                            self.ui.profiles_loaded = page.next_after.is_none();
                            if after.is_none()
                                && self.profile_id.is_empty()
                                && self.selected_profile.is_empty()
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
                        if let Some(json) = value.get("json").and_then(|v| v.as_str()) {
                            let core = value
                                .pointer("/candidate/core/version")
                                .and_then(|v| v.as_str())
                                .unwrap_or("历史版本")
                                .to_owned();
                            self.open_history_diff(ui::HistoryPreview {
                                json: json.into(),
                                core,
                            });
                            self.message =
                                "已准备历史与当前草稿的比较；确认后才会替换草稿。".into();
                        }
                    }
                    Command::GetSettings => {
                        if let Ok(s) = serde_json::from_value::<Settings>(value) {
                            // A read updates the saved baseline even when editing superseded it.
                            // Completing initial loading prevents status polling from retrying forever.
                            let can_replace_draft = !self.ui.settings_dirty
                                && pending.settings_context.as_ref()
                                    == Some(&self.settings_context());
                            self.ui.saved_settings = Some(s.clone());
                            self.ui.settings_loaded = true;
                            if can_replace_draft {
                                self.core_path = s.core_path.display().to_string();
                                self.asset_root = self
                                    .ui
                                    .workspace
                                    .asset_roots
                                    .get(&self.core_path)
                                    .cloned()
                                    .unwrap_or_else(|| {
                                        s.core_path
                                            .parent()
                                            .filter(|p| !p.as_os_str().is_empty())
                                            .unwrap_or(&self.root)
                                            .display()
                                            .to_string()
                                    });
                                self.settings = s;
                            }
                            self.maybe_restore_core();

                            self.start_dependencies("Status", "", None);
                        }
                    }
                    Command::ValidateDraft { .. } => {
                        if let Ok(c) = serde_json::from_value::<Candidate>(value) {
                            self.message = format!("Core 配置校验通过：{}", c.core.version);
                            self.accept_candidate(c);
                        }
                    }
                    Command::GetOperation { request_id } => {
                        self.resolve_operation(&request_id, value);
                    }
                    Command::DeleteProfile { profile_id, .. } => self.profile_deleted(&profile_id),
                    Command::GetLogs => {
                        self.logs = serde_json::from_value(value).unwrap_or_default()
                    }
                    Command::TestRoute { .. } => {
                        self.route_result = serde_json::to_string_pretty(&value).unwrap_or_default()
                    }
                    Command::TestPac { .. } => {
                        self.ui.pac_test = serde_json::to_string_pretty(&value).unwrap_or_default();
                        self.message = "PAC 执行测试完成。".into();
                    }
                    Command::TestLatency { .. } => {
                        self.ui.latency_result =
                            serde_json::to_string_pretty(&value).unwrap_or_default();
                        self.message = "TCP 延迟探测完成。".into();
                    }
                    Command::TestRunningRoute {
                        generation_id,
                        context,
                    } => {
                        if self
                            .status
                            .as_ref()
                            .and_then(|s| s.generation.as_ref())
                            .is_some_and(|g| g.id == generation_id)
                        {
                            self.ui.route_tested = Some((generation_id, context));
                            self.route_result =
                                serde_json::to_string_pretty(&value).unwrap_or_default();
                        }
                    }
                    Command::Shutdown => {
                        self.shutdown_requested = false;
                        self.request_exit(ui::ExitKind::Gui);
                    }
                    Command::CreateProfile { .. } => {
                        if let Ok(profile) = serde_json::from_value(value) {
                            self.imported_profile(profile);
                        }
                        self.refresh_profiles();
                    }
                    Command::ApplyProfile { .. } => {
                        self.applied_draft(
                            &pending.context.profile,
                            pending.submitted_json.as_deref(),
                        );
                        self.clear_request_failure();
                        self.ui.onboarding = false;
                        self.add_event("信息", "配置已应用", response.request_id.clone());
                        self.message = "配置已应用，正在读取最新运行状态。".into();
                        self.invalidate_candidate();
                        self.send(Command::GetStatus);
                        self.refresh_profiles();
                        if self.selected_profile == pending.context.profile {
                            self.send(Command::GetProfile {
                                profile_id: self.profile_id.clone(),
                            });
                        }
                    }
                    Command::SaveSettings { settings } => {
                        self.accept_saved_settings(settings, pending.asset_root);
                        self.message = "设置已保存；运行中的 Core 保持原版本。".into();
                        self.add_event("信息", "设置已保存", response.request_id);
                        self.send(Command::GetStatus);
                    }
                    Command::ResolveProxyConflict => {
                        self.clear_request_failure();
                        self.message =
                            "已保留当前系统代理设置并解除冲突，可重新检查或启动 Core。".into();
                        self.add_event(
                            "信息",
                            "已保留外部系统代理并解除租约冲突",
                            response.request_id,
                        );
                        self.send(Command::GetStatus);
                    }
                    Command::StopCore | Command::RestartCore | Command::SetProxyMode { .. } => {
                        self.clear_request_failure();
                        let label = ui::command_name(&command);
                        self.message = format!("{label}已完成，正在刷新状态。");
                        self.add_event("信息", format!("{label}已完成"), response.request_id);
                        self.send(Command::GetStatus);
                    }
                    _ => {
                        self.message = "操作已完成，正在刷新状态".into();
                        self.send(Command::GetStatus);
                    }
                },
            }
        }
    }
    fn load(&mut self, p: Profile) {
        self.stash_draft();
        self.view_epoch = self.view_epoch.wrapping_add(1);
        self.ui.guided = Default::default();
        self.selected_profile = p.id.clone();
        self.profile_id = p.id;
        self.saved_json = p.json.clone();
        self.editor = p.json;
        self.revision = p.revision;
        self.restore_draft();
        self.reset_undo(&self.editor.clone());
        self.ui.pac_notes.clear();
        self.ui.pac_test.clear();
        self.ui.pac_checked = false;
        self.ui.pac_domain_filter.clear();
        self.ui.pac_domain_kind_filter = Default::default();
        self.ui.pac_selected_domains.clear();
        self.ui.pac_batch_report = None;
        self.ui.pac_template_key = None;
        self.ui.allow_dynamic = false;
        self.invalidate_candidate();
        self.history.clear();
        self.send(Command::ListHistory {
            profile_id: self.profile_id.clone(),
        });
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
                            "{} 已验证并选用。请保存设置，再检查并应用配置。",
                            bundle.version
                        );
                    } else {
                        self.dependency_message = format!(
                            "{} 已验证，已保留期间的新设置。需要时可选用该版本。",
                            bundle.version
                        );
                    }
                } else {
                    self.dependency_message =
                        "当前没有已验证的受管版本，请下载安装官方依赖或手动选择已有 Core。".into();
                }
                self.dependency_report = report;
            }
            Err(error) => self.dependency_message = error.to_string(),
        }
    }
    /// Uncloaks (if still in background startup), reveals, and focuses the main viewport.
    #[cfg(any(windows, test))]
    pub(super) fn reveal_window(&mut self, ctx: &egui::Context) {
        self.ui.background_hide = ui::BackgroundHide::None;
        if let Some(wakeup) = &self.wakeup {
            wakeup.set_cloaked(false);
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        ctx.request_repaint();
    }

    /// Completes the `--background` hide-after-frame-1 sequence without flashing on Windows.
    pub(super) fn advance_background_hide(&mut self, ctx: &egui::Context) {
        match self.ui.background_hide {
            ui::BackgroundHide::None => {}
            ui::BackgroundHide::HideAfterFirstFrame => {
                if ctx.cumulative_pass_nr() == 0 {
                    ctx.request_repaint();
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    self.ui.background_hide = ui::BackgroundHide::UncloakAfterHide;
                    if let Some(wakeup) = &self.wakeup {
                        wakeup.request();
                    } else {
                        ctx.request_repaint();
                    }
                }
            }
            ui::BackgroundHide::UncloakAfterHide => {
                if let Some(wakeup) = &self.wakeup {
                    wakeup.set_cloaked(false);
                }
                self.ui.background_hide = ui::BackgroundHide::None;
            }
        }
    }

    #[cfg(any(windows, test))]
    fn tray_action(&mut self, action: &str, ctx: &egui::Context) {
        if let Some(profile) = action.strip_prefix("profile:") {
            self.open_profile_from_tray(profile, ctx);
            return;
        }
        match action {
            "open" => self.reveal_window(ctx),
            "exit" => {
                if !self.shutdown_requested {
                    self.reveal_window(ctx);
                    self.open_exit_dialog();
                }
            }
            "exit-gui" => {
                if !self.busy && !self.shutdown_requested && self.ui.uncertain.is_none() {
                    self.request_exit(ui::ExitKind::Gui);
                }
            }
            "quit" => {
                self.request_exit(ui::ExitKind::Shutdown);
                if !self.ui.connected || self.ui.uncertain.is_some() {
                    self.reveal_window(ctx);
                }
            }
            _ if self.shutdown_requested => {}
            "stop" => self.send(Command::StopCore),
            "start" => {
                if self.committed_id().is_some() {
                    self.send(Command::RestartCore);
                } else {
                    self.ui.page = Page::Profiles;
                    self.reveal_window(ctx);
                }
            }
            "direct" => self.send(Command::SetProxyMode {
                mode: ProxyMode::Direct,
                http_inbound_tag: self.runtime_http_tag(),
                pac: None,
            }),
            "global" => self.send(Command::SetProxyMode {
                mode: ProxyMode::Global,
                http_inbound_tag: self.runtime_http_tag(),
                pac: None,
            }),
            "pac" => self.send(Command::SetProxyMode {
                mode: ProxyMode::Pac,
                http_inbound_tag: self.runtime_http_tag(),
                pac: Some(self.running_pac_draft()),
            }),
            _ => {}
        }
    }
    fn advance_shutdown(&mut self) {
        if self.shutdown_requested
            && !self.busy
            && !self.exit
            && self.ui.connected
            && self.ui.uncertain.is_none()
            && self.drafts_ready_for_exit(ui::ExitKind::Shutdown)
        {
            self.send(Command::Shutdown);
        }
    }
}
impl eframe::App for App {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if !self.exit {
            self.stash_draft();
        }
        if let Some(wakeup) = &self.wakeup {
            wakeup.close();
        }
    }
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.receive();
        self.receive_dependencies();
        self.flush_draft_if_due();
        ctx.request_repaint_after(Duration::from_millis(300));
        #[cfg(windows)]
        {
            let tray_state = crate::tray::State {
                status: self.tray_status(),
                can_write: self.write_blocker().is_none(),
                can_stop: self.ui.connected && !self.busy && self.ui.uncertain.is_none(),
                can_exit_gui: !self.busy && !self.shutdown_requested && self.ui.uncertain.is_none(),
                running: self.is_running(),
                quitting: self.shutdown_requested,
            };
            let profiles = self
                .profiles
                .iter()
                .map(|p| {
                    let running = self.is_running() && self.committed_id() == Some(p.id.as_str());
                    let label = format!(
                        "{}{}{}",
                        self.ui.workspace.name(&p.id),
                        if running { " · 运行中" } else { "" },
                        if p.id == self.profile_id {
                            " · 正在编辑"
                        } else {
                            ""
                        }
                    );
                    (p.id.clone(), label)
                })
                .collect();
            if let Some(tray) = &mut self.tray {
                tray.update(tray_state, profiles);
            }
            let actions = self.tray.as_ref().map(|t| t.actions()).unwrap_or_default();
            for action in actions {
                self.tray_action(&action, ctx);
            }
        }
        self.advance_background_hide(ctx);
        self.advance_shutdown();
        if self.exit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            if let Some(wakeup) = &self.wakeup {
                wakeup.request();
            } else {
                ctx.request_repaint();
            }
            return;
        }
        #[cfg(windows)]
        {
            if ctx.input(|i| i.viewport().close_requested()) && self.tray.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
        #[cfg(windows)]
        let close_exits = self.tray.is_none();
        #[cfg(not(windows))]
        let close_exits = true;
        if close_exits && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_exit(ui::ExitKind::Gui);
            ctx.request_repaint();
        }
        if self.last_poll.elapsed() > Duration::from_secs(2)
            && !self.busy
            && !self.shutdown_requested
        {
            self.send(Command::GetStatus);
            if self.ui.page == Page::Logs && !self.ui.logs_paused {
                self.send(Command::GetLogs);
            }
            self.last_poll = Instant::now();
        }
        self.render(ctx);
    }
}
pub(crate) fn default_config() -> String {
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
