use super::*;

fn app() -> App {
    let (tx, _requests) = mpsc::sync_channel(MAX_PENDING);
    let (_responses, rx) = mpsc::channel();
    let mut app = App::with_channels("unused-ui-test".into(), tx, rx);
    app.ui.connected = true;
    app.ui.setup_initialized = true;
    app.ui.onboarding = false;
    app.ui.settings_loaded = true;
    app.ui.profiles_loaded = true;
    let json = default_config();
    let revision = hash(json.as_bytes());
    app.load(Profile {
        id: "work".into(),
        json: json.clone(),
        revision: revision.clone(),
    });
    app.profiles.push(ProfileSummary {
        id: "work".into(),
        revision: revision.clone(),
    });
    app.ui
        .workspace
        .names
        .insert("work".into(), "工作网络".into());
    app.core_path = "C:\\Apps\\Xray\\xray.exe".into();
    app.status = Some(Status {
        generation: Some(Generation {
            id: "generation-1".into(),
            transaction_id: "transaction".into(),
            request_id: "request".into(),
            candidate: Candidate {
                id: "candidate".into(),
                digest: "digest".into(),
                profile_id: "work".into(),
                base_revision: revision.clone(),
                revision,
                config_path: "config.json".into(),
                core: CoreIdentity {
                    path: "frozen/core".into(),
                    sha256: "sha".into(),
                    version: "Xray test".into(),
                },
                resources: Resources {
                    id: "resources".into(),
                    root: "frozen/resources".into(),
                    files: vec![],
                    complete: true,
                    notes: vec![],

                    dynamic: vec![],
                },
            },
            proxy_mode: ProxyMode::Direct,
            http_inbound_tag: "http-in".into(),
            running: true,

            pac: None,
        }),
        core_state: "Running".into(),
        proxy_state: "Unmanaged".into(),
        runtime_instance_id: Some("instance".into()),
        supervisor_epoch: "epoch".into(),
        last_error: None,
        recovery_required: false,
        winhttp: None,
        listener_state: "Ready".into(),
        pac_state: "Stopped".into(),
        auto_connect_notice: None,
    });
    app.ui.runtime = Some(RuntimeConfig {
        generation_id: "generation-1".into(),
        json,
    });
    app
}
fn render(app: &mut App, ctx: &egui::Context, width: f32, height: f32) {
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            ..Default::default()
        },
        |ctx| app.render(ctx),
    );
    assert!(!output.shapes.is_empty());
    let primitives = ctx.tessellate(output.shapes, output.pixels_per_point);
    assert!(!primitives.is_empty());
}

#[test]
fn native_pages_and_dialogs_render_in_both_themes_at_supported_sizes() {
    let mut app = app();
    let ctx = egui::Context::default();
    configure(&ctx);
    for appearance in [Appearance::Light, Appearance::Dark] {
        appearance.apply(&ctx);
        for (width, height) in [(1100.0, 760.0), (760.0, 500.0)] {
            for page in [
                Page::Overview,
                Page::Profiles,
                Page::Pac,
                Page::Diagnostics,
                Page::Logs,
                Page::Settings,
            ] {
                app.ui.page = page;
                render(&mut app, &ctx, width, height);
            }
            for tab in [
                ProfileTab::Summary,
                ProfileTab::Inbounds,
                ProfileTab::Outbounds,
                ProfileTab::Routing,
                ProfileTab::Json,
                ProfileTab::History,
            ] {
                app.ui.page = Page::Profiles;
                app.ui.profile_tab = tab;
                render(&mut app, &ctx, width, height);
            }
            for tab in [
                SettingsTab::Core,
                SettingsTab::Startup,
                SettingsTab::Appearance,
                SettingsTab::Storage,
            ] {
                app.ui.page = Page::Settings;
                app.ui.settings_tab = tab;
                render(&mut app, &ctx, width, height);
            }
            for dialog in [
                Dialog::Confirm(Action::Stop),
                Dialog::Confirm(Action::Restart),
                Dialog::UnsavedExit(ExitKind::Shutdown),
                Dialog::Rename("work".into()),
                Dialog::Create,
            ] {
                app.ui.dialog = Some(dialog);
                render(&mut app, &ctx, width, height);
                app.ui.dialog = None;
            }
        }
    }
}

#[test]
fn onboarding_and_native_candidate_preview_render_without_mock_results() {
    let mut app = app();
    let ctx = egui::Context::default();
    configure(&ctx);
    app.ui.onboarding = true;
    app.ui.page = Page::Overview;
    for step in 0..3 {
        app.ui.setup_step = step;
        render(&mut app, &ctx, 760.0, 500.0);
    }
    let candidate = app
        .status
        .as_ref()
        .unwrap()
        .generation
        .as_ref()
        .unwrap()
        .candidate
        .clone();
    app.accept_candidate(candidate);
    render(&mut app, &ctx, 760.0, 500.0);
}

#[test]
fn runtime_proxy_actions_use_runtime_tags_instead_of_an_unapplied_draft() {
    let mut app = app();
    app.http_tag = "draft-only-tag".into();
    assert_eq!(app.runtime_http_tag(), "http-in");
    app.status.as_mut().unwrap().proxy_state = "Released".into();
    assert_eq!(app.proxy_label(), "未由本程序接管");
    app.ui.connected = false;
    assert_eq!(app.proxy_label(), "状态待确认");
}

#[test]
fn log_pause_retains_a_snapshot_and_level_detection_does_not_invent_core_levels() {
    let mut app = app();
    app.logs = vec!["unmarked output\n[Warning] retry".into()];
    let rows = app.log_rows();
    assert!(rows.iter().any(|(level, _)| level == "未标级别"));
    app.ui.frozen_logs = rows.clone();
    app.ui.logs_paused = true;
    app.logs.clear();
    app.add_event("信息", "later", String::new());
    assert_eq!(app.log_rows(), rows);
    app.ui.logs_paused = false;
    assert_ne!(app.log_rows(), rows);
}

#[test]
fn pointer_navigation_and_preview_only_apply_after_confirmation() {
    let mut app = app();
    let (tx, requests) = mpsc::sync_channel(MAX_PENDING);
    app.tx = tx;
    let ctx = egui::Context::default();
    configure(&ctx);
    let frame = |app: &mut App, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 760.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.render(ctx),
        )
    };
    fn text_position(shape: &egui::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|s| text_position(s, label)),
            _ => None,
        }
    }
    let click = |app: &mut App, label: &str| {
        // Settle layout before targeting the rendered label, then send real egui input.
        frame(app, vec![]);
        let output = frame(app, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|s| text_position(&s.shape, label))
            .unwrap_or_else(|| panic!("Missing button: {label}"));
        frame(
            app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        frame(
            app,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    };
    click(&mut app, "配置");
    assert!(app.ui.page == Page::Profiles);
    assert!(requests.try_iter().all(|r| !r.command.is_write()));
    let candidate = app
        .status
        .as_ref()
        .unwrap()
        .generation
        .as_ref()
        .unwrap()
        .candidate
        .clone();
    app.accept_candidate(candidate.clone());
    click(&mut app, "返回编辑");
    assert!(!app.ui.preview_open);
    assert!(requests.try_iter().all(|r| !r.command.is_write()));
    app.accept_candidate(candidate.clone());
    click(&mut app, "确认应用");
    assert!(requests.try_iter().any(|r| matches!(
        r.command,
        Command::ApplyProfile { candidate_id, .. } if candidate_id == candidate.id
    )));
    assert!(app.busy);
}

#[cfg(windows)]
#[test]
fn windows_path_fields_and_clipboard_hide_prefixes_without_rewriting_paths() {
    let mut app = app();
    app.root = PathBuf::from(r"\\?\D:\Xray Pilot 数据\data");
    app.core_path = r"\\?\C:\Xray\folder.\xray.exe".into();
    app.asset_root = r"\\?\UNC\server\share\资源".into();
    let core = app.core_path.clone();
    let assets = app.asset_root.clone();
    app.ui.saved_settings = Some(Settings {
        core_path: core.clone().into(),
        ..Default::default()
    });
    app.ui.page = Page::Settings;
    let ctx = egui::Context::default();
    configure(&ctx);
    let frame = |app: &mut App, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 760.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.render(ctx),
        )
    };
    fn label_position(output: &egui::FullOutput, label: &str) -> Option<egui::Pos2> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            _ => None,
        })
    }
    frame(&mut app, vec![]);
    let output = frame(&mut app, vec![]);
    assert!(label_position(&output, r"C:\Xray\folder.\xray.exe").is_some());
    assert!(label_position(&output, r"\\server\share\资源").is_some());
    assert!(label_position(&output, &core).is_none());
    assert_eq!(app.core_path, core);
    assert_eq!(app.asset_root, assets);
    assert_eq!(
        app.ui.saved_settings.as_ref().unwrap().core_path,
        PathBuf::from(&core)
    );
    app.ui.settings_tab = SettingsTab::Storage;
    frame(&mut app, vec![]);
    let output = frame(&mut app, vec![]);
    assert!(label_position(&output, r"D:\Xray Pilot 数据\data").is_some());
    let pos = label_position(&output, "复制目录路径").unwrap();
    frame(
        &mut app,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let output = frame(
        &mut app,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(
                command, egui::OutputCommand::CopyText(text) if text == r"D:\Xray Pilot 数据\data"
            ))
    );
    assert_eq!(app.root, PathBuf::from(r"\\?\D:\Xray Pilot 数据\data"));
}

fn text_bounds(output: &egui::FullOutput, label: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing rendered label: {label}"))
}

fn enclosing_rect(output: &egui::FullOutput, label: &str, min_height: f32) -> egui::Rect {
    let text = text_bounds(output, label);
    let mut rects = vec![];
    for shape in &output.shapes {
        if matches!(&shape.shape, egui::Shape::Text(t) if t.galley.text() == label) {
            break;
        }
        painted_rects(&shape.shape, &mut rects);
    }
    rects
        .into_iter()
        .rev()
        .find(|rect| rect.contains(text.center()) && rect.height() >= min_height - 0.1)
        .unwrap()
}

fn painted_rects(shape: &egui::Shape, rects: &mut Vec<egui::Rect>) {
    match shape {
        egui::Shape::Rect(rect) => rects.push(rect.rect),
        egui::Shape::Vec(shapes) => {
            for shape in shapes {
                painted_rects(shape, rects);
            }
        }
        _ => {}
    }
}

fn capture_layout(
    ctx: &egui::Context,
    output: egui::FullOutput,
    name: &str,
    width: f32,
    height: f32,
) {
    let Some(root) = std::env::var_os("XRAY_UI_CAPTURE_DIR") else {
        return;
    };
    let root = PathBuf::from(root);
    std::fs::create_dir_all(&root).unwrap();
    let atlas = ctx.fonts(|fonts| fonts.image());
    std::fs::write(
        root.join(format!("{name}.rgba")),
        atlas
            .pixels
            .iter()
            .flat_map(|p| p.to_array())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let meshes: Vec<_> = ctx.tessellate(output.shapes, output.pixels_per_point).into_iter()
        .map(|primitive| {
            let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else { panic!("Unexpected callback"); };
            assert_eq!(mesh.texture_id, egui::TextureId::Managed(0));
            serde_json::json!({
                "clip": [primitive.clip_rect.min.x, primitive.clip_rect.min.y, primitive.clip_rect.max.x, primitive.clip_rect.max.y],
                "indices": mesh.indices,
                "vertices": mesh.vertices.iter().map(|v| [v.pos.x,v.pos.y,v.uv.x,v.uv.y,
                    v.color.r() as f32/255.0,v.color.g() as f32/255.0,v.color.b() as f32/255.0,v.color.a() as f32/255.0]).collect::<Vec<_>>()
            })
        }).collect();
    std::fs::write(
        root.join(format!("{name}.json")),
        serde_json::to_vec(&serde_json::json!({
            "width":width, "height":height, "atlas_size":atlas.size, "meshes":meshes
        }))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn toolbars_and_profile_cards_align_at_all_supported_widths() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        for width in [760.0, 980.0, 1100.0] {
            let mut app = app();
            let ctx = egui::Context::default();
            configure(&ctx);
            appearance.apply(&ctx);
            let frame = |app: &mut App| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 760.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.render(ctx),
                )
            };
            let settle = |app: &mut App| {
                frame(app);
                frame(app);
                frame(app)
            };
            app.ui.page = Page::Logs;
            let output = settle(&mut app);
            let filter = ["搜索消息或请求 ID", "全部", "全部来源"]
                .map(|s| enclosing_rect(&output, s, CONTROL_HEIGHT));
            for rect in filter {
                assert!(
                    (rect.height() - CONTROL_HEIGHT).abs() < 1.0,
                    "filter height: {rect:?}"
                );
                assert!(
                    (rect.center().y - filter[0].center().y).abs() < 1.0,
                    "filter alignment: {filter:?}"
                );
                assert!(rect.right() <= width - 20.0, "filter overflow: {rect:?}");
            }
            for pair in filter.windows(2) {
                assert!(pair[0].right() < pair[1].left());
            }
            let actions = ["暂停显示", "读取 Core 日志", "复制筛选结果"]
                .map(|s| enclosing_rect(&output, s, CONTROL_HEIGHT));
            for rect in actions {
                assert!((rect.height() - CONTROL_HEIGHT).abs() < 1.0);
                assert!((rect.center().y - actions[0].center().y).abs() < 1.0);
            }
            assert!(
                (text_bounds(&output, "0 条结果").center().y - actions[0].center().y).abs() < 1.0
            );
            for pair in actions.windows(2) {
                assert!(pair[0].right() < pair[1].left());
            }
            capture_layout(
                &ctx,
                output,
                &format!("{appearance:?}-{width}-logs"),
                width,
                760.0,
            );

            app.ui.page = Page::Profiles;
            app.profile_id.clear();
            app.selected_profile.clear();
            app.profiles.clear();
            let output = settle(&mut app);
            let actions =
                ["新建", "导入", "刷新"].map(|s| enclosing_rect(&output, s, CONTROL_HEIGHT));
            if width == 1100.0 {
                for rect in actions {
                    assert!((rect.left() - actions[0].left()).abs() < 1.0);
                    assert!((rect.width() - actions[0].width()).abs() < 1.0);
                }
                for pair in actions.windows(2) {
                    assert!(pair[0].bottom() < pair[1].top());
                }
                let picker = enclosing_rect(&output, "我的配置 · 0", CONTROL_HEIGHT);
                let editor = enclosing_rect(&output, "添加第一份配置", CONTROL_HEIGHT);
                assert!((picker.top() - editor.top()).abs() < 1.0);
                assert!(
                    (picker.bottom() - editor.bottom()).abs() < 1.0,
                    "empty cards: {picker:?}, {editor:?}"
                );
                assert!(picker.right() < editor.left());
            } else {
                for rect in actions {
                    assert!((rect.top() - actions[0].top()).abs() < 1.0);
                }
                for pair in actions.windows(2) {
                    assert!(pair[0].right() < pair[1].left());
                }
            }
            capture_layout(
                &ctx,
                output,
                &format!("{appearance:?}-{width}-empty-profiles"),
                width,
                760.0,
            );
            // Also capture the remaining pages using the same real egui fonts and meshes.
            for (page, name) in [
                (Page::Overview, "overview"),
                (Page::Diagnostics, "diagnostics"),
                (Page::Settings, "settings"),
            ] {
                app.ui.page = page;
                let output = settle(&mut app);
                capture_layout(
                    &ctx,
                    output,
                    &format!("{appearance:?}-{width}-{name}"),
                    width,
                    760.0,
                );
            }
        }
    }
}

#[test]
fn dialogs_stay_inside_the_smallest_window_in_both_themes() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        for (dialog, title, action, name) in [
            (Dialog::Create, "添加 / 导入配置", "添加配置", "import"),
            (
                Dialog::Rename("work".into()),
                "修改配置显示名称",
                "保存名称",
                "rename",
            ),
            (
                Dialog::Confirm(Action::Restart),
                "启动 / 重启上次应用版本？",
                "启动 / 重启",
                "restart",
            ),
        ] {
            let mut app = app();
            app.ui.import_json = true;
            app.ui.new_name = "中文配置".into();
            app.ui.rename = "工作网络".into();
            app.ui.dialog = Some(dialog);
            let ctx = egui::Context::default();
            configure(&ctx);
            appearance.apply(&ctx);
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(760.0, 500.0));
            let mut output = None;
            for frame in 0..12 {
                output = Some(ctx.run(
                    egui::RawInput {
                        screen_rect: Some(viewport),
                        time: Some(frame as f64 * 0.05),
                        ..Default::default()
                    },
                    |ctx| app.render(ctx),
                ));
            }
            let output = output.unwrap();
            // Modal shapes follow the underlying page; the page itself may also
            // contain the title's coordinates, so inspect paint order here.
            let title_bounds = text_bounds(&output, title);
            let mut rects = vec![];
            for shape in &output.shapes {
                painted_rects(&shape.shape, &mut rects);
            }
            let modal = rects
                .into_iter()
                .rev()
                .find(|rect| {
                    rect.contains(title_bounds.center()) && rect.height() >= CONTROL_HEIGHT * 2.0
                })
                .unwrap();
            assert!(modal.width() < viewport.width() - 20.0);
            assert!(viewport.contains_rect(modal), "dialog overflow: {modal:?}");
            // The footer must be visible and use the same height as other controls.
            let cancel = enclosing_rect(&output, "取消", CONTROL_HEIGHT);
            let confirm = enclosing_rect(&output, action, CONTROL_HEIGHT);
            assert!(
                modal.contains_rect(cancel),
                "{name}: modal {modal:?}, cancel {cancel:?}"
            );
            assert!(
                modal.contains_rect(confirm),
                "{name}: modal {modal:?}, confirm {confirm:?}"
            );
            assert!((confirm.height() - CONTROL_HEIGHT).abs() < 1.0);
            assert!(
                (cancel.center().y - confirm.center().y).abs() < 1.0,
                "{name}: cancel {cancel:?}, confirm {confirm:?}"
            );
            assert!(cancel.right() < confirm.left());
            capture_layout(
                &ctx,
                output,
                &format!("{appearance:?}-760-{name}"),
                760.0,
                500.0,
            );
        }
    }
}

#[test]
fn long_incomplete_import_keeps_title_and_footer_visible_and_saves_original_only() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        for (width, height) in [(760.0, 500.0), (1100.0, 760.0)] {
            let mut app = app();
            let (tx, requests) = mpsc::sync_channel(MAX_PENDING);
            app.tx = tx;
            app.pending.clear();
            app.begin_create(true, false);
            app.ui.new_name = "待补充配置".into();
            app.ui.import_source = Some(std::env::temp_dir().join("config.json"));
            let assets = app.asset_root.clone();
            app.ui.new_json = serde_json::to_string_pretty(&serde_json::json!({
                "metadata":{"long_line":"keep-native-value-".repeat(100)},
                "outbounds":(0..50).map(|i| serde_json::json!({"tag":format!("node-{i}")})).collect::<Vec<_>>(),
                "routing":{"rules":[{"outboundTag":"api"}]}
            })).unwrap();
            let original = app.ui.new_json.clone();
            app.ui.failure = Some(Failure {
                error: Error::new("STATIC_VALIDATION_FAILED", "previous import failed"),
                request: String::new(),
            });
            let ctx = egui::Context::default();
            configure(&ctx);
            appearance.apply(&ctx);
            let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
            let frame = |app: &mut App, events, time| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(viewport),
                        events,
                        time: Some(time),
                        ..Default::default()
                    },
                    |ctx| app.render(ctx),
                )
            };
            let mut output = frame(&mut app, vec![], 0.0);
            for n in 1..12 {
                output = frame(&mut app, vec![], n as f64 * 0.05);
            }
            for label in ["添加 / 导入配置", "取消", "保存待补充配置"] {
                let bounds = text_bounds(&output, label);
                let clip = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(t) if t.galley.text() == label => Some(shape.clip_rect),
                        _ => None,
                    })
                    .unwrap();
                assert!(viewport.contains_rect(bounds), "viewport clips {label}");
                assert!(clip.contains_rect(bounds), "scroll area clips {label}");
                if label != "添加 / 导入配置" {
                    assert!(clip.contains_rect(enclosing_rect(&output, label, CONTROL_HEIGHT)));
                }
            }
            let button = text_bounds(&output, "保存待补充配置").center();
            capture_layout(
                &ctx,
                output,
                &format!("{appearance:?}-{width}-incomplete-import"),
                width,
                height,
            );
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(button),
                    egui::Event::PointerButton {
                        pos: button,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                0.7,
            );
            frame(
                &mut app,
                vec![egui::Event::PointerButton {
                    pos: button,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                0.8,
            );
            let sent: Vec<_> = requests.try_iter().collect();
            assert_eq!(app.asset_root, assets);
            assert!(sent.iter().any(
                |r| matches!(&r.command, Command::CreateProfile { json, .. } if json == &original)
            ));
            assert!(!sent.iter().any(|r| matches!(
                r.command,
                Command::ApplyProfile { .. } | Command::RestartCore
            )));
        }
    }
}

fn json_viewport_rects(output: &egui::FullOutput) -> Vec<egui::Rect> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect)
                if [
                    Color32::from_rgb(13, 18, 25),
                    Color32::from_rgb(245, 247, 250),
                ]
                .contains(&rect.fill) =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .collect()
}

#[test]
fn profile_editors_follow_parent_size_for_short_and_long_documents_and_resizing() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        for long in [false, true] {
            let mut app = app();
            app.ui.page = Page::Profiles;
            app.ui.profile_tab = ProfileTab::Json;
            app.editor = if long {
                serde_json::to_string_pretty(&serde_json::json!({"metadata": {
                    "long_line": "native-value-".repeat(200),
                    "rows": (0..200).collect::<Vec<_>>()
                }}))
                .unwrap()
            } else {
                "{}".into()
            };
            let original = app.editor.clone();
            let ctx = egui::Context::default();
            configure(&ctx);
            appearance.apply(&ctx);
            let mut large = None;
            for (width, height) in [
                (1100.0, 760.0),
                (1920.0, 1040.0),
                (1440.0, 900.0),
                (760.0, 500.0),
                (1920.0, 1040.0),
            ] {
                let mut output = None;
                for _ in 0..12 {
                    output = Some(ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            events: if height == 500.0 {
                                vec![
                                    egui::Event::PointerMoved(egui::pos2(
                                        width - 30.0,
                                        height - 100.0,
                                    )),
                                    egui::Event::MouseWheel {
                                        unit: egui::MouseWheelUnit::Point,
                                        delta: egui::vec2(0.0, -100.0),
                                        modifiers: egui::Modifiers::NONE,
                                    },
                                ]
                            } else {
                                vec![]
                            },
                            ..Default::default()
                        },
                        |ctx| app.render(ctx),
                    ));
                }
                let output = output.unwrap();
                let editors = json_viewport_rects(&output);
                assert_eq!(editors.len(), 1);
                let editor = editors[0];
                let footer = enclosing_rect(&output, "检查并预览", CONTROL_HEIGHT);
                assert!(editor.height() >= 79.0, "collapsed editor: {editor:?}");
                assert!(
                    editor.width() > 250.0 && editor.right() <= width - 24.0,
                    "editor width: {editor:?}"
                );
                assert!(
                    editor.bottom() < footer.top(),
                    "editor overlaps actions: {editor:?}, {footer:?}"
                );
                if width == 1920.0 {
                    assert!(
                        editor.height() > 350.0,
                        "unused parent height: {editor:?}, footer: {footer:?}"
                    );
                    assert!(editor.width() > 1200.0, "unused parent width: {editor:?}");
                    assert!(footer.bottom() < height - 45.0);
                    if let Some(previous) = large {
                        assert_eq!(
                            editor.size(),
                            previous,
                            "resize did not recover original viewport size"
                        );
                    }
                    large = Some(editor.size());
                }
                assert_eq!(app.editor, original);
                capture_layout(
                    &ctx,
                    output,
                    &format!(
                        "{appearance:?}-{width}-adaptive-profile-{}",
                        if long { "long" } else { "short" }
                    ),
                    width,
                    height,
                );
            }
        }
    }
}

#[test]
fn json_editor_fills_nested_parent_and_blank_area_accepts_input() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        let ctx = egui::Context::default();
        configure(&ctx);
        appearance.apply(&ctx);
        let mut json = "{}".to_string();
        let mut time = 0.0;
        for size in [
            egui::vec2(320.0, 120.0),
            egui::vec2(850.0, 480.0),
            egui::vec2(400.0, 240.0),
        ] {
            let parent = egui::Rect::from_min_size(egui::pos2(50.0, 50.0), size);
            let mut frame = |events| {
                time += 0.05;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1100.0, 760.0),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.scope_builder(egui::UiBuilder::new().max_rect(parent), |ui| {
                                json_editor(ui, &mut json, "nested-json", ui.available_height());
                            });
                        });
                    },
                )
            };
            let mut output = frame(vec![]);
            for _ in 0..4 {
                output = frame(vec![]);
            }
            let rects = json_viewport_rects(&output);
            assert_eq!(rects.len(), 1);
            assert!((rects[0].width() - size.x).abs() < 1.0);
            assert!((rects[0].height() - size.y).abs() < 1.0);
            let pos = parent.center();
            frame(vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            frame(vec![egui::Event::Text(" ".into())]);
        }
        assert_eq!(
            json, "{}   ",
            "blank editor space must be an editable surface"
        );
    }
}

#[test]
fn import_and_preview_json_viewports_grow_with_their_dialogs() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        let mut previous_import = 0.0;
        let mut previous_preview = 0.0;
        for (width, height) in [(760.0, 500.0), (1100.0, 760.0), (1440.0, 1000.0)] {
            let ctx = egui::Context::default();
            configure(&ctx);
            appearance.apply(&ctx);
            let mut app = app();
            app.begin_create(true, false);
            app.ui.new_json = "{}".into();
            let frame = |app: &mut App| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, height),
                        )),
                        ..Default::default()
                    },
                    |ctx| app.render(ctx),
                )
            };
            let mut output = frame(&mut app);
            for _ in 0..12 {
                output = frame(&mut app);
            }
            let editors = json_viewport_rects(&output);
            assert_eq!(editors.len(), 1);
            assert!(editors[0].height() > previous_import, "import did not grow");
            previous_import = editors[0].height();
            assert!(text_bounds(&output, "取消").bottom() < height);
            capture_layout(
                &ctx,
                output,
                &format!("{appearance:?}-{width}-adaptive-import"),
                width,
                height,
            );
            app.ui.dialog = None;
            let candidate = app
                .status
                .as_ref()
                .unwrap()
                .generation
                .as_ref()
                .unwrap()
                .candidate
                .clone();
            app.accept_candidate(candidate);
            let mut output = frame(&mut app);
            for _ in 0..12 {
                output = frame(&mut app);
            }
            let editors = json_viewport_rects(&output);
            assert_eq!(editors.len(), 2);
            assert!(editors[0].height() >= previous_preview, "preview shrank");
            if height == 1000.0 {
                assert!(editors[0].height() > previous_preview);
            }
            previous_preview = editors[0].height();
            assert!((editors[0].height() - editors[1].height()).abs() < 1.0);
            assert!(editors[0].right() < editors[1].left());
            assert!(text_bounds(&output, "确认应用").bottom() < height);
            capture_layout(
                &ctx,
                output,
                &format!("{appearance:?}-{width}-adaptive-preview"),
                width,
                height,
            );
        }
    }
}

fn preview_warning_texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text().starts_with("尚未覆盖字段：") => {
                Some((
                    text.galley.text().to_string(),
                    egui::Rect::from_min_size(text.pos, text.galley.size()),
                ))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn long_preview_diagnostics_scroll_without_overlap_and_new_candidate_starts_at_top() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        for (width, height) in [(760.0, 500.0), (1100.0, 760.0), (1920.0, 1040.0)] {
            let ctx = egui::Context::default();
            configure(&ctx);
            appearance.apply(&ctx);
            let mut app = app();
            app.editor = serde_json::to_string_pretty(&serde_json::json!({
                "metadata":{"long_line":"fixture-".repeat(200),"rows":(0..180).collect::<Vec<_>>()}
            }))
            .unwrap();
            let mut candidate = app
                .status
                .as_ref()
                .unwrap()
                .generation
                .as_ref()
                .unwrap()
                .candidate
                .clone();
            candidate.revision = hash(app.editor.as_bytes());
            candidate.resources.complete = false;
            candidate.resources.notes = (0..40).map(|n| format!("Uncovered dependency capability /vendor/field-{n}/{}; automatic recovery disabled", "nested/".repeat(12))).collect();
            app.accept_candidate(candidate.clone());
            let mut time = 0.0;
            let mut frame = |app: &mut App, events| {
                time += 0.05;
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, height),
                        )),
                        time: Some(time),
                        events,
                        ..Default::default()
                    },
                    |ctx| app.render(ctx),
                )
            };
            let settle = |app: &mut App, frame: &mut dyn FnMut(&mut App, Vec<egui::Event>) -> egui::FullOutput| {
                let mut output = frame(app, vec![]);
                for _ in 0..12 { output = frame(app, vec![]); }
                output
            };
            let mut output = settle(&mut app, &mut frame);
            let modal = enclosing_rect(&output, "预览本次应用", 36.0);
            let point = modal.center();
            let wheel = |point, dy| {
                vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, dy),
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            };
            for _ in 0..8 {
                output = frame(&mut app, wheel(point, -400.0));
            }
            let title = "查看 40 项资源与自动恢复限制";
            let toggle = text_bounds(&output, title).center();
            frame(
                &mut app,
                vec![
                    egui::Event::PointerMoved(toggle),
                    egui::Event::PointerButton {
                        pos: toggle,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            frame(
                &mut app,
                vec![egui::Event::PointerButton {
                    pos: toggle,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            output = settle(&mut app, &mut frame);
            let mut saw_last = false;
            for _ in 0..50 {
                let notes = preview_warning_texts(&output);
                assert!(!notes.is_empty(), "expanded warning details missing");
                for pair in notes.windows(2) {
                    assert!(
                        pair[0].1.bottom() < pair[1].1.top(),
                        "overlapping notes: {pair:?}"
                    );
                }
                saw_last |= notes.iter().any(|(t, _)| t.contains("/field-39/"));
                let pos = output
                    .shapes
                    .iter()
                    .filter_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if text.galley.text().starts_with("尚未覆盖字段：") =>
                        {
                            let bounds = egui::Rect::from_min_size(text.pos, text.galley.size())
                                .intersect(shape.clip_rect);
                            (bounds.height() > 2.0).then_some(bounds)
                        }
                        _ => None,
                    })
                    .max_by(|a, b| a.height().total_cmp(&b.height()))
                    .unwrap()
                    .center();
                output = frame(&mut app, wheel(pos, -160.0));
            }
            assert!(saw_last, "last warning must be reachable by scrolling");
            for label in ["预览本次应用", "返回编辑", "确认应用"] {
                let bounds = text_bounds(&output, label);
                assert!(bounds.top() > 0.0 && bounds.bottom() < height);
            }
            capture_layout(
                &ctx,
                output,
                &format!("{appearance:?}-{width}-preview-many-notes"),
                width,
                height,
            );
            frame(&mut app, vec![egui::Event::PointerMoved(egui::Pos2::ZERO)]);
            settle(&mut app, &mut frame);
            app.ui.preview_open = false;
            candidate.id = "fresh-candidate".into();
            app.accept_candidate(candidate);
            let output = settle(&mut app, &mut frame);
            let target = text_bounds(&output, "目标配置");
            assert!(target.top() > text_bounds(&output, "预览本次应用").bottom());
            assert!(target.bottom() < json_viewport_rects(&output)[0].top());
            assert!(
                preview_warning_texts(&output).is_empty(),
                "fresh details should be collapsed"
            );
            let summary = text_bounds(
                &output,
                "配置已通过 Core 校验。未分类字段无法冻结；启动和恢复时仍需 Core 接受，失败会停止并报告。",
            );
            assert!(summary.top() > target.bottom());
            assert!(summary.bottom() < json_viewport_rects(&output)[0].top());
            capture_layout(
                &ctx,
                output,
                &format!("{appearance:?}-{width}-preview-limit-summary"),
                width,
                height,
            );
        }
    }
}

fn overlapping_visible_text(output: &egui::FullOutput) -> Vec<(String, String)> {
    let mut labels = vec![];
    for shape in &output.shapes {
        if let egui::Shape::Text(text) = &shape.shape {
            if text.galley.text().trim().is_empty() {
                continue;
            }
            // Respect right alignment and wrapped-row indentation. A whole
            // galley rectangle would report intentional empty space as overlap.
            for row in &text.galley.rows {
                let rect = row
                    .visuals
                    .mesh_bounds
                    .translate(text.pos.to_vec2() + row.pos.to_vec2())
                    .intersect(shape.clip_rect);
                if rect.width() > 2.0 && rect.height() > 2.0 {
                    labels.push((text.galley.text(), rect));
                }
            }
        }
    }
    let mut overlaps = vec![];
    for (index, (a, ar)) in labels.iter().enumerate() {
        for (b, br) in labels.iter().skip(index + 1) {
            let intersection = ar.intersect(*br);
            if intersection.width() > 2.0 && intersection.height() > 2.0 {
                overlaps.push((a.chars().take(60).collect(), b.chars().take(60).collect()));
            }
        }
    }
    overlaps
}

#[test]
fn long_content_on_all_pages_keeps_visible_text_separate() {
    let mut failures = vec![];
    for appearance in [Appearance::Light, Appearance::Dark] {
        for (width, height) in [(760.0, 500.0), (1100.0, 760.0), (1920.0, 1040.0)] {
            let mut cases = vec![
                (Page::Overview, ProfileTab::Summary, SettingsTab::Core),
                (Page::Diagnostics, ProfileTab::Summary, SettingsTab::Core),
                (Page::Logs, ProfileTab::Summary, SettingsTab::Core),
            ];
            cases.extend(
                [
                    ProfileTab::Summary,
                    ProfileTab::Inbounds,
                    ProfileTab::Outbounds,
                    ProfileTab::Routing,
                    ProfileTab::Json,
                    ProfileTab::History,
                ]
                .map(|tab| (Page::Profiles, tab, SettingsTab::Core)),
            );
            cases.extend(
                [
                    SettingsTab::Core,
                    SettingsTab::Startup,
                    SettingsTab::Appearance,
                    SettingsTab::Storage,
                ]
                .map(|tab| (Page::Settings, ProfileTab::Summary, tab)),
            );
            for (page, profile_tab, settings_tab) in cases {
                let ctx = egui::Context::default();
                configure(&ctx);
                appearance.apply(&ctx);
                let mut app = app();
                app.ui
                    .workspace
                    .names
                    .insert("work".into(), "长配置显示名称".repeat(10));
                app.root = PathBuf::from(format!(
                    "C:\\Long data path\\{}",
                    "fixture-directory\\".repeat(20)
                ));
                app.core_path = format!(
                    "C:\\Long core path\\{}xray.exe",
                    "fixture-directory\\".repeat(20)
                );
                app.asset_root =
                    format!("C:\\Long asset path\\{}", "fixture-directory\\".repeat(20));
                app.ui.connected = false;
                app.status.as_mut().unwrap().recovery_required = true;
                app.ui.failure = Some(Failure {
                    error: Error::new("TEST_ERROR", "long error evidence\n".repeat(40)),
                    request: "request-test".into(),
                });
                app.ui.page = page;
                app.ui.profile_tab = profile_tab;
                app.ui.settings_tab = settings_tab;
                app.status.as_mut().unwrap().last_error = Some(Error::new(
                    "RECOVERY_REQUIRED",
                    "recovery evidence\n".repeat(30),
                ));
                app.ui.operation_details = "fixture operation evidence\n".repeat(40);
                app.route_result = serde_json::to_string(&RouteResult {
                    kind: "StaticPrediction".into(),
                    match_state: "Unknown".into(),
                    rule_index: None,
                    expected_outbound: None,
                    actual_outbound: "Unverified".into(),
                    reason: "预测依据需要核对。\n".repeat(30),
                })
                .unwrap();
                let entries: Vec<_> = (0..20)
                    .map(|n| {
                        serde_json::json!({
                            "tag":format!("entry-{n}-{}", "very-long-native-tag-".repeat(4)),
                            "protocol":"socks",
                            "native_field_name_requiring_wrap_".repeat(8):"fixture-value",
                        })
                    })
                    .collect();
                app.editor = serde_json::to_string_pretty(&serde_json::json!({
                    "inbounds":entries,"outbounds":entries,"routing":{"rules":entries}
                }))
                .unwrap();
                app.history = (0..20)
                    .map(|n| {
                        let mut g = app
                            .status
                            .as_ref()
                            .unwrap()
                            .generation
                            .as_ref()
                            .unwrap()
                            .clone();
                        g.id = format!("history-{n}");
                        g.candidate.core.version =
                            "Xray fixture long version description ".repeat(8);
                        g
                    })
                    .collect();
                app.dependency_report.installations = (0..12)
                    .map(|n| crate::dependencies::Installation {
                        version: format!("v26.11.{n}"),
                        install_id: format!("fixture-{n}"),
                        valid: false,
                        error: Some("fixture dependency error\n".repeat(30)),
                    })
                    .collect();
                app.dependency_message = "依赖检查结果需要继续核对。\n".repeat(40);
                app.logs = vec!["[warning] fixture long log line ".repeat(20); 60];
                let frame = |app: &mut App, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            events,
                            ..Default::default()
                        },
                        |ctx| app.render(ctx),
                    )
                };
                let mut output = frame(&mut app, vec![]);
                for _ in 0..12 {
                    output = frame(&mut app, vec![]);
                }
                let mut expanded = std::collections::HashSet::new();
                for step in 0..24 {
                    let overlaps = overlapping_visible_text(&output);
                    if !overlaps.is_empty() {
                        failures.push(format!(
                            "{appearance:?} {width} {page:?} / {profile_tab:?} / {settings_tab:?} step {step}: {:?}",
                            overlaps.into_iter().take(6).collect::<Vec<_>>()
                        ));
                        break;
                    }
                    let toggle = output.shapes.iter().find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if [
                                "恢复证据与技术详情",
                                "错误详情与请求身份",
                                "最近查询的操作证据",
                                "预测依据与限制",
                                "原生字段",
                                "高级：原生字段编辑",
                                "依赖检查详情",
                            ]
                            .contains(&text.galley.text())
                                && !expanded.contains(text.galley.text()) =>
                        {
                            let rect = text
                                .galley
                                .rect
                                .translate(text.pos.to_vec2())
                                .intersect(shape.clip_rect);
                            (rect.height() > 8.0)
                                .then(|| (text.galley.text().to_string(), rect.center()))
                        }
                        _ => None,
                    });
                    if let Some((label, pos)) = toggle {
                        expanded.insert(label);
                        frame(
                            &mut app,
                            vec![
                                egui::Event::PointerMoved(pos),
                                egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed: true,
                                    modifiers: egui::Modifiers::NONE,
                                },
                            ],
                        );
                        frame(
                            &mut app,
                            vec![egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            }],
                        );
                        for _ in 0..12 {
                            output = frame(&mut app, vec![]);
                        }
                        continue;
                    }
                    output = frame(
                        &mut app,
                        vec![
                            egui::Event::PointerMoved(egui::pos2(width - 30.0, height * 0.5)),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                delta: egui::vec2(0.0, -200.0),
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                    );
                }
                if appearance == Appearance::Light
                    && width == 760.0
                    && (page == Page::Diagnostics
                        || page == Page::Logs
                        || (page == Page::Profiles && profile_tab == ProfileTab::Json)
                        || (page == Page::Settings
                            && matches!(settings_tab, SettingsTab::Core | SettingsTab::Storage)))
                {
                    capture_layout(
                        &ctx,
                        output,
                        &format!("audit-{page:?}-{profile_tab:?}-{settings_tab:?}"),
                        width,
                        height,
                    );
                }
                if matches!(
                    profile_tab,
                    ProfileTab::Inbounds | ProfileTab::Outbounds | ProfileTab::Routing
                ) {
                    assert!(
                        expanded.contains(if profile_tab == ProfileTab::Inbounds {
                            "原生字段"
                        } else {
                            "高级：原生字段编辑"
                        }),
                        "native fields were not exercised: {appearance:?} {width} {page:?}/{profile_tab:?} expanded={expanded:?}"
                    );
                }
                if page == Page::Diagnostics {
                    assert!(expanded.contains("最近查询的操作证据"));
                }
                if page == Page::Settings && settings_tab == SettingsTab::Core {
                    assert!(expanded.contains("依赖检查详情"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn long_feedback_preserves_the_workspace_without_a_full_message_button() {
    let ctx = egui::Context::default();
    configure(&ctx);
    let mut app = app();
    app.message = "长状态消息不应挤占工作区\n".repeat(80);
    let frame = |app: &mut App, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(760.0, 500.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.render(ctx),
        )
    };
    let mut output = frame(&mut app, vec![]);
    for _ in 0..12 {
        output = frame(&mut app, vec![]);
    }
    let heading = text_bounds(&output, "Core 运行中");
    assert!(
        heading.top() > 60.0 && heading.bottom() < 400.0,
        "feedback consumed workspace: {heading:?}"
    );
    assert!(!output.shapes.iter().any(|shape| {
        matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == "完整消息")
    }));
    capture_layout(&ctx, output, "audit-long-feedback", 760.0, 500.0);
}

#[test]
fn every_confirmation_dialog_keeps_actions_visible_with_long_native_names() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        for (width, height) in [(760.0, 500.0), (1100.0, 760.0), (1920.0, 1040.0)] {
            let actions = [
                Action::Switch("work".into()),
                Action::Discard("work".into()),
                Action::Delete {
                    profile: "work".into(),
                    revision: "revision".into(),
                },
                Action::Stop,
                Action::Restart,
                Action::Proxy(ProxyMode::Global),
                Action::Proxy(ProxyMode::Direct),
                Action::AdoptDraft("work".into()),
            ];
            for action in actions {
                let ctx = egui::Context::default();
                configure(&ctx);
                appearance.apply(&ctx);
                let mut app = app();
                app.ui.runtime = None;
                app.status
                    .as_mut()
                    .unwrap()
                    .generation
                    .as_mut()
                    .unwrap()
                    .http_inbound_tag = "long-native-tag-".repeat(150);
                app.ui
                    .workspace
                    .names
                    .insert("work".into(), "长配置名称".repeat(16));
                let (title, _, button) = action.copy();
                app.ui.dialog = Some(Dialog::Confirm(action));
                let mut output = None;
                for _ in 0..12 {
                    output = Some(ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, height),
                            )),
                            ..Default::default()
                        },
                        |ctx| app.render(ctx),
                    ));
                }
                let output = output.unwrap();
                for label in [title, "取消", button] {
                    let rect = text_bounds(&output, label);
                    let clip = output
                        .shapes
                        .iter()
                        .find_map(|s| match &s.shape {
                            egui::Shape::Text(t) if t.galley.text() == label => Some(s.clip_rect),
                            _ => None,
                        })
                        .unwrap();
                    assert!(
                        rect.top() > 0.0 && rect.bottom() < height && clip.contains_rect(rect),
                        "clipped {label}"
                    );
                }
                if button == "确认接管" && width == 760.0 {
                    capture_layout(
                        &ctx,
                        output,
                        &format!("{appearance:?}-760-audit-long-confirmation"),
                        width,
                        height,
                    );
                }
            }
        }
    }
}

fn block_draft_writes(app: &mut App) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("config")).unwrap();
    std::fs::write(root.path().join("config/editor-drafts"), "blocked").unwrap();
    app.root = root.path().into();
    app.ui.persistence = true;
    app.editor = "{\"unsaved\":true}".into();
    app.draft_edited();
    root
}

#[test]
fn all_normal_exit_entries_block_on_draft_failure_and_allow_retry_or_explicit_discard() {
    for entry in ["tray-gui", "tray-quit", "window"] {
        for discard in [false, true] {
            let mut app = app();
            let root = block_draft_writes(&mut app);
            let ctx = egui::Context::default();
            match entry {
                "tray-gui" => app.tray_action("exit-gui", &ctx),
                "tray-quit" => app.tray_action("quit", &ctx),
                _ => app.request_exit(ExitKind::Gui),
            }
            app.advance_shutdown();
            assert!(!app.exit, "{entry}");
            assert!(!app.shutdown_requested, "{entry}");
            assert!(
                matches!(app.ui.dialog, Some(Dialog::UnsavedExit(_))),
                "{entry}"
            );
            assert!(app.ui.draft_dirty_since.is_some());
            assert!(app.ui.pending_draft_writes.contains("work"));
            let export = tempfile::tempdir().unwrap();
            assert!(app.export_unsaved_drafts(&export.path().join("drafts.json")));
            let value: serde_json::Value =
                serde_json::from_slice(&std::fs::read(export.path().join("drafts.json")).unwrap())
                    .unwrap();
            assert_eq!(value["drafts"]["work"]["json"], app.editor);
            assert!(!app.exit, "export alone must not exit");
            let kind = if entry.ends_with("quit") {
                ExitKind::Shutdown
            } else {
                ExitKind::Gui
            };
            if discard {
                app.discard_unsaved_and_exit(kind);
            } else {
                std::fs::remove_file(root.path().join("config/editor-drafts")).unwrap();
                app.request_exit(kind);
                app.advance_shutdown();
                assert!(app.ui.pending_draft_writes.is_empty());
                assert!(app.ui.draft_dirty_since.is_none());
                assert_eq!(
                    Draft::load(root.path(), "work").unwrap().unwrap().json,
                    app.editor
                );
                assert!(app.ui.failure.is_none());
            }
            if matches!(kind, ExitKind::Gui) {
                assert!(app.exit);
            } else {
                assert!(app.shutdown_requested);
            }
        }
    }
}

#[test]
fn background_draft_write_failures_are_retried_even_after_switching_profiles() {
    let mut app = app();
    let root = tempfile::tempdir().unwrap();
    app.root = root.path().into();
    app.ui.persistence = true;
    app.editor = "{\"s\":2}".into();
    assert!(app.stash_draft());
    app.load(Profile {
        id: "other".into(),
        json: "{}".into(),
        revision: hash(b"{}"),
    });
    let path = root.path().join("config/editor-drafts/work.json");
    let before = std::fs::read(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    app.applied_draft("work", Some("{\"s\":1}"));
    assert!(app.ui.pending_draft_writes.contains("work"));
    assert!(app.ui.draft_dirty_since.is_some());
    assert!(!app.drafts_ready_for_exit(ExitKind::Gui));
    assert_eq!(app.ui.drafts["work"].json, "{\"s\":2}");
    std::fs::remove_dir(&path).unwrap();
    std::fs::write(&path, before).unwrap();
    app.ui.draft_dirty_since = Some(Instant::now() - Duration::from_secs(2));
    app.flush_draft_if_due();
    assert!(app.ui.pending_draft_writes.is_empty());
    assert!(app.ui.failure.is_none());
    assert!(app.ui.dialog.is_none());
    assert!(app.message.is_empty());
    assert_eq!(
        Draft::load(root.path(), "work")
            .unwrap()
            .unwrap()
            .base_revision,
        hash(b"{\"s\":1}")
    );
}

#[test]
fn explicit_discard_does_not_authorize_later_edits_to_be_lost() {
    let mut app = app();
    let _root = block_draft_writes(&mut app);
    assert!(!app.stash_draft());
    app.discard_unsaved_and_exit(ExitKind::Shutdown);
    app.editor = "{\"newer\":true}".into();
    app.draft_edited();
    assert!(!app.drafts_ready_for_exit(ExitKind::Gui));
    assert!(!app.exit);
}

fn guided_frame(
    app: &mut App,
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    width: f32,
    height: f32,
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, height),
            )),
            events,
            ..Default::default()
        },
        |ctx| app.render(ctx),
    )
}
fn guided_click(app: &mut App, ctx: &egui::Context, label: &str, width: f32, height: f32) {
    let mut output = guided_frame(app, ctx, vec![], width, height);
    for _ in 0..12 {
        output = guided_frame(app, ctx, vec![], width, height);
    }
    let pos = text_bounds(&output, label).center();
    guided_frame(
        app,
        ctx,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
        width,
        height,
    );
    guided_frame(
        app,
        ctx,
        vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
        width,
        height,
    );
}

#[test]
fn new_wizard_keeps_actions_visible_and_only_saves_incomplete_native_profiles() {
    use crate::guided_config::{Scene, Template};
    for appearance in [Appearance::Light, Appearance::Dark] {
        for (width, height) in [(760.0, 500.0), (1100.0, 760.0)] {
            for template in Template::ALL {
                let mut app = app();
                let (tx, requests) = mpsc::sync_channel(MAX_PENDING);
                app.tx = tx;
                app.pending.clear();
                app.begin_create(false, false);
                app.ui.new_name = "手工节点".into();
                app.ui.new_scene = Scene::Split;
                app.ui.new_template = template;
                let ctx = egui::Context::default();
                configure(&ctx);
                appearance.apply(&ctx);
                guided_click(&mut app, &ctx, "开始填写", width, height);
                assert!(app.ui.new_guided_started);
                let mut output = guided_frame(&mut app, &ctx, vec![], width, height);
                for _ in 0..12 {
                    output = guided_frame(&mut app, &ctx, vec![], width, height);
                }
                let viewport =
                    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, height));
                for label in ["新建配置", "取消", "保存待补充配置"] {
                    assert!(
                        viewport.contains_rect(text_bounds(&output, label)),
                        "{template:?} {label}"
                    );
                }
                if template == Template::VlessReality {
                    capture_layout(
                        &ctx,
                        output,
                        &format!("{appearance:?}-{width}-guided-create"),
                        width,
                        height,
                    );
                }
                guided_click(&mut app, &ctx, "保存待补充配置", width, height);
                assert!(app.ui.dialog.is_none());
                let request = requests.try_recv().unwrap();
                let Command::CreateProfile { profile_id, json } = request.command else {
                    panic!("wizard must only create a profile")
                };
                let value: serde_json::Value = serde_json::from_str(&json).unwrap();
                assert_eq!(value["outbounds"][0]["tag"], "node-1");
                assert_eq!(value["routing"]["rules"], serde_json::json!([]));
                assert!(requests.try_recv().is_err());
                assert!(app.ui.workspace.node_names.contains_key(&profile_id));
                assert_eq!(app.mode, ProxyMode::Direct);
            }
        }
    }
}

#[test]
fn issue_action_opens_the_node_form_and_edits_only_the_draft() {
    let mut app = app();
    let (tx, requests) = mpsc::sync_channel(MAX_PENDING);
    app.tx = tx;
    app.pending.clear();
    let mut config: serde_json::Value = serde_json::from_str(&app.editor).unwrap();
    config["outbounds"][0] = serde_json::json!({"tag":"node-1","protocol":"socks","settings":{"servers":[{"port":0,"custom":"preserved"}]},"extension":{"keep":true}});
    app.editor = serde_json::to_string_pretty(&config).unwrap();
    app.ui.page = Page::Profiles;
    app.ui.profile_tab = ProfileTab::Summary;
    app.ui.guided.native = true;
    let original = app.editor.clone();
    let runtime = app.ui.runtime.as_ref().unwrap().json.clone();
    let ctx = egui::Context::default();
    configure(&ctx);
    guided_click(&mut app, &ctx, "查看 2 项补充 / 核对提示", 1100.0, 760.0);
    guided_click(&mut app, &ctx, "去补充", 1100.0, 760.0);
    assert_eq!(app.ui.profile_tab, ProfileTab::Outbounds);
    for _ in 0..12 {
        guided_frame(&mut app, &ctx, vec![], 1100.0, 760.0);
    }
    assert_eq!(app.editor, original);
    guided_frame(
        &mut app,
        &ctx,
        vec![egui::Event::Text("server.invalid".into())],
        1100.0,
        760.0,
    );
    let edited: serde_json::Value = serde_json::from_str(&app.editor).unwrap();
    assert_eq!(
        edited["outbounds"][0]["settings"]["servers"][0]["address"],
        "server.invalid"
    );
    assert_eq!(
        edited["outbounds"][0]["extension"],
        config["outbounds"][0]["extension"]
    );
    assert_eq!(
        edited["outbounds"][0]["settings"]["servers"][0]["custom"],
        "preserved"
    );
    assert_eq!(app.ui.runtime.as_ref().unwrap().json, runtime);
    assert!(requests.try_recv().is_err());
    assert!(
        !xray_config::inspect_config(&edited)
            .iter()
            .any(|i| i.path.ends_with("/address"))
    );
}

#[test]
fn a_new_wizard_hides_authentication_even_if_the_previous_wizard_revealed_it() {
    use crate::guided_config::{Scene, Template};
    let mut app = app();
    let ctx = egui::Context::default();
    configure(&ctx);
    let prepare = |app: &mut App| {
        app.begin_create(false, false);
        app.ui.new_guided_started = true;
        let mut value = crate::guided_config::scene(Scene::Single, Template::VlessTls);
        value["outbounds"][0]["settings"]["vnext"][0]["users"][0]["id"] =
            serde_json::json!("test-credential-only");
        app.ui.new_json = serde_json::to_string(&value).unwrap();
    };
    prepare(&mut app);
    guided_click(&mut app, &ctx, "显示认证信息", 1100.0, 1040.0);
    let output = guided_frame(&mut app, &ctx, vec![], 1100.0, 1040.0);
    assert!(output.shapes.iter().any(
        |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="test-credential-only")
    ));
    prepare(&mut app);
    let output = guided_frame(&mut app, &ctx, vec![], 1100.0, 1040.0);
    assert!(!output.shapes.iter().any(
        |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text()=="test-credential-only")
    ));
}

#[test]
fn guided_node_and_routing_forms_fit_the_parent_without_overlapping_text() {
    use crate::guided_config::{Scene, Template};
    for appearance in [Appearance::Light, Appearance::Dark] {
        for (width, height) in [(760.0, 500.0), (1100.0, 760.0), (1920.0, 1040.0)] {
            for tab in [ProfileTab::Outbounds, ProfileTab::Routing] {
                let mut app = app();
                let mut value = crate::guided_config::scene(Scene::Split, Template::Socks);
                value["outbounds"][0]["settings"]["servers"][0] =
                    serde_json::json!({"address":"proxy.example.com","port":443});
                value["routing"]["rules"] = serde_json::json!([
                    {"type":"field","domain":["domain:example.com","full:only.example.org"],"outboundTag":"node-1"},
                    {"type":"field","ip":["192.0.2.0/24"],"outboundTag":"direct"}
                ]);
                app.editor = serde_json::to_string_pretty(&value).unwrap();
                app.ui.page = Page::Profiles;
                app.ui.profile_tab = tab;
                app.ui.guided.focus = Some("/outbounds/0/settings/servers/0/address".into());
                app.ui.workspace.node_names.insert(
                    "work".into(),
                    std::collections::BTreeMap::from([("node-1".into(), "香港节点".into())]),
                );
                let original = app.editor.clone();
                let ctx = egui::Context::default();
                configure(&ctx);
                appearance.apply(&ctx);
                let mut output = guided_frame(&mut app, &ctx, vec![], width, height);
                for _ in 0..12 {
                    output = guided_frame(&mut app, &ctx, vec![], width, height);
                }
                assert!(
                    overlapping_visible_text(&output).is_empty(),
                    "{appearance:?} {width} {tab:?}: {:?}",
                    overlapping_visible_text(&output)
                );
                assert_eq!(app.editor, original);
                if tab == ProfileTab::Routing && width >= 1100.0 {
                    let first_match = text_bounds(&output, "example.com\nfull:only.example.org");
                    let footer = text_bounds(&output, "检查并预览");
                    assert!(
                        first_match.bottom() < footer.top(),
                        "first rule input must be visible: {first_match:?} / {footer:?}"
                    );
                }
                capture_layout(
                    &ctx,
                    output,
                    &format!("{appearance:?}-{width}-guided-{tab:?}"),
                    width,
                    height,
                );
            }
        }
    }
}

#[test]
fn unfinished_number_notice_locates_its_field_without_losing_input_or_overlapping_controls() {
    for appearance in [Appearance::Light, Appearance::Dark] {
        for (width, height) in [(760.0, 500.0), (1100.0, 760.0), (1920.0, 1040.0)] {
            let mut app = app();
            app.ui.page = Page::Profiles;
            app.ui.profile_tab = ProfileTab::Json;
            let path = "/inbounds/0/port";
            let source = serde_json::from_str::<serde_json::Value>(&app.editor)
                .unwrap()
                .pointer(path)
                .unwrap()
                .to_string();
            app.ui.number_inputs.insert(
                path.into(),
                crate::workspace::NumberInput {
                    source,
                    text: "-".into(),
                },
            );
            app.draft_edited();
            let before = app.editor.clone();
            let ctx = egui::Context::default();
            configure(&ctx);
            appearance.apply(&ctx);
            for _ in 0..4 {
                guided_frame(&mut app, &ctx, vec![], width, height);
            }
            // The smallest window stacks the picker above the editor; scroll the page to its body.
            if width < 1100.0 {
                guided_frame(
                    &mut app,
                    &ctx,
                    vec![
                        egui::Event::PointerMoved(egui::pos2(width - 80.0, height - 100.0)),
                        egui::Event::MouseWheel {
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -240.0),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    width,
                    height,
                );
                for _ in 0..12 {
                    guided_frame(&mut app, &ctx, vec![], width, height);
                }
            }
            guided_click(&mut app, &ctx, "定位未完成数值", width, height);
            assert_eq!(app.ui.profile_tab, ProfileTab::Inbounds);
            assert!(app.ui.guided.native);
            let mut output = guided_frame(&mut app, &ctx, vec![], width, height);
            for _ in 0..4 {
                output = guided_frame(&mut app, &ctx, vec![], width, height);
            }
            assert!(
                overlapping_visible_text(&output).is_empty(),
                "{appearance:?} {width}: {:?}",
                overlapping_visible_text(&output)
            );
            assert_eq!(app.editor, before);
            assert_eq!(app.ui.number_inputs[path].text, "-");
        }
    }
}

#[test]
fn pac_draft_is_per_profile_persisted_and_can_be_tested() {
    let mut app = app();
    app.ui.page = Page::Pac;
    let mut draft = app.pac_draft();
    assert_eq!(draft.port, xray_model::DEFAULT_PAC_PORT);
    assert!(draft.template);
    draft.template = false;
    draft.generated = false;
    draft.script = "function FindProxyForURL(){ return 'DIRECT'; }".into();
    draft.port = 18081;
    app.save_pac_draft(draft.clone());
    assert_eq!(app.pac_draft().script, draft.script);
    assert_eq!(app.pac_draft().port, 18081);
    let ctx = egui::Context::default();
    configure(&ctx);
    app.ui.connected = true;
    render(&mut app, &ctx, 1100.0, 760.0);
}

#[test]
fn structured_edits_can_be_undone_and_redone() {
    let mut app = app();
    let original = app.editor.clone();
    let edited = original.replace("\"loglevel\": \"warning\"", "\"loglevel\": \"info\"");
    assert_ne!(edited, original);
    app.editor = edited.clone();
    app.draft_edited();
    assert_eq!(app.ui.undo.len(), 1);
    app.undo_edit();
    assert_eq!(app.editor, original);
    app.redo_edit();
    assert_eq!(app.editor, edited);
}

#[test]
fn proxy_mode_switch_uses_the_running_profile_pac_draft() {
    let mut app = app();
    app.ui.workspace.pac.insert(
        "work".into(),
        PacDraft {
            generated: false,
            template: false,
            template_mode: Default::default(),
            domains: Vec::new(),
            script: "function FindProxyForURL(){ return 'DIRECT'; }".into(),
            port: 18080,
        },
    );
    app.ui.workspace.pac.insert(
        "other".into(),
        PacDraft {
            generated: false,
            template: false,
            template_mode: Default::default(),
            domains: Vec::new(),
            script: "function FindProxyForURL(){ return 'PROXY 127.0.0.1:1'; }".into(),
            port: 18081,
        },
    );
    app.profile_id = "other".into();
    assert_eq!(app.pac_draft().port, 18081);
    assert_eq!(app.running_pac_draft().port, 18080);
}

#[test]
fn pac_endpoint_refreshes_after_config_adds_or_renames_an_http_inbound() {
    let mut app = app();
    app.http_tag = "http-in".into();
    app.editor = r#"{"outbounds":[{"tag":"direct","protocol":"freedom"}]}"#.into();
    assert!(app.pac_endpoint().is_err());
    app.editor = r#"{"inbounds":[{"tag":"new-http","listen":"127.0.0.1","port":18089,"protocol":"http"}],"outbounds":[{"tag":"direct","protocol":"freedom"}]}"#.into();
    let endpoint = app.pac_endpoint().unwrap();
    assert_eq!(endpoint.port(), 18089);
    assert_eq!(app.http_tag, "new-http");
}

#[test]
fn pac_template_domain_list_is_saved_per_profile() {
    let mut app = app();
    let mut draft = app.pac_draft();
    assert_eq!(draft.template_mode, xray_model::PacTemplateMode::All);
    draft.template_mode = xray_model::PacTemplateMode::ProxyList;
    draft.domains = vec!["example.com".into(), "*.internal.test".into()];
    app.save_pac_draft(draft);
    let loaded = app.pac_draft();
    assert_eq!(loaded.template_mode, xray_model::PacTemplateMode::ProxyList);
    assert_eq!(loaded.domains, ["example.com", "*.internal.test"]);
}

#[test]
fn stopped_core_is_restarted_when_the_last_state_was_running() {
    let mut app = app();
    app.ui.connected = true;
    app.ui.settings_loaded = true;
    app.settings.restore_last_state = true;
    app.ui.workspace.last_core_running = true;
    if let Some(status) = app.status.as_mut() {
        status.core_state = "Stopped".into();
        status.recovery_required = false;
        if let Some(generation) = status.generation.as_mut() {
            generation.running = false;
        }
    }
    app.maybe_restore_core();
    assert!(app.ui.restore_attempted);
}

#[test]
fn explicit_stop_clears_the_restore_intent() {
    let mut app = app();
    app.ui.connected = true;
    app.ui.workspace.last_core_running = true;
    app.send(Command::StopCore);
    assert!(!app.ui.workspace.last_core_running);
}

#[test]
fn diagnostics_avoids_unverified_placeholders_and_shows_recovery_context() {
    let mut app = app();
    let ctx = egui::Context::default();
    configure(&ctx);
    app.ui.page = Page::Diagnostics;
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 760.0),
            )),
            ..Default::default()
        },
        |ctx| app.render(ctx),
    );
    let labels: Vec<&str> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect();
    assert!(!labels.contains(&"真实网络连通性"));
    assert!(!labels.contains(&"实际出口"));
    assert!(labels.contains(&"恢复状态"));
    assert!(labels.contains(&"代理模式"));
}

#[test]
fn tray_exit_opens_one_confirmation_with_remembered_default() {
    let mut app = app();
    let ctx = egui::Context::default();
    app.ui.workspace.exit_default = crate::workspace::ExitDefault::Shutdown;
    app.tray_action("exit", &ctx);
    assert!(matches!(app.ui.dialog, Some(Dialog::ExitChoice)));
    assert_eq!(app.ui.exit_choice, ExitKind::Shutdown);
    assert!(app.ui.exit_remember);
}

#[test]
fn empty_pac_page_does_not_show_a_json_parse_error() {
    let mut app = app();
    app.profile_id.clear();
    app.selected_profile.clear();
    app.editor.clear();
    app.ui.page = Page::Pac;
    let ctx = egui::Context::default();
    configure(&ctx);
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 760.0),
            )),
            ..Default::default()
        },
        |ctx| app.render(ctx),
    );
    let labels: Vec<&str> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect();
    assert!(!labels.iter().any(|label| label.contains("无法解析")));
    assert!(!labels.iter().any(|label| label.contains("EOF")));
    assert!(labels.iter().any(|label| label.contains("尚未选择配置")));
}

#[test]
fn onboarding_actions_share_one_row() {
    let mut app = app();
    app.ui.onboarding = true;
    app.ui.setup_step = 0;
    app.core_path = "C:\\Xray\\xray.exe".into();
    let ctx = egui::Context::default();
    configure(&ctx);
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 760.0),
            )),
            ..Default::default()
        },
        |ctx| app.render(ctx),
    );
    let save = text_bounds(&output, "保存并继续");
    let later = text_bounds(&output, "稍后设置，进入总览");
    assert!((save.center().y - later.center().y).abs() < 4.0);
}

#[test]
fn selecting_proxy_list_mode_persists_direct_template() {
    let mut app = app();
    app.ui.page = Page::Pac;
    let ctx = egui::Context::default();
    configure(&ctx);
    let frame = |app: &mut App, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 760.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.render(ctx),
        )
    };
    let first = frame(&mut app, vec![]);
    let mode = text_bounds(&first, "全部代理（本地/局域网直连）").center();
    let click = |pos| {
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ]
    };
    frame(&mut app, click(mode));
    let opened = frame(&mut app, vec![]);
    let choice = text_bounds(&opened, "仅代理清单中的域名").center();
    frame(&mut app, click(choice));
    frame(&mut app, vec![]);
    let draft = app.pac_draft();
    assert_eq!(draft.template_mode, xray_model::PacTemplateMode::ProxyList);
    assert!(draft.template);
    assert!(draft.domains.is_empty());
    assert!(draft.script.contains("return \"DIRECT\""));
}

#[test]
fn adding_a_domain_reactivates_and_saves_the_template() {
    let mut app = app();
    app.ui.page = Page::Pac;
    let mut draft = app.pac_draft();
    draft.template = false;
    draft.generated = true;
    draft.script = "function FindProxyForURL(){ return 'PROXY 127.0.0.1:1'; }".into();
    app.save_pac_draft(draft);
    app.ui.pac_domain_input = "example.com".into();
    let ctx = egui::Context::default();
    configure(&ctx);
    let frame = |app: &mut App, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 760.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.render(ctx),
        )
    };
    let output = frame(&mut app, vec![]);
    let add = text_bounds(&output, "添加").center();
    let events = vec![
        egui::Event::PointerMoved(add),
        egui::Event::PointerButton {
            pos: add,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::PointerButton {
            pos: add,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        },
    ];
    frame(&mut app, events);
    frame(&mut app, vec![]);
    let saved = app.pac_draft();
    assert!(saved.template);
    assert!(!saved.generated);
    assert_eq!(saved.template_mode, xray_model::PacTemplateMode::ProxyList);
    assert_eq!(saved.domains, ["example.com"]);
    assert!(saved.script.contains(r#"return "PROXY"#));
    assert!(saved.script.contains(r#"return "DIRECT""#));
}

#[test]
fn pac_page_exposes_numbered_workflow_steps() {
    let mut app = app();
    app.ui.page = Page::Pac;
    let ctx = egui::Context::default();
    configure(&ctx);
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 1600.0),
            )),
            ..Default::default()
        },
        |ctx| app.render(ctx),
    );
    let labels: Vec<&str> = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.text()),
            _ => None,
        })
        .collect();
    for step in [
        "步骤 1 · 选择代理范围",
        "步骤 2 · 添加代理域名",
        "步骤 3 · 生成并检查模板",
        "步骤 4 · 执行 FindProxyForURL 测试",
        "步骤 5 · 应用到运行版本",
    ] {
        assert!(labels.contains(&step), "missing {step}");
    }
    assert!(labels.contains(&"生成并检查模板"));
    assert!(labels.contains(&"测试当前模板"));
    assert!(labels.contains(&"检查并预览 PAC 应用"));
}

#[test]
fn history_diff_dialog_loads_only_after_confirmation() {
    let mut app = app();
    let before = app.editor.clone();
    let historical = before.replace("\"loglevel\": \"warning\"", "\"loglevel\": \"info\"");
    app.ui.history_preview = Some(HistoryPreview {
        json: historical.clone(),
        core: "Xray historical".into(),
    });
    app.ui.dialog = Some(Dialog::HistoryDiff);
    let ctx = egui::Context::default();
    configure(&ctx);
    let _output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 760.0),
            )),
            ..Default::default()
        },
        |ctx| app.render(ctx),
    );
    for _ in 0..3 {
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 760.0),
                )),
                ..Default::default()
            },
            |ctx| app.render(ctx),
        );
    }
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 760.0),
            )),
            ..Default::default()
        },
        |ctx| app.render(ctx),
    );
    assert!(text_bounds(&output, "比较历史版本与当前草稿").top() > 0.0);
    let load = text_bounds(&output, "载入历史为草稿").center();
    assert_eq!(app.editor, before);
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 760.0),
            )),
            events: vec![
                egui::Event::PointerMoved(load),
                egui::Event::PointerButton {
                    pos: load,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: load,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ctx| app.render(ctx),
    );
    let _ = output;
    assert_eq!(app.editor, historical);
    assert!(app.ui.history_preview.is_none());
}
