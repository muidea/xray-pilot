use super::*;

impl App {
    pub(in crate::app) fn profiles_page(&mut self, ui: &mut egui::Ui, viewport_height: f32) {
        let page_top = ui.cursor().top();
        heading(
            ui,
            "配置",
            "选择、编辑与运行互相独立；每次应用前，先检查并确认变更。",
        );
        let enabled = self.write_blocker().is_none();
        if ui.available_width() >= 780.0 {
            let picker_width = 216.0;
            let height = (viewport_height - (ui.cursor().top() - page_top)).max(400.0);
            let editor_width = ui.available_width() - picker_width - ui.spacing().item_spacing.x;
            ui.horizontal_top(|ui| {
                let picker = ui.allocate_ui_with_layout(
                    egui::vec2(picker_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.profile_picker(ui, enabled, true),
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(editor_width, height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| self.profile_editor(ui, enabled, picker.response.rect.height()),
                );
            });
        } else {
            self.profile_picker(ui, enabled, false);
            ui.add_space(16.0);
            let height = (viewport_height - (ui.cursor().top() - page_top)).max(400.0);
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), height),
                egui::Layout::top_down(egui::Align::Min),
                |ui| self.profile_editor(ui, enabled, 0.0),
            );
        }
    }
    fn profile_picker(&mut self, ui: &mut egui::Ui, enabled: bool, vertical: bool) {
        card(ui, |ui| {
            ui.label(
                RichText::new(format!("我的配置 · {}", self.profiles.len()))
                    .small()
                    .weak(),
            );
            if vertical && !self.profiles.is_empty() {
                egui::ScrollArea::vertical()
                    .max_height(430.0)
                    .id_salt("profile-list")
                    .show(ui, |ui| {
                        for profile in self.profiles.clone() {
                            self.profile_choice(ui, profile);
                        }
                    });
            } else if !self.profiles.is_empty() {
                egui::ScrollArea::horizontal()
                    .id_salt("profile-strip")
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for profile in self.profiles.clone() {
                                self.profile_choice(ui, profile);
                            }
                        });
                    });
            }
            if vertical {
                self.profile_actions(ui, enabled, ui.available_width());
            } else {
                ui.horizontal(|ui| self.profile_actions(ui, enabled, 80.0));
            }
        });
    }
    fn profile_actions(&mut self, ui: &mut egui::Ui, enabled: bool, width: f32) {
        for label in ["新建", "导入", "刷新"] {
            if ui
                .add_enabled(
                    enabled || label == "刷新",
                    egui::Button::new(label).min_size(egui::vec2(width, CONTROL_HEIGHT)),
                )
                .clicked()
            {
                match label {
                    "新建" => self.begin_create(false, false),
                    "导入" => self.begin_create(true, false),
                    _ => {
                        self.refresh_profiles();
                        if !self.profile_id.is_empty() {
                            self.send(Command::GetProfile {
                                profile_id: self.profile_id.clone(),
                            });
                        }
                    }
                }
            }
        }
    }
    fn profile_choice(&mut self, ui: &mut egui::Ui, p: ProfileSummary) {
        let dirty = if p.id == self.profile_id {
            self.editor != self.saved_json
        } else {
            self.ui.drafts.contains_key(&p.id)
        };
        let running = self.committed_id() == Some(p.id.as_str()) && self.is_running();
        let label = format!(
            "{}\n{}{}",
            self.ui.workspace.name(&p.id),
            if running { "运行中" } else { "未运行" },
            if dirty { " · 草稿" } else { "" }
        );
        if ui
            .add_sized(
                [ui.available_width().clamp(125.0, 184.0), 56.0],
                egui::Button::new(label)
                    .wrap_mode(egui::TextWrapMode::Truncate)
                    .selected(self.selected_profile == p.id),
            )
            .clicked()
            && self.selected_profile != p.id
        {
            if self.editor != self.saved_json {
                self.ui.dialog = Some(Dialog::Confirm(Action::Switch(p.id)));
            } else {
                self.switch_profile(p.id);
            }
        }
    }
    fn profile_editor(&mut self, ui: &mut egui::Ui, enabled: bool, min_height: f32) {
        if self.profile_id.is_empty() {
            card_with_min_height(ui, min_height, |ui| {
                ui.label(RichText::new("添加第一份配置").strong().size(20.0));
                ui.label("导入服务商提供的原生 JSON，或新建本地直连模板。添加不会自动启动 Core。");
            });
            return;
        }
        card_with_min_height(ui, min_height, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(self.ui.workspace.name(&self.profile_id))
                        .strong()
                        .size(21.0),
                );
                ui.label(
                    RichText::new(if self.editor != self.saved_json {
                        "未应用草稿"
                    } else {
                        "已保存"
                    })
                    .small()
                    .weak(),
                );
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("修改名称").clicked() {
                    self.ui.rename = self.ui.workspace.name(&self.profile_id).into();
                    self.ui.dialog = Some(Dialog::Rename(self.profile_id.clone()));
                }
                if ui.add_enabled(enabled, egui::Button::new("复制")).clicked() {
                    self.begin_create(false, true);
                }
                if ui
                    .add_enabled(
                        enabled
                            && self.selected_profile == self.profile_id
                            && self.committed_id() != Some(self.profile_id.as_str()),
                        egui::Button::new("删除"),
                    )
                    .on_hover_text("上次已应用的配置仍被运行记录引用，需先应用另一份配置")
                    .clicked()
                {
                    self.ui.dialog = Some(Dialog::Confirm(Action::Delete {
                        profile: self.profile_id.clone(),
                        revision: self.revision.clone(),
                    }));
                }
                #[cfg(windows)]
                if ui.button("导出 JSON").clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .set_file_name("config.json")
                        .save_file()
                {
                    if let Err(error) = std::fs::write(path, &self.editor) {
                        self.fail(error.into(), String::new());
                    } else {
                        self.message = "配置已导出。".into();
                    }
                }
            });
            ui.label(
                RichText::new(format!(
                    "正在编辑：草稿 · 标识：{} · 编辑不会改变当前运行版本",
                    self.profile_id
                ))
                .small()
                .weak(),
            );
            if self.ui.draft_conflict {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "草稿基于旧的已保存版本。请先确认是保留草稿内容还是放弃草稿。",
                );
                ui.horizontal_wrapped(|ui| {
                    if ui.button("保留草稿，基于最新版本重新检查").clicked() {
                        self.ui.dialog =
                            Some(Dialog::Confirm(Action::AdoptDraft(self.profile_id.clone())));
                    }
                    if ui.button("放弃草稿").clicked() {
                        self.ui.dialog =
                            Some(Dialog::Confirm(Action::Discard(self.profile_id.clone())));
                    }
                });
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                for (tab, title) in [
                    (ProfileTab::Summary, "概览"),
                    (ProfileTab::Inbounds, "入站"),
                    (ProfileTab::Outbounds, "出站"),
                    (ProfileTab::Routing, "路由"),
                    (ProfileTab::Json, "原始 JSON"),
                    (ProfileTab::History, "历史"),
                ] {
                    ui.selectable_value(&mut self.ui.profile_tab, tab, title);
                }
            });
            ui.separator();
            // A small window may need page scrolling, but the editor retains
            // a usable minimum. Larger parents give all remaining height to it.
            let mut remaining = ui.available_rect_before_wrap();
            remaining.max.y = remaining.max.y.max(remaining.min.y + 300.0);
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(remaining)
                    .layout(egui::Layout::bottom_up(egui::Align::Min)),
                |ui| {
                    self.profile_footer(ui, enabled);
                    ui.separator();
                    let size = ui.available_size();
                    ui.allocate_ui_with_layout(
                        size,
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            if self.ui.profile_tab == ProfileTab::Json {
                                self.profile_body(
                                    ui,
                                    enabled && self.selected_profile == self.profile_id,
                                );
                            } else {
                                egui::ScrollArea::vertical()
                                    .id_salt("profile-body")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        self.profile_body(
                                            ui,
                                            enabled && self.selected_profile == self.profile_id,
                                        )
                                    });
                            }
                        },
                    );
                },
            );
        });
    }
    fn profile_footer(&mut self, ui: &mut egui::Ui, enabled: bool) {
        ui.label(
            RichText::new("草稿独立保存，只有确认应用后才会更新运行版本。")
                .small()
                .weak(),
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    enabled && self.editor != self.saved_json,
                    egui::Button::new("放弃草稿"),
                )
                .clicked()
            {
                self.ui.dialog = Some(Dialog::Confirm(Action::Discard(self.profile_id.clone())));
            }
            if primary(
                ui,
                "检查并预览",
                enabled
                    && !self.ui.draft_conflict
                    && !self.core_path.is_empty()
                    && self.selected_profile == self.profile_id,
            ) {
                self.validate_draft();
            }
            if self.candidate_matches() && ui.button("查看已检查的预览").clicked() {
                self.ui.preview_open = true;
            }
            if self.core_path.is_empty() && ui.button("先准备 Core").clicked() {
                self.ui.page = Page::Settings;
                self.ui.settings_tab = SettingsTab::Core;
            }
        });
        let current = self
            .committed_id()
            .map(|id| self.ui.workspace.name(id))
            .unwrap_or("无");
        ui.label(
            RichText::new(format!(
                "当前{}：{current}",
                if self.is_running() {
                    "运行"
                } else {
                    "上次应用"
                }
            ))
            .small()
            .weak(),
        );
    }
    pub(in crate::app) fn profile_body(&mut self, ui: &mut egui::Ui, enabled: bool) {
        match self.ui.profile_tab {
            ProfileTab::Summary => {
                if let Ok(value) = xray_config::parse_document(self.editor.as_bytes()) {
                    config_review(ui, &value);
                    ui.separator();
                    for (label, key) in [
                        ("入站数量", "/inbounds"),
                        ("出站数量", "/outbounds"),
                        ("路由规则", "/routing/rules"),
                    ] {
                        line(
                            ui,
                            label,
                            &value
                                .pointer(key)
                                .and_then(|v| v.as_array())
                                .map_or(0, Vec::len)
                                .to_string(),
                        );
                    }
                    line(
                        ui,
                        "默认出站",
                        value
                            .pointer("/outbounds/0/tag")
                            .and_then(|v| v.as_str())
                            .unwrap_or("未命名 / 未设置"),
                    );
                } else {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "JSON 语法无法解析，请在原始 JSON 中修正。",
                    );
                }
                ui.add_space(12.0);
                ui.label("编辑不会立即切换连接");
                ui.label(RichText::new("入站、出站和路由使用原生字段；未知字段和规则顺序保留。结构化编辑与原始 JSON 始终指向同一份草稿。").weak());
                if ui.button("编辑原始 JSON").clicked() {
                    self.ui.profile_tab = ProfileTab::Json;
                }
                ui.add_space(10.0);
                ui.add_enabled_ui(enabled, |ui| {
                    self.proxy_draft_controls(ui);
                });
            }
            ProfileTab::Json => {
                if ui
                    .add_enabled(enabled, egui::Button::new("格式化 JSON"))
                    .clicked()
                {
                    match serde_json::from_str::<serde_json::Value>(&self.editor) {
                        Ok(value) => {
                            self.editor = serde_json::to_string_pretty(&value).unwrap();
                            self.draft_edited();
                        }
                        Err(error) => self.fail(error.into(), String::new()),
                    }
                }
                let response = ui
                    .add_enabled_ui(enabled, |ui| {
                        let note_height = ui.text_style_height(&egui::TextStyle::Small)
                            + ui.spacing().item_spacing.y;
                        let height = (ui.available_height() - note_height).max(80.0);
                        json_editor(ui, &mut self.editor, "profile-json", height)
                    })
                    .inner;
                if response.changed() {
                    self.draft_edited();
                }
                ui.label(
                    RichText::new("原生 JSON · 未识别字段保留 · 应用前需真实 Core 校验")
                        .small()
                        .weak(),
                );
            }
            ProfileTab::History => {
                if self.history.is_empty() {
                    ui.label("暂无已提交的历史版本。");
                }
                for g in self.history.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.monospace(format!(
                            "{} · {}",
                            g.id.chars().take(8).collect::<String>(),
                            g.candidate.core.version
                        ));
                        if ui
                            .add_enabled(enabled, egui::Button::new("载入为草稿"))
                            .clicked()
                        {
                            self.ui.dialog = Some(Dialog::Confirm(Action::History(g.id)));
                        }
                    });
                }
                ui.label(
                    RichText::new(
                        "载入历史只替换草稿；仍需使用当前选定的 Core 重新检查、预览和确认应用。",
                    )
                    .small()
                    .weak(),
                );
            }
            tab => {
                let section = match tab {
                    ProfileTab::Inbounds => "inbounds",
                    ProfileTab::Outbounds => "outbounds",
                    _ => "rules",
                };
                if let Ok(mut value) = serde_json::from_str(&self.editor) {
                    let changed = ui
                        .add_enabled_ui(enabled, |ui| {
                            crate::native_editor::edit(ui, &mut value, section)
                        })
                        .inner;
                    if changed {
                        self.editor = serde_json::to_string_pretty(&value).unwrap();
                        self.draft_edited();
                    }
                } else {
                    ui.label("请先在原始 JSON 中修正语法。");
                }
                ui.label(
                    RichText::new(if tab == ProfileTab::Routing {
                        "按原生顺序匹配；静态预测不代表实际流量已通过该出站。"
                    } else if tab == ProfileTab::Outbounds {
                        "第一项为未命中路由规则时的默认出站，调整顺序会改变默认行为。"
                    } else {
                        "系统代理接管前，需要验证兼容的回环 HTTP 入口。"
                    })
                    .small()
                    .weak(),
                );
            }
        }
    }
    pub(in crate::app) fn proxy_draft_controls(&mut self, ui: &mut egui::Ui) {
        let before = (self.mode, self.http_tag.clone());
        ui.label("应用后的系统代理方式");
        egui::ComboBox::from_id_salt("draft-mode")
            .selected_text(mode_name(self.mode))
            .show_ui(ui, |ui| {
                for mode in [ProxyMode::Direct, ProxyMode::Global] {
                    ui.selectable_value(&mut self.mode, mode, mode_name(mode));
                }
            });
        if self.mode == ProxyMode::Global {
            ui.label("用于接管代理的 HTTP 入站 Tag");
            ui.add(singleline(&mut self.http_tag, f32::INFINITY));
        }
        ui.label(
            RichText::new("这里仅设置下次应用方式。解除接管会恢复先前设置，其他代理可能仍然有效。")
                .small()
                .weak(),
        );
        if before != (self.mode, self.http_tag.clone()) {
            self.invalidate_candidate();
        }
    }
}
