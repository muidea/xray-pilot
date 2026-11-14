use super::*;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use xray_config::geodata::{GeoData, GeoSiteCategorySummary};
use xray_model::{PacMode, hash};
use xray_pac::PacDomainKind;

const QUICK_GEOSITES: &[(&str, &str)] = &[
    ("private", "局域网"),
    ("cn", "国内常见"),
    ("geolocation-!cn", "海外常见"),
    ("category-ads-all", "广告屏蔽"),
    ("apple-cn", "苹果国内"),
    ("google", "Google"),
    ("github", "GitHub"),
];

fn hash_list_and_context(
    geosites: &[String],
    domains: &[String],
    config_json: Option<&str>,
    geo_stamp: Option<(u64, Option<std::time::SystemTime>)>,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    geosites.hash(&mut hasher);
    domains.hash(&mut hasher);
    config_json.hash(&mut hasher);
    geo_stamp.hash(&mut hasher);
    hasher.finish()
}

fn kind_label(kind: PacDomainKind) -> &'static str {
    match kind {
        PacDomainKind::Exact => "精确",
        PacDomainKind::Subdomain => "仅子域",
        PacDomainKind::DomainAndSub => "含主域",
    }
}

fn mode_label(mode: PacMode) -> &'static str {
    match mode {
        PacMode::AutoRules => "按当前路由规则自动生成（推荐）",
        PacMode::DirectList => "选中 GeoSite 与域名直连，其余代理",
        PacMode::ProxyList => "仅代理选中 GeoSite 与域名，其余直连",
        PacMode::All => "全部代理（本地/局域网直连）",
        PacMode::Custom => "自定义 PAC 脚本",
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
    pub(super) fn refresh_geodata_cache(&mut self) -> Option<GeoDataCache> {
        let asset_root = self.asset_root.trim().to_owned();
        if asset_root.is_empty() {
            self.ui.geodata_cache = None;
            return None;
        }
        let geosite_path = std::path::Path::new(&asset_root).join("geosite.dat");
        let Ok(meta) = std::fs::metadata(&geosite_path) else {
            self.ui.geodata_cache = None;
            return None;
        };
        let stamp = (meta.len(), meta.modified().ok());
        if let Some(cached) = &self.ui.geodata_cache
            && cached.asset_root == asset_root
            && cached.stamp == stamp
        {
            return Some(cached.clone());
        }
        let Ok(mut data) = GeoData::load_from_asset_root(std::path::Path::new(&asset_root)) else {
            self.ui.geodata_cache = None;
            return None;
        };
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.editor) {
            data.preload_ext_from_config(&value);
        }
        let categories = data.categories();
        let entry = GeoDataCache {
            asset_root,
            stamp,
            data,
            categories,
        };
        self.ui.geodata_cache = Some(entry.clone());
        Some(entry)
    }

    fn update_pac_check_state(&mut self, script: &str, mut notes: Vec<String>) {
        match xray_pac::validate(script) {
            Ok(()) => {
                self.ui.pac_checked = true;
                for note in xray_pac::compat_notes(script) {
                    if !notes.contains(&note) {
                        notes.push(note);
                    }
                }
                self.ui.pac_notes = notes;
            }
            Err(error) => {
                self.ui.pac_checked = false;
                notes.insert(0, format!("语法校验未通过：{}", error.message));
                self.ui.pac_notes = notes;
            }
        }
    }

    fn sync_pac_script(
        &mut self,
        draft: &mut PacDraft,
        endpoint: Option<std::net::SocketAddr>,
        cached_geo: Option<&GeoDataCache>,
    ) -> bool {
        if draft.mode == PacMode::Custom {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            draft.script.hash(&mut hasher);
            let script_hash = hasher.finish();
            let custom_key = (
                self.profile_id.clone(),
                std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                PacMode::Custom,
                script_hash,
            );
            if self.ui.pac_compile_key.as_ref() != Some(&custom_key) {
                self.ui.pac_compile_key = Some(custom_key);
                self.ui.pac_test.clear();
                self.ui.pac_compile_stats = xray_pac::PacCompileStats {
                    script_bytes: draft.script.len(),
                    ..Default::default()
                };
                if draft.script.trim().is_empty() {
                    self.ui.pac_checked = false;
                    self.ui.pac_notes.clear();
                } else {
                    self.update_pac_check_state(&draft.script, Vec::new());
                }
            }
            return false;
        }

        let Some(endpoint) = endpoint else {
            return false;
        };
        let config_str = matches!(draft.mode, PacMode::AutoRules).then_some(self.editor.as_str());
        let key = (
            self.profile_id.clone(),
            endpoint,
            draft.mode,
            hash_list_and_context(
                &draft.geosites,
                &draft.domains,
                config_str,
                cached_geo.map(|c| c.stamp),
            ),
        );
        if self.ui.pac_compile_key.as_ref() == Some(&key) && !draft.script.is_empty() {
            return false;
        }
        self.ui.pac_compile_key = Some(key);
        let config_value = serde_json::from_str::<serde_json::Value>(&self.editor)
            .unwrap_or_else(|_| serde_json::json!({}));
        match xray_pac::compile_draft(&config_value, endpoint, draft, cached_geo.map(|c| &c.data)) {
            Ok(generated) => {
                let script_changed = draft.script != generated.script;
                if script_changed {
                    draft.script = generated.script;
                    self.ui.pac_test.clear();
                }
                self.ui.pac_compile_stats = generated.stats;
                self.update_pac_check_state(&draft.script, generated.notes);
                script_changed
            }
            Err(error) => {
                self.ui.pac_checked = false;
                self.ui.pac_notes = vec![error.message.clone()];
                self.message = error.message;
                false
            }
        }
    }

    fn commit_pac_list_change(
        &mut self,
        draft: &mut PacDraft,
        endpoint: Option<std::net::SocketAddr>,
    ) {
        if !matches!(draft.mode, PacMode::DirectList | PacMode::ProxyList) {
            draft.mode = PacMode::ProxyList;
        }
        self.ui.pac_compile_key = None;
        let cached_geo = self.refresh_geodata_cache();
        self.sync_pac_script(draft, endpoint, cached_geo.as_ref());
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
            if let Some(spec) = xray_pac::normalize_geosite_spec(input) {
                if draft.geosites.iter().any(|g| g.eq_ignore_ascii_case(&spec)) {
                    self.message = format!("GeoSite 分组已在清单中：{spec}");
                    return false;
                }
                draft.geosites.push(spec.clone());
                self.ui.pac_domain_input.clear();
                self.commit_pac_list_change(draft, endpoint);
                self.message = format!("已添加并保存 GeoSite 分组：{spec}");
                return true;
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
                    self.commit_pac_list_change(draft, endpoint);
                    self.message = format!("已添加并保存域名：{domain}");
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
            self.commit_pac_list_change(draft, endpoint);
            self.message = if parsed.invalid.is_empty() && parsed.duplicates == 0 {
                format!("已批量添加并保存 {added_count} 条域名。")
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
            self.commit_pac_list_change(&mut draft, endpoint);
        } else if !parsed.added.is_empty() {
            draft.domains.extend(parsed.added.iter().cloned());
            self.commit_pac_list_change(&mut draft, endpoint);
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
        let count = draft.domains.len() + draft.geosites.len();
        draft.domains.clear();
        draft.geosites.clear();
        self.ui.pac_selected_domains.clear();
        self.ui.pac_domain_filter.clear();
        let endpoint = self.pac_endpoint().ok();
        self.commit_pac_list_change(&mut draft, endpoint);
        self.message = format!("已清空 {count} 项清单规则。");
    }

    fn extract_pac_domains_from_config(
        &mut self,
        draft: &mut PacDraft,
        endpoint: Option<std::net::SocketAddr>,
    ) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&self.editor) else {
            self.message = "当前 JSON 无法解析，无法提取路由规则。".into();
            return;
        };
        let extracted = xray_pac::extract_domains_from_config(&value);
        let mut existing_domains: HashSet<String> = draft
            .domains
            .iter()
            .map(|d| d.to_ascii_lowercase())
            .collect();
        let mut existing_geosites: HashSet<String> = draft
            .geosites
            .iter()
            .map(|g| g.to_ascii_lowercase())
            .collect();
        let mut added_domains = 0usize;
        let mut added_geosites = 0usize;
        let mut duplicates = 0usize;

        for tag in extracted.geosites {
            if existing_geosites.insert(tag.to_ascii_lowercase()) {
                draft.geosites.push(tag);
                added_geosites += 1;
            } else {
                duplicates += 1;
            }
        }
        for domain in extracted.domains {
            if existing_domains.insert(domain.to_ascii_lowercase()) {
                draft.domains.push(domain);
                added_domains += 1;
            } else {
                duplicates += 1;
            }
        }
        if added_domains > 0 || added_geosites > 0 {
            self.commit_pac_list_change(draft, endpoint);
        }
        let mut parts = vec![format!(
            "已从路由提取：{added_geosites} 个 GeoSite 分组、{added_domains} 条域名"
        )];
        if duplicates > 0 {
            parts.push(format!("已存在 {duplicates} 项"));
        }
        let skipped = extracted.skipped_regexp + extracted.skipped_other;
        if skipped > 0 {
            parts.push(format!("跳过 {skipped} 条正则或关键字规则"));
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
        cleaned.sort_by_key(|d| {
            d.strip_prefix("full:")
                .or_else(|| d.strip_prefix("*."))
                .unwrap_or(d)
                .to_ascii_lowercase()
        });
        let removed_dups = before.saturating_sub(cleaned.len());
        draft.domains = cleaned;
        draft.geosites.sort();
        draft.geosites.dedup();
        self.ui
            .pac_selected_domains
            .retain(|selected| draft.domains.iter().any(|d| d == selected));
        self.commit_pac_list_change(draft, endpoint);
        self.message = format!("已整理并按域名排序（去除 {removed_dups} 条重复）。");
    }

    fn pac_geosite_section(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut PacDraft,
        endpoint: Option<std::net::SocketAddr>,
        categories: &[GeoSiteCategorySummary],
    ) -> bool {
        let mut changed = false;
        ui.add_space(6.0);
        ui.label(RichText::new("GeoSite 分组").strong());

        if categories.is_empty() {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "未检测到 geosite.dat（可在「设置 · 内核与资源」下载或配置资源目录）。",
            );
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("常用速选：").small().weak());
                for (name, title) in QUICK_GEOSITES {
                    let Some(cat) = categories.iter().find(|c| c.name == *name) else {
                        continue;
                    };
                    let spec = format!("geosite:{name}");
                    let selected = draft.geosites.iter().any(|g| g.eq_ignore_ascii_case(&spec));
                    let resp = ui
                        .selectable_label(selected, format!("{title} ({name})"))
                        .on_hover_text(format!(
                            "{spec} · 可导出 {} 条域名（含 {} 条规则总数）",
                            cat.pac_compatible(),
                            cat.total()
                        ));
                    if resp.clicked() {
                        if selected {
                            draft.geosites.retain(|g| !g.eq_ignore_ascii_case(&spec));
                        } else {
                            draft.geosites.push(spec);
                        }
                        self.ui.pac_compile_key = None;
                        changed = true;
                    }
                }
            });
        }

        if !draft.geosites.is_empty() {
            ui.add_space(4.0);
            let mut remove_spec = None;
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("已选分组：").small().weak());
                for spec in &draft.geosites {
                    if ui
                        .button(format!("{spec} ×"))
                        .on_hover_text("点击移除此 GeoSite 分组")
                        .clicked()
                    {
                        remove_spec = Some(spec.clone());
                    }
                }
            });
            if let Some(target) = remove_spec {
                draft.geosites.retain(|g| g != &target);
                self.ui.pac_compile_key = None;
                changed = true;
            }
        }

        if !categories.is_empty() {
            egui::CollapsingHeader::new(format!(
                "浏览 geosite.dat 全部分组（{} 类）",
                categories.len()
            ))
            .id_salt("pac-geosite-browser")
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("搜索分组");
                    ui.add(
                        crate::widgets::singleline(&mut self.ui.pac_geosite_search, f32::INFINITY)
                            .hint_text("输入关键词过滤，如 apple / steam / google / cn"),
                    );
                });
                let needle = self.ui.pac_geosite_search.trim().to_ascii_lowercase();
                let matches: Vec<&GeoSiteCategorySummary> = categories
                    .iter()
                    .filter(|c| needle.is_empty() || c.name.contains(&needle))
                    .collect();
                let row_height = 26.0;
                egui::ScrollArea::vertical()
                    .id_salt("pac-geosite-scroll")
                    .max_height(180.0)
                    .auto_shrink([false, true])
                    .show_rows(ui, row_height, matches.len(), |ui, range| {
                        for cat in &matches[range] {
                            let spec = format!("geosite:{}", cat.name);
                            let mut checked =
                                draft.geosites.iter().any(|g| g.eq_ignore_ascii_case(&spec));
                            let label = if cat.regex_count > 0 {
                                format!(
                                    "{}  ({} 域名 · 忽略 {} 正则)",
                                    cat.name,
                                    cat.pac_compatible(),
                                    cat.regex_count
                                )
                            } else {
                                format!("{}  ({} 域名)", cat.name, cat.pac_compatible())
                            };
                            if ui.checkbox(&mut checked, label).changed() {
                                if checked {
                                    if !draft.geosites.iter().any(|g| g.eq_ignore_ascii_case(&spec))
                                    {
                                        draft.geosites.push(spec);
                                    }
                                } else {
                                    draft.geosites.retain(|g| !g.eq_ignore_ascii_case(&spec));
                                }
                                self.ui.pac_compile_key = None;
                                changed = true;
                            }
                        }
                    });
            });
        }
        if changed {
            self.commit_pac_list_change(draft, endpoint);
        }
        changed
    }

    fn pac_domains_section(
        &mut self,
        ui: &mut egui::Ui,
        draft: &mut PacDraft,
        endpoint: Option<std::net::SocketAddr>,
    ) {
        ui.label(RichText::new("2 · 自定义域名清单").strong().size(18.0));
        ui.label(
            RichText::new(
                "可输入单条域名、geosite:分组（支持 @attr），或直接粘贴多行/逗号分隔清单，回车即保存。",
            )
            .small()
            .weak(),
        );
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let input_width = (ui.available_width() - 88.0).max(160.0);
            let response = ui.add(
                crate::widgets::singleline(&mut self.ui.pac_domain_input, input_width)
                    .hint_text("example.com 或 *.example.com / full:exact.com / geosite:cn"),
            );
            let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let clicked = ui.button("添加").clicked();
            if enter || clicked {
                self.add_pac_domain(draft, endpoint);
            }
        });

        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    !self.profile_id.is_empty(),
                    egui::Button::new("从路由规则提取"),
                )
                .on_hover_text("从当前配置的 /routing/rules 提取普通域名与 geosite 分组")
                .clicked()
            {
                self.extract_pac_domains_from_config(draft, endpoint);
            }
            if ui.button("批量粘贴…").clicked() {
                self.ui.pac_batch_input = if draft.domains.is_empty() {
                    String::new()
                } else {
                    xray_pac::format_domain_list(&draft.domains)
                };
                self.ui.pac_batch_report = None;
                self.ui.pac_batch_replace = false;
                self.ui.dialog = Some(Dialog::PacBatch);
            }
            if ui
                .add_enabled(
                    !draft.domains.is_empty() || !draft.geosites.is_empty(),
                    egui::Button::new("整理去重"),
                )
                .clicked()
            {
                self.dedup_and_sort_pac_domains(draft, endpoint);
            }
            if ui
                .add_enabled(!draft.domains.is_empty(), egui::Button::new("复制清单"))
                .clicked()
            {
                ui.ctx()
                    .copy_text(xray_pac::format_domain_list(&draft.domains));
                self.message = format!("已复制 {} 条域名到剪贴板。", draft.domains.len());
            }
        });

        if draft.domains.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "自定义域名清单为空；可直接选择上方 GeoSite 分组，或手动添加额外域名。",
                )
                .small()
                .weak(),
            );
            return;
        }

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            ui.label("筛选");
            ui.add(
                crate::widgets::singleline(&mut self.ui.pac_domain_filter, 210.0)
                    .hint_text("按域名搜索"),
            );
            if !self.ui.pac_domain_filter.is_empty() && ui.small_button("清除").clicked() {
                self.ui.pac_domain_filter.clear();
            }
            for (kind, label) in [
                (PacDomainFilter::All, "全部"),
                (PacDomainFilter::DomainAndSub, "含主域"),
                (PacDomainFilter::Subdomain, "仅子域"),
                (PacDomainFilter::Exact, "精确"),
            ] {
                ui.selectable_value(&mut self.ui.pac_domain_kind_filter, kind, label);
            }
        });

        let needle = self.ui.pac_domain_filter.trim().to_ascii_lowercase();
        let filtered: Vec<(usize, String, PacDomainKind)> = draft
            .domains
            .iter()
            .enumerate()
            .filter_map(|(idx, domain)| {
                let kind = xray_pac::domain_kind(domain);
                let kind_ok = match self.ui.pac_domain_kind_filter {
                    PacDomainFilter::All => true,
                    PacDomainFilter::DomainAndSub => kind == PacDomainKind::DomainAndSub,
                    PacDomainFilter::Subdomain => kind == PacDomainKind::Subdomain,
                    PacDomainFilter::Exact => kind == PacDomainKind::Exact,
                };
                if !kind_ok {
                    return None;
                }
                if !needle.is_empty() && !domain.to_ascii_lowercase().contains(&needle) {
                    return None;
                }
                Some((idx, domain.clone(), kind))
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
                self.commit_pac_list_change(draft, endpoint);
                self.message = format!("已批量移除 {removed} 条域名。");
                return;
            }
            if ui.button("清空清单").clicked() {
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
        let row_height = 28.0;
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
                    .max_height(240.0)
                    .auto_shrink([false, true])
                    .show_rows(ui, row_height, filtered.len(), |ui, range| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for (orig_index, domain, kind) in &filtered[range] {
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), 26.0),
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
            self.commit_pac_list_change(draft, endpoint);
            self.message = format!("已移除域名：{removed}");
        }
    }

    pub(in crate::app) fn pac_page(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "PAC 自动代理",
            "结合当前配置路由与 GeoSite 自动生成分流 PAC，实时检查，测试后一键应用。",
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
        let cached_geo = self.refresh_geodata_cache();
        if self.sync_pac_script(&mut draft, endpoint, cached_geo.as_ref()) {
            changed = true;
        }

        // Card 1: mode & GeoSite picker.
        card(ui, |ui| {
            ui.label(RichText::new("1 · 分流模式与 GeoSite").strong().size(18.0));
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
                ui.label("PAC 策略");
                egui::ComboBox::from_id_salt("pac-mode-select")
                    .width(280.0)
                    .selected_text(mode_label(draft.mode))
                    .show_ui(ui, |ui| {
                        for mode in [
                            PacMode::AutoRules,
                            PacMode::DirectList,
                            PacMode::ProxyList,
                            PacMode::All,
                            PacMode::Custom,
                        ] {
                            if ui
                                .selectable_value(&mut draft.mode, mode, mode_label(mode))
                                .clicked()
                            {
                                self.ui.pac_compile_key = None;
                                self.sync_pac_script(&mut draft, endpoint, cached_geo.as_ref());
                                self.save_pac_draft(draft.clone());
                                self.message = format!("已切换 PAC 策略：{}", mode_label(mode));
                                changed = false;
                            }
                        }
                    });
            });

            match draft.mode {
                PacMode::AutoRules => {
                    ui.label(
                        RichText::new(
                            "直接读取当前配置的「路由规则」，自动展开 geosite: 分组并折叠冗余子域，其他流量走 Xray 兜底。",
                        )
                        .small()
                        .weak(),
                    );
                }
                PacMode::DirectList => {
                    ui.label(
                        RichText::new(
                            "仅下列选中的 GeoSite 分组与自定义域名直连，其余流量进入 Xray。",
                        )
                        .small()
                        .weak(),
                    );
                }
                PacMode::ProxyList => {
                    ui.label(
                        RichText::new("仅下列选中的 GeoSite 分组与自定义域名走代理，其余直连。")
                            .small()
                            .weak(),
                    );
                }
                PacMode::All => {
                    ui.label(
                        RichText::new(
                            "所有请求（本地回环及 .lan/.local 局域网除外）统一进入 Xray。",
                        )
                        .small()
                        .weak(),
                    );
                }
                PacMode::Custom => {
                    ui.label(
                        RichText::new("使用下方「高级设置」中用户自定义的 JavaScript 脚本。")
                            .small()
                            .weak(),
                    );
                }
            }

            let stats = &self.ui.pac_compile_stats;
            let total_entries =
                stats.domain_count + stats.full_count + stats.subdomain_count + stats.keyword_count;
            if draft.mode != PacMode::All {
                let mut summary = format!(
                    "脚本体积：{:.1} KiB",
                    stats.script_bytes.max(draft.script.len()) as f64 / 1024.0
                );
                if total_entries > 0 || !stats.resolved_geosites.is_empty() {
                    summary = format!(
                        "已展开 {} 个 GeoSite · {} 条生效规则（含折叠 {} 条冗余子域）· {summary}",
                        stats.resolved_geosites.len(),
                        total_entries,
                        stats.folded_count
                    );
                }
                ui.label(RichText::new(summary).small().weak());
            }

            if matches!(draft.mode, PacMode::DirectList | PacMode::ProxyList) {
                let categories = cached_geo
                    .as_ref()
                    .map(|c| c.categories.as_slice())
                    .unwrap_or(&[]);
                changed |= self.pac_geosite_section(ui, &mut draft, endpoint, categories);
            }
        });

        // Card 2: custom domain list (only when DirectList / ProxyList is active).
        if matches!(draft.mode, PacMode::DirectList | PacMode::ProxyList) {
            ui.add_space(12.0);
            card(ui, |ui| {
                self.pac_domains_section(ui, &mut draft, endpoint);
            });
        }

        // Card 3: live status, test & apply.
        ui.add_space(12.0);
        card(ui, |ui| {
            ui.label(RichText::new("测试与一键应用").strong().size(18.0));
            line(
                ui,
                "脚本状态",
                if draft.script.trim().is_empty() {
                    "等待有效配置入站"
                } else if self.ui.pac_checked {
                    "已通过自动语法检查"
                } else {
                    "语法校验未通过"
                },
            );
            if !self.ui.pac_notes.is_empty() {
                egui::CollapsingHeader::new(format!(
                    "编译与兼容提示（{} 条）",
                    self.ui.pac_notes.len()
                ))
                .default_open(!self.ui.pac_checked || self.ui.pac_notes.len() <= 2)
                .show(ui, |ui| {
                    for note in &self.ui.pac_notes {
                        ui.label(RichText::new(note).small().weak());
                    }
                });
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("测试 URL");
                let input_w = (ui.available_width() - 190.0).max(160.0);
                ui.add(
                    crate::widgets::singleline(&mut self.ui.pac_test_url, input_w)
                        .hint_text("https://example.com/"),
                );
                if ui
                    .add_enabled(
                        self.ui.connected && !draft.script.trim().is_empty() && !self.busy,
                        egui::Button::new("测试命中结果"),
                    )
                    .clicked()
                {
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
            });

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
            } else if !self.ui.pac_test.is_empty() {
                ui.monospace(&self.ui.pac_test);
            }

            ui.separator();
            if let Some(pac) = self
                .status
                .as_ref()
                .and_then(|s| s.generation.as_ref())
                .and_then(|g| g.pac.as_ref())
            {
                line(ui, "当前 PAC URL", &pac.url);
                if pac.revision == hash(draft.script.as_bytes()) {
                    ui.label(RichText::new("当前运行版本已同步此 PAC 脚本。").weak());
                } else {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "当前运行版本仍是旧脚本；点击下方按钮检查并应用新规则。",
                    );
                }
            } else {
                ui.label(RichText::new("当前运行版本未启用 PAC 模式。").weak());
            }

            if primary(
                ui,
                "检查并应用 PAC",
                endpoint.is_some()
                    && self.ui.pac_checked
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

        // Collapsible Advanced settings.
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
                            draft.mode = PacMode::Custom;
                            self.ui.pac_compile_key = None;
                            self.sync_pac_script(&mut draft, endpoint, cached_geo.as_ref());
                            changed = true;
                        }
                        Err(error) => self.message = format!("读取 PAC 文件失败：{error}"),
                    }
                }
                if draft.mode != PacMode::AutoRules && ui.button("恢复自动路由生成").clicked()
                {
                    draft.mode = PacMode::AutoRules;
                    self.ui.pac_compile_key = None;
                    self.sync_pac_script(&mut draft, endpoint, cached_geo.as_ref());
                    changed = true;
                }
            });
            ui.label(
                RichText::new(
                    "直接编辑脚本会自动切换到「自定义脚本」模式；自定义脚本在独立沙盒评估。",
                )
                .small()
                .weak(),
            );
            let response = egui::ScrollArea::vertical()
                .id_salt("pac-script")
                .max_height(320.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut draft.script)
                            .code_editor()
                            .desired_rows(14)
                            .desired_width(f32::INFINITY),
                    )
                })
                .inner;
            if response.changed() {
                draft.mode = PacMode::Custom;
                self.ui.pac_compile_key = None;
                self.sync_pac_script(&mut draft, endpoint, cached_geo.as_ref());
                changed = true;
            }
        });
        if changed {
            self.save_pac_draft(draft);
        }
    }
}
