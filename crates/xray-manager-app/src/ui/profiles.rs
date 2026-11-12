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
            self.has_draft_edits()
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
            if self.has_draft_edits() {
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
                ui.label("选择使用场景和节点模板，填写连接信息；也可导入已有 JSON。添加不会自动启动 Core。");
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
                    RichText::new(if self.has_draft_edits() {
                        "未应用草稿"
                    } else {
                        "已保存"
                    })
                    .small()
                    .weak(),
                );
                if ui.button("修改名称").clicked() {
                    self.ui.rename = self.ui.workspace.name(&self.profile_id).into();
                    self.ui.dialog = Some(Dialog::Rename(self.profile_id.clone()));
                }
                if ui
                    .add_enabled(
                        enabled && self.ui.number_inputs.is_empty(),
                        egui::Button::new("复制"),
                    )
                    .on_hover_text("请先补全未完成的数值输入，再复制配置")
                    .clicked()
                {
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
                if ui
                    .add_enabled(
                        self.ui.number_inputs.is_empty(),
                        egui::Button::new("导出 JSON"),
                    )
                    .on_hover_text("请先补全未完成的数值输入，再导出 JSON；输入已随草稿保留")
                    .clicked()
                {
                    self.ui.dialog = Some(Dialog::Confirm(Action::ExportJson));
                }
            });
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
                    (ProfileTab::Inbounds, "本地入口"),
                    (ProfileTab::Outbounds, "节点"),
                    (ProfileTab::Routing, "分流规则"),
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
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    enabled && !self.ui.undo.is_empty(),
                    egui::Button::new("撤销编辑"),
                )
                .clicked()
            {
                self.undo_edit();
            }
            if ui
                .add_enabled(
                    enabled && !self.ui.redo.is_empty(),
                    egui::Button::new("重做"),
                )
                .clicked()
            {
                self.redo_edit();
            }
            if ui
                .add_enabled(
                    enabled && self.has_draft_edits(),
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
        if let Some(path) = self.ui.number_inputs.keys().next().cloned() {
            ui.add(
                egui::Label::new(format!(
                    "有 {} 项未完成的数值输入，已随草稿保留。补全后可检查应用。",
                    self.ui.number_inputs.len()
                ))
                .wrap(),
            );
            if ui.button("定位未完成数值").clicked() {
                self.ui.profile_tab = if path.starts_with("/inbounds/") {
                    ProfileTab::Inbounds
                } else if path.starts_with("/outbounds/") {
                    ProfileTab::Outbounds
                } else {
                    ProfileTab::Routing
                };
                self.ui.guided.native = true;
                self.ui.guided.focus = Some(path);
            }
        }
        match self.ui.profile_tab {
            ProfileTab::Summary => {
                if let Ok(value) = xray_config::parse_document(self.editor.as_bytes()) {
                    if let Some(path) = config_review_action(ui, &value) {
                        self.ui.profile_tab = if path.starts_with("/outbounds") {
                            ProfileTab::Outbounds
                        } else if path.starts_with("/routing") {
                            ProfileTab::Routing
                        } else if path.starts_with("/inbounds") {
                            ProfileTab::Inbounds
                        } else {
                            ProfileTab::Json
                        };
                        self.ui.guided.native = false;
                        self.ui.guided.notice = Some(format!("待补充字段：{path}"));
                        self.ui.guided.focus = Some(path);
                    }
                    let names = self
                        .ui
                        .workspace
                        .node_names
                        .get(&self.profile_id)
                        .cloned()
                        .unwrap_or_default();
                    crate::guided_config::summary(ui, &value, &names);
                } else {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "JSON 语法无法解析，请在原始 JSON 中修正。",
                    );
                }
                ui.add_space(12.0);
                ui.label("编辑不会立即切换连接");
                ui.label(RichText::new("节点按协议填写，分流规则按顺序匹配。高级字段与原始 JSON 保留，所有编辑指向同一份草稿。").weak());
                if ui.button("编辑原始 JSON").clicked() {
                    self.ui.profile_tab = ProfileTab::Json;
                }
                ui.add_space(10.0);
                if ui
                    .checkbox(
                        &mut self.ui.allow_dynamic,
                        "允许动态外部依赖路径（不保证回滚内容）",
                    )
                    .on_hover_text("默认要求相对资源路径并冻结副本；显式允许后，应用与启动前会重新核对观察哈希，自动连接与自动恢复被禁用")
                    .changed()
                {
                    self.invalidate_candidate();
                }
                ui.add_enabled_ui(enabled, |ui| {
                    self.proxy_draft_controls(ui);
                });
            }
            ProfileTab::Json => {
                if ui
                    .add_enabled(
                        enabled && self.ui.number_inputs.is_empty(),
                        egui::Button::new("格式化 JSON"),
                    )
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
                    .add_enabled_ui(enabled && self.ui.number_inputs.is_empty(), |ui| {
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
                ui.label(
                    RichText::new(
                        "历史版本按提交顺序展示。先比较差异，再决定是否载入为草稿；载入后仍需检查并应用。",
                    )
                    .small()
                    .weak(),
                );
                if self.history.is_empty() {
                    ui.label("暂无已提交的历史版本。");
                }
                for g in self.history.clone() {
                    let current = self.committed_id() == Some(g.id.as_str());
                    ui.horizontal_wrapped(|ui| {
                        ui.monospace(format!(
                            "{} · {} · {}{}{}",
                            g.id.chars().take(8).collect::<String>(),
                            g.candidate.core.version,
                            mode_name(g.proxy_mode),
                            g.pac
                                .as_ref()
                                .map(|p| format!(
                                    " · PAC {}",
                                    p.revision.chars().take(8).collect::<String>()
                                ))
                                .unwrap_or_default(),
                            if current { " · 当前运行" } else { "" }
                        ));
                        if ui
                            .add_enabled(enabled, egui::Button::new("比较并载入…"))
                            .clicked()
                        {
                            if !self.ui.number_inputs.is_empty() {
                                self.message =
                                    "请先补全未完成的数值输入，或明确放弃草稿，再比较历史。".into();
                            } else {
                                self.message = "正在准备历史与当前草稿的比较…".into();
                                self.send(Command::StageHistory {
                                    generation_id: g.id.clone(),
                                    expected_revision: self.revision.clone(),
                                });
                            }
                        }
                    });
                }
            }
            tab => {
                let section = match tab {
                    ProfileTab::Inbounds => "inbounds",
                    ProfileTab::Outbounds => "outbounds",
                    _ => "rules",
                };
                if let Ok(mut value) = serde_json::from_str(&self.editor) {
                    let mut names = self
                        .ui
                        .workspace
                        .node_names
                        .get(&self.profile_id)
                        .cloned()
                        .unwrap_or_default();
                    if let Some(notice) = &self.ui.guided.notice {
                        ui.add(
                            egui::Label::new(
                                RichText::new(notice).color(ui.visuals().warn_fg_color),
                            )
                            .wrap(),
                        );
                    }
                    let pending_focus = self.ui.guided.focus.clone();
                    let mut changed = false;
                    let mut names_changed = false;
                    ui.push_id((&self.profile_id, self.view_epoch), |ui| {
                        ui.add_enabled_ui(enabled, |ui| {
                            if tab != ProfileTab::Inbounds {
                                ui.horizontal_wrapped(|ui| {
                                    ui.selectable_value(
                                        &mut self.ui.guided.native,
                                        false,
                                        "常用表单",
                                    );
                                    ui.selectable_value(
                                        &mut self.ui.guided.native,
                                        true,
                                        "高级：原生字段编辑",
                                    );
                                });
                            }
                            if tab == ProfileTab::Inbounds || self.ui.guided.native {
                                changed |= crate::native_editor::edit_focused(
                                    ui,
                                    &mut value,
                                    section,
                                    pending_focus.as_deref(),
                                    &mut self.ui.number_inputs,
                                );
                                self.ui.guided.focus = None;
                            } else if self.ui.number_inputs.is_empty() {
                                let result = if tab == ProfileTab::Outbounds {
                                    crate::guided_config::edit_nodes(
                                        ui,
                                        &mut value,
                                        &mut names,
                                        &mut self.ui.guided,
                                    )
                                } else {
                                    crate::guided_config::edit_routes(
                                        ui,
                                        &mut value,
                                        &names,
                                        &mut self.ui.guided,
                                    )
                                };
                                changed |= result.changed;
                                names_changed = result.names_changed;
                                if result.raw {
                                    self.ui.guided.native = true;
                                    self.ui.guided.focus = pending_focus;
                                }
                            }
                        });
                    });
                    if names_changed {
                        self.ui
                            .workspace
                            .node_names
                            .insert(self.profile_id.clone(), names);
                        self.save_workspace();
                    }
                    if changed {
                        self.ui.guided.notice = None;
                        self.editor = serde_json::to_string_pretty(&value).unwrap();
                        self.draft_edited();
                    }
                } else {
                    ui.label("请先在原始 JSON 中修正语法。");
                }
                if tab == ProfileTab::Routing
                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.editor)
                {
                    let conflicts = xray_config::conflicts::conflicts(&value);
                    if !conflicts.is_empty() {
                        ui.separator();
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            format!("发现 {} 项静态顺序/遮蔽风险", conflicts.len()),
                        );
                        for conflict in conflicts {
                            ui.label(
                                RichText::new(format!(
                                    "规则 {}：{}",
                                    conflict.index, conflict.message
                                ))
                                .small(),
                            );
                        }
                    }
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
                for mode in [ProxyMode::Direct, ProxyMode::Global, ProxyMode::Pac] {
                    ui.selectable_value(&mut self.mode, mode, mode_name(mode));
                }
            });
        if matches!(self.mode, ProxyMode::Global | ProxyMode::Pac) {
            ui.label("接管系统代理使用的本地 HTTP 入口");
            let listeners: Vec<_> = serde_json::from_str::<serde_json::Value>(&self.editor)
                .ok()
                .and_then(|v| v.get("inbounds").and_then(|a| a.as_array()).cloned())
                .unwrap_or_default()
                .into_iter()
                .filter(|v| matches!(v["protocol"].as_str(), Some("http" | "mixed")))
                .filter_map(|v| {
                    let tag = v["tag"].as_str()?.to_owned();
                    Some((
                        tag.clone(),
                        format!(
                            "{}:{} · {}",
                            v["listen"].as_str().unwrap_or("默认监听"),
                            v.get("port")
                                .map(|p| p.to_string())
                                .unwrap_or_else(|| "待补充端口".into()),
                            tag
                        ),
                    ))
                })
                .collect();
            let selected = listeners
                .iter()
                .find(|(tag, _)| tag == &self.http_tag)
                .map(|(_, label)| label.clone())
                .unwrap_or_else(|| "请选择本地 HTTP 入口".into());
            egui::ComboBox::from_id_salt("proxy-http-listener")
                .width(ui.available_width().min(420.0))
                .selected_text(selected)
                .show_ui(ui, |ui| {
                    for (tag, label) in &listeners {
                        ui.selectable_value(&mut self.http_tag, tag.clone(), label);
                    }
                });
            if listeners.is_empty() {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "请先在本地入口中添加回环 HTTP 监听。应用前还会检查端口占用和入口兼容性。",
                );
            }
        }
        if self.mode == ProxyMode::Pac && self.pac_draft().script.trim().is_empty() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "PAC 脚本为空；可在 PAC 页生成或填写后再应用。",
            );
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
