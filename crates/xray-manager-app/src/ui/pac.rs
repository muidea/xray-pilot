use super::*;
use xray_model::{PacTemplateMode, hash};

impl App {
    fn pac_check(&mut self, script: &str) -> bool {
        self.ui.pac_test.clear();
        match xray_pac::validate(script) {
            Ok(()) => {
                self.ui.pac_checked = true;
                self.ui.pac_notes = xray_pac::compat_notes(script);
                self.message = if self.ui.pac_notes.is_empty() {
                    "PAC 检查通过。".into()
                } else {
                    "PAC 检查通过，但存在 Windows JScript 兼容性提示。".into()
                };
                true
            }
            Err(error) => {
                self.ui.pac_checked = false;
                self.ui.pac_notes.clear();
                self.message = format!("PAC 检查失败：{}", error.message);
                false
            }
        }
    }

    fn clear_pac_checks(&mut self) {
        self.ui.pac_checked = false;
        self.ui.pac_test.clear();
        self.ui.pac_notes.clear();
    }

    fn add_pac_domain(&mut self, draft: &mut PacDraft) -> bool {
        let input = self.ui.pac_domain_input.trim();
        if input.is_empty() {
            return false;
        }
        match xray_pac::normalize_domain(input) {
            Ok(domain) => {
                if draft
                    .domains
                    .iter()
                    .any(|existing| existing.eq_ignore_ascii_case(&domain))
                {
                    self.message = format!("域名已在清单中：{domain}");
                    return false;
                }
                draft.domains.push(domain.clone());
                self.ui.pac_domain_input.clear();
                self.message = format!("已添加并保存代理域名：{domain}");
                true
            }
            Err(error) => {
                self.message = error.message;
                false
            }
        }
    }

    pub(in crate::app) fn pac_page(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "PAC 自动代理",
            "按 1–5 步完成：选择范围、添加域名、生成检查、执行测试，最后应用。",
        );
        let mut draft = self.pac_draft();
        let mut changed = false;
        let has_config = !self.profile_id.is_empty() && !self.editor.trim().is_empty();
        let endpoint_result = if has_config {
            self.pac_endpoint()
        } else {
            Err(String::new())
        };
        let endpoint = endpoint_result.as_ref().ok().copied();
        if draft.template
            && let Some(endpoint) = endpoint
        {
            match xray_pac::template(endpoint, draft.template_mode, &draft.domains) {
                Ok(script) if draft.script != script => {
                    draft.script = script;
                    self.clear_pac_checks();
                    changed = true;
                }
                Ok(_) => {}
                Err(error) => self.message = error.message,
            }
        }

        // Step 1: scope.
        card(ui, |ui| {
            ui.label(RichText::new("步骤 1 · 选择代理范围").strong().size(18.0));
            if let Some(endpoint) = endpoint {
                line(ui, "代理目标", &endpoint.to_string());
            } else if !has_config {
                ui.label(RichText::new("尚未选择配置；添加并检查配置后即可设置 PAC。").weak());
            } else {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    endpoint_result
                        .as_ref()
                        .err()
                        .cloned()
                        .unwrap_or_else(|| "请先添加回环、无认证的 HTTP 或 Mixed 入站。".into()),
                );
            }
            ui.horizontal_wrapped(|ui| {
                ui.label("代理范围");
                egui::ComboBox::from_id_salt("pac-template-mode")
                    .width(260.0)
                    .selected_text(match draft.template_mode {
                        PacTemplateMode::All => "全部代理（本地/局域网直连）",
                        PacTemplateMode::ProxyList => "仅代理清单中的域名",
                        PacTemplateMode::DirectList => "清单中的域名直连，其余代理",
                    })
                    .show_ui(ui, |ui| {
                        for (mode, label) in [
                            (PacTemplateMode::All, "全部代理（本地/局域网直连）"),
                            (PacTemplateMode::ProxyList, "仅代理清单中的域名"),
                            (PacTemplateMode::DirectList, "清单中的域名直连，其余代理"),
                        ] {
                            if ui
                                .selectable_value(&mut draft.template_mode, mode, label)
                                .clicked()
                            {
                                draft.template = true;
                                draft.generated = false;
                                self.clear_pac_checks();
                                self.save_pac_draft(draft.clone());
                                self.message = "代理范围已保存；请继续添加域名或生成检查。".into();
                                changed = false;
                            }
                        }
                    });
            });
            match draft.template_mode {
                PacTemplateMode::All => {
                    ui.label(
                        RichText::new("所有请求（本地和局域网除外）进入 Xray。")
                            .small()
                            .weak(),
                    );
                }
                PacTemplateMode::ProxyList => {
                    ui.label(
                        RichText::new("只有清单中的域名进入 Xray，其余直连；清单为空时全部直连。")
                            .small()
                            .weak(),
                    );
                }
                PacTemplateMode::DirectList => {
                    ui.label(
                        RichText::new("清单中的域名直连，其余进入 Xray；清单为空时全部代理。")
                            .small()
                            .weak(),
                    );
                }
            }
            if !draft.template {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "当前使用自定义脚本；选择代理范围或添加域名会切换回内置模板。",
                );
            }
        });

        // Step 2: domains.
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(RichText::new("步骤 2 · 添加代理域名").strong().size(18.0));
            ui.label(
                RichText::new(
                    "添加或移除后会立即保存草稿，并自动切回内置模板；是否应用仍由步骤 5 决定。",
                )
                .small()
                .weak(),
            );
            ui.horizontal(|ui| {
                ui.label("域名");
                let response = ui.add(
                    crate::widgets::singleline(&mut self.ui.pac_domain_input, 360.0)
                        .hint_text("example.com 或 *.example.com"),
                );
                let enter =
                    response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
                if (ui.button("添加").clicked() || enter) && self.add_pac_domain(&mut draft) {
                    if draft.template_mode == PacTemplateMode::All {
                        draft.template_mode = PacTemplateMode::ProxyList;
                    }
                    draft.template = true;
                    draft.generated = false;
                    self.clear_pac_checks();
                    self.save_pac_draft(draft.clone());
                    changed = false;
                }
            });
            let mut remove = None;
            if draft.domains.is_empty() {
                if draft.template_mode != PacTemplateMode::All {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        if draft.template_mode == PacTemplateMode::ProxyList {
                            "清单为空：当前不会代理任何普通域名。"
                        } else {
                            "清单为空：当前会代理所有普通域名。"
                        },
                    );
                }
            } else {
                if draft.template_mode == PacTemplateMode::All {
                    ui.label(
                        RichText::new("当前为全部代理，以下清单暂不生效。")
                            .small()
                            .weak(),
                    );
                }
                ui.horizontal_wrapped(|ui| {
                    for (index, domain) in draft.domains.iter().enumerate() {
                        ui.group(|ui| {
                            ui.label(domain);
                            if ui.small_button("移除").clicked() {
                                remove = Some(index);
                            }
                        });
                    }
                });
            }
            if let Some(index) = remove {
                draft.domains.remove(index);
                draft.template = true;
                draft.generated = false;
                self.clear_pac_checks();
                self.save_pac_draft(draft.clone());
                changed = false;
            }
        });

        // Step 3: generate/check.
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(RichText::new("步骤 3 · 生成并检查模板").strong().size(18.0));
            ui.label(
                RichText::new(
                    "模板会随代理范围和域名清单自动更新；此步骤确认脚本可解析且存在 FindProxyForURL。",
                )
                .small()
                .weak(),
            );
            line(
                ui,
                "模板状态",
                if draft.template {
                    "内置模板（随范围/清单更新）"
                } else {
                    "自定义脚本（高级设置）"
                },
            );
            line(
                ui,
                "检查状态",
                if self.ui.pac_checked {
                    "已通过语法检查"
                } else {
                    "尚未检查"
                },
            );
            if !self.ui.pac_notes.is_empty() {
                egui::CollapsingHeader::new("检查提示").show(ui, |ui| {
                    for note in &self.ui.pac_notes {
                        ui.label(RichText::new(note).small().weak());
                    }
                });
            }
            if primary(
                ui,
                "生成并检查模板",
                !draft.script.trim().is_empty() && !self.busy,
            ) && self.pac_check(&draft.script)
            {
                self.save_pac_draft(draft.clone());
                changed = false;
            }
        });

        // Step 4: test.
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(
                RichText::new("步骤 4 · 执行 FindProxyForURL 测试")
                    .strong()
                    .size(18.0),
            );
            ui.label(
                RichText::new("用测试 URL 确认当前脚本返回 DIRECT 还是 PROXY；测试不代表浏览器缓存或实际出口。")
                    .small()
                    .weak(),
            );
            ui.horizontal(|ui| {
                ui.label("测试 URL");
                ui.add(
                    crate::widgets::singleline(&mut self.ui.pac_test_url, f32::INFINITY)
                        .hint_text("https://example.com/"),
                );
            });
            if primary(
                ui,
                "测试当前模板",
                self.ui.connected && !draft.script.trim().is_empty() && !self.busy,
            ) {
                match xray_pac::normalize_test_url(&self.ui.pac_test_url) {
                    Ok(url) => {
                        self.ui.pac_test_url = url.clone();
                        self.send(Command::TestPac {
                            script: draft.script.clone(),
                            url,
                        });
                    }
                    Err(error) => self.message = error.message,
                }
            }
            if let Ok(result) = serde_json::from_str::<serde_json::Value>(&self.ui.pac_test) {
                let decision = &result["decision"];
                if let (Some(kind), Some(raw)) =
                    (decision["kind"].as_str(), decision["raw"].as_str())
                {
                    line(ui, "最近测试结果", &format!("{kind} · {raw}"));
                }
                if let Some(notes) = result["notes"].as_array() {
                    for note in notes.iter().filter_map(|n| n.as_str()) {
                        ui.label(RichText::new(note).small().weak());
                    }
                }
                egui::CollapsingHeader::new("原始测试结果").show(ui, |ui| {
                    ui.monospace(&self.ui.pac_test);
                });
            } else if !self.ui.pac_test.is_empty() {
                ui.monospace(&self.ui.pac_test);
            }
        });

        // Step 5: apply.
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(RichText::new("步骤 5 · 应用到运行版本").strong().size(18.0));
            ui.label(
                RichText::new("保存草稿不会改变当前运行版本；应用成功后 PAC URL 才会更新。")
                    .small()
                    .weak(),
            );
            if let Some(pac) = self
                .status
                .as_ref()
                .and_then(|s| s.generation.as_ref())
                .and_then(|g| g.pac.as_ref())
            {
                line(ui, "当前 PAC URL", &pac.url);
                if pac.revision == hash(draft.script.as_bytes()) {
                    ui.label(RichText::new("当前运行版本已应用此脚本。").weak());
                } else {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "当前运行版本仍是旧脚本；请重新检查并应用。",
                    );
                }
            } else {
                ui.label(RichText::new("当前运行版本未启用 PAC。").weak());
            }
            if primary(
                ui,
                "检查并预览 PAC 应用",
                endpoint.is_some()
                    && !self.busy
                    && self.ui.connected
                    && self.write_blocker().is_none(),
            ) {
                self.save_pac_draft(draft.clone());
                self.mode = ProxyMode::Pac;
                self.validate_draft();
                changed = false;
            }
        });

        // Advanced.
        ui.add_space(12.0);
        egui::CollapsingHeader::new("高级设置（端口、导入、自定义脚本）").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("PAC 服务端口");
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut draft.port)
                            .range(1..=65535)
                            .speed(1),
                    )
                    .changed();
                ui.label(RichText::new("默认 18080，仅监听 127.0.0.1").small().weak());
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        !self.profile_id.is_empty() && self.write_blocker().is_none(),
                        egui::Button::new("从当前配置生成"),
                    )
                    .on_hover_text("按原生域名规则生成自定义脚本；普通用户优先使用内置模板")
                    .clicked()
                {
                    match serde_json::from_str::<serde_json::Value>(&self.editor) {
                        Ok(value) => match self.pac_endpoint() {
                            Ok(endpoint) => {
                                let generated = xray_pac::generate(&value, endpoint);
                                draft.script = generated.script;
                                draft.generated = true;
                                draft.template = false;
                                self.clear_pac_checks();
                                self.ui.pac_notes = generated.notes;
                                changed = true;
                            }
                            Err(error) => self.message = format!("无法生成 PAC：{error}"),
                        },
                        Err(error) => self.message = format!("当前 JSON 无法解析：{error}"),
                    }
                }
                #[cfg(windows)]
                if ui
                    .add_enabled(
                        !self.profile_id.is_empty() && self.write_blocker().is_none(),
                        egui::Button::new("导入 PAC 文件…"),
                    )
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("PAC / JavaScript", &["pac", "js"])
                        .pick_file()
                {
                    match std::fs::read_to_string(path) {
                        Ok(script) => {
                            draft.script = script;
                            draft.generated = false;
                            draft.template = false;
                            self.clear_pac_checks();
                            changed = true;
                        }
                        Err(error) => self.message = format!("读取 PAC 文件失败：{error}"),
                    }
                }
            });
            ui.label(
                RichText::new(
                    "自定义脚本在独立上下文中执行；网络类辅助函数会明确报告 Unsupported。",
                )
                .small()
                .weak(),
            );
            let response = egui::ScrollArea::vertical()
                .id_salt("pac-script")
                .max_height(360.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut draft.script)
                            .code_editor()
                            .desired_rows(16)
                            .desired_width(f32::INFINITY),
                    )
                })
                .inner;
            if response.changed() {
                draft.generated = false;
                draft.template = false;
                self.clear_pac_checks();
                changed = true;
            }
        });
        if changed {
            self.save_pac_draft(draft);
        }
    }
}
