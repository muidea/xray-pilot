use super::*;

#[test]
fn templates_require_provider_data_and_never_disable_certificate_checks() {
    for template in Template::ALL {
        let value = scene(Scene::Single, template);
        let node = &value["outbounds"][0];
        assert_eq!(node["tag"], "node-1");
        assert!(
            node.pointer("/streamSettings/tlsSettings/allowInsecure")
                .is_none()
        );
        let path = if matches!(
            template,
            Template::VlessReality | Template::VlessTls | Template::Vmess
        ) {
            "/settings/vnext/0"
        } else {
            "/settings/servers/0"
        };
        assert!(node.pointer(&format!("{path}/address")).is_none());
        assert!(node.pointer(&format!("{path}/port")).is_none());
        assert!(!xray_config::inspect_config(&value).is_empty());
        assert_eq!(value["inbounds"][0]["listen"], "127.0.0.1");
        assert_eq!(value["inbounds"][1]["listen"], "127.0.0.1");
        assert_eq!(value.pointer("/routing/rules").unwrap(), &json!([]));
    }
    let direct = scene(Scene::Direct, Template::VlessReality);
    assert_eq!(direct["outbounds"][0]["protocol"], "freedom");
    assert!(xray_config::inspect_config(&direct).is_empty());
}

#[test]
fn patch_fills_missing_paths_and_preserves_native_extensions_and_other_users() {
    let mut node = json!({"protocol":"vless", "settings":{"vnext":[{"custom":42,"users":[{"id":"first","flow":"custom"},{"id":"second"}]}]}, "custom":{"enabled":true}});
    let old = node.clone();
    patch(
        &mut node,
        "/settings/vnext/0/address",
        json!("server.invalid"),
    )
    .unwrap();
    patch(
        &mut node,
        "/streamSettings/realitySettings/publicKey",
        json!("provider-key"),
    )
    .unwrap();
    assert_eq!(node["custom"], old["custom"]);
    assert_eq!(
        node["settings"]["vnext"][0]["users"],
        old["settings"]["vnext"][0]["users"]
    );
    assert_eq!(node["settings"]["vnext"][0]["custom"], 42);
    let mut missing = json!({"protocol":"vless"});
    patch(
        &mut missing,
        "/settings/vnext/0/users/0/id",
        json!("provided-id"),
    )
    .unwrap();
    assert_eq!(
        missing.pointer("/settings/vnext/0/users/0/id"),
        Some(&json!("provided-id"))
    );
    for malformed in [
        json!({"settings":null}),
        json!({"settings":{"vnext":"keep"}}),
        json!({"settings":{"vnext":[false]}}),
    ] {
        let mut value = malformed.clone();
        assert!(patch(&mut value, "/settings/vnext/0/address", json!("new")).is_err());
        assert_eq!(value, malformed);
    }
}

#[test]
fn default_selection_keeps_rule_order_and_blocks_deleting_referenced_nodes() {
    let mut config = scene(Scene::Split, Template::Socks);
    config["outbounds"]
        .as_array_mut()
        .unwrap()
        .push(Template::Http.node("second"));
    config["routing"]["rules"] = json!([{"type":"field","domain":["domain:example.com"],"outboundTag":"node-1"},{"custom":true}]);
    let rules = config["routing"].clone();
    assert!(set_default(&mut config, 3));
    assert_eq!(config["outbounds"][0]["tag"], "second");
    assert_eq!(config["routing"], rules);
    let before = config.clone();
    assert!(remove_node(&mut config, 1).is_err());
    assert_eq!(config, before);
    assert!(remove_node(&mut config, 0).is_err());
    assert!(remove_node(&mut config, 2).is_ok());
    assert_eq!(config["routing"], rules);
}

#[test]
fn generated_tags_avoid_existing_nodes_and_internal_targets() {
    let config = json!({"outbounds":[{"tag":"node-1"},{"tag":"node-3"}],"api":{"tag":"node-2"},"reverse":{"portals":[{"tag":"node-4"}]}});
    assert_eq!(next_tag(&config, &BTreeMap::new()), "node-5");
    assert_eq!(
        next_tag(
            &config,
            &BTreeMap::from([("node-5".into(), "deleted name".into())])
        ),
        "node-6"
    );
}

#[test]
fn compound_routing_is_never_simplified() {
    let simple = json!({"type":"field","domain":["domain:example.com"],"outboundTag":"direct"});
    assert!(simple_rule(&simple));
    for (field, value) in [
        ("port", json!("443")),
        ("ip", json!(["192.0.2.1"])),
        ("custom", json!({})),
        ("balancerTag", json!("balancer")),
    ] {
        let mut compound = simple.clone();
        compound[field] = value;
        assert!(!simple_rule(&compound));
    }
}

#[test]
fn bare_domains_are_explicit_and_native_matchers_keep_their_meaning() {
    assert_eq!(
        domain_lines("example.com\n full:only.example.com\nregexp:.*\\.invalid\n"),
        json!([
            "domain:example.com",
            "full:only.example.com",
            "regexp:.*\\.invalid",
            ""
        ])
    );
    assert!(!simple_rule(
        &json!({"type":"field","domain":["native-substring"],"outboundTag":"direct"})
    ));
}

fn form_frame(
    ctx: &eframe::egui::Context,
    node: &mut Value,
    events: Vec<eframe::egui::Event>,
    focus: Option<&str>,
    width: f32,
) -> eframe::egui::FullOutput {
    use eframe::egui;
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    super::node_form(ui, node, focus);
                });
            });
        },
    )
}

#[test]
fn guided_forms_preserve_incomplete_and_unrecognized_native_data_when_viewed() {
    use eframe::egui;
    for dark in [true, false] {
        for width in [440.0, 780.0] {
            for template in Template::ALL {
                let ctx = egui::Context::default();
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let mut node = template.node("keep");
                node["custom"] = json!({"unknown":true,"list":[1,2,3]});
                let before = node.clone();
                for _ in 0..3 {
                    form_frame(&ctx, &mut node, vec![], None, width);
                }
                assert_eq!(node, before, "{template:?} {width}");
            }
        }
    }
    for mut node in [
        json!(null),
        json!({"protocol":"custom","settings":null}),
        json!({"protocol":"vless","settings":{"vnext":"keep"}}),
    ] {
        let before = node.clone();
        form_frame(&egui::Context::default(), &mut node, vec![], None, 440.0);
        assert_eq!(node, before);
    }
}

#[test]
fn invalid_port_input_survives_frames_and_can_be_repaired_without_losing_extensions() {
    use eframe::egui;
    let ctx = egui::Context::default();
    let mut node = Template::Socks.node("node");
    node["custom"] = json!({"keep":42});
    form_frame(
        &ctx,
        &mut node,
        vec![],
        Some("/settings/servers/0/port"),
        780.0,
    );
    form_frame(
        &ctx,
        &mut node,
        vec![egui::Event::Text("-".into())],
        None,
        780.0,
    );
    assert_eq!(node.pointer("/settings/servers/0/port"), Some(&json!("-")));
    form_frame(&ctx, &mut node, vec![], None, 780.0);
    assert_eq!(node.pointer("/settings/servers/0/port"), Some(&json!("-")));
    let modifiers = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };
    form_frame(
        &ctx,
        &mut node,
        vec![
            egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            },
            egui::Event::Text("443".into()),
        ],
        None,
        780.0,
    );
    assert_eq!(node.pointer("/settings/servers/0/port"), Some(&json!(443)));
    assert_eq!(node["custom"], json!({"keep":42}));
    let config = json!({"outbounds":[node]});
    assert!(
        !xray_config::inspect_config(&config)
            .iter()
            .any(|i| i.path.ends_with("/port"))
    );
}

#[test]
#[ignore = "requires explicitly selected official Core via XRAY_TEST_CORE; validation only, no Core startup"]
fn official_core_accepts_completed_guided_templates() {
    use std::process::Command;
    let core = std::env::var_os("XRAY_TEST_CORE").expect("XRAY_TEST_CORE");
    let root = tempfile::tempdir().unwrap();
    let keys = Command::new(&core).arg("x25519").output().unwrap();
    assert!(keys.status.success());
    let keys = String::from_utf8(keys.stdout).unwrap();
    let public = keys
        .lines()
        .find_map(|line| {
            let (label, value) = line.split_once(':')?;
            let label = label.to_ascii_lowercase();
            (label == "password" || label.contains("public")).then_some(value)
        })
        .expect("official x25519 public key")
        .trim();
    for template in Template::ALL {
        let mut config = scene(Scene::Single, template);
        let node = &mut config["outbounds"][0];
        let user_protocol = matches!(
            template,
            Template::VlessReality | Template::VlessTls | Template::Vmess
        );
        let server = if user_protocol {
            "/settings/vnext/0"
        } else {
            "/settings/servers/0"
        };
        patch(node, &format!("{server}/address"), json!("example.com")).unwrap();
        patch(node, &format!("{server}/port"), json!(443)).unwrap();
        if user_protocol {
            patch(
                node,
                &format!("{server}/users/0/id"),
                json!("11111111-1111-4111-8111-111111111111"),
            )
            .unwrap();
        }
        if matches!(template, Template::Trojan | Template::Shadowsocks) {
            patch(
                node,
                &format!("{server}/password"),
                json!("test-only-password"),
            )
            .unwrap();
        }
        if template == Template::Shadowsocks {
            patch(node, &format!("{server}/method"), json!("aes-128-gcm")).unwrap();
        }
        if template == Template::VlessReality {
            patch(node,"/streamSettings/realitySettings",json!({"serverName":"example.com","fingerprint":"chrome","publicKey":public,"shortId":"abcd"})).unwrap();
        }
        let path = root.path().join("config.json");
        std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let result = Command::new(&core)
            .args(["run", "-test", "-config"])
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{template:?}: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        // Cover the conditional transport fields without requiring a remote server.
        if matches!(template, Template::VlessTls | Template::Trojan) {
            for network in ["ws", "grpc"] {
                config["outbounds"][0]["streamSettings"]["network"] = json!(network);
                if network == "ws" {
                    config["outbounds"][0]["streamSettings"]["wsSettings"] =
                        json!({"path":"/proxy","headers":{"Host":"example.com"}});
                } else {
                    config["outbounds"][0]["streamSettings"]["grpcSettings"] =
                        json!({"serviceName":"proxy"});
                }
                std::fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
                let result = Command::new(&core)
                    .args(["run", "-test", "-config"])
                    .arg(&path)
                    .output()
                    .unwrap();
                assert!(
                    result.status.success(),
                    "{template:?} {network}: {} {}",
                    String::from_utf8_lossy(&result.stdout),
                    String::from_utf8_lossy(&result.stderr)
                );
            }
        }
    }
}

#[test]
fn focus_selects_later_servers_users_and_accounts_without_changing_neighbors() {
    use eframe::egui;
    for protocol in ["vless", "vmess", "socks", "http"] {
        let ctx = egui::Context::default();
        let key = if matches!(protocol, "vless" | "vmess") {
            "vnext"
        } else {
            "servers"
        };
        let member = if matches!(protocol, "vless" | "vmess") {
            "id"
        } else {
            "user"
        };
        let mut node = json!({"protocol":protocol,"custom":{"keep":true},"settings":{key:[{"address":"first.example","port":443,"custom":1,"users":[{member:"first"}]},{"address":"second.example","port":8443,"custom":2,"users":[{member:"second-first"},{member:"second-second","custom":"preserve"}]}]}});
        let before = node.clone();
        let path = format!("/settings/{key}/1/users/1/{member}");
        form_frame(&ctx, &mut node, vec![], Some(&path), 780.0);
        let modifiers = egui::Modifiers {
            ctrl: true,
            command: true,
            ..Default::default()
        };
        form_frame(
            &ctx,
            &mut node,
            vec![
                egui::Event::Key {
                    key: egui::Key::A,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                },
                egui::Event::Text("replacement".into()),
            ],
            None,
            780.0,
        );
        assert_eq!(
            node.pointer(&path),
            Some(&json!("replacement")),
            "{protocol}"
        );
        let mut expected = before;
        patch(&mut expected, &path, json!("replacement")).unwrap();
        assert_eq!(node, expected, "{protocol}");
    }
}

#[test]
fn geosite_routing_preset_adds_domestic_bypass_and_adblock_rules() {
    use eframe::egui;
    let ctx = egui::Context::default();
    let mut cfg = scene(Scene::Single, Template::VlessReality);
    cfg["outbounds"]
        .as_array_mut()
        .unwrap()
        .push(json!({"tag": "block", "protocol": "blackhole"}));
    let names = BTreeMap::new();
    let mut state = State::default();
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(820.0, 900.0),
            )),
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                super::ui::edit_routes(ui, &mut cfg, &names, &mut state);
            });
        },
    );
    let preset_pos = output
        .shapes
        .iter()
        .find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.text() == "国内外分流预设 (GeoSite)" => {
                Some(s.shape.visual_bounding_rect().center())
            }
            _ => None,
        })
        .expect("preset button visible");
    let _ = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(820.0, 900.0),
            )),
            events: vec![
                egui::Event::PointerMoved(preset_pos),
                egui::Event::PointerButton {
                    pos: preset_pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: preset_pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let outcome = super::ui::edit_routes(ui, &mut cfg, &names, &mut state);
                assert!(outcome.changed);
            });
        },
    );
    let rules = cfg.pointer("/routing/rules").unwrap().as_array().unwrap();
    assert_eq!(rules.len(), 3);
    assert_eq!(rules[0]["domain"], json!(["geosite:private", "geosite:cn"]));
    assert_eq!(rules[1]["domain"], json!(["geosite:category-ads-all"]));
    assert_eq!(rules[2]["ip"], json!(["geoip:private", "geoip:cn"]));
}
