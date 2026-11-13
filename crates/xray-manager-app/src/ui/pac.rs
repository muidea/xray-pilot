use super::*;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use xray_model::{PacTemplateMode, hash};
use xray_pac::PacDomainKind;

fn domains_hash(domains: &[String]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    domains.hash(&mut hasher);
    hasher.finish()
}

fn kind_label(kind: PacDomainKind) -> &'static str {
    match kind {
        PacDomainKind::Exact => "精确",
        PacDomainKind::Subdomain => "仅子域",
        PacDomainKind::DomainAndSub => "含主域",
    }
}

fn format_invalid_preview(invalid: &[(usize, String, String)]) -> String {
    let preview: Vec<String> = invalid
        .iter()
        .take(5)
        .map(|(line, token, _)| format!("第 {line} 行：{token}"))
        .collect();
    if invalid.len() > 5 {
        format!("{} 等共 {} 项无效", preview.join("；"), invalid.len())
    } else {
        preview.join("；")
    }
}

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

    fn sync_pac_template(
        &mut self,
        draft: &mut PacDraft,
        endpoint: Option<std::net::SocketAddr>,
    ) -> bool {
        if !draft.template {
            return false;
        }
        let Some(endpoint) = endpoint else {
            return false;
        };
        let key = (
            self.profile_id.clone(),
            endpoint,
            draft.template_mode,
            domains_hash(&draft.domains),
        );
        if self.ui.pac_template_key.as_ref() == Some(&key) && !draft.script.is_empty() {
            return false;
        }
        self.ui.pac_template_key = Some(key);
        match xray_pac::template(endpoint, draft.template_mode, &draft.domains) {
            Ok(script) if draft.script != script => {
                draft.script = script;
                self.clear_pac_checks();
                true
            }
            Ok(_) => false,
            Err(error) => {
                self.message = error.message;
                false
            }
        }
    }

    fn commit_pac_domains(&mut self, draft: &mut PacDraft, endpoint: Option<std::net::SocketAddr>) {
        if draft.template_mode == PacTemplateMode::All && !draft.domains.is_empty() {
            draft.template_mode = PacTemplateMode::ProxyList;
        }
        draft.template = true;
        draft.generated = false;
        self.ui.pac_template_key = None;
        self.clear_pac_checks();
        self.sync_pac_template(draft, endpoint);
        self.save_pac_draft(draft.clone());
    }

    fn add_pac_domain(
        &mut self,
        draft: &mut PacDraft,
        endpoint: Option<std::net::SocketAddr>,
    ) -> bool {
        let input = self.ui.pac_domain_input.trim();
        if input.is_empty() {
            return false;
        }
        let single_token = !input
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, ',' | ';'));
        if single_token {
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
                    self.commit_pac_domains(draft, endpoint);
                    self.message = format!("已添加并保存代理域名：{domain}");
                    true
                }
                Err(error) => {
                    self.message = error.message;
                    false
                }
            }
        } else {
            let parsed = xray_pac::parse_domain_batch(input, &draft.domains);
            if parsed.added.is_empty() {
                self.message = if !parsed.invalid.is_empty() {
                    format!("未添加域名：{}", format_invalid_preview(&parsed.invalid))
                } else {
                    format!("输入的 {} 个域名均已在清单中。", parsed.duplicates)
                };
                return false;
            }
            let added_count = parsed.added.len();
            draft.domains.extend(parsed.added);
            self.ui.pac_domain_input.clear();
            self.commit_pac_domains(draft, endpoint);
            self.message = if parsed.invalid.is_empty() && parsed.duplicates == 0 {
                format!("已批量添加并保存 {added_count} 条代理域名。")
            } else {
                format!(
                    "已添加 {added_count} 条域名，跳过重复 {} 条，无效 {} 条。",
                    parsed.duplicates,
                    parsed.invalid.len()
                )
            };
            true
        }
    }

    pub(super) fn apply_pac_batch(&mut self, input: &str, replace: bool) -> bool {
        let mut draft = self.pac_draft();
        let endpoint = self.pac_endpoint().ok();
        let existing = if replace {
            Vec::new()
        } else {
            draft.domains.clone()
        };
        let parsed = xray_pac::parse_domain_batch(input, &existing);
        if replace && parsed.added.is_empty() && !parsed.invalid.is_empty() {
            let detail = format!(
                "未替换清单，请修正无效条目：{}",
                format_invalid_preview(&parsed.invalid)
            );
            self.ui.pac_batch_report = Some(detail.clone());
            self.message = detail;
            return false;
        }
        if replace {
            draft.domains = parsed.added.clone();
            self.ui.pac_selected_domains.clear();
            self.commit_pac_domains(&mut draft, endpoint);
        } else if !parsed.added.is_empty() {
            draft.domains.extend(parsed.added.iter().cloned());
            self.commit_pac_domains(&mut draft, endpoint);
        }

        let summary = if replace {
            format!(
                "已替换为 {} 条域名（跳过重复 {} 条）",
                draft.domains.len(),
                parsed.duplicates
            )
        } else {
            format!(
                "已追加 {} 条域名（跳过重复 {} 条）",
                parsed.added.len(),
                parsed.duplicates
            )
        };
        if parsed.invalid.is_empty() {
            self.ui.pac_batch_report = None;
            self.message = format!("{summary}。");
            true
        } else {
            let detail = format!(
                "{summary}；以下格式无效未导入：{}",
                format_invalid_preview(&parsed.invalid)
            );
            self.ui.pac_batch_report = Some(detail.clone());
            self.message = detail;
            false
        }
    }

    pub(super) fn clear_all_pac_domains(&mut self) {
        let mut draft = self.pac_draft();
        let count = draft.domains.len();
        draft.domains.clear();
        self.ui.pac_selected_domains.clear();
        self.ui.pac_domain_filter.clear();
        let endpoint = self.pac_endpoint().ok();
        self.commit_pac_domains(&mut draft, endpoint);
        self.message = format!("已清空 {count} 条代理域名。");
    }

    fn extract_pac_domains_from_config(
        &mut self,
        draft: &mut PacDraft,
        endpoint: Option<std::net::SocketAddr>,
    ) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.editor) else {
            self.message = "当前 JSON 无法解析，无法提取路由域名。".into();
            return;
        };
        let extracted = xray_pac::extract_domains_from_config(&value);
        let mut existing: HashSet<String> = draft
            .domains
            .iter()
            .map(|d| d.to_ascii_lowercase())
            .collect();
        let mut added = 0usize;
        let mut duplicates = 0usize;
        for domain in extracted.domains {
            if existing.insert(domain.to_ascii_lowercase()) {
                draft.domains.push(domain);
                added += 1;
            } else {
                duplicates += 1;
            }
        }
        if added > 0 {
            self.commit_pac_domains(draft, endpoint);
        }
        let mut parts = vec![format!("从路由提取：新增 {added} 条")];
        if duplicates > 0 {
            parts.push(format!("已存在 {duplicates} 条"));
        }
        if extracted.skipped_geosite > 0 {
            parts.push(format!(
                "跳过 {} 条 geosite: 规则（geosite 依赖 geosite.dat 在 Xray Core 内部生效，无法直接转为 PAC 清单）",
                extracted.skipped_geosite
            ));
        }
        let other_skipped =
            extracted.skipped_regexp + extracted.skipped_ext + extracted.skipped_other;
        if other_skipped > 0 {
            parts.push(format!("跳过 {other_skipped} 条正则/扩展/关键字规则"));
        }
        self.message = format!("{}。", parts.join("；"));
    }

    fn dedup_and_sort_pac_domains(
        &mut self,
        draft: &mut PacDraft,
        endpoint: Option<std::net::SocketAddr>,
    ) {
        let before = draft.domains.len();
        let mut seen = HashSet::new();
        let mut cleaned = Vec::with_capacity(before);
        for domain in &draft.domains {
            let normalized = xray_pac::normalize_domain(domain).unwrap_or_else(|_| domain.clone());
            if seen.insert(normalized.to_ascii_lowercase()) {
                cleaned.push(normalized);
            }
        }
        cleaned.sort_by_key(|d| d.to_ascii_lowercase());
        let removed = before.saturating_sub(cleaned.len());
        draft.domains = cleaned;
        self.commit_pac_domains(draft, endpoint);
        self.message = if removed > 0 {
            format!("已去重并排序，移除 {removed} 条重复域名。")
        } else {
            "域名清单已按字母排序。".into()
        };
    }

    fn pac_domains_section(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut PacDraft,
        endpoint: Option<std::net::SocketAddr>,
    ) {
        ui.label(RichText::new("步骤 2 · 添加代理域名").strong().size(18.0));
        ui.label(
            RichText::new(
                "支持单条或批量粘贴/导入域名；添加或移除后会立即保存草稿并自动切回内置模板，是否应用仍由步骤 5 决定。",
            )
            .small()
            .weak(),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label("域名");
            let response = ui.add(
                crate::widgets::singleline(&mut self.ui.pac_domain_input, 340.0)
                    .hint_text("example.com、*.example.com，或用逗号/空格分隔多条"),
            );
            let enter =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            if ui.button("添加").clicked() || enter {
                self.add_pac_domain(draft, endpoint);
            }
            if ui.button("批量粘贴 / 编辑…").clicked() {
                self.ui.pac_batch_report = None;
                self.ui.pac_batch_replace = false;
                self.ui.dialog = Some(Dialog::PacBatch);
            }
        });

        ui.horizontal_wrapped(|ui| {
            #[cfg(windows)]
            if ui.button("导入文本列表…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("域名列表", &["txt", "list", "conf"])
                    .pick_file()
            {
                match std::fs::read_to_string(path) {
                    Ok(content) => {
                        self.apply_pac_batch(&content, false);
                        *draft = self.pac_draft();
                    }
                    Err(error) => self.message = format!("读取域名列表失败：{error}"),
                }
            }
            #[cfg(windows)]
            if ui
                .add_enabled(!draft.domains.is_empty(), egui::Button::new("导出列表…"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("纯文本", &["txt"])
                    .set_file_name("pac-domains.txt")
                    .save_file()
            {
                let content = xray_pac::format_domain_list(&draft.domains);
                if let Err(error) = std::fs::write(path, content) {
                    self.message = format!("导出域名列表失败：{error}");
                } else {
                    self.message = format!("已导出 {} 条代理域名。", draft.domains.len());
                }
            }
            if ui
                .add_enabled(!draft.domains.is_empty(), egui::Button::new("复制列表"))
                .clicked()
            {
                ui.ctx()
                    .copy_text(xray_pac::format_domain_list(&draft.domains));
                self.message = format!("已复制 {} 条域名到剪贴板。", draft.domains.len());
            }
            let has_json = !self.profile_id.is_empty() && !self.editor.trim().is_empty();
            if ui
                .add_enabled(has_json, egui::Button::new("从路由规则提取"))
                .on_hover_text("从当前配置的 routing.rules 提取明文域名；geosite: 规则会跳过")
                .clicked()
            {
                self.extract_pac_domains_from_config(draft, endpoint);
            }
            if ui
                .add_enabled(draft.domains.len() > 1, egui::Button::new("去重排序"))
                .clicked()
            {
                self.dedup_and_sort_pac_domains(draft, endpoint);
            }
        });

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
            return;
        }

        if draft.template_mode == PacTemplateMode::All {
            ui.label(
                RichText::new("当前为全部代理，以下清单暂不生效。")
                    .small()
                    .weak(),
            );
        }

        ui.horizontal_wrapped(|ui| {
            ui.add(
                crate::widgets::singleline(&mut self.ui.pac_domain_filter, 220.0)
                    .hint_text("搜索/筛选域名…"),
            );
            egui::ComboBox::from_id_salt("pac-domain-kind-filter")
                .width(120.0)
                .selected_text(match self.ui.pac_domain_kind_filter {
                    PacDomainFilter::All => "全部类型",
                    PacDomainFilter::DomainAndSub => "含主域",
                    PacDomainFilter::Subdomain => "仅子域 (*.)",
                    PacDomainFilter::Exact => "精确 (full:)",
                })
                .show_ui(ui, |ui| {
                    for (kind, label) in [
                        (PacDomainFilter::All, "全部类型"),
                        (PacDomainFilter::DomainAndSub, "含主域"),
                        (PacDomainFilter::Subdomain, "仅子域 (*.)"),
                        (PacDomainFilter::Exact, "精确 (full:)"),
                    ] {
                        ui.selectable_value(&mut self.ui.pac_domain_kind_filter, kind, label);
                    }
                });
            if !self.ui.pac_domain_filter.is_empty() && ui.button("清除筛选").clicked() {
                self.ui.pac_domain_filter.clear();
            }
        });

        let query = self.ui.pac_domain_filter.trim().to_ascii_lowercase();
        let kind_filter = self.ui.pac_domain_kind_filter;
        let filtered: Vec<(usize, String, PacDomainKind)> = draft
            .domains
            .iter()
            .enumerate()
            .filter_map(|(index, domain)| {
                let kind = xray_pac::domain_kind(domain);
                let kind_ok = match kind_filter {
                    PacDomainFilter::All => true,
                    PacDomainFilter::DomainAndSub => kind == PacDomainKind::DomainAndSub,
                    PacDomainFilter::Subdomain => kind == PacDomainKind::Subdomain,
                    PacDomainFilter::Exact => kind == PacDomainKind::Exact,
                };
                if !kind_ok {
                    return None;
                }
                if !query.is_empty() && !domain.to_ascii_lowercase().contains(&query) {
                    return None;
                }
                Some((index, domain.clone(), kind))
            })
            .collect();

        let total = draft.domains.len();
        let shown = filtered.len();
        let selected_count = self
            .ui
            .pac_selected_domains
            .iter()
            .filter(|selected| draft.domains.iter().any(|d| d == *selected))
            .count();

        ui.horizontal_wrapped(|ui| {
            let stats = if shown == total {
                format!("共 {total} 条 · 已选 {selected_count} 条")
            } else {
                format!("共 {total} 条 · 匹配 {shown} 条 · 已选 {selected_count} 条")
            };
            ui.label(RichText::new(stats).small().weak());
            if ui
                .add_enabled(!filtered.is_empty(), egui::Button::new("全选当前"))
                .clicked()
            {
                for (_, domain, _) in &filtered {
                    self.ui.pac_selected_domains.insert(domain.clone());
                }
            }
            if ui
                .add_enabled(selected_count > 0, egui::Button::new("取消选择"))
                .clicked()
            {
                self.ui.pac_selected_domains.clear();
            }
            if ui
                .add_enabled(
                    selected_count > 0,
                    egui::Button::new(format!("移除所选 ({selected_count})")),
                )
                .clicked()
            {
                let before = draft.domains.len();
                draft
                    .domains
                    .retain(|domain| !self.ui.pac_selected_domains.contains(domain));
                let removed = before.saturating_sub(draft.domains.len());
                self.ui.pac_selected_domains.clear();
                self.commit_pac_domains(draft, endpoint);
                self.message = format!("已批量移除 {removed} 条代理域名。");
                return;
            }
            if ui.button("清空全部").clicked() {
                self.ui.dialog = Some(Dialog::Confirm(Action::ClearPacDomains(
                    self.profile_id.clone(),
                )));
            }
        });

        if filtered.is_empty() {
            ui.label(RichText::new("没有匹配当前筛选条件的域名。").small().weak());
            return;
        }

        let mut remove_index = None;
        let row_height = 30.0;
        egui::Frame::new()
            .stroke(egui::Stroke::new(
                1.0,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ))
            .corner_radius(6.0)
            .inner_margin(egui::Margin::symmetric(8, 6))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("pac-domain-list")
                    .max_height(260.0)
                    .auto_shrink([false, true])
                    .show_rows(ui, row_height, filtered.len(), |ui, range| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for (orig_index, domain, kind) in &filtered[range] {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), 28.0),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let mut checked = self.ui.pac_selected_domains.contains(domain);
                                    if ui.checkbox(&mut checked, "").changed() {
                                        if checked {
                                            self.ui.pac_selected_domains.insert(domain.clone());
                                        } else {
                                            self.ui.pac_selected_domains.remove(domain);
                                        }
                                    }
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui.small_button("移除").clicked() {
                                                remove_index = Some(*orig_index);
                                            }
                                            ui.label(
                                                RichText::new(kind_label(*kind)).small().weak(),
                                            );
                                            ui.with_layout(
                                                egui::Layout::left_to_right(egui::Align::Center),
                                                |ui| {
                                                    ui.add(
                                                        egui::Label::new(
                                                            RichText::new(domain).monospace(),
                                                        )
                                                        .truncate(),
                                                    )
                                                    .on_hover_text(domain);
                                                },
                                            );
                                        },
                                    );
                                },
                            );
                        }
                    });
            });

        if let Some(index) = remove_index {
            let removed = draft.domains.remove(index);
            self.ui.pac_selected_domains.remove(&removed);
            self.commit_pac_domains(draft, endpoint);
            self.message = format!("已移除代理域名：{removed}");
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
        if self.sync_pac_template(&mut draft, endpoint) {
            changed = true;
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
                                self.ui.pac_template_key = None;
                                self.clear_pac_checks();
                                self.sync_pac_template(&mut draft, endpoint);
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
            self.pac_domains_section(ui, &mut draft, endpoint);
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
                                self.ui.pac_template_key = None;
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
                            self.ui.pac_template_key = None;
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
                self.ui.pac_template_key = None;
                self.clear_pac_checks();
                changed = true;
            }
        });
        if changed {
            self.save_pac_draft(draft);
        }
    }
}
