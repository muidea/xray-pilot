use super::*;

fn dialog_title(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.label(RichText::new(title).strong().size(22.0));
    if !subtitle.is_empty() {
        ui.add(egui::Label::new(RichText::new(subtitle).small().weak()).wrap());
    }
    ui.add_space(10.0);
}

fn dialog_actions(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(6.0);
}

fn danger_button(ui: &mut egui::Ui, label: &str) -> bool {
    ui.add(egui::Button::new(
        RichText::new(label).color(ui.visuals().error_fg_color),
    ))
    .clicked()
}

impl Action {
    pub(super) fn copy(&self) -> (&'static str, &'static str, &'static str) {
        match self {
            Action::Switch(_) => (
                "切换编辑对象？",
                "当前草稿会独立保留，返回此配置可继续编辑。切换编辑对象不会切换运行配置。",
                "保留草稿并切换",
            ),
            Action::Discard(_) => (
                "放弃当前草稿？",
                "恢复到最近读取的已保存版本，运行配置保持不变。",
                "放弃草稿",
            ),
            Action::Delete { .. } => (
                "删除此配置？",
                "将删除配置和本地草稿。上次已应用配置仍被引用时不能删除，请先应用另一份配置。",
                "删除配置",
            ),
            Action::Stop => (
                "停止 Core？",
                "停止当前 Core，并条件恢复本程序接管前的代理设置。其他程序的代理可能仍有效。",
                "停止 Core",
            ),
            Action::Restart => (
                "启动 / 重启上次应用版本？",
                "继续使用已应用配置与冻结的 Core。未应用草稿和下次 Core 选择不会自动生效。",
                "启动 / 重启",
            ),
            Action::Proxy(ProxyMode::Global) => (
                "接管系统代理？",
                "将系统代理指向当前运行配置的本机 HTTP 入口。后台会验证入口，真实网络与实际出口仍需另行验证。",
                "确认接管",
            ),
            Action::Proxy(ProxyMode::Direct) => (
                "恢复接管前的代理设置？",
                "释放本程序持有的代理控制权并恢复先前设置，其他程序的代理可能仍然启用。",
                "恢复先前设置",
            ),
            Action::ExportJson => (
                "导出配置？",
                "配置可能包含节点地址、UUID、密码、私钥等敏感信息；导出文件不受本程序数据目录保护，请妥善保管。",
                "导出…",
            ),
            Action::Proxy(ProxyMode::Pac) => (
                "使用 PAC 自动代理？",
                "后台会发布不可变 PAC 版本并验证后设置当前用户 PAC URL；实际客户端缓存与出口仍需另行验证。",
                "确认切换",
            ),
            Action::AdoptDraft(_) => (
                "保留草稿并重新检查？",
                "保留草稿内容，将最新已保存版本作为新的基准。请在预览中仔细核对差异，确认后才会应用。",
                "保留草稿内容",
            ),
            Action::ClearPacDomains(_) => (
                "清空 PAC 代理域名清单？",
                "将移除当前配置 PAC 模板中的全部域名，并立即更新草稿；运行版本需在步骤 5 应用后才会变化。",
                "确认清空",
            ),
            Action::ResolveProxyConflict => (
                "保留当前系统代理并解除冲突？",
                "本程序不会修改或覆盖当前 Windows 系统代理设置，仅放弃恢复先前保存的旧根快照并重新以当前系统代理作为基准，从而恢复正常操作与自启能力。",
                "保留当前设置并解除",
            ),
        }
    }
}

impl App {
    pub(in crate::app) fn dialog(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.ui.dialog.take() else {
            return;
        };
        let response = egui::Modal::new(egui::Id::new("workspace-dialog")).show(ctx, |ui| {
            let width = if matches!(dialog, Dialog::Create | Dialog::PacBatch) {
                900.0
            } else {
                680.0
            };
            ui.set_width((ctx.content_rect().width() - 100.0).clamp(300.0, width));
            match &dialog {
                Dialog::Create => self.create_dialog_contents(ui),
                Dialog::Rename(profile) => self.rename_dialog_contents(ui, profile),
                Dialog::Confirm(action) => self.confirm_dialog_contents(ui, action, ctx),
                Dialog::HistoryDiff => self.history_diff_dialog_contents(ui),
                Dialog::ExitChoice => self.exit_choice_dialog_contents(ui),
                Dialog::UnsavedExit(kind) => self.unsaved_exit_dialog_contents(ui, *kind),
                Dialog::PacBatch => self.pac_batch_dialog_contents(ui),
            }
        });
        if response.inner && !response.should_close() {
            self.ui.dialog = Some(dialog);
        }
    }
    fn unsaved_exit_dialog_contents(&mut self, ui: &mut egui::Ui, kind: ExitKind) -> bool {
        dialog_title(
            ui,
            "草稿未保存，暂未退出",
            "部分修改尚未写入磁盘；可返回编辑、重试保存，或明确放弃后退出。",
        );
        egui::ScrollArea::vertical()
            .id_salt("unsaved-exit-body")
            .max_height((ui.ctx().content_rect().height() - 260.0).max(100.0))
            .show(ui, |ui| {
                ui.label("导出可在其他可写目录保留副本；放弃后不会自动恢复这些修改。");
                ui.add_space(6.0);
                let mut profiles: Vec<_> = self.ui.pending_draft_writes.iter().collect();
                profiles.sort();
                for profile in profiles {
                    line(ui, "未保存配置", self.ui.workspace.name(profile));
                }
            });
        dialog_actions(ui);
        let mut keep_open = true;
        ui.horizontal_wrapped(|ui| {
            if ui.button("返回编辑").clicked() {
                keep_open = false;
            }
            if primary(ui, "重试保存并退出", true) {
                self.request_exit(kind);
                if matches!(kind, ExitKind::Shutdown) {
                    self.advance_shutdown();
                }
                keep_open = false;
            }
            #[cfg(windows)]
            if ui.button("导出未保存草稿…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .set_file_name("xray-pilot-unsaved-drafts.json")
                    .save_file()
            {
                self.export_unsaved_drafts(&path);
            }
            #[cfg(not(windows))]
            if ui.button("复制未保存草稿").clicked() {
                ui.ctx()
                    .copy_text(serde_json::to_string_pretty(&self.unsaved_drafts_json()).unwrap());
            }
            if danger_button(ui, "放弃未保存修改并退出") {
                self.discard_unsaved_and_exit(kind);
                keep_open = false;
            }
        });
        keep_open
    }
    fn exit_choice_dialog_contents(&mut self, ui: &mut egui::Ui) -> bool {
        dialog_title(
            ui,
            "退出 Xray Pilot",
            "请选择退出方式；默认值会在下次打开此对话框时预选。",
        );
        ui.radio_value(
            &mut self.ui.exit_choice,
            ExitKind::Gui,
            "仅退出界面，后台和 Core 继续运行",
        );
        ui.label(
            RichText::new("托盘仍可重新打开界面；适合暂时收起窗口。")
                .small()
                .weak(),
        );
        ui.add_space(6.0);
        ui.radio_value(
            &mut self.ui.exit_choice,
            ExitKind::Shutdown,
            "退出整个程序，停止 Core 并恢复代理",
        );
        ui.label(
            RichText::new("会等待当前操作完成；结果不明时先恢复连接并查询原操作。")
                .small()
                .weak(),
        );
        ui.add_space(8.0);
        ui.checkbox(&mut self.ui.exit_remember, "记住为默认退出方式");
        dialog_actions(ui);
        ui.horizontal_wrapped(|ui| {
            if ui.button("取消").clicked() {
                return false;
            }
            if primary(ui, "继续退出", !self.exit) {
                let choice = self.ui.exit_choice;
                if self.ui.exit_remember {
                    self.ui.workspace.exit_default = match choice {
                        ExitKind::Gui => crate::workspace::ExitDefault::Gui,
                        ExitKind::Shutdown => crate::workspace::ExitDefault::Shutdown,
                    };
                    self.save_workspace();
                }
                self.request_exit(choice);
                return false;
            }
            true
        })
        .inner
    }
    fn history_diff_dialog_contents(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(preview) = self.ui.history_preview.clone() else {
            return false;
        };
        dialog_title(
            ui,
            "比较历史版本与当前草稿",
            &format!(
                "历史 Core：{}；差异中 - 为当前草稿，+ 为历史版本。载入只替换草稿，不会立即回滚运行版本。",
                preview.core
            ),
        );
        let lines = diff_lines(&self.editor, &preview.json);
        let added = lines.iter().filter(|line| line.starts_with('+')).count();
        let removed = lines.iter().filter(|line| line.starts_with('-')).count();
        if lines.is_empty() {
            ui.label("当前草稿与历史版本内容相同。");
        } else {
            ui.label(format!("共 {added} 行新增、{removed} 行删除。"));
            egui::ScrollArea::vertical()
                .id_salt("history-diff")
                .max_height((ui.ctx().content_rect().height() - 260.0).max(120.0))
                .show(ui, |ui| {
                    for line in lines.iter().take(300) {
                        ui.label(
                            RichText::new(line)
                                .monospace()
                                .color(if line.starts_with('+') {
                                    accent(ui)
                                } else {
                                    ui.visuals().warn_fg_color
                                }),
                        );
                    }
                    if lines.len() > 300 {
                        ui.label(RichText::new("差异较长，仅显示前 300 行。").small().weak());
                    }
                });
        }
        dialog_actions(ui);
        ui.horizontal_wrapped(|ui| {
            if ui.button("保留当前草稿").clicked() {
                self.ui.history_preview = None;
                return false;
            }
            if primary(ui, "载入历史为草稿", true) {
                self.load_history_preview();
                return false;
            }
            true
        })
        .inner
    }
    fn confirm_dialog_contents(
        &mut self,
        ui: &mut egui::Ui,
        action: &Action,
        ctx: &egui::Context,
    ) -> bool {
        let (title, note, confirm) = action.copy();
        dialog_title(ui, title, note);
        if matches!(action, Action::Switch(_) | Action::Proxy(ProxyMode::Global)) {
            egui::ScrollArea::vertical()
                .id_salt(("confirm-body", title))
                .max_height((ctx.content_rect().height() - 220.0).max(100.0))
                .show(ui, |ui| {
                    if let Action::Switch(id) = action {
                        line(ui, "切换到", self.ui.workspace.name(id));
                    }
                    if let Action::Proxy(ProxyMode::Global) = action {
                        line(ui, "使用 HTTP 入站", &self.runtime_http_tag());
                    }
                });
        }
        dialog_actions(ui);
        ui.horizontal_wrapped(|ui| {
            let cancel = ui.button("取消").clicked();
            let confirm = primary(ui, confirm, !self.exit);
            if confirm {
                self.execute_action(action.clone(), ctx);
            }
            !cancel && !confirm
        })
        .inner
    }
    fn create_dialog_contents(&mut self, ui: &mut egui::Ui) -> bool {
        dialog_title(
            ui,
            if self.ui.import_json {
                "添加 / 导入配置"
            } else {
                "新建配置"
            },
            if self.ui.import_json {
                "导入原生 JSON；名称、资源目录和节点参数由你确认，不自动推测。"
            } else {
                "选择场景与连接模板；保存不会启动 Core。"
            },
        );
        let mut can_import = false;
        let mut needs_work = false;
        // Keep the title and action row outside the scrolling body. Long JSON,
        // errors and field advice must never push the confirmation buttons away.
        let body_height = (ui.ctx().content_rect().height() - 180.0).max(200.0);
        egui::ScrollArea::vertical()
            .id_salt("create-body")
            .max_height(body_height)
            .show(ui, |ui| {
                let body_top = ui.cursor().top();
                ui.set_min_width(ui.available_width());
                ui.label("配置显示名称");
                ui.add(singleline(&mut self.ui.new_name, f32::INFINITY).hint_text("例如：工作网络"));
                ui.label(RichText::new("名称支持中文；原有字段与路由顺序保留，不自动猜测节点参数。").small().weak());
                if self.ui.import_json {
                    #[cfg(windows)]
                    if ui.button("从 JSON 文件读取…").clicked()
                        && let Some(path) = rfd::FileDialog::new().add_filter("原生 Xray JSON", &["json"]).pick_file()
                    {
                        match std::fs::read_to_string(&path) {
                            Ok(json) => {
                                self.ui.new_json = json;
                                if self.ui.new_name.trim().is_empty() {
                                    self.ui.new_name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
                                }
                                self.ui.import_source = Some(path);
                                self.ui.import_error = None;
                            }
                            Err(error) => self.ui.import_error = Some(error.to_string()),
                        }
                    }
                    if let Some(path) = &self.ui.import_source {
                        ui.add(egui::Label::new(RichText::new(display_path(path)).small().weak()).wrap());
                        ui.checkbox(&mut self.ui.import_use_source_assets, "使用配置文件所在目录查找资源")
                            .on_hover_text("默认沿用设置中的资源目录。仅在 geo 文件或相对证书路径基于配置所在目录时勾选。");
                    }
                    if let Some(error) = &self.ui.import_error {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    ui.horizontal(|ui| {
                        ui.label("原生 JSON 内容");
                        if ui.button("格式化 JSON").clicked()
                            && let Ok(value) = xray_config::parse_document(self.ui.new_json.as_bytes())
                        {
                            self.ui.new_json = serde_json::to_string_pretty(&value).unwrap();
                        }
                    });
                    let used_height = ui.cursor().top() - body_top;
                    let height = (body_height - used_height - 100.0).max(100.0);
                    json_editor(ui, &mut self.ui.new_json, "import-json", height);
                } else {
                    use crate::guided_config::{Scene, Template};
                    if !self.ui.new_guided_started {
                        ui.label("选择使用场景");
                        ui.horizontal_wrapped(|ui| {
                            for scene in Scene::ALL { ui.selectable_value(&mut self.ui.new_scene, scene, scene.label()); }
                        });
                        if self.ui.new_scene != Scene::Direct {
                            ui.label("选择与服务端一致的连接模板");
                            egui::ComboBox::from_id_salt("create-template").selected_text(self.ui.new_template.label()).show_ui(ui, |ui| {
                                for template in Template::ALL { ui.selectable_value(&mut self.ui.new_template, template, template.label()); }
                            });
                        }
                        ui.label("模板自动准备本机 HTTP / SOCKS 入口及基础出站。远端地址、端口和认证参数按服务端资料填写。");
                        if ui.button("开始填写").clicked() {
                            self.ui.new_json = serde_json::to_string_pretty(&crate::guided_config::scene(self.ui.new_scene, self.ui.new_template)).unwrap();
                            self.ui.new_guided_started = true;
                            if self.ui.new_scene != Scene::Direct {
                                self.ui.new_node_names.insert("node-1".into(), format!("{} 节点", self.ui.new_template.label()));
                            }
                        }
                    } else {
                        ui.label(RichText::new(format!("{} · {}", self.ui.new_scene.label(), if self.ui.new_scene == Scene::Direct { "本地诊断" } else { self.ui.new_template.label() })).strong());
                        if let Ok(mut value) = xray_config::parse_document(self.ui.new_json.as_bytes()) {
                            let mut changed = false;
                            if self.ui.new_scene != Scene::Direct {
                                ui.label("节点显示名称");
                                let name = self.ui.new_node_names.entry("node-1".into()).or_default();
                                ui.add(singleline(name, f32::INFINITY).char_limit(80));
                                changed |= ui.push_id(("new-node", self.ui.new_editor_epoch), |ui| crate::guided_config::node_form(ui, &mut value["outbounds"][0], None)).inner;
                            }
                            if changed { self.ui.new_json = serde_json::to_string_pretty(&value).unwrap(); }
                            if self.ui.new_scene == Scene::Split {
                                ui.label("保存后进入「分流规则」，添加需要走代理、直连或阻止的网站。未设置例外时默认通过此节点。");
                            }
                            crate::guided_config::summary(ui, &value, &self.ui.new_node_names);
                        }
                        ui.label(RichText::new("本机端口将在检查和应用时核对，不会自动替换正在使用的端口。系统代理接管在应用预览前单独选择。").small().weak());
                    }
                }
                match xray_config::parse_document(self.ui.new_json.as_bytes()) {
                    Ok(value) => {
                        can_import = self.ui.import_json || self.ui.new_guided_started;
                        if can_import { needs_work = config_review(ui, &value); }
                    }
                    Err(error) => {
                        ui.colored_label(ui.visuals().error_fg_color, "JSON 暂时无法解析，请修正后保存；当前输入会保留。");
                        ui.add(egui::Label::new(error.message).wrap());
                    }
                }
            });
        dialog_actions(ui);
        ui.horizontal_wrapped(|ui| {
            if ui.button("取消").clicked() {
                return false;
            }
            if primary(
                ui,
                if needs_work {
                    "保存待补充配置"
                } else {
                    "添加配置"
                },
                can_import && self.write_blocker().is_none() && !self.ui.new_name.trim().is_empty(),
            ) {
                if self.ui.import_use_source_assets
                    && let Some(parent) = self.ui.import_source.as_ref().and_then(|p| p.parent())
                {
                    self.asset_root = parent.display().to_string();
                    self.settings_edited();
                }
                let profile_id = format!("profile-{}", id());
                self.ui.workspace.names.insert(
                    profile_id.clone(),
                    self.ui.new_name.trim().chars().take(80).collect(),
                );
                if !self.ui.new_node_names.is_empty() {
                    self.ui
                        .workspace
                        .node_names
                        .insert(profile_id.clone(), self.ui.new_node_names.clone());
                }
                self.save_workspace();
                self.send(Command::CreateProfile {
                    profile_id,
                    json: self.ui.new_json.clone(),
                });
                return false;
            }
            ui.label(RichText::new("仅保存配置，不启动 Core").small().weak());
            true
        })
        .inner
    }
    fn rename_dialog_contents(&mut self, ui: &mut egui::Ui, profile: &str) -> bool {
        dialog_title(
            ui,
            "修改配置显示名称",
            "名称不改变配置标识、JSON 内容或运行版本。",
        );
        ui.label("配置显示名称");
        ui.add(singleline(&mut self.ui.rename, f32::INFINITY));
        dialog_actions(ui);
        ui.horizontal_wrapped(|ui| {
            if ui.button("取消").clicked() {
                return false;
            }
            if primary(ui, "保存名称", !self.ui.rename.trim().is_empty()) {
                self.ui.workspace.names.insert(
                    profile.into(),
                    self.ui.rename.trim().chars().take(80).collect(),
                );
                self.save_workspace();
                return false;
            }
            true
        })
        .inner
    }
    pub(in crate::app) fn execute_action(&mut self, action: Action, _ctx: &egui::Context) {
        match action {
            Action::Switch(id) => self.switch_profile(id),
            Action::Discard(profile) => {
                if self.selected_profile != profile || self.profile_id != profile {
                    self.message = "编辑对象已变化，请重新确认放弃草稿。".into();
                    return;
                }
                self.ui.unreadable_drafts.remove(&profile);
                self.editor = self.saved_json.clone();
                self.ui.number_inputs.clear();
                self.ui.draft_conflict = false;
                self.draft_edited();
                self.stash_draft();
            }
            Action::AdoptDraft(profile) => {
                if self.selected_profile != profile || self.profile_id != profile {
                    self.message = "编辑对象已变化，请重新确认草稿处理。".into();
                    return;
                }
                self.ui.draft_conflict = false;
                self.draft_edited();
                self.stash_draft();
            }
            Action::ClearPacDomains(profile) => {
                if self.selected_profile != profile || self.profile_id != profile {
                    self.message = "编辑对象已变化，请重新确认清空域名。".into();
                    return;
                }
                self.clear_all_pac_domains();
            }
            Action::Delete { profile, revision } => self.send(Command::DeleteProfile {
                profile_id: profile,
                expected_revision: revision,
            }),
            Action::ExportJson => {
                #[cfg(windows)]
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("config.json")
                    .save_file()
                {
                    if let Err(error) = std::fs::write(path, &self.editor) {
                        self.fail(error.into(), String::new());
                    } else {
                        self.message = "配置已导出；文件可能包含认证信息。".into();
                    }
                }
                #[cfg(not(windows))]
                {
                    self.message = "当前平台不提供图形文件选择器。".into();
                }
            }
            Action::Stop => self.send(Command::StopCore),
            Action::Restart => self.send(Command::RestartCore),
            Action::ResolveProxyConflict => self.send(Command::ResolveProxyConflict),
            Action::Proxy(mode) => self.send(Command::SetProxyMode {
                mode,
                http_inbound_tag: self.runtime_http_tag(),
                pac: (mode == ProxyMode::Pac).then(|| self.running_pac_draft()),
            }),
        }
    }
    pub(in crate::app) fn runtime_http_tag(&self) -> String {
        if let Some(g) = self.status.as_ref().and_then(|s| s.generation.as_ref()) {
            if let Some(config) = self.ui.runtime.as_ref().filter(|c| c.generation_id == g.id)
                && let Ok(value) = serde_json::from_str::<serde_json::Value>(&config.json)
                && let Some(inbounds) = value.get("inbounds").and_then(|v| v.as_array())
            {
                let tags: Vec<_> = inbounds
                    .iter()
                    .filter(|i| i.get("protocol").and_then(|v| v.as_str()) == Some("http"))
                    .filter_map(|i| i.get("tag").and_then(|v| v.as_str()))
                    .collect();
                if tags.contains(&g.http_inbound_tag.as_str()) {
                    return g.http_inbound_tag.clone();
                }
                if let Some(tag) = tags.first() {
                    return (*tag).into();
                }
            }
            return g.http_inbound_tag.clone();
        }
        String::new()
    }
    pub(in crate::app) fn preview(&mut self, ctx: &egui::Context) {
        if !self.candidate_matches() {
            self.ui.preview_open = false;
            return;
        }
        let candidate = self.candidate.clone().unwrap();
        let response = egui::Modal::new(egui::Id::new("candidate-preview")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 100.0).clamp(300.0, 900.0));
            dialog_title(
                ui,
                "预览本次应用",
                "以下内容将作为同一个运行版本应用；确认后才会切换 Core 与配置。",
            );
            let body_height = (ctx.content_rect().height() - 260.0).max(120.0);
            egui::ScrollArea::vertical()
                .id_salt(("candidate-preview-body", &candidate.id))
                .max_height(body_height)
                .show(ui, |ui| {
                    ui.push_id(&candidate.id, |ui| {
                        self.preview_body(ui, &candidate, body_height)
                    });
                });
            dialog_actions(ui);
            ui.horizontal_wrapped(|ui| {
                let cancel = ui.button("返回编辑").clicked();
                let apply = primary(
                    ui,
                    if self.is_running() {
                        "确认应用"
                    } else {
                        "确认并启动"
                    },
                    self.write_blocker().is_none() && self.candidate_matches(),
                );
                if apply {
                    self.send(Command::ApplyProfile {
                        candidate_id: candidate.id,
                        candidate_digest: candidate.digest,
                        proxy_mode: self.mode,
                        http_inbound_tag: self.http_tag.clone(),
                        pac: (self.mode == ProxyMode::Pac).then(|| self.pac_draft()),
                    });
                }
                cancel || apply
            })
            .inner
        });
        if response.inner || response.should_close() {
            self.ui.preview_open = false;
        }
    }
    fn preview_body(&self, ui: &mut egui::Ui, candidate: &Candidate, viewport_height: f32) {
        let body_top = ui.cursor().top();
        line(
            ui,
            "目标配置",
            self.ui.workspace.name(&candidate.profile_id),
        );
        line(ui, "通过校验的 Core", &candidate.core.version);
        line(ui, "系统代理方式", mode_name(self.mode));
        if matches!(self.mode, ProxyMode::Global | ProxyMode::Pac) {
            line(ui, "接管使用的 HTTP Tag", &self.http_tag);
        }
        if self.mode == ProxyMode::Pac {
            let draft = self.pac_draft();
            line(ui, "PAC 服务端口", &draft.port.to_string());
            line(
                ui,
                "PAC 策略",
                match draft.mode {
                    xray_model::PacMode::AutoRules => "按路由规则自动生成（支持 GeoSite）",
                    xray_model::PacMode::DirectList => "选中 GeoSite 与域名直连，其余代理",
                    xray_model::PacMode::ProxyList => "仅代理选中 GeoSite 与域名，其余直连",
                    xray_model::PacMode::All => "全部代理（本地/局域网直连）",
                    xray_model::PacMode::Custom => "自定义 PAC 脚本",
                },
            );
        }
        if let Ok(value) = xray_config::parse_document(self.editor.as_bytes()) {
            let names = self
                .ui
                .workspace
                .node_names
                .get(&candidate.profile_id)
                .cloned()
                .unwrap_or_default();
            crate::guided_config::summary(ui, &value, &names);
        }
        if !candidate.resources.complete {
            ui.add(egui::Label::new(
                RichText::new("配置已通过 Core 校验。未分类字段无法冻结；启动和恢复时仍需 Core 接受，失败会停止并报告。")
                    .color(ui.visuals().warn_fg_color),
            ).wrap());
            if !candidate.resources.dynamic.is_empty() {
                ui.label(format!(
                    "其中 {} 项为显式动态外部文件；自动连接、异常重试及自动恢复旧 Core 不可用。",
                    candidate.resources.dynamic.len()
                ));
            }
        }
        ui.separator();
        // The viewport bounds scrolling, while its document stays unbounded.
        // Never constrain the document Ui to the viewport height: expanded
        // details and long diagnostics must retain their natural row heights.
        let used_height = ui.cursor().top() - body_top;
        let json_height = (viewport_height - used_height - 200.0).max(100.0);
        ui.columns(2, |columns| {
            columns[0].label(if self.is_running() {
                "当前运行版本"
            } else {
                "上次应用版本（当前未运行）"
            });
            let mut old = self
                .ui
                .runtime
                .as_ref()
                .filter(|r| {
                    self.status
                        .as_ref()
                        .and_then(|s| s.generation.as_ref())
                        .is_some_and(|g| g.id == r.generation_id)
                })
                .map(|r| r.json.clone())
                .unwrap_or_else(|| "暂无已读取的运行配置".into());
            json_viewer(
                &mut columns[0],
                &mut old,
                "preview-running-json",
                json_height,
            );
            columns[1].label("待应用草稿");
            let mut json = self.editor.clone();
            json_viewer(
                &mut columns[1],
                &mut json,
                "preview-draft-json",
                json_height,
            );
        });
        egui::CollapsingHeader::new("与此配置已保存版本的差异（最多展示 200 行）").show(ui, |ui| {
            let lines = diff_lines(&self.saved_json, &self.editor);
            if lines.is_empty() {
                ui.label("与此配置的已保存版本相同。");
            }
            for line in lines.into_iter().take(200) {
                ui.label(
                    RichText::new(&line)
                        .monospace()
                        .color(if line.starts_with('+') {
                            accent(ui)
                        } else {
                            ui.visuals().warn_fg_color
                        }),
                );
            }
        });
        ui.separator();
        line(
            ui,
            "冻结资源文件数",
            &candidate.resources.files.len().to_string(),
        );
        if !candidate.resources.notes.is_empty() {
            egui::CollapsingHeader::new(format!(
                "查看 {} 项资源与自动恢复限制",
                candidate.resources.notes.len()
            ))
            .id_salt("resource-coverage-notes")
            .show(ui, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(
                            "以下是管理器的自动恢复覆盖范围提示，不表示配置非法；原始字段会保留。",
                        )
                        .small()
                        .weak(),
                    )
                    .wrap(),
                );
                egui::ScrollArea::vertical()
                    .id_salt("resource-coverage-notes-scroll")
                    .max_height((viewport_height * 0.3).clamp(100.0, 220.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for note in &candidate.resources.notes {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(resource_note(note))
                                        .color(ui.visuals().warn_fg_color),
                                )
                                .wrap(),
                            );
                        }
                    });
            });
        }
        egui::CollapsingHeader::new("冻结资源清单与 Core 身份").show(ui, |ui| {
            ui.monospace(format!("Core SHA-256: {}", candidate.core.sha256));
            for file in &candidate.resources.files {
                ui.monospace(format!(
                    "{}\nSHA-256: {}",
                    display_path(&file.relative_path),
                    file.sha256
                ));
            }
        });
        ui.label(RichText::new("已由真实 Core 执行配置校验；这不代表远端节点可连接。正式应用时仍需检查依赖和入口。").small().weak());
    }
    fn pac_batch_dialog_contents(&mut self, ui: &mut egui::Ui) -> bool {
        dialog_title(
            ui,
            "批量导入 / 编辑 PAC 域名",
            "每行一个域名，或用逗号、分号、空格分隔；支持 example.com、*.example.com、full:example.com，自动忽略空行与 # / // 注释。",
        );
        let body_height = (ui.ctx().content_rect().height() - 260.0).max(160.0);
        egui::ScrollArea::vertical()
            .id_salt("pac-batch-body")
            .max_height(body_height)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(
                        &mut self.ui.pac_batch_replace,
                        false,
                        "追加到现有清单（自动去重）",
                    );
                    ui.radio_value(&mut self.ui.pac_batch_replace, true, "替换当前全部清单");
                    if ui.button("填入当前清单").clicked() {
                        self.ui.pac_batch_input =
                            xray_pac::format_domain_list(&self.pac_draft().domains);
                        self.ui.pac_batch_report = None;
                    }
                });
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .id_salt("pac-batch-text")
                    .max_height((body_height - 90.0).clamp(120.0, 320.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.ui.pac_batch_input)
                                .code_editor()
                                .desired_rows(12)
                                .desired_width(f32::INFINITY)
                                .hint_text("example.com\n*.internal.example\nfull:api.example.org"),
                        );
                    });
                if let Some(report) = &self.ui.pac_batch_report {
                    ui.add_space(6.0);
                    ui.colored_label(ui.visuals().warn_fg_color, report);
                }
            });
        dialog_actions(ui);
        let mut keep_open = true;
        ui.horizontal_wrapped(|ui| {
            if ui.button("取消").clicked() {
                keep_open = false;
            }
            let can_apply = !self.ui.pac_batch_input.trim().is_empty() || self.ui.pac_batch_replace;
            if primary(ui, "导入并保存", can_apply) {
                let batch = self.ui.pac_batch_input.clone();
                let replace = self.ui.pac_batch_replace;
                if self.apply_pac_batch(&batch, replace) {
                    self.ui.pac_batch_input.clear();
                    self.ui.pac_batch_report = None;
                    keep_open = false;
                }
            }
        });
        keep_open
    }
}

/// Explain coverage limitations without changing the persisted backend evidence.
fn resource_note(note: &str) -> String {
    if let Some(path) = note
        .strip_prefix("Uncovered dependency capability ")
        .and_then(|s| s.strip_suffix("; automatic recovery disabled"))
    {
        format!("尚未覆盖字段：{path}")
    } else {
        match note {
            "Automatic recovery covers HTTP/SOCKS inbounds only" => {
                "自动恢复目前仅覆盖 HTTP / SOCKS 入站。".into()
            }
            "Outbound protocol dependency coverage is incomplete" => {
                "此出站协议的依赖覆盖尚不完整。".into()
            }
            _ => note.into(),
        }
    }
}
