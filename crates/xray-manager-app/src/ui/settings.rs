use super::*;

impl App {
    pub(in crate::app) fn settings_page(&mut self, ui: &mut egui::Ui) {
        heading(ui, "设置", "管理依赖、启动方式与显示偏好。");
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (SettingsTab::Core, "Core 与依赖"),
                (SettingsTab::Startup, "启动行为"),
                (SettingsTab::Appearance, "外观"),
                (SettingsTab::Storage, "数据与存储"),
            ] {
                ui.selectable_value(&mut self.ui.settings_tab, tab, label);
            }
        });
        ui.add_space(12.0);
        let before = self.settings_context();
        card(ui, |ui| {
            match self.ui.settings_tab {
                SettingsTab::Core => self.core_settings(ui),
                SettingsTab::Startup => {
                    ui.label(RichText::new("启动行为").strong().size(18.0));
                    ui.add_enabled_ui(self.can_save_settings(), |ui| {
                        ui.checkbox(
                            &mut self.settings.start_on_logon,
                            "登录 Windows 后自启（隐藏到系统托盘）",
                        );
                        ui.label(RichText::new("登录自启不等同于启动 Core。").small().weak());
                        ui.add_space(10.0);
                        ui.checkbox(
                            &mut self.settings.restore_last_state,
                            "启动时恢复上次 Core 运行状态",
                        );
                        ui.label(
                            RichText::new(
                                "上次退出前 Core 正在运行，则组件启动后自动恢复；手动停止的状态不会恢复。包含动态外部依赖时会跳过自动连接；未分类字段会尝试启动和恢复，失败时由 Core 启动结果兜底。",
                            )
                            .small()
                            .weak(),
                        );
                        ui.add_space(6.0);
                        ui.checkbox(&mut self.settings.connect_on_start, "后台启动后总是自动连接");
                        ui.label(
                            RichText::new(
                                "即使上次为停止状态，也尝试启动上次已成功应用的版本；恢复异常时先处理安全恢复。",
                            )
                            .small()
                            .weak(),
                        );
                    });
                }
                SettingsTab::Appearance => {
                    ui.label(RichText::new("显示风格").strong().size(18.0));
                    ui.label("立即生效，并记住当前用户的选择。跟随系统会响应系统主题变化。");
                    ui.horizontal_wrapped(|ui| {
                        for (appearance, label) in [
                            (Appearance::Light, "浅色"),
                            (Appearance::Dark, "深色"),
                            (Appearance::System, "跟随系统"),
                        ] {
                            if ui
                                .add_sized(
                                    [130.0, 65.0],
                                    egui::Button::new(label)
                                        .selected(self.appearance == appearance),
                                )
                                .clicked()
                            {
                                self.set_appearance(appearance, ui.ctx());
                            }
                        }
                    });
                    ui.label(
                        RichText::new("状态同时使用文字表达，所有页面、对话框使用一致的显示风格。")
                            .small()
                            .weak(),
                    );
                }
                SettingsTab::Storage => {
                    ui.label(RichText::new("数据与存储").strong().size(18.0));
                    ui.label("Windows 默认使用程序安装目录下的 data；显式 --data-dir 参数优先。");
                    ui.label("当前数据目录");
                    ui.monospace(display_path(&self.root));
                    if ui.button("复制目录路径").clicked() {
                        ui.ctx().copy_text(display_path(&self.root));
                    }
                    ui.label(
                        RichText::new(
                            "安装目录需可写。更换数据目录请先退出程序，再通过启动参数指定。",
                        )
                        .weak(),
                    );
                    ui.separator();
                    ui.label("草稿与配置显示名称保存在本数据目录中，草稿不改变已应用的原生配置。草稿可能包含节点认证信息，请与正式配置一起妥善保管。");
                    ui.separator();
                    ui.label("会话日志");
                    let log_path = self.root.join("logs/supervisor.log");
                    ui.monospace(display_path(&log_path));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("复制日志路径").clicked() {
                            ui.ctx().copy_text(display_path(&log_path));
                            self.message = "已复制会话日志路径到剪贴板。".into();
                        }
                        if ui.button("复制日志内容").clicked() {
                            self.copy_session_logs(ui.ctx());
                        }
                        #[cfg(windows)]
                        if ui.button("导出会话日志…").clicked() {
                            self.export_session_logs_with_dialog();
                        }
                    });
                    ui.label(
                        RichText::new(
                            "日志带 UTC 时间戳与操作/代次上下文，凭据字段自动脱敏；超过 4 MiB 轮转一个备份，备份保留 7 天。",
                        )
                        .small()
                        .weak(),
                    );
                }
            }
            if matches!(
                self.ui.settings_tab,
                SettingsTab::Core | SettingsTab::Startup
            ) {
                ui.separator();
                if primary(ui, "保存设置", self.can_save_settings()) {
                    self.settings.core_path = self.core_path.clone().into();
                    self.send(Command::SaveSettings {
                        settings: self.settings.clone(),
                    });
                }
                if self.status.as_ref().is_some_and(|s| s.recovery_required)
                    && self.can_save_settings()
                {
                    ui.label(
                        RichText::new("当前处于安全恢复状态，仍可修改并保存启动或路径设置（如取消开机自启）；运行变更请先处理恢复。")
                            .small()
                            .weak(),
                    );
                }
                ui.label(
                    RichText::new("保存设置不会替换运行中的 Core；请重新检查配置并确认应用。")
                        .small()
                        .weak(),
                );
            }
        });
        if before != self.settings_context() {
            self.settings_edited();
        }
    }
    pub(in crate::app) fn core_settings(&mut self, ui: &mut egui::Ui) {
        let enabled = self.can_save_settings();
        ui.label(RichText::new("Core 与依赖").strong().size(18.0));
        ui.label("从官方来源准备 Xray Core、geoip.dat 与 geosite.dat，校验后按版本管理。");
        ui.hyperlink_to(
            "打开 XTLS / Xray-core 官方发布页",
            "https://github.com/XTLS/Xray-core/releases",
        );
        if !cfg!(windows) {
            ui.label(
                RichText::new(
                    "自动安装当前支持 Windows x64；本平台请手动指定官方 Core 与资源目录。",
                )
                .weak(),
            );
        }
        ui.horizontal(|ui| {
            ui.label("官方版本");
            ui.add(singleline(&mut self.dependency_version, 160.0));
        });
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    enabled && cfg!(windows) && self.dependency_job.is_none(),
                    egui::Button::new("下载 / 修复官方依赖"),
                )
                .clicked()
            {
                self.start_dependencies("Install", "", Some(ui.ctx().clone()));
            }
            if ui
                .add_enabled(
                    cfg!(windows) && self.dependency_job.is_none(),
                    egui::Button::new("检查已安装依赖"),
                )
                .clicked()
            {
                self.start_dependencies("Status", "", Some(ui.ctx().clone()));
            }
        });
        if self.dependency_job.is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.spinner();
                ui.label("正在下载或校验依赖，可能需要数分钟；完成后不会覆盖期间的新设置。");
            });
        }
        if !self.dependency_message.is_empty() {
            ui.label(&self.dependency_message);
        }
        for installation in self.dependency_report.installations.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{} · {}",
                    installation.version,
                    if installation.valid {
                        "已验证"
                    } else {
                        "需要修复"
                    }
                ));
                if ui
                    .add_enabled(
                        enabled
                            && cfg!(windows)
                            && installation.valid
                            && self.dependency_job.is_none(),
                        egui::Button::new("选用此版本"),
                    )
                    .clicked()
                {
                    self.dependency_version = installation.version;
                    self.start_dependencies(
                        "Select",
                        &installation.install_id,
                        Some(ui.ctx().clone()),
                    );
                }
            });
            if let Some(error) = installation.error {
                egui::CollapsingHeader::new("依赖检查详情")
                    .id_salt(installation.install_id)
                    .show(ui, |ui| {
                        ui.monospace(error);
                    });
            }
        }
        ui.separator();
        ui.add_enabled_ui(enabled, |ui| {
            ui.label("下次检查 / 应用使用的官方 Core（绝对路径）");
            path_input(ui, &mut self.core_path);
            #[cfg(windows)]
            if ui.button("选择已有 Core…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Xray Core", &["exe"])
                    .pick_file()
            {
                self.core_path = path.display().to_string();
                if let Some(parent) = path.parent() {
                    self.asset_root = parent.display().to_string();
                }
            }
            ui.label("资源目录（geo 文件及相对证书路径的根目录）");
            path_input(ui, &mut self.asset_root);
        });
        let running = self
            .status
            .as_ref()
            .filter(|_| self.is_running())
            .and_then(|s| s.generation.as_ref());
        line(
            ui,
            "运行中的 Core",
            running
                .map(|g| g.candidate.core.version.as_str())
                .unwrap_or("未运行"),
        );
        if let Some(settings) = &self.ui.saved_settings {
            line(
                ui,
                "已保存的下次使用路径",
                &display_path(&settings.core_path),
            );
        }
        ui.label(
            RichText::new(
                "安装、选用和保存依赖都不会立即更换运行版本；正式应用时使用冻结的 Core 与资源。",
            )
            .small()
            .weak(),
        );
    }
    pub(in crate::app) fn setup(&mut self, ui: &mut egui::Ui) {
        if !self.ui.settings_loaded || !self.ui.profiles_loaded {
            heading(ui, "欢迎使用 Xray Pilot", "正在读取已有配置与设置…");
            ui.spinner();
            return;
        }
        heading(ui, "首次设置", "准备依赖、添加配置，再检查并明确确认启动。");
        ui.horizontal_wrapped(|ui| {
            for (i, label) in ["1  准备依赖", "2  添加配置", "3  检查并启动"]
                .iter()
                .enumerate()
            {
                ui.label(RichText::new(*label).color(if i == self.ui.setup_step {
                    accent(ui)
                } else {
                    ui.visuals().weak_text_color()
                }));
            }
        });
        ui.add_space(15.0);
        card(ui, |ui| {
            match self.ui.setup_step {
                0 => {
                    let before = self.settings_context();
                    self.core_settings(ui);
                    if before != self.settings_context() {
                        self.settings_edited();
                    }
                }
                1 => {
                    ui.label(RichText::new("添加第一份配置").strong().size(21.0));
                    ui.label("导入原生 JSON，或创建只监听回环地址的本地直连模板。不包含远端节点，不自动接管系统代理。");
                    ui.horizontal_wrapped(|ui| {
                        if ui
                            .add_enabled(
                                self.write_blocker().is_none(),
                                egui::Button::new("使用本地直连模板"),
                            )
                            .clicked()
                        {
                            self.begin_create(false, false);
                            self.ui.new_name = "本地直连".into();
                        }
                        if ui
                            .add_enabled(
                                self.write_blocker().is_none(),
                                egui::Button::new("导入原生 JSON"),
                            )
                            .clicked()
                        {
                            self.begin_create(true, false);
                        }
                    });
                    if !self.profile_id.is_empty() {
                        line(ui, "已准备配置", self.ui.workspace.name(&self.profile_id));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("上一步").clicked() {
                            self.ui.setup_step = 0;
                        }
                        if primary(
                            ui,
                            "下一步：检查配置",
                            !self.profile_id.is_empty() && !self.busy,
                        ) {
                            self.ui.setup_step = 2;
                            self.mode = ProxyMode::Direct;
                            self.invalidate_candidate();
                        }
                    });
                }
                _ => {
                    ui.label(RichText::new("检查后再启动").strong().size(21.0));
                    line(ui, "配置", self.ui.workspace.name(&self.profile_id));
                    line(ui, "Core", &display_path_text(&self.core_path));
                    line(ui, "系统代理", mode_name(self.mode));
                    ui.label(RichText::new("启动 Core 不代表其他程序已使用代理，也不代表网络连通性或实际出口已验证。").weak());
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("上一步").clicked() {
                            self.ui.setup_step = 1;
                        }
                        if ui.button("先编辑配置").clicked() {
                            self.ui.page = Page::Profiles;
                        }
                        if primary(
                            ui,
                            "检查并预览",
                            self.write_blocker().is_none()
                                && !self.profile_id.is_empty()
                                && !self.core_path.is_empty()
                                && !self.ui.draft_conflict,
                        ) {
                            self.validate_draft();
                        }
                    });
                }
            }
            ui.add_space(15.0);
            ui.horizontal_wrapped(|ui| {
                if self.ui.setup_step == 0
                    && primary(
                        ui,
                        "保存并继续",
                        self.write_blocker().is_none() && !self.core_path.is_empty(),
                    )
                {
                    self.settings.core_path = self.core_path.clone().into();
                    self.ui.advance_setup_after_save = true;
                    self.send(Command::SaveSettings {
                        settings: self.settings.clone(),
                    });
                }
                if ui.button("稍后设置，进入总览").clicked() {
                    self.ui.onboarding = false;
                }
            });
        });
    }
}
