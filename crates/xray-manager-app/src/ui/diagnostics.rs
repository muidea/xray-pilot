use super::*;

impl App {
    pub(in crate::app) fn diagnostics(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "诊断",
            "进程、代理归属与本机探针分别呈现；路由只做静态预测，实际出口需外部验证。",
        );
        card(ui, |ui| {
            ui.label(RichText::new("本机状态").strong().size(18.0));
            line(
                ui,
                "后台连接",
                if self.ui.connected {
                    "已连接"
                } else {
                    "中断 · 下列为最后已知状态"
                },
            );
            line(
                ui,
                "Core 进程",
                self.status
                    .as_ref()
                    .map(|s| match s.core_state.as_str() {
                        "Running" => "运行中",
                        "Degraded" => "降级 / 等待恢复",
                        _ => "已停止",
                    })
                    .unwrap_or("未读取"),
            );
            line(ui, "系统代理归属", &self.proxy_label());
            if let Some(status) = &self.status {
                line(
                    ui,
                    "代理模式",
                    status
                        .generation
                        .as_ref()
                        .map(|g| mode_name(g.proxy_mode))
                        .unwrap_or("未应用"),
                );
                line(
                    ui,
                    "恢复状态",
                    if status.recovery_required {
                        "需要安全恢复"
                    } else {
                        "正常"
                    },
                );
                if let Some(notice) = &status.auto_connect_notice {
                    line(ui, "自动连接", notice);
                }
                if let Some(error) = &status.last_error {
                    line(ui, "最近错误", &error.code);
                    egui::CollapsingHeader::new("错误详情").show(ui, |ui| {
                        ui.add(egui::Label::new(&error.message).wrap());
                    });
                }
            }
            if self.can_resolve_proxy_conflict()
                && ui.button("保留当前系统代理并解除冲突…").clicked()
            {
                self.ui.dialog = Some(Dialog::Confirm(Action::ResolveProxyConflict));
            }
            line(
                ui,
                "本地入口",
                match self.status.as_ref().map(|s| s.listener_state.as_str()) {
                    Some("Ready") => "就绪（本机探针已通过）",
                    Some("Starting") => "启动或恢复中",
                    Some(_) => "未运行",
                    None => "未读取",
                },
            );
            line(
                ui,
                "PAC 服务",
                match self.status.as_ref().map(|s| s.pac_state.as_str()) {
                    Some("Serving") => "已发布",
                    Some(_) => "未发布",
                    None => "未读取",
                },
            );
            if let Some(winhttp) = self.status.as_ref().and_then(|s| s.winhttp.as_ref()) {
                ui.label(
                    RichText::new(
                        "用途：查看 WinHTTP 客户端（独立于 WinINet 系统代理）的默认会话代理来源；只读，不影响本程序的代理接管。",
                    )
                    .small()
                    .weak(),
                );
                let summary = match winhttp.access_type.as_str() {
                    "NAMED_PROXY" => "显式代理",
                    "NO_PROXY" => "不使用代理",
                    _ => "默认 / 系统代理",
                };
                line(ui, "WinHTTP 会话代理", summary);
                egui::CollapsingHeader::new("WinHTTP 只读诊断").show(ui, |ui| {
                    line(ui, "访问类型", &winhttp.access_type);
                    line(
                        ui,
                        "代理",
                        if winhttp.proxy.is_empty() {
                            "（空）"
                        } else {
                            &winhttp.proxy
                        },
                    );
                    line(
                        ui,
                        "绕过",
                        if winhttp.bypass.is_empty() {
                            "（空）"
                        } else {
                            &winhttp.bypass
                        },
                    );
                    ui.label(
                        RichText::new(
                            "由 WinHTTP 默认会话只读获取；本程序不修改机器级或 WinHTTP 代理。",
                        )
                        .small()
                        .weak(),
                    );
                });
            }
            if ui.button("重新读取状态").clicked() {
                self.send(Command::GetStatus);
            }
            if !self.ui.operation_details.is_empty() {
                egui::CollapsingHeader::new("最近查询的操作证据").show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("operation-details")
                        .max_height(240.0)
                        .show(ui, |ui| ui.monospace(&self.ui.operation_details));
                });
            }
            if let Some(request_id) = self.failed_request.clone()
                && ui.button("查询最近失败请求").clicked()
            {
                self.send(Command::GetOperation { request_id });
            }
        });
        ui.add_space(16.0);
        card(ui, |ui| {
            ui.label(
                RichText::new("当前运行配置 · 路由静态预测")
                    .strong()
                    .size(18.0),
            );
            if let Some(id) = self.committed_id() {
                line(ui, "运行配置", self.ui.workspace.name(id));
            }
            ui.label(
                RichText::new(
                    "用途：核对某个目标会命中当前运行配置的哪条 routing 规则和出站；只做静态预测，不发起连接，也不代替实际访问。",
                )
                .small()
                .weak(),
            );
            let field_width = (ui.available_width() - 164.0).max(140.0);
            egui::Grid::new("route-fields")
                .num_columns(2)
                .min_col_width(148.0)
                .min_row_height(CONTROL_HEIGHT)
                .spacing(egui::vec2(16.0, 10.0))
                .show(ui, |ui| {
                    ui.label("目标域名或 IP");
                    ui.add(singleline(&mut self.route_host, field_width));
                    ui.end_row();
                    ui.label("端口 / 网络");
                    ui.horizontal(|ui| {
                        ui.add(singleline(&mut self.ui.route_port, 100.0));
                        egui::ComboBox::from_id_salt("route-network")
                            .width(100.0)
                            .selected_text(&self.route_network)
                            .show_ui(ui, |ui| {
                                for network in ["tcp", "udp"] {
                                    ui.selectable_value(
                                        &mut self.route_network,
                                        network.into(),
                                        network,
                                    );
                                }
                            });
                    });
                    ui.end_row();
                });
            egui::CollapsingHeader::new("高级匹配条件（可选）").show(ui, |ui| {
                egui::Grid::new("route-advanced-fields")
                    .num_columns(2)
                    .min_col_width(148.0)
                    .min_row_height(CONTROL_HEIGHT)
                    .spacing(egui::vec2(16.0, 10.0))
                    .show(ui, |ui| {
                        ui.label("入站 Tag");
                        ui.add(singleline(&mut self.route_inbound, field_width));
                        ui.end_row();
                        ui.label("已知目标 IP");
                        ui.add(singleline(&mut self.ui.route_ip, field_width));
                        ui.end_row();
                        ui.label("来源 IP / 端口");
                        ui.horizontal(|ui| {
                            ui.add(singleline(&mut self.ui.route_source, field_width - 110.0));
                            ui.add(singleline(&mut self.ui.route_source_port, 100.0));
                        });
                        ui.end_row();
                        ui.label("用户 / 协议");
                        ui.horizontal(|ui| {
                            ui.add(singleline(
                                &mut self.ui.route_user,
                                field_width / 2.0 - 4.0,
                            ));
                            ui.add(singleline(
                                &mut self.ui.route_protocol,
                                field_width / 2.0 - 4.0,
                            ));
                        });
                        ui.end_row();
                        ui.label("属性 key=value");
                        ui.add(singleline(&mut self.ui.route_attrs, field_width));
                        ui.end_row();
                    });
                ui.label(
                    RichText::new(
                        "仅填写已知条件；留空表示该条件未知。DNS 解析、sniffing 和未支持的前序条件都可能使结果不确定。",
                    )
                    .small()
                    .weak(),
                );
            });
            if primary(
                ui,
                "预测命中规则与出站",
                self.ui.connected && self.is_running() && !self.busy,
            ) {
                let port = if self.ui.route_port.trim().is_empty() {
                    Some(None)
                } else {
                    self.ui
                        .route_port
                        .trim()
                        .parse::<u16>()
                        .ok()
                        .filter(|p| *p > 0)
                        .map(Some)
                };
                let ip_valid = self.ui.route_ip.trim().is_empty()
                    || self.ui.route_ip.trim().parse::<std::net::IpAddr>().is_ok();
                if let Some(port) = port.filter(|_| ip_valid && !self.route_host.trim().is_empty())
                {
                    let optional = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
                    let source_port = optional(&self.ui.route_source_port)
                        .map(|v| v.parse::<u16>())
                        .transpose();
                    let attrs = optional(&self.ui.route_attrs)
                        .map(|text| {
                            text.lines()
                                .filter(|line| !line.trim().is_empty())
                                .map(|line| {
                                    line.split_once('=')
                                        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
                                        .ok_or(())
                                })
                                .collect::<std::result::Result<
                                    std::collections::BTreeMap<String, String>,
                                    (),
                                >>()
                        })
                        .transpose();
                    if source_port.is_err() || attrs.is_err() {
                        self.message = "来源端口或属性格式无效；属性请使用每行 key=value。".into();
                    } else {
                        let context = RouteContext {
                            host: self.route_host.trim().into(),
                            port,
                            inbound_tag: optional(&self.route_inbound),
                            network: optional(&self.route_network),
                            target_ip: optional(&self.ui.route_ip).or_else(|| {
                                self.route_host
                                    .trim()
                                    .parse::<std::net::IpAddr>()
                                    .ok()
                                    .map(|ip| ip.to_string())
                            }),
                            source_ip: optional(&self.ui.route_source),
                            source_port: source_port.unwrap_or_default(),
                            user: optional(&self.ui.route_user),
                            protocol: optional(&self.ui.route_protocol),
                            attrs: attrs.unwrap_or_default(),
                        };
                        if let Some(g) = self.status.as_ref().and_then(|s| s.generation.as_ref()) {
                            self.send(Command::TestRunningRoute {
                                generation_id: g.id.clone(),
                                context,
                            });
                        }
                    }
                } else {
                    self.message = "请输入目标、1–65535 的端口和有效的目标 IP（可留空）。".into();
                }
            }
            ui.label(RichText::new("仅使用冻结的当前运行配置。域名解析、sniffing 或未支持的前序条件可能使结果未知。").small().weak());
            if let Some((_, context)) = &self.ui.route_tested {
                line(
                    ui,
                    "本次预测目标",
                    &format!(
                        "{}:{}",
                        context.host,
                        context
                            .port
                            .map(|p| p.to_string())
                            .unwrap_or_else(|| "未指定".into())
                    ),
                );
            }
            if let Ok(result) = serde_json::from_str::<RouteResult>(&self.route_result) {
                ui.separator();
                line(
                    ui,
                    "静态预测",
                    result.expected_outbound.as_deref().unwrap_or("未知"),
                );
                line(
                    ui,
                    "命中规则",
                    &result
                        .rule_index
                        .map(|i| (i + 1).to_string())
                        .unwrap_or_else(|| "无确定命中".into()),
                );
                egui::CollapsingHeader::new("预测依据与限制").show(ui, |ui| {
                    ui.label(result.reason);
                    ui.monospace(&self.route_result);
                });
            }
        });
        ui.add_space(16.0);
        card(ui, |ui| {
            ui.label(RichText::new("出站 TCP 延迟探测").strong().size(18.0));
            ui.label(
                RichText::new(
                    "仅测量到目标主机和端口的 TCP 握手时间，不经过 Xray，也不代表远端服务可用。",
                )
                .small()
                .weak(),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label("主机");
                ui.add(singleline(&mut self.ui.latency_host, 260.0));
                ui.label("端口");
                ui.add(singleline(&mut self.ui.latency_port, 80.0));
                if primary(
                    ui,
                    "开始探测",
                    self.ui.connected
                        && !self.busy
                        && !self.ui.latency_host.trim().is_empty()
                        && !self.ui.latency_port.trim().is_empty(),
                ) {
                    match self.ui.latency_port.trim().parse::<u16>() {
                        Ok(port) if port > 0 => {
                            self.send(Command::TestLatency {
                                host: self.ui.latency_host.trim().into(),
                                port,
                            });
                        }
                        _ => self.message = "端口必须是 1–65535 的数字。".into(),
                    }
                }
            });
            if let Ok(result) = serde_json::from_str::<serde_json::Value>(&self.ui.latency_result) {
                ui.separator();
                line(ui, "目标地址", result["address"].as_str().unwrap_or("未知"));
                line(
                    ui,
                    "TCP 握手",
                    &format!("{} ms", result["ms"].as_u64().unwrap_or_default()),
                );
                egui::CollapsingHeader::new("原始结果").show(ui, |ui| {
                    ui.monospace(&self.ui.latency_result);
                });
            } else if !self.ui.latency_result.is_empty() {
                ui.separator();
                ui.monospace(&self.ui.latency_result);
            }
        });
    }
    pub(in crate::app) fn log_rows(&self) -> Vec<(String, String)> {
        if self.ui.logs_paused {
            return self.ui.frozen_logs.clone();
        }
        let mut rows = vec![];
        if self.ui.log_source != 2 {
            for event in &self.ui.events {
                rows.push((
                    event.level.into(),
                    format!(
                        "[操作 / {}] [{}] {}{}",
                        event.level,
                        event.timestamp,
                        event.message,
                        if event.request.is_empty() {
                            String::new()
                        } else {
                            format!("\nrequest_id: {}", event.request)
                        }
                    ),
                ));
            }
        }
        if self.ui.log_source != 1 {
            for chunk in self.logs.iter().rev() {
                for line in chunk.lines() {
                    let lower = line.to_lowercase();
                    let level = if lower.contains("[error]") || lower.contains("[fatal]") {
                        "错误"
                    } else if lower.contains("[warning]") || lower.contains("[warn]") {
                        "警告"
                    } else if lower.contains("[info]") || lower.contains("[debug]") {
                        "信息"
                    } else {
                        "未标级别"
                    };
                    let formatted = xray_runtime::localize_log_line(line);
                    rows.push((level.into(), format!("[Core] {formatted}")));
                }
            }
        }
        rows
    }

    pub(super) fn format_log_rows(rows: &[(String, String)]) -> String {
        rows.iter()
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Reads persisted `logs/supervisor.log.1` (if present) and `logs/supervisor.log` in chronological order.
    pub(super) fn read_session_logs(&self) -> std::result::Result<Option<String>, String> {
        let logs_dir = self.root.join("logs");
        let mut combined = String::new();
        for name in ["supervisor.log.1", "supervisor.log"] {
            let path = logs_dir.join(name);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let chunk = String::from_utf8_lossy(&bytes);
                    if !combined.is_empty() && !combined.ends_with('\n') && !chunk.is_empty() {
                        combined.push('\n');
                    }
                    for segment in chunk.split_inclusive('\n') {
                        let (line, ending) = match segment.strip_suffix('\n') {
                            Some(stripped) => (stripped, "\n"),
                            None => (segment, ""),
                        };
                        combined.push_str(&xray_runtime::localize_log_line(line));
                        combined.push_str(ending);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("{}: {error}", display_path(&path))),
            }
        }
        if combined.trim().is_empty() {
            Ok(None)
        } else {
            Ok(Some(combined))
        }
    }

    #[cfg(any(windows, test))]
    pub(super) fn write_log_export(
        path: &std::path::Path,
        content: &str,
    ) -> std::result::Result<(), String> {
        std::fs::write(path, content).map_err(|error| error.to_string())
    }

    pub(super) fn copy_session_logs(&mut self, ctx: &egui::Context) {
        match self.read_session_logs() {
            Ok(Some(text)) => {
                ctx.copy_text(text);
                self.message = "已复制持久化会话日志到剪贴板。".into();
            }
            Ok(None) => self.message = "暂无已落盘的会话日志。".into(),
            Err(error) => self.message = format!("读取会话日志失败：{error}"),
        }
    }

    #[cfg(windows)]
    pub(super) fn export_text_with_dialog(
        &mut self,
        default_name: &str,
        content: &str,
        ok_message: &str,
    ) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("日志文件", &["log", "txt"])
            .set_file_name(default_name)
            .save_file()
        {
            match Self::write_log_export(&path, content) {
                Ok(()) => self.message = ok_message.into(),
                Err(error) => self.message = format!("导出日志失败：{error}"),
            }
        }
    }

    #[cfg(windows)]
    pub(super) fn export_session_logs_with_dialog(&mut self) {
        match self.read_session_logs() {
            Ok(Some(text)) => {
                self.export_text_with_dialog(
                    "xray-pilot-supervisor.log",
                    &text,
                    "已导出持久化会话日志。",
                );
            }
            Ok(None) => self.message = "暂无已落盘的会话日志。".into(),
            Err(error) => self.message = format!("读取会话日志失败：{error}"),
        }
    }

    pub(in crate::app) fn logs_page(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "日志",
            "查看 Core 输出与界面操作记录，按消息或请求身份定位问题。",
        );
        let search_width = (ui.available_width() - 298.0).max(120.0);
        ui.horizontal(|ui| {
            ui.label("搜索");
            ui.add(
                singleline(&mut self.ui.log_search, search_width).hint_text("搜索消息或请求 ID"),
            );
            egui::ComboBox::from_id_salt("log-level")
                .width(110.0)
                .selected_text(&self.ui.log_level)
                .show_ui(ui, |ui| {
                    for level in ["全部", "信息", "警告", "错误", "未标级别"] {
                        ui.selectable_value(&mut self.ui.log_level, level.into(), level);
                    }
                });
            ui.add_enabled_ui(!self.ui.logs_paused, |ui| {
                egui::ComboBox::from_id_salt("log-source")
                    .width(130.0)
                    .selected_text(["全部来源", "界面操作", "Core 输出"][self.ui.log_source])
                    .show_ui(ui, |ui| {
                        for (i, label) in ["全部来源", "界面操作", "Core 输出"].iter().enumerate()
                        {
                            ui.selectable_value(&mut self.ui.log_source, i, *label);
                        }
                    });
            });
        });
        let search = self.ui.log_search.to_lowercase();
        let rows: Vec<_> = self
            .log_rows()
            .into_iter()
            .filter(|(level, text)| {
                (self.ui.log_level == "全部" || *level == self.ui.log_level)
                    && text.to_lowercase().contains(&search)
            })
            .collect();
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "{} 条结果{}",
                    rows.len(),
                    if self.ui.logs_paused {
                        " · 显示已暂停"
                    } else {
                        ""
                    }
                ))
                .small()
                .weak(),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                #[cfg(windows)]
                if ui
                    .add_enabled(!rows.is_empty(), egui::Button::new("导出日志…"))
                    .clicked()
                {
                    let content = Self::format_log_rows(&rows);
                    let count = rows.len();
                    self.export_text_with_dialog(
                        "xray-pilot-logs.txt",
                        &content,
                        &format!("已导出 {count} 条日志。"),
                    );
                }
                if ui
                    .add_enabled(!rows.is_empty(), egui::Button::new("复制筛选结果"))
                    .clicked()
                {
                    ui.ctx().copy_text(Self::format_log_rows(&rows));
                    self.message = format!("已复制 {} 条日志到剪贴板。", rows.len());
                }
                if ui.button("读取 Core 日志").clicked() {
                    self.send(Command::GetLogs);
                }
                if ui
                    .button(if self.ui.logs_paused {
                        "继续显示"
                    } else {
                        "暂停显示"
                    })
                    .clicked()
                {
                    if !self.ui.logs_paused {
                        self.ui.frozen_logs = self.log_rows();
                    }
                    self.ui.logs_paused = !self.ui.logs_paused;
                }
            });
        });
        card(ui, |ui| {
            ui.set_min_height(230.0);
            if rows.is_empty() {
                ui.label(RichText::new("暂无匹配的日志。").weak());
            }
            egui::ScrollArea::vertical()
                .max_height(450.0)
                .id_salt("log-content")
                .show(ui, |ui| {
                    for (idx, (level, text)) in rows.iter().enumerate() {
                        let color = if level == "错误" {
                            ui.visuals().error_fg_color
                        } else if level == "警告" {
                            ui.visuals().warn_fg_color
                        } else {
                            ui.visuals().text_color()
                        };
                        ui.push_id(idx, |ui| {
                            ui.horizontal(|ui| {
                                let text_width = (ui.available_width() - 56.0).max(120.0);
                                ui.allocate_ui(egui::vec2(text_width, 0.0), |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(text).monospace().color(color),
                                        )
                                        .wrap(),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::TOP),
                                    |ui| {
                                        if ui
                                            .small_button("复制")
                                            .on_hover_text("复制此条日志")
                                            .clicked()
                                        {
                                            ui.ctx().copy_text(text.clone());
                                            self.message = "已复制该条日志到剪贴板。".into();
                                        }
                                    },
                                );
                            });
                        });
                        ui.separator();
                    }
                });
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("持久化会话日志：").small().weak());
            if ui.button("复制会话日志").clicked() {
                self.copy_session_logs(ui.ctx());
            }
            #[cfg(windows)]
            if ui.button("导出会话日志…").clicked() {
                self.export_session_logs_with_dialog();
            }
        });
        ui.label(RichText::new("Core 输出与操作记录有容量上限，重启后不保留本次界面的操作列表。日志可能包含目标地址或认证信息。").small().weak());
    }
}
