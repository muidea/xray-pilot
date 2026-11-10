use super::*;

impl App {
    pub(in crate::app) fn diagnostics(&mut self, ui: &mut egui::Ui) {
        heading(
            ui,
            "诊断",
            "进程、代理归属与实际网络分别呈现。路由只做静态预测。",
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
            line(ui, "真实网络连通性", "未检测");
            line(ui, "实际出口", "未验证");
            if ui.button("重新读取状态").clicked() {
                self.send(Command::GetStatus);
            }
            if !self.ui.operation_details.is_empty() {
                egui::CollapsingHeader::new("最近查询的操作证据").show(ui, |ui| {
                    ui.monospace(&self.ui.operation_details);
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
                    ui.label("入站 Tag（可选）");
                    ui.add(singleline(&mut self.route_inbound, field_width));
                    ui.end_row();
                    ui.label("已知目标 IP（可选）");
                    ui.add(singleline(&mut self.ui.route_ip, field_width));
                    ui.end_row();
                });
            if primary(
                ui,
                "预测运行版本的规则匹配",
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
                    };
                    if let Some(g) = self.status.as_ref().and_then(|s| s.generation.as_ref()) {
                        self.send(Command::TestRunningRoute {
                            generation_id: g.id.clone(),
                            context,
                        });
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
                line(ui, "实际出口", "未验证");
                egui::CollapsingHeader::new("预测依据与限制").show(ui, |ui| {
                    ui.label(result.reason);
                    ui.monospace(&self.route_result);
                });
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
                        "[操作 / {}] {}{}",
                        event.level,
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
                    rows.push((level.into(), format!("[Core] {line}")));
                }
            }
        }
        rows
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
                if ui.button("复制筛选结果").clicked() {
                    ui.ctx().copy_text(
                        rows.iter()
                            .map(|(_, text)| text.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    );
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
                .max_height(470.0)
                .id_salt("log-content")
                .show(ui, |ui| {
                    for (level, text) in rows {
                        let color = if level == "错误" {
                            ui.visuals().error_fg_color
                        } else if level == "警告" {
                            ui.visuals().warn_fg_color
                        } else {
                            ui.visuals().text_color()
                        };
                        ui.label(RichText::new(text).monospace().color(color));
                        ui.separator();
                    }
                });
        });
        ui.label(RichText::new("Core 输出与操作记录有容量上限，重启后不保留本次界面的操作列表。日志可能包含目标地址或认证信息。").small().weak());
    }
}
