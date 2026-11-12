use super::*;
use crate::{
    path_display::{path as display_path, text as display_path_text},
    preferences::Appearance,
    widgets::{CONTROL_HEIGHT, singleline},
    workspace::{Draft, Workspace},
};
use egui::{Color32, RichText};

#[path = "ui/configuration.rs"]
mod configuration;
#[path = "ui/diagnostics.rs"]
mod diagnostics;
#[path = "ui/dialogs.rs"]
mod dialogs;
use configuration::{config_review, config_review_action, json_editor, json_viewer};
#[path = "ui/layout.rs"]
mod layout;
#[path = "ui/pac.rs"]
mod pac;
#[path = "ui/profiles.rs"]
mod profiles;
#[path = "ui/settings.rs"]
mod settings;
#[cfg(test)]
#[path = "ui/tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Page {
    #[default]
    Overview,
    Profiles,
    Pac,
    Diagnostics,
    Logs,
    Settings,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ProfileTab {
    #[default]
    Summary,
    Inbounds,
    Outbounds,
    Routing,
    Json,
    History,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SettingsTab {
    #[default]
    Core,
    Startup,
    Appearance,
    Storage,
}
#[derive(Clone)]
pub(super) enum Action {
    Switch(String),
    Discard(String),
    Delete {
        profile: String,
        revision: String,
    },
    Stop,
    Restart,
    #[cfg_attr(not(windows), allow(dead_code))]
    ExportJson,
    Proxy(ProxyMode),
    AdoptDraft(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ExitKind {
    Gui,
    Shutdown,
}
pub(super) enum Dialog {
    UnsavedExit(ExitKind),
    Confirm(Action),
    HistoryDiff,
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    ExitChoice,
    Create,
    Rename(String),
}
#[derive(Clone)]
pub(super) struct HistoryPreview {
    pub json: String,
    pub core: String,
}
pub(super) struct Uncertain {
    pub request_id: String,
    pub command: Command,
    pub json: Option<String>,
    pub profile: String,
    pub assets: String,
}
pub(super) struct Failure {
    error: Error,
    request: String,
}
#[derive(Clone)]
struct Event {
    level: &'static str,
    message: String,
    request: String,
    at: Instant,
}
pub(super) struct UiState {
    pub page: Page,
    profile_tab: ProfileTab,
    settings_tab: SettingsTab,
    pub workspace: Workspace,
    pub connected: bool,
    pub settings_loaded: bool,
    pub profiles_loaded: bool,
    pub saved_settings: Option<Settings>,
    pub settings_dirty: bool,
    pub persistence: bool,
    pub workspace_valid: bool,
    unreadable_drafts: std::collections::HashSet<String>,
    deleted_profiles: std::collections::HashSet<String>,
    pub validation_epoch: u64,
    pub preview_open: bool,
    pub runtime: Option<RuntimeConfig>,
    pub uncertain: Option<Uncertain>,
    pub operation_details: String,
    pub(super) failure: Option<Failure>,
    pub(super) drafts: HashMap<String, Draft>,
    pub(super) profile_cache: HashMap<String, Profile>,
    draft_dirty_since: Option<Instant>,
    pub(super) number_inputs: crate::workspace::NumberInputs,
    pending_draft_writes: std::collections::HashSet<String>,
    discarded_draft_state: Option<String>,
    draft_conflict: bool,
    pub(super) dialog: Option<Dialog>,
    new_name: String,
    new_json: String,
    import_json: bool,
    new_scene: crate::guided_config::Scene,
    new_template: crate::guided_config::Template,
    new_guided_started: bool,
    new_editor_epoch: u64,
    new_node_names: std::collections::BTreeMap<String, String>,
    pub(super) guided: crate::guided_config::State,
    import_error: Option<String>,
    import_source: Option<PathBuf>,
    import_use_source_assets: bool,
    rename: String,
    pub onboarding: bool,
    setup_initialized: bool,
    setup_step: usize,
    advance_setup_after_save: bool,
    pub logs_paused: bool,
    frozen_logs: Vec<(String, String)>,
    log_search: String,
    log_level: String,
    log_source: usize,
    events: Vec<Event>,
    route_port: String,
    route_ip: String,
    route_source: String,
    route_source_port: String,
    route_user: String,
    route_protocol: String,
    route_attrs: String,
    pub route_tested: Option<(String, RouteContext)>,
    pub pac_test_url: String,
    pub pac_domain_input: String,
    pub restore_attempted: bool,
    pub exit_choice: ExitKind,
    pub exit_remember: bool,
    pub pac_test: String,
    pub pac_notes: Vec<String>,
    pub pac_checked: bool,
    pub history_preview: Option<HistoryPreview>,
    pub latency_host: String,
    pub latency_port: String,
    pub latency_result: String,
    undo: Vec<String>,
    redo: Vec<String>,
    undo_at: Option<Instant>,
    last_seen_editor: String,
    restoring_undo: bool,
    pub(super) allow_dynamic: bool,
}
impl Default for UiState {
    fn default() -> Self {
        Self {
            page: Page::Overview,
            profile_tab: ProfileTab::Summary,
            settings_tab: SettingsTab::Core,
            workspace: Workspace::default(),
            connected: false,
            settings_loaded: false,
            profiles_loaded: false,
            saved_settings: None,
            settings_dirty: false,
            persistence: false,
            workspace_valid: true,
            unreadable_drafts: Default::default(),
            deleted_profiles: Default::default(),
            validation_epoch: 0,
            preview_open: false,
            runtime: None,
            uncertain: None,
            operation_details: String::new(),
            failure: None,
            drafts: HashMap::new(),
            profile_cache: HashMap::new(),
            draft_dirty_since: None,
            number_inputs: Default::default(),
            pending_draft_writes: Default::default(),
            discarded_draft_state: None,
            draft_conflict: false,
            dialog: None,
            new_name: String::new(),
            new_json: String::new(),
            import_json: false,
            new_scene: Default::default(),
            new_template: Default::default(),
            new_guided_started: false,
            new_editor_epoch: 0,
            new_node_names: Default::default(),
            guided: Default::default(),
            import_error: None,
            import_source: None,
            import_use_source_assets: false,
            rename: String::new(),
            onboarding: true,
            setup_initialized: false,
            setup_step: 0,
            advance_setup_after_save: false,
            logs_paused: false,
            frozen_logs: vec![],
            log_search: String::new(),
            log_level: "全部".into(),
            log_source: 0,
            events: vec![],
            route_port: "443".into(),
            route_ip: String::new(),
            route_source: String::new(),
            route_source_port: String::new(),
            route_user: String::new(),
            route_protocol: String::new(),
            route_attrs: String::new(),
            route_tested: None,
            pac_test_url: "https://example.com/".into(),
            pac_domain_input: String::new(),
            restore_attempted: false,
            exit_choice: ExitKind::Gui,
            exit_remember: true,
            pac_test: String::new(),
            pac_notes: vec![],
            pac_checked: false,
            history_preview: None,
            latency_host: String::new(),
            latency_port: "443".into(),
            latency_result: String::new(),
            undo: vec![],
            redo: vec![],
            undo_at: None,
            last_seen_editor: String::new(),
            restoring_undo: false,
            allow_dynamic: false,
        }
    }
}

pub(super) fn configure(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let candidates = if cfg!(windows) {
        let windows = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\Windows"));
        vec![
            windows.join("Fonts/msyh.ttc"),
            windows.join("Fonts/simhei.ttf"),
        ]
    } else {
        vec![
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
            PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
        ]
    };
    for path in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert("cjk".into(), egui::FontData::from_owned(bytes).into());
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("cjk".into());
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = CONTROL_HEIGHT;
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(25.0));
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        let dark = style.visuals.dark_mode;
        style.visuals.widgets.noninteractive.fg_stroke.color = if dark {
            Color32::from_rgb(220, 227, 234)
        } else {
            Color32::from_rgb(35, 45, 55)
        };
        style.visuals.weak_text_color = Some(if dark {
            Color32::from_rgb(157, 169, 181)
        } else {
            Color32::from_rgb(93, 105, 117)
        });
        style.visuals.panel_fill = if dark {
            Color32::from_rgb(17, 22, 30)
        } else {
            Color32::from_rgb(244, 246, 248)
        };
        style.visuals.window_fill = if dark {
            Color32::from_rgb(25, 31, 41)
        } else {
            Color32::WHITE
        };
        style.visuals.selection.bg_fill = if dark {
            Color32::from_rgb(24, 58, 51)
        } else {
            Color32::from_rgb(218, 239, 232)
        };
        style.visuals.selection.stroke = egui::Stroke::new(
            1.0,
            if dark {
                Color32::from_rgb(99, 211, 180)
            } else {
                Color32::from_rgb(8, 126, 112)
            },
        );
        style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(7);
        style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(7);
        style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(7);
    });
}
fn accent(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(99, 211, 180)
    } else {
        Color32::from_rgb(8, 126, 112)
    }
}
fn card<R>(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    card_with_min_height(ui, 0.0, body)
}
fn card_with_min_height<R>(
    ui: &mut egui::Ui,
    height: f32,
    body: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(ui.visuals().window_fill())
        .stroke(egui::Stroke::new(
            1.0,
            ui.visuals().widgets.noninteractive.bg_stroke.color,
        ))
        .corner_radius(13)
        .inner_margin(20)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            // The requested outer height includes both one-point frame borders.
            ui.set_min_height((height - 42.0).max(0.0));
            body(ui)
        })
        .inner
}
fn primary(ui: &mut egui::Ui, text: &str, enabled: bool) -> bool {
    let color = accent(ui);
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(text).color(if ui.visuals().dark_mode {
            Color32::from_rgb(16, 37, 31)
        } else {
            Color32::WHITE
        }))
        .fill(color),
    )
    .clicked()
}
fn heading(ui: &mut egui::Ui, title: &str, note: &str) {
    ui.heading(title);
    ui.label(RichText::new(note).weak());
    ui.add_space(12.0);
}
fn line(ui: &mut egui::Ui, label: &str, value: &str) {
    let label_width = 148.0_f32.min(ui.available_width() * 0.4);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(label_width, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_min_width(label_width);
                ui.add(egui::Label::new(RichText::new(label).weak()).wrap());
            },
        );
        ui.add(egui::Label::new(value).wrap());
    });
}
fn path_input(ui: &mut egui::Ui, value: &mut String) -> egui::Response {
    let mut shown = display_path_text(value);
    let response = ui.add(singleline(&mut shown, f32::INFINITY));
    // Merely rendering a friendly path must not alter extended-path semantics.
    if response.changed() {
        *value = shown;
    }
    response
}
fn mode_name(mode: ProxyMode) -> &'static str {
    match mode {
        ProxyMode::Direct => "不接管系统代理",
        ProxyMode::Global => "使用本机 HTTP 代理",
        ProxyMode::Pac => "使用 PAC 自动代理",
    }
}
pub(super) fn command_name(command: &Command) -> &'static str {
    match command {
        Command::ValidateDraft { .. } => "检查配置",
        Command::ApplyProfile { .. } => "应用配置",
        Command::CreateProfile { .. } => "创建配置",
        Command::DeleteProfile { .. } => "删除配置",
        Command::StageHistory { .. } => "载入历史",
        Command::SaveSettings { .. } => "保存设置",
        Command::StopCore => "停止 Core",
        Command::RestartCore => "重启 Core",
        Command::SetProxyMode { .. } => "切换系统代理",
        Command::Shutdown => "退出程序",
        Command::TestRoute { .. } | Command::TestRunningRoute { .. } => "路由静态预测",
        Command::TestPac { .. } => "PAC 执行测试",
        Command::TestLatency { .. } => "TCP 延迟探测",
        Command::GetOperation { .. } | Command::GetOperationDetails { .. } => "查询操作结果",
        _ => "读取后台状态",
    }
}
fn error_advice(code: &str) -> &'static str {
    match code {
        "CONNECTION_LOST" | "IPC_TIMEOUT" => "后台响应中断，请先查询原操作结果，再进行其他变更。",
        "RECOVERY_REQUIRED" | "OPERATION_RECOVERY_REQUIRED" | "PROXY_RECOVERY_REQUIRED" => {
            "存在未完成的恢复，请检查状态和证据；不要覆盖其他程序的设置。"
        }
        "PROXY_CONFLICT" | "PROXY_OWNERSHIP_CONFLICT" | "PROXY_LEASE_CONFLICT" => {
            "系统代理已被其他程序修改，请先核对代理归属。"
        }
        "GENERATION_CONFLICT" | "REVISION_CONFLICT" => {
            "运行版本或已保存配置已变化，请刷新状态并重新检查。"
        }
        "HTTP_READY_FAILED" | "LOCAL_PROXY_NOT_READY" | "CORE_START_FAILED" => {
            "Core 或本机入口未就绪，请检查端口占用与 Core 日志，修正后重新检查。"
        }
        "PROXY_ENDPOINT_INVALID" => {
            "接管需要无认证、固定数值端口的回环 HTTP 入口，请检查运行配置及入站 Tag。"
        }
        "INVALID_CORE_PATH" | "INVALID_CORE" | "CORE_INTEGRITY_FAILED" => {
            "Core 路径或文件校验失败，请重新选择或修复官方依赖，再检查配置。"
        }
        "CORE_VALIDATION_FAILED" | "CONFIG_PARSE_FAILED" | "STATIC_VALIDATION_FAILED" => {
            "配置检查未通过，请修正 JSON 或资源路径后重试。"
        }
        "DRAFT_SAVE_FAILED" | "DRAFT_RECOVERY_REQUIRED" => {
            "草稿尚未写入磁盘，修改仍保留在界面中。请检查数据目录权限，或先导出草稿。"
        }
        "PROFILE_IN_USE" => "该配置仍由上次应用版本引用，请先应用另一份配置后再删除。",
        "DEPENDENCY_INSTALL_FAILED" => {
            "依赖安装失败，请检查网络和官方版本号；已有运行版本不会自动替换。"
        }
        _ => "操作未能完成，请查看详情与日志，修正原因后再继续。",
    }
}

impl App {
    pub(super) fn is_running(&self) -> bool {
        self.status
            .as_ref()
            .is_some_and(|s| s.core_state == "Running")
    }
    pub(super) fn committed_id(&self) -> Option<&str> {
        self.status
            .as_ref()
            .and_then(|s| s.generation.as_ref())
            .map(|g| g.candidate.profile_id.as_str())
    }
    pub(super) fn connection_restored(&mut self) {
        if !self.ui.connected {
            self.add_event("信息", "后台已连接", String::new());
            if self.ui.uncertain.is_none() {
                self.message.clear();
            }
        }
        self.ui.connected = true;
        if self.ui.uncertain.is_none()
            && self
                .ui
                .failure
                .as_ref()
                .is_some_and(|f| f.error.code == "CONNECTION_LOST")
        {
            self.ui.failure = None;
        }
    }
    pub(super) fn has_draft_conflict(&self) -> bool {
        self.ui.draft_conflict
    }
    pub(super) fn write_blocker(&self) -> Option<&'static str> {
        if !self.ui.connected {
            Some("后台未连接，当前展示最后已知状态")
        } else if self.ui.uncertain.is_some() {
            Some("上次操作结果待确认，请先查询")
        } else if self.busy {
            Some("正在执行操作，请稍候")
        } else if self.shutdown_requested {
            Some("正在等待停止并退出")
        } else if self.status.as_ref().is_some_and(|s| s.recovery_required) {
            Some("恢复尚未完成，普通变更已暂停")
        } else {
            None
        }
    }
    pub(super) fn add_event(
        &mut self,
        level: &'static str,
        message: impl Into<String>,
        request: impl Into<String>,
    ) {
        self.ui.events.insert(
            0,
            Event {
                level,
                message: message.into(),
                request: request.into(),
                at: Instant::now(),
            },
        );
        self.ui.events.truncate(300);
    }
    pub(super) fn fail(&mut self, error: Error, request: String) {
        self.message = error_advice(&error.code).into();
        self.add_event(
            "错误",
            format!("{} · {}", self.message, error),
            request.clone(),
        );
        self.ui.failure = Some(Failure { error, request });
    }
    pub(super) fn clear_request_failure(&mut self) {
        // A successful backend request cannot repair local draft/settings IO.
        // Events and the backend operation record retain the failed request.
        if self
            .ui
            .failure
            .as_ref()
            .is_some_and(|failure| !failure.request.is_empty())
        {
            self.ui.failure = None;
        }
        self.failed_request = None;
    }
    pub(super) fn accept_saved_settings(&mut self, settings: Settings, assets: String) {
        self.ui
            .workspace
            .asset_roots
            .insert(settings.core_path.display().to_string(), assets.clone());
        self.ui.settings_loaded = true;
        self.ui.settings_dirty = self.core_path != settings.core_path.display().to_string()
            || self.asset_root != assets
            || self.settings.start_on_logon != settings.start_on_logon
            || self.settings.connect_on_start != settings.connect_on_start
            || self.settings.restore_last_state != settings.restore_last_state;
        self.finish_setup_save(&settings);
        self.ui.saved_settings = Some(settings);
        self.save_workspace();
    }
    pub(super) fn save_workspace(&mut self) {
        if !self.ui.workspace_valid {
            self.message =
                "原界面资料无法读取，未覆盖 gui.json；名称暂保留在当前会话，请先备份并修复该文件。"
                    .into();
            return;
        }
        if self.ui.persistence
            && let Err(error) = self.ui.workspace.save(&self.root)
        {
            self.fail(error, String::new());
        }
    }
    pub(super) fn has_draft_edits(&self) -> bool {
        self.editor != self.saved_json || !self.ui.number_inputs.is_empty()
    }
    pub(super) fn draft_edited(&mut self) {
        if !self.ui.restoring_undo {
            let now = Instant::now();
            let new_group = self
                .ui
                .undo_at
                .is_none_or(|at| now.duration_since(at) > Duration::from_millis(600));
            if new_group && self.editor != self.ui.last_seen_editor {
                self.ui.undo.push(self.ui.last_seen_editor.clone());
                if self.ui.undo.len() > 50 {
                    self.ui.undo.remove(0);
                }
                self.ui.redo.clear();
            }
            self.ui.undo_at = Some(now);
            self.ui.last_seen_editor = self.editor.clone();
        }
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.editor) {
            self.ui
                .number_inputs
                .retain(|path, input| value.pointer(path).is_some_and(|n| input.matches(n)));
        }
        self.invalidate_candidate();
        self.ui.draft_dirty_since = Some(Instant::now());
    }
    pub(super) fn reset_undo(&mut self, editor: &str) {
        self.ui.undo.clear();
        self.ui.redo.clear();
        self.ui.undo_at = None;
        self.ui.last_seen_editor = editor.into();
    }
    pub(super) fn undo_edit(&mut self) {
        if let Some(previous) = self.ui.undo.pop() {
            let current = std::mem::replace(&mut self.editor, previous);
            self.ui.redo.push(current);
            self.ui.restoring_undo = true;
            self.draft_edited();
            self.ui.restoring_undo = false;
            self.ui.last_seen_editor = self.editor.clone();
            self.ui.undo_at = None;
            self.message = "已撤销上一步编辑。".into();
        }
    }
    pub(super) fn redo_edit(&mut self) {
        if let Some(next) = self.ui.redo.pop() {
            let current = std::mem::replace(&mut self.editor, next);
            self.ui.undo.push(current);
            self.ui.restoring_undo = true;
            self.draft_edited();
            self.ui.restoring_undo = false;
            self.ui.last_seen_editor = self.editor.clone();
            self.ui.undo_at = None;
            self.message = "已重做编辑。".into();
        }
    }
    /// Snapshot the visible editor and retry every pending write, including other profiles.
    pub(super) fn stash_draft(&mut self) -> bool {
        if !self.profile_id.is_empty() && !self.ui.deleted_profiles.contains(&self.profile_id) {
            let dirty = self.has_draft_edits() || self.ui.draft_conflict;
            if dirty {
                let base_revision = if self.ui.draft_conflict {
                    self.ui
                        .drafts
                        .get(&self.profile_id)
                        .map(|d| d.base_revision.clone())
                        .unwrap_or_else(|| self.revision.clone())
                } else {
                    self.revision.clone()
                };
                self.ui.drafts.insert(
                    self.profile_id.clone(),
                    Draft {
                        number_inputs: self.ui.number_inputs.clone(),
                        base_revision,
                        json: self.editor.clone(),
                    },
                );
                self.ui.pending_draft_writes.insert(self.profile_id.clone());
            } else if !self.ui.unreadable_drafts.contains(&self.profile_id) {
                self.ui.drafts.remove(&self.profile_id);
                self.ui.pending_draft_writes.insert(self.profile_id.clone());
            }
        }
        self.flush_pending_drafts()
    }
    fn flush_pending_drafts(&mut self) -> bool {
        let pending: Vec<_> = self.ui.pending_draft_writes.iter().cloned().collect();
        for profile in pending {
            let result = if !self.ui.persistence {
                Ok(())
            } else if self.ui.unreadable_drafts.contains(&profile) {
                Err(Error::new(
                    "DRAFT_RECOVERY_REQUIRED",
                    "原草稿无法读取，请先导出当前修改，或明确选择保留 / 放弃草稿。",
                ))
            } else if let Some(draft) = self.ui.drafts.get(&profile) {
                draft.save(&self.root, &profile)
            } else {
                Draft::remove(&self.root, &profile)
            };
            match result {
                Ok(()) => {
                    self.ui.pending_draft_writes.remove(&profile);
                }
                Err(error) => self.fail(
                    Error::new(
                        "DRAFT_SAVE_FAILED",
                        format!(
                            "配置 {} 的草稿未保存：{error}",
                            self.ui.workspace.name(&profile)
                        ),
                    ),
                    String::new(),
                ),
            }
        }
        let saved = self.ui.pending_draft_writes.is_empty();
        if saved
            && self
                .ui
                .failure
                .as_ref()
                .is_some_and(|f| f.error.code == "DRAFT_SAVE_FAILED")
        {
            self.ui.failure = None;
            self.message.clear();
        }
        if saved && matches!(self.ui.dialog, Some(Dialog::UnsavedExit(_))) {
            self.ui.dialog = None;
        }
        self.ui.draft_dirty_since = if saved { None } else { Some(Instant::now()) };
        saved
    }
    fn draft_state(&self) -> String {
        let drafts: std::collections::BTreeMap<_, _> = self.ui.drafts.iter().collect();
        hash(
            &serde_json::to_vec(&(
                &self.profile_id,
                &self.editor,
                &self.revision,
                &self.ui.number_inputs,
                drafts,
            ))
            .unwrap(),
        )
    }
    pub(super) fn open_history_diff(&mut self, preview: HistoryPreview) {
        self.ui.history_preview = Some(preview);
        self.ui.dialog = Some(Dialog::HistoryDiff);
    }
    pub(super) fn load_history_preview(&mut self) -> bool {
        let Some(preview) = self.ui.history_preview.take() else {
            return false;
        };
        self.editor = preview.json;
        self.ui.number_inputs.clear();
        self.invalidate_candidate();
        self.draft_edited();
        self.stash_draft();
        self.message = "历史版本已载入为草稿；请重新检查并预览。".into();
        true
    }
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    pub(super) fn open_exit_dialog(&mut self) {
        self.ui.exit_choice = match self.ui.workspace.exit_default {
            crate::workspace::ExitDefault::Gui => ExitKind::Gui,
            crate::workspace::ExitDefault::Shutdown => ExitKind::Shutdown,
        };
        self.ui.exit_remember = true;
        self.ui.dialog = Some(Dialog::ExitChoice);
    }
    pub(super) fn request_exit(&mut self, kind: ExitKind) {
        if self.exit
            || (matches!(kind, ExitKind::Gui)
                && (self.busy || self.shutdown_requested || self.ui.uncertain.is_some()))
        {
            return;
        }
        if matches!(kind, ExitKind::Shutdown) {
            if let Some(status) = &self.status {
                let running = status.core_state == "Running";
                if self.ui.workspace.last_core_running != running {
                    self.ui.workspace.last_core_running = running;
                    self.save_workspace();
                }
            }
            self.shutdown_requested = true;
        } else if self.drafts_ready_for_exit(kind) {
            self.exit = true;
        }
    }
    pub(super) fn drafts_ready_for_exit(&mut self, kind: ExitKind) -> bool {
        if self.ui.discarded_draft_state.as_ref() == Some(&self.draft_state()) || self.stash_draft()
        {
            return true;
        }
        self.shutdown_requested = false;
        self.ui.dialog = Some(Dialog::UnsavedExit(kind));
        self.message = "草稿未保存，已取消退出。请重试保存、导出或明确放弃未保存修改。".into();
        if let Some(wakeup) = &self.wakeup {
            wakeup.reveal();
        }
        false
    }
    fn discard_unsaved_and_exit(&mut self, kind: ExitKind) {
        // Consent only applies to this exact editor/draft state. Later edits need a new save gate.
        self.ui.discarded_draft_state = Some(self.draft_state());
        self.request_exit(kind);
    }
    fn unsaved_drafts_json(&self) -> serde_json::Value {
        let drafts: std::collections::BTreeMap<_, _> = self
            .ui
            .pending_draft_writes
            .iter()
            .map(|profile| (profile, self.ui.drafts.get(profile)))
            .collect();
        serde_json::json!({"format": "xray-pilot-unsaved-drafts-v1", "drafts": drafts})
    }
    #[cfg(any(windows, test))]
    fn export_unsaved_drafts(&mut self, destination: &std::path::Path) -> bool {
        match xray_config::write_json(destination, &self.unsaved_drafts_json()) {
            Ok(()) => {
                self.message = format!(
                    "未保存草稿已导出到 {}；可返回编辑或重试保存。",
                    display_path(destination)
                );
                true
            }
            Err(error) => {
                self.fail(error, String::new());
                false
            }
        }
    }
    pub(super) fn flush_draft_if_due(&mut self) {
        if self
            .ui
            .draft_dirty_since
            .is_some_and(|since| since.elapsed() > Duration::from_millis(800))
        {
            self.stash_draft();
        }
    }
    pub(super) fn restore_draft(&mut self) {
        self.ui.number_inputs.clear();
        self.ui.deleted_profiles.remove(&self.profile_id);
        self.ui.profile_cache.insert(
            self.profile_id.clone(),
            Profile {
                id: self.profile_id.clone(),
                json: self.saved_json.clone(),
                revision: self.revision.clone(),
            },
        );
        if !self.ui.drafts.contains_key(&self.profile_id) && self.ui.persistence {
            match Draft::load(&self.root, &self.profile_id) {
                Ok(Some(draft)) => {
                    self.ui.drafts.insert(self.profile_id.clone(), draft);
                }
                Ok(None) => {}
                Err(error) => {
                    self.ui.unreadable_drafts.insert(self.profile_id.clone());
                    self.fail(error, String::new());
                }
            }
        }
        self.ui.draft_conflict = false;
        if let Some(draft) = self.ui.drafts.get(&self.profile_id) {
            if draft.json != self.saved_json || !draft.number_inputs.is_empty() {
                self.editor = draft.json.clone();
                self.ui.number_inputs = draft.number_inputs.clone();
                self.ui.draft_conflict = draft.base_revision != self.revision;
            } else {
                self.ui.drafts.remove(&self.profile_id);
            }
        }
    }
    pub(super) fn switch_profile(&mut self, id: String) {
        self.stash_draft();
        if !self.ui.connected {
            if let Some(profile) = self.ui.profile_cache.get(&id).cloned() {
                self.load(profile);
            } else {
                self.message = "此配置尚未缓存，请重新连接后台后读取。".into();
            }
        } else {
            self.send(Command::GetProfile { profile_id: id });
        }
    }
    pub(super) fn profile_deleted(&mut self, profile: &str) {
        self.ui.deleted_profiles.insert(profile.into());
        self.ui.drafts.remove(profile);
        self.ui.profile_cache.remove(profile);
        self.ui.workspace.names.remove(profile);
        self.ui.workspace.node_names.remove(profile);
        self.save_workspace();
        if self.ui.persistence
            && let Err(error) = Draft::remove(&self.root, profile)
        {
            self.fail(error, String::new());
        }
        if self.profile_id == profile && self.selected_profile == profile {
            self.profile_id.clear();
            self.selected_profile.clear();
            self.saved_json.clear();
            self.editor.clear();
            self.ui.number_inputs.clear();
            self.revision.clear();
            self.view_epoch = self.view_epoch.wrapping_add(1);
            self.invalidate_candidate();
        }
        self.refresh_profiles();
    }
    pub(super) fn applied_draft(&mut self, profile_id: &str, json: Option<&str>) {
        let Some(json) = json else {
            return;
        };
        let revision = hash(json.as_bytes());
        if self.profile_id == profile_id {
            self.saved_json = json.into();
            self.revision = revision.clone();
            self.ui.draft_conflict = false;
            self.stash_draft();
        } else if let Some(draft) = self.ui.drafts.get_mut(profile_id) {
            if draft.json == json && draft.number_inputs.is_empty() {
                self.ui.drafts.remove(profile_id);
            } else {
                draft.base_revision = revision;
            }
            self.ui.pending_draft_writes.insert(profile_id.into());
            self.flush_pending_drafts();
        }
        self.ui.profile_cache.insert(
            profile_id.into(),
            Profile {
                id: profile_id.into(),
                json: json.into(),
                revision: hash(json.as_bytes()),
            },
        );
    }
    #[cfg(any(windows, test))]
    pub(super) fn open_profile_from_tray(&mut self, profile: &str, ctx: &egui::Context) {
        if self.busy
            || self.shutdown_requested
            || self.ui.uncertain.is_some()
            || !self.profiles.iter().any(|p| p.id == profile)
        {
            return;
        }
        self.ui.page = Page::Profiles;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        if self.selected_profile == profile {
            return;
        }
        if self.has_draft_edits() {
            self.ui.dialog = Some(Dialog::Confirm(Action::Switch(profile.into())));
        } else {
            self.switch_profile(profile.into());
        }
    }
    pub(super) fn begin_create(&mut self, import: bool, duplicate: bool) {
        self.ui.new_name = if duplicate {
            format!("{} 副本", self.ui.workspace.name(&self.profile_id))
        } else {
            String::new()
        };
        self.ui.new_json = if duplicate {
            self.editor.clone()
        } else {
            default_config()
        };
        self.ui.import_json = import || duplicate;
        self.ui.new_scene = Default::default();
        self.ui.new_template = Default::default();
        self.ui.new_guided_started = false;
        self.ui.new_editor_epoch = self.ui.new_editor_epoch.wrapping_add(1);
        self.ui.new_node_names = if duplicate {
            self.ui
                .workspace
                .node_names
                .get(&self.profile_id)
                .cloned()
                .unwrap_or_default()
        } else {
            Default::default()
        };
        self.ui.import_error = None;
        self.ui.import_source = None;
        self.ui.import_use_source_assets = false;
        self.ui.dialog = Some(Dialog::Create);
    }
    pub(super) fn imported_profile(&mut self, profile: Profile) {
        let needs_review = xray_config::parse_document(profile.json.as_bytes())
            .is_ok_and(|value| !xray_config::inspect_config(&value).is_empty());
        if self
            .ui
            .failure
            .as_ref()
            .is_some_and(|f| f.error.code == "STATIC_VALIDATION_FAILED")
        {
            self.ui.failure = None;
        }
        self.load(profile);
        self.clear_request_failure();
        if needs_review {
            self.ui.page = Page::Profiles;
            self.ui.profile_tab = ProfileTab::Summary;
            self.message = "配置原文已保留，请按字段提示补充 / 核对后检查；尚未应用。".into();
        } else {
            self.message = "配置已添加，尚未应用".into();
        }
    }
    pub(super) fn validate_draft(&mut self) {
        if !self.ui.number_inputs.is_empty() {
            self.message = "有未完成的数值输入，请在高级字段中补全或明确放弃草稿后再检查。".into();
            return;
        }
        if self.ui.draft_conflict {
            self.message = "草稿基于旧版本，请先确认如何处理草稿冲突。".into();
            return;
        }
        self.send(Command::ValidateDraft {
            profile_id: self.profile_id.clone(),
            expected_revision: self.revision.clone(),
            json: self.editor.clone(),
            core_path: self.core_path.clone().into(),
            asset_root: self.asset_root.clone().into(),
            allow_dynamic: self.ui.allow_dynamic,
        });
    }
    pub(super) fn finish_setup_save(&mut self, settings: &Settings) {
        if self.ui.advance_setup_after_save {
            self.ui.advance_setup_after_save = false;
            if settings.core_path.display().to_string() == self.core_path
                && !self.core_path.is_empty()
            {
                self.ui.setup_step = 1;
            }
        }
    }
    pub(super) fn resolve_operation(&mut self, request_id: &str, value: serde_json::Value) {
        self.ui.operation_details = serde_json::to_string_pretty(&value).unwrap_or_default();
        let Some(uncertain) = &self.ui.uncertain else {
            self.message = "已读取操作记录。".into();
            return;
        };
        if uncertain.request_id != request_id {
            return;
        }
        if value.is_null() {
            self.message = "后台尚无可确认的终态记录，请保留请求身份并继续查询。".into();
            return;
        }
        let identity = value
            .get("request")
            .and_then(|r| r.get("request_id"))
            .or_else(|| value.get("request_id"))
            .and_then(|v| v.as_str());
        if identity != Some(request_id) {
            self.message = "返回记录与原请求身份不一致，结果仍待确认。".into();
            return;
        }
        let response = value
            .get("response")
            .cloned()
            .and_then(|v| serde_json::from_value::<Response>(v).ok());
        let summary_state = value.get("state").and_then(|v| v.as_str());
        if response
            .as_ref()
            .is_some_and(|r| r.request_id != request_id)
        {
            self.message = "响应身份不一致，原操作结果仍待确认。".into();
            return;
        }
        let expected = serde_json::to_value(&uncertain.command).unwrap_or_default();
        if value
            .pointer("/request/command")
            .is_some_and(|command| command != &expected)
            || value
                .get("command")
                .is_some_and(|command| Some(command) != expected.get("operation"))
        {
            self.message = "操作内容与原请求不一致，结果仍待确认。".into();
            return;
        }
        let terminal = response.is_some() || matches!(summary_state, Some("Succeeded" | "Failed"));
        if !terminal {
            self.message = "原操作尚未形成终态，请检查恢复状态后继续查询。".into();
            self.send(Command::GetStatus);
            return;
        }
        let uncertain = self.ui.uncertain.take().unwrap();
        let success = response.as_ref().is_some_and(|r| r.result.is_ok())
            || summary_state == Some("Succeeded");
        self.failed_request = None;
        if success {
            self.clear_request_failure();
            match uncertain.command {
                Command::ApplyProfile { .. } => {
                    // The captured JSON, rather than today's selected editor, was applied.
                    self.applied_draft(&uncertain.profile, uncertain.json.as_deref());
                    self.invalidate_candidate();
                    self.ui.onboarding = false;
                    self.refresh_profiles();
                    if self.selected_profile == uncertain.profile {
                        self.send(Command::GetProfile {
                            profile_id: uncertain.profile,
                        });
                    }
                }
                Command::CreateProfile { profile_id, .. } => {
                    self.send(Command::GetProfile { profile_id });
                    self.refresh_profiles();
                }
                Command::DeleteProfile { profile_id, .. } => self.profile_deleted(&profile_id),
                Command::SaveSettings { settings } => {
                    self.accept_saved_settings(settings, uncertain.assets);
                }
                Command::Shutdown => {
                    self.shutdown_requested = false;
                    self.request_exit(ExitKind::Gui);
                }
                _ => {
                    self.invalidate_candidate();
                    self.refresh_profiles();
                }
            }
            self.message = "已确认原操作成功，正在读取最新状态。".into();
            self.add_event("信息", "原请求查询确认成功", request_id);
        } else {
            let error = response
                .and_then(|r| r.result.err())
                .or_else(|| {
                    value
                        .get("error")
                        .cloned()
                        .and_then(|v| serde_json::from_value(v).ok())
                })
                .unwrap_or_else(|| Error::new("OPERATION_FAILED", "The original operation failed"));
            self.fail(error, request_id.into());
            self.failed_request = Some(request_id.into());
        }
        self.send(Command::GetStatus);
    }
}
