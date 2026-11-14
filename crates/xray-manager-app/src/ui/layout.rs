use super::*;

impl App {
    pub(in crate::app) fn render(&mut self, ctx: &egui::Context) {
        if self.ui.settings_loaded
            && self.ui.profiles_loaded
            && self.ui.connected
            && !self.ui.setup_initialized
        {
            self.ui.setup_initialized = true;
            self.ui.onboarding = self.core_path.is_empty()
                || self.profiles.is_empty()
                || self.committed_id().is_none();
            self.ui.setup_step = if self.core_path.is_empty() {
                0
            } else if self.profiles.is_empty() {
                1
            } else {
                2
            };
        }
        let compact = ctx.content_rect().width() < 900.0;
        egui::SidePanel::left("navigation")
            .exact_width(if compact { 84.0 } else { 190.0 })
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.window_fill())
                    .inner_margin(14),
            )
            .show(ctx, |ui| {
                ui.add_space(14.0);
                ui.label(
                    RichText::new(if compact { "XP" } else { "Xray Pilot" })
                        .strong()
                        .size(21.0)
                        .color(accent(ui)),
                );
                ui.add_space(27.0);
                for (page, title, short) in [
                    (Page::Overview, "总览", "总览"),
                    (Page::Profiles, "配置", "配置"),
                    (Page::Pac, "PAC", "PAC"),
                    (Page::Diagnostics, "诊断", "诊断"),
                    (Page::Logs, "日志", "日志"),
                    (Page::Settings, "设置", "设置"),
                ] {
                    if ui
                        .add_sized(
                            [ui.available_width(), 42.0],
                            egui::Button::new(if compact { short } else { title })
                                .selected(self.ui.page == page),
                        )
                        .clicked()
                    {
                        self.ui.page = page;
                    }
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    if !compact {
                        ui.label(
                            RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                                .small()
                                .weak(),
                        );
                    }
                });
            });
        egui::TopBottomPanel::top("toolbar")
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.window_fill())
                    .inner_margin(egui::Margin::symmetric(22, 12)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("工作空间  /  ").weak());
                    ui.label(match self.ui.page {
                        Page::Overview => "总览",
                        Page::Profiles => "配置",
                        Page::Pac => "PAC",
                        Page::Diagnostics => "诊断",
                        Page::Logs => "日志",
                        Page::Settings => "设置",
                    });
                });
            });
        let has_message = !self.message.trim().is_empty()
            && !matches!(self.message.as_str(), "后台已连接。" | "正在连接后台…");
        if has_message || self.busy || self.shutdown_requested || self.dependency_job.is_some() {
            egui::TopBottomPanel::bottom("feedback")
                .frame(
                    egui::Frame::new()
                        .fill(ctx.style().visuals.window_fill())
                        .inner_margin(egui::Margin::symmetric(20, 10)),
                )
                .show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        if self.busy || self.dependency_job.is_some() {
                            ui.spinner();
                        }
                        if self.shutdown_requested {
                            ui.label(if !self.ui.connected || self.ui.uncertain.is_some() {
                                "正在确认后台连接与原操作结果，确认后将安全退出。"
                            } else {
                                "完成当前操作后，将停止 Core、恢复代理并退出。"
                            });
                        } else if self.busy {
                            ui.label("操作正在执行，仍可浏览页面与日志。");
                        } else {
                            ui.add(
                                egui::Label::new(RichText::new(&self.message).small()).truncate(),
                            );
                        }
                    });
                });
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(24),
            )
            .show(ctx, |ui| {
                let content_height = ui.available_height() - 20.0;
                egui::ScrollArea::vertical()
                    .id_salt("main-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        // Keep scroll documents unbounded, even when banners are
                        // taller than the viewport. Editors receive a size budget.
                        let content_top = ui.cursor().top();
                        self.banners(ui);
                        let page_height =
                            (content_height - (ui.cursor().top() - content_top)).max(0.0);
                        if self.ui.page == Page::Overview && self.ui.onboarding {
                            self.setup(ui);
                        } else {
                            match self.ui.page {
                                Page::Overview => self.overview(ui),
                                Page::Profiles => self.profiles_page(ui, page_height),
                                Page::Pac => self.pac_page(ui),
                                Page::Diagnostics => self.diagnostics(ui),
                                Page::Logs => self.logs_page(ui),
                                Page::Settings => self.settings_page(ui),
                            }
                        }
                        ui.add_space(20.0);
                    });
            });
        self.dialog(ctx);
        if self.ui.dialog.is_none() && self.ui.preview_open {
            self.preview(ctx);
        }
    }
    pub(in crate::app) fn set_appearance(&mut self, appearance: Appearance, ctx: &egui::Context) {
        self.appearance = appearance;
        appearance.apply(ctx);
        if self.ui.persistence
            && let Err(error) = appearance.save(&self.root)
        {
            self.fail(error, String::new());
        }
    }
    pub(in crate::app) fn banners(&mut self, ui: &mut egui::Ui) {
        if !self.ui.connected {
            card(ui, |ui| {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "后台未连接 · 当前展示最后一次已知状态",
                );
                ui.label(
                    "进程和系统代理的最新状态尚未确认。可继续查看已载入内容，写操作暂不可用。",
                );
                if ui.button("重新连接并读取状态").clicked() {
                    self.send(Command::GetStatus);
                    if !self.ui.settings_loaded {
                        self.send(Command::GetSettings);
                    }
                    if !self.ui.profiles_loaded {
                        self.refresh_profiles();
                    }
                }
            });
            ui.add_space(14.0);
        }
        if let Some(request) = self.ui.uncertain.as_ref().map(|u| u.request_id.clone()) {
            card(ui, |ui| {
                ui.colored_label(ui.visuals().warn_fg_color, "操作结果待确认，请勿重复提交");
                ui.label("后台响应中断，当前状态不能证明原操作是否已经完成。");
                ui.monospace(&request);
                if ui.button("查询原操作结果").clicked() {
                    self.send(Command::GetOperation {
                        request_id: request,
                    });
                }
            });
            ui.add_space(14.0);
        }
        if self.status.as_ref().is_some_and(|s| s.recovery_required) {
            card(ui, |ui| {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "存在未完成的恢复，普通变更已暂停",
                );
                ui.label("先检查下方错误与操作证据。涉及外部代理变更时，保留其他程序的设置，不自动覆盖。");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("重新查询恢复状态").clicked() {
                        self.send(Command::GetStatus);
                    }
                    if ui.button("查看诊断").clicked() {
                        self.ui.page = Page::Diagnostics;
                    }
                    if ui
                        .add_enabled(
                            self.ui.connected && !self.busy && self.ui.uncertain.is_none(),
                            egui::Button::new("安全停止 Core"),
                        )
                        .clicked()
                    {
                        self.ui.dialog = Some(Dialog::Confirm(Action::Stop));
                    }
                    if self.can_resolve_proxy_conflict()
                        && ui.button("保留当前系统代理并解除冲突…").clicked()
                    {
                        self.ui.dialog = Some(Dialog::Confirm(Action::ResolveProxyConflict));
                    }
                });
                if let Some(error) = self.status.as_ref().and_then(|s| s.last_error.as_ref()) {
                    egui::CollapsingHeader::new("恢复证据与技术详情").show(ui, |ui| {
                        ui.monospace(error.to_string());
                    });
                }
            });
            ui.add_space(14.0);
        }
        if let Some(failure) = &self.ui.failure {
            card(ui, |ui| {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    error_advice(&failure.error.code),
                );
                egui::CollapsingHeader::new("错误详情与请求身份").show(ui, |ui| {
                    ui.monospace(failure.error.to_string());
                    if !failure.request.is_empty() {
                        ui.monospace(format!("request_id: {}", failure.request));
                    }
                });
            });
            ui.add_space(14.0);
        } else if let Some(error) = self
            .status
            .as_ref()
            .filter(|s| !s.recovery_required)
            .and_then(|s| s.last_error.as_ref())
        {
            card(ui, |ui| {
                ui.colored_label(ui.visuals().warn_fg_color, "后台最近记录的错误");
                ui.label(error_advice(&error.code));
                egui::CollapsingHeader::new("后台错误详情").show(ui, |ui| {
                    ui.monospace(error.to_string());
                });
            });
            ui.add_space(14.0);
        }
    }
    pub(in crate::app) fn overview(&mut self, ui: &mut egui::Ui) {
        heading(ui, "总览", "关注运行状态，把常用操作留在手边。");
        let running = self.is_running();
        let enabled = self.write_blocker().is_none();
        let generation = self.status.as_ref().and_then(|s| s.generation.clone());
        card(ui, |ui| {
            ui.label(RichText::new("XRAY CORE").small().weak());
            ui.label(
                RichText::new(if !self.ui.connected {
                    "状态待确认"
                } else if running {
                    "Core 运行中"
                } else if self
                    .status
                    .as_ref()
                    .is_some_and(|s| s.core_state == "Degraded")
                {
                    "Core 正在等待恢复"
                } else {
                    "Core 已停止"
                })
                .size(29.0)
                .strong(),
            );
            if let Some(g) = &generation {
                ui.label(
                    RichText::new(format!(
                        "{} · {}{}",
                        self.ui.workspace.name(&g.candidate.profile_id),
                        g.candidate.core.version,
                        if self.ui.connected {
                            ""
                        } else {
                            " · 最后已知状态"
                        }
                    ))
                    .weak(),
                );
            } else {
                ui.label("选择一份配置，检查后开始运行。");
            }
            if let Some(notice) = self
                .status
                .as_ref()
                .and_then(|status| status.auto_connect_notice.as_deref())
            {
                ui.colored_label(ui.visuals().warn_fg_color, notice);
            }
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if running {
                    if ui
                        .add_enabled(
                            self.ui.connected && !self.busy && self.ui.uncertain.is_none(),
                            egui::Button::new("停止 Core"),
                        )
                        .clicked()
                    {
                        self.ui.dialog = Some(Dialog::Confirm(Action::Stop));
                    }
                    if ui
                        .add_enabled(enabled, egui::Button::new("重启当前运行版本"))
                        .clicked()
                    {
                        self.ui.dialog = Some(Dialog::Confirm(Action::Restart));
                    }
                } else {
                    if primary(ui, "选择配置并启动", enabled) {
                        self.ui.page = Page::Profiles;
                    }
                    if generation.is_some()
                        && ui
                            .add_enabled(enabled, egui::Button::new("启动上次应用版本"))
                            .clicked()
                    {
                        self.ui.dialog = Some(Dialog::Confirm(Action::Restart));
                    }
                }
                if ui.button("管理配置").clicked() {
                    self.ui.page = Page::Profiles;
                }
            });
        });
        ui.add_space(16.0);
        let runtime = self
            .ui
            .runtime
            .as_ref()
            .filter(|r| generation.as_ref().is_some_and(|g| g.id == r.generation_id));
        let endpoint = runtime
            .and_then(|r| serde_json::from_str::<serde_json::Value>(&r.json).ok())
            .and_then(|v| {
                v.get("inbounds")?
                    .as_array()?
                    .iter()
                    .find(|i| i.get("protocol").and_then(|v| v.as_str()) == Some("http"))
                    .cloned()
            })
            .map(|i| {
                format!(
                    "{}:{}",
                    i.get("listen")
                        .and_then(|v| v.as_str())
                        .unwrap_or("0.0.0.0"),
                    i.get("port").map(|v| v.to_string()).unwrap_or_default()
                )
            })
            .unwrap_or_else(|| "无已读取的 HTTP 入口".into());
        let name = generation
            .as_ref()
            .map(|g| self.ui.workspace.name(&g.candidate.profile_id).to_string())
            .unwrap_or_else(|| "无运行配置".into());
        let proxy = self.proxy_label();
        ui.columns(3, |columns| {
            for (col, (title, value, note)) in columns.iter_mut().zip([
                (
                    "当前运行配置",
                    if running {
                        name.as_str()
                    } else {
                        "无运行配置"
                    },
                    "草稿编辑不会改变运行版本",
                ),
                (
                    "本机 HTTP 入口",
                    endpoint.as_str(),
                    if running {
                        "真实网络未检测"
                    } else {
                        "Core 当前未运行"
                    },
                ),
                ("系统代理", proxy.as_str(), "未接管不代表其他代理已关闭"),
            ]) {
                card(col, |ui| {
                    ui.set_min_height(100.0);
                    ui.label(RichText::new(title).small().weak());
                    ui.add(egui::Label::new(RichText::new(value).strong().size(17.0)).truncate())
                        .on_hover_text(value);
                    ui.label(RichText::new(note).small().weak());
                });
            }
        });
        ui.add_space(16.0);
        card(ui, |ui| {
            ui.label(RichText::new("连接方式").strong().size(18.0));
            ui.label(
                RichText::new("仅影响系统代理；Core 仍按原生 JSON 的路由和出站转发。")
                    .small()
                    .weak(),
            );
            let owned = self
                .status
                .as_ref()
                .is_some_and(|s| s.proxy_state == "Owned");
            ui.horizontal_wrapped(|ui| {
                let committed = self
                    .status
                    .as_ref()
                    .and_then(|s| s.generation.as_ref())
                    .map(|g| g.proxy_mode);
                for mode in [ProxyMode::Direct, ProxyMode::Global, ProxyMode::Pac] {
                    let selected = if owned {
                        committed == Some(mode)
                    } else {
                        mode == ProxyMode::Direct
                    };
                    if ui
                        .add_enabled(
                            enabled && (mode == ProxyMode::Direct || running),
                            egui::Button::new(mode_name(mode)).selected(selected),
                        )
                        .clicked()
                        && !selected
                    {
                        self.ui.dialog = Some(Dialog::Confirm(Action::Proxy(mode)));
                    }
                }
            });
            ui.label(
                RichText::new("解除接管时恢复接管前的设置，其他程序的代理仍可能有效。")
                    .small()
                    .weak(),
            );
            line(ui, "网络连通性", "未检测");
            line(ui, "实际出口", "未验证");
            if ui.button("打开路由诊断").clicked() {
                self.ui.page = Page::Diagnostics;
            }
        });
        ui.add_space(16.0);
        card(ui, |ui| {
            ui.label(RichText::new("最近活动").strong().size(18.0));
            if self.ui.events.is_empty() {
                ui.label(RichText::new("暂无操作记录").weak());
            }
            for event in self.ui.events.iter().take(4) {
                ui.label(&event.message);
                ui.label(
                    RichText::new(format!("{} 秒前", event.at.elapsed().as_secs()))
                        .small()
                        .weak(),
                );
            }
            ui.separator();
            line(
                ui,
                "运行中的 Core",
                if running {
                    generation
                        .as_ref()
                        .map(|g| g.candidate.core.version.as_str())
                        .unwrap_or("未知")
                } else {
                    "未运行"
                },
            );
            let next_core = if self.core_path.is_empty() {
                "尚未选择".into()
            } else {
                display_path_text(&self.core_path)
            };
            line(ui, "下次检查使用", &next_core);
            if ui.button("查看日志").clicked() {
                self.ui.page = Page::Logs;
            }
        });
    }
    pub(in crate::app) fn proxy_label(&self) -> String {
        if !self.ui.connected {
            return "状态待确认".into();
        }
        match self.status.as_ref().map(|s| s.proxy_state.as_str()) {
            Some("Owned") => "由本程序接管".into(),
            Some("Unmanaged" | "Released") => "未由本程序接管".into(),
            Some("Conflict") => "代理设置存在外部冲突".into(),
            Some("RecoveryRequired" | "RestoreFailed") => "需要恢复处理".into(),
            Some(other) => format!("代理状态：{other}"),
            None => "尚未读取".into(),
        }
    }
}
