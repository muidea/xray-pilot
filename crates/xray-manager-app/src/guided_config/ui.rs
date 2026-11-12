use super::*;
use eframe::egui::{self, RichText};

#[derive(Default)]
pub struct State {
    pub template: Template,
    pub focus: Option<String>,
    pub notice: Option<String>,
    pub native: bool,
}
#[derive(Default)]
pub struct Outcome {
    pub changed: bool,
    pub names_changed: bool,
    pub raw: bool,
}

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Required,
    Secret,
    Port,
}

fn field(
    ui: &mut egui::Ui,
    value: &mut Value,
    path: &str,
    label: &str,
    hint: &str,
    kind: Kind,
    focus: Option<&str>,
) -> bool {
    ui.push_id(path, |ui| {
        ui.label(label);
        let current = value.pointer(path);
        if current.is_some_and(|v| !(v.is_string() || matches!(kind, Kind::Port) && v.is_number()))
        {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "原字段类型与表单不同，请使用原生字段编辑；内容已保留。",
            );
            return false;
        }
        let mut text = current
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string())
            })
            .unwrap_or_default();
        let presets: &[&str] = if path.ends_with("/method") {
            &[
                "aes-128-gcm",
                "aes-256-gcm",
                "chacha20-ietf-poly1305",
                "2022-blake3-aes-128-gcm",
                "2022-blake3-aes-256-gcm",
                "2022-blake3-chacha20-poly1305",
            ]
        } else if path.ends_with("/fingerprint") {
            &["chrome", "firefox", "safari", "edge", "randomized"]
        } else if path.ends_with("/flow") {
            &["", "xtls-rprx-vision"]
        } else {
            &[]
        };
        let mut preset_changed = false;
        if !presets.is_empty() {
            ui.menu_button("选择常用值…", |ui| {
                for preset in presets {
                    if ui
                        .button(if preset.is_empty() { "留空" } else { preset })
                        .clicked()
                    {
                        text = (*preset).into();
                        preset_changed = true;
                        ui.close();
                    }
                }
            });
        }
        let show_id = ui.make_persistent_id("show-secret");
        let mut show_secret = ui
            .data_mut(|data| data.get_temp::<bool>(show_id))
            .unwrap_or(false);
        if matches!(kind, Kind::Secret) {
            ui.checkbox(&mut show_secret, "显示认证信息");
            ui.data_mut(|data| data.insert_temp(show_id, show_secret));
        }
        let response = ui.add(
            crate::widgets::singleline(&mut text, f32::INFINITY)
                .hint_text(hint)
                .password(matches!(kind, Kind::Secret) && !show_secret),
        );
        if focus.is_some_and(|f| f == path) {
            response.request_focus();
            response.scroll_to_me(Some(egui::Align::Center));
        }
        let changed = response.changed() || preset_changed;
        if changed {
            let replacement = if matches!(kind, Kind::Port) {
                text.parse::<u64>()
                    .map(Value::from)
                    .unwrap_or_else(|_| Value::String(text.clone()))
            } else {
                Value::String(text.clone())
            };
            if let Err(error) = patch(value, path, replacement) {
                ui.colored_label(ui.visuals().warn_fg_color, error);
                return false;
            }
        }
        if text.trim().is_empty() && !matches!(kind, Kind::Text) {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "待补充：请填写节点提供方给出的信息。",
            );
        } else if matches!(kind, Kind::Port)
            && text.parse::<u16>().ok().filter(|p| *p > 0).is_none()
        {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "端口应为 1–65535 的整数；当前输入作为草稿保留。",
            );
        }
        changed
    })
    .inner
}

fn choice(
    ui: &mut egui::Ui,
    value: &mut Value,
    path: &str,
    label: &str,
    default: &str,
    choices: &[(&str, &str)],
) -> bool {
    ui.label(label);
    if value.pointer(path).is_some_and(|v| !v.is_string()) {
        ui.label("字段类型不符，请使用原始 JSON 修正。");
        return false;
    }
    let original = value
        .pointer(path)
        .and_then(Value::as_str)
        .unwrap_or(default)
        .to_owned();
    let mut selected = original.clone();
    let label = choices
        .iter()
        .find(|(v, _)| *v == selected)
        .map(|(_, l)| *l)
        .unwrap_or(&selected);
    egui::ComboBox::from_id_salt(path)
        .width(ui.available_width().min(420.0))
        .selected_text(label)
        .show_ui(ui, |ui| {
            for (v, label) in choices {
                ui.selectable_value(&mut selected, (*v).into(), *label);
            }
        });
    if original != selected {
        return patch(value, path, Value::String(selected)).is_ok();
    }
    false
}

fn entry_picker(
    ui: &mut egui::Ui,
    node: &mut Value,
    path: &str,
    label: &str,
    seed: Value,
    focus: Option<&str>,
) -> (usize, bool) {
    let id = ui.make_persistent_id(("entry-picker", path));
    let count = node
        .pointer(path)
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let mut selected = ui
        .data_mut(|d| d.get_temp::<usize>(id))
        .unwrap_or(0)
        .min(count.saturating_sub(1));
    if let Some(index) = focus
        .and_then(|f| f.strip_prefix(&format!("{path}/")))
        .and_then(|f| f.split('/').next())
        .and_then(|f| f.parse::<usize>().ok())
    {
        selected = index.min(count.saturating_sub(1));
    }
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt(("entries", path))
            .selected_text(format!("{label} {} / {}", selected + 1, count.max(1)))
            .show_ui(ui, |ui| {
                for index in 0..count {
                    ui.selectable_value(&mut selected, index, format!("{label} {}", index + 1));
                }
            });
        if ui.button(format!("添加{label}")).clicked() {
            if node.pointer(path).is_none() {
                changed |= patch(node, path, json!([])).is_ok();
            }
            if let Some(entries) = node.pointer_mut(path).and_then(Value::as_array_mut) {
                entries.push(seed);
                selected = entries.len() - 1;
                changed = true;
            }
        }
        if ui
            .add_enabled(count > 1, egui::Button::new(format!("删除此{label}")))
            .clicked()
            && let Some(entries) = node.pointer_mut(path).and_then(Value::as_array_mut)
        {
            entries.remove(selected);
            selected = selected.min(entries.len() - 1);
            changed = true;
        }
    });
    ui.data_mut(|d| d.insert_temp(id, selected));
    (selected, changed)
}

pub fn node_form(ui: &mut egui::Ui, node: &mut Value, focus: Option<&str>) -> bool {
    let protocol = node
        .get("protocol")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    if !matches!(
        protocol.as_str(),
        "vless" | "vmess" | "trojan" | "shadowsocks" | "socks" | "http"
    ) {
        ui.label("此协议保留原生编辑方式，可在下方高级编辑中补充。");
        return false;
    }
    let count_path = if matches!(protocol.as_str(), "vless" | "vmess") {
        "/settings/vnext"
    } else {
        "/settings/servers"
    };
    let seed = if protocol == "vless" {
        json!({"users":[{"encryption":"none"}]})
    } else if protocol == "vmess" {
        json!({"users":[{"security":"auto"}]})
    } else {
        json!({})
    };
    let (server_index, mut changed) = entry_picker(ui, node, count_path, "服务器", seed, focus);
    let server = format!("{count_path}/{server_index}");
    let mut user_index = 0;
    if matches!(protocol.as_str(), "vless" | "vmess") {
        let seed = if protocol == "vless" {
            json!({"encryption":"none"})
        } else {
            json!({"security":"auto"})
        };
        let (index, edited) =
            entry_picker(ui, node, &format!("{server}/users"), "用户", seed, focus);
        user_index = index;
        changed |= edited;
    }
    changed |= field(
        ui,
        node,
        &format!("{server}/address"),
        "服务器地址",
        "域名或 IP 地址，不带协议前缀",
        Kind::Required,
        focus,
    );
    changed |= field(
        ui,
        node,
        &format!("{server}/port"),
        "服务器端口",
        "按服务端资料填写",
        Kind::Port,
        focus,
    );
    match protocol.as_str() {
        "vless" | "vmess" => {
            changed |= field(
                ui,
                node,
                &format!("{server}/users/{user_index}/id"),
                "用户 ID / UUID",
                "服务端提供的用户 ID",
                Kind::Secret,
                focus,
            );
        }
        "trojan" | "shadowsocks" => {
            changed |= field(
                ui,
                node,
                &format!("{server}/password"),
                "密码",
                "服务端提供的密码",
                Kind::Secret,
                focus,
            );
            if protocol == "shadowsocks" {
                changed |= field(
                    ui,
                    node,
                    &format!("{server}/method"),
                    "加密方式",
                    "例如：aes-128-gcm；必须与服务端一致",
                    Kind::Required,
                    focus,
                );
            }
        }
        _ => {
            let auth_path = format!("{server}/users");
            let has_auth = node
                .pointer(&auth_path)
                .is_some_and(|a| a.as_array().is_none_or(|a| !a.is_empty()));
            let mut auth = has_auth;
            if ui.checkbox(&mut auth, "上游需要账号密码认证").changed() {
                // Removal is explicit and scoped to this server's authentication array.
                if auth {
                    changed |= patch(node, &auth_path, json!([{}])).is_ok();
                } else if let Some(server) =
                    node.pointer_mut(&server).and_then(Value::as_object_mut)
                {
                    server.remove("users");
                    changed = true;
                }
            }
            if auth {
                let (index, edited) = entry_picker(ui, node, &auth_path, "账号", json!({}), focus);
                user_index = index;
                changed |= edited;
                changed |= field(
                    ui,
                    node,
                    &format!("{server}/users/{user_index}/user"),
                    "用户名",
                    "上游账号",
                    Kind::Required,
                    focus,
                );
                changed |= field(
                    ui,
                    node,
                    &format!("{server}/users/{user_index}/pass"),
                    "密码",
                    "上游密码",
                    Kind::Secret,
                    focus,
                );
            }
        }
    }
    ui.separator();
    ui.label(RichText::new("安全与传输").strong());
    changed |= choice(
        ui,
        node,
        "/streamSettings/security",
        "连接安全",
        "none",
        &[
            ("none", "无额外 TLS / Reality"),
            ("tls", "TLS"),
            ("reality", "Reality"),
        ],
    );
    match node
        .pointer("/streamSettings/security")
        .and_then(Value::as_str)
    {
        Some("tls") => {
            changed |= field(
                ui,
                node,
                "/streamSettings/tlsSettings/serverName",
                "TLS 服务器名称（SNI）",
                "按服务端资料填写；留空时使用服务器地址",
                Kind::Text,
                focus,
            );
            if node
                .pointer("/streamSettings/tlsSettings/allowInsecure")
                .and_then(Value::as_bool)
                == Some(true)
            {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    "原配置关闭了证书验证，请核对安全性。",
                );
                if ui.button("恢复证书验证").clicked() {
                    changed |= patch(
                        node,
                        "/streamSettings/tlsSettings/allowInsecure",
                        json!(false),
                    )
                    .is_ok();
                }
            }
        }
        Some("reality") => {
            changed |= field(
                ui,
                node,
                "/streamSettings/realitySettings/serverName",
                "Reality 服务器名称（SNI）",
                "服务端指定的目标名称",
                Kind::Required,
                focus,
            );
            let key = if node
                .pointer("/streamSettings/realitySettings/password")
                .is_some()
            {
                "password"
            } else {
                "publicKey"
            };
            changed |= field(
                ui,
                node,
                &format!("/streamSettings/realitySettings/{key}"),
                "Reality 公钥",
                "服务端提供的公钥",
                Kind::Required,
                focus,
            );
            changed |= field(
                ui,
                node,
                "/streamSettings/realitySettings/shortId",
                "Short ID",
                "按服务端资料填写；仅在服务端允许时留空",
                Kind::Text,
                focus,
            );
            changed |= field(
                ui,
                node,
                "/streamSettings/realitySettings/fingerprint",
                "客户端指纹",
                "例如 chrome；按节点资料选择",
                Kind::Required,
                focus,
            );
        }
        _ => {}
    }
    changed |= choice(
        ui,
        node,
        "/streamSettings/network",
        "传输方式",
        "tcp",
        &[
            ("tcp", "TCP"),
            ("raw", "RAW"),
            ("ws", "WebSocket"),
            ("grpc", "gRPC"),
            ("httpupgrade", "HTTPUpgrade"),
            ("kcp", "mKCP"),
            ("quic", "QUIC"),
            ("splithttp", "SplitHTTP"),
            ("xhttp", "XHTTP"),
        ],
    );
    match node
        .pointer("/streamSettings/network")
        .and_then(Value::as_str)
    {
        Some("ws") => {
            changed |= field(
                ui,
                node,
                "/streamSettings/wsSettings/path",
                "WebSocket 路径",
                "例如 /；必须与服务端一致",
                Kind::Text,
                focus,
            );
            changed |= field(
                ui,
                node,
                "/streamSettings/wsSettings/headers/Host",
                "WebSocket Host",
                "按服务端资料填写",
                Kind::Text,
                focus,
            );
        }
        Some("grpc") => {
            changed |= field(
                ui,
                node,
                "/streamSettings/grpcSettings/serviceName",
                "gRPC 服务名称",
                "必须与服务端一致",
                Kind::Text,
                focus,
            );
        }
        Some("httpupgrade") => {
            changed |= field(
                ui,
                node,
                "/streamSettings/httpupgradeSettings/path",
                "HTTPUpgrade 路径",
                "例如 /；必须与服务端一致",
                Kind::Text,
                focus,
            );
            changed |= field(
                ui,
                node,
                "/streamSettings/httpupgradeSettings/host",
                "HTTPUpgrade Host",
                "按服务端资料填写",
                Kind::Text,
                focus,
            );
        }
        Some("kcp") => {
            changed |= field(
                ui,
                node,
                "/streamSettings/kcpSettings/seed",
                "mKCP 种子",
                "与服务端一致；留空使用默认",
                Kind::Text,
                focus,
            );
        }
        Some("quic") => {
            changed |= field(
                ui,
                node,
                "/streamSettings/quicSettings/security",
                "QUIC 安全",
                "none 或服务端指定值",
                Kind::Text,
                focus,
            );
            changed |= field(
                ui,
                node,
                "/streamSettings/quicSettings/key",
                "QUIC 密钥",
                "按服务端资料填写",
                Kind::Secret,
                focus,
            );
        }
        Some("splithttp" | "xhttp") => {
            let prefix = if node
                .pointer("/streamSettings/network")
                .and_then(Value::as_str)
                == Some("splithttp")
            {
                "/streamSettings/splithttpSettings"
            } else {
                "/streamSettings/xhttpSettings"
            };
            changed |= field(
                ui,
                node,
                &format!("{prefix}/path"),
                "HTTP 路径",
                "例如 /；必须与服务端一致",
                Kind::Text,
                focus,
            );
            changed |= field(
                ui,
                node,
                &format!("{prefix}/host"),
                "HTTP Host",
                "按服务端资料填写",
                Kind::Text,
                focus,
            );
        }
        _ => {}
    }
    egui::CollapsingHeader::new("高级选项").open(focus.filter(|f| f.starts_with(&format!("{server}/users/{user_index}/")) && !f.ends_with("/id")).map(|_| true)).show(ui, |ui| {
        if protocol == "vless" {
            changed |= field(ui, node, &format!("{server}/users/{user_index}/flow"), "Flow", "按服务端资料填写；需要时使用 xtls-rprx-vision", Kind::Text, focus);
            changed |= field(ui, node, &format!("{server}/users/{user_index}/encryption"), "VLESS 加密", "通常为 none；以服务端资料为准", Kind::Required, focus);
        }
        if protocol == "vmess" {
            changed |= field(ui, node, &format!("{server}/users/{user_index}/security"), "VMess 加密", "auto 或服务端指定值", Kind::Text, focus);
        }
        ui.label(RichText::new("其他字段可在下方高级编辑中修改。更改安全或传输方式会保留原有参数，请检查预览后应用。").small().weak());
    });
    changed
}

fn targets(config: &Value, names: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let mut targets: Vec<_> = config
        .get("outbounds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|n| {
            let tag = n.get("tag").and_then(Value::as_str)?.to_owned();
            let name = node_name(n, names);
            Some((tag, name))
        })
        .collect();
    if let Some(tag) = config.pointer("/api/tag").and_then(Value::as_str) {
        targets.push((tag.into(), format!("API · {tag}")));
    }
    if let Some(portals) = config.pointer("/reverse/portals").and_then(Value::as_array) {
        for portal in portals {
            if let Some(tag) = portal.get("tag").and_then(Value::as_str) {
                targets.push((tag.into(), format!("内部反向代理 · {tag}")));
            }
        }
    }
    targets
}

fn default_picker(ui: &mut egui::Ui, config: &mut Value, names: &BTreeMap<String, String>) -> bool {
    let Some(nodes) = config.get("outbounds").and_then(Value::as_array) else {
        return false;
    };
    let mut selected = 0;
    ui.horizontal_wrapped(|ui| {
        ui.label("默认访问方式")
            .on_hover_text("未命中分流规则时采用此方式");
        egui::ComboBox::from_id_salt("default-node")
            .width(ui.available_width().min(420.0))
            .selected_text(
                nodes
                    .first()
                    .map(|n| {
                        format!(
                            "{} · {}",
                            node_name(n, names),
                            n["protocol"].as_str().unwrap_or("未知协议")
                        )
                    })
                    .unwrap_or_else(|| "请添加节点".into()),
            )
            .show_ui(ui, |ui| {
                for (index, node) in nodes.iter().enumerate() {
                    ui.selectable_value(
                        &mut selected,
                        index,
                        format!(
                            "{} · {}",
                            node_name(node, names),
                            node["protocol"].as_str().unwrap_or("未知协议")
                        ),
                    );
                }
            });
    });
    set_default(config, selected)
}

pub fn edit_nodes(
    ui: &mut egui::Ui,
    config: &mut Value,
    names: &mut BTreeMap<String, String>,
    state: &mut State,
) -> Outcome {
    let mut out = Outcome::default();
    let focus = state.focus.take();
    out.changed |= default_picker(ui, config, names);
    ui.separator();
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_id_salt("node-template")
            .selected_text(state.template.label())
            .show_ui(ui, |ui| {
                for template in Template::ALL {
                    ui.selectable_value(&mut state.template, template, template.label());
                }
            });
        if ui.button("添加节点").clicked() {
            let tag = next_tag(config, names);
            if config.get("outbounds").is_none() {
                out.changed |= patch(config, "/outbounds", json!([])).is_ok();
            }
            if let Some(nodes) = config.get_mut("outbounds").and_then(Value::as_array_mut) {
                nodes.push(state.template.node(&tag));
                names.insert(tag, format!("{} 节点", state.template.label()));
                let group = if matches!(
                    state.template,
                    Template::VlessReality | Template::VlessTls | Template::Vmess
                ) {
                    "vnext"
                } else {
                    "servers"
                };
                state.focus = Some(format!(
                    "/outbounds/{}/settings/{group}/0/address",
                    nodes.len() - 1
                ));
                out.changed = true;
                out.names_changed = true;
            } else {
                state.notice = Some("出站字段不是数组，请在原始 JSON 中修正。".into());
            }
        }
    });
    ui.label(
        RichText::new("按服务端资料选择模板；地址、端口和认证信息需自行补充。所有修改仅影响草稿。")
            .small()
            .weak(),
    );
    let mut remove = None;
    if let Some(nodes) = config.get_mut("outbounds").and_then(Value::as_array_mut) {
        for (index, node) in nodes.iter_mut().enumerate() {
            let tag = node
                .get("tag")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let protocol = node
                .get("protocol")
                .and_then(Value::as_str)
                .unwrap_or("未知协议")
                .to_owned();
            let title = format!(
                "{} · {}{} · {}",
                node_name(node, names),
                protocol,
                if index == 0 { " · 默认" } else { "" },
                node.pointer("/settings/vnext/0")
                    .or_else(|| node.pointer("/settings/servers/0"))
                    .map(|server| format!(
                        "{}:{}",
                        server["address"].as_str().unwrap_or("待补充地址"),
                        server
                            .get("port")
                            .map(Value::to_string)
                            .unwrap_or_else(|| "待补充端口".into())
                    ))
                    .unwrap_or_else(|| "本地处理 / 原生配置".into())
            );
            let prefix = format!("/outbounds/{index}");
            let local_focus = focus
                .as_deref()
                .and_then(|f| f.strip_prefix(&format!("{prefix}/")))
                .map(|f| format!("/{f}"));
            let local_focus = local_focus.map(|path| {
                if path.ends_with("/vnext") || path.ends_with("/servers") {
                    format!("{path}/0/address")
                } else if path.ends_with("/users") {
                    format!("{path}/0/id")
                } else {
                    path
                }
            });
            let targeted = focus
                .as_deref()
                .is_some_and(|f| f == prefix || f.starts_with(&format!("{prefix}/")));
            if targeted
                && local_focus
                    .as_deref()
                    .is_some_and(|path| path == "/protocol" || path == "/tag")
            {
                out.raw = true;
            }
            ui.push_id((&tag, index), |ui| {
                egui::CollapsingHeader::new(title)
                    .id_salt("node-card")
                    .open(if targeted { Some(true) } else { None })
                    .show(ui, |ui| {
                        if !tag.is_empty() {
                            let mut name = node_name(node, names);
                            ui.label("节点显示名称");
                            if ui
                                .add(crate::widgets::singleline(&mut name, f32::INFINITY))
                                .changed()
                            {
                                names.insert(tag.clone(), name.chars().take(80).collect());
                                out.names_changed = true;
                            }
                        }
                        out.changed |= node_form(ui, node, local_focus.as_deref());
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("查看原生字段").clicked() {
                                out.raw = true;
                            }
                            if ui
                                .add_enabled(index != 0, egui::Button::new("删除节点"))
                                .clicked()
                            {
                                remove = Some(index);
                            }
                        });
                    });
            });
        }
    } else {
        ui.label("出站字段不是数组，可切换到原始 JSON 修正。");
    }
    if let Some(index) = remove {
        match remove_node(config, index) {
            Ok(()) => {
                out.changed = true;
                state.notice = None;
            }
            Err(error) => state.notice = Some(error.into()),
        }
    }
    out
}

pub fn edit_routes(
    ui: &mut egui::Ui,
    config: &mut Value,
    names: &BTreeMap<String, String>,
    state: &mut State,
) -> Outcome {
    let mut out = Outcome::default();
    out.changed |= default_picker(ui, config, names);
    let targets = targets(config, names);
    let focus = state.focus.take();
    ui.label(RichText::new("规则从上到下匹配，先命中的规则生效。重复或重叠规则请核对顺序；未命中时使用默认访问方式。").small().weak());
    ui.horizontal_wrapped(|ui| {
        for (label, protocol) in [
            ("指定网站走代理", "proxy"),
            ("指定网站直连", "freedom"),
            ("阻止访问", "blackhole"),
        ] {
            if ui.button(label).clicked() {
                let target = config
                    .get("outbounds")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|n| {
                        let p = n.get("protocol").and_then(Value::as_str).unwrap_or("");
                        if protocol == "proxy" {
                            !matches!(p, "freedom" | "blackhole")
                        } else {
                            p == protocol
                        }
                    })
                    .and_then(|n| n.get("tag"))
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                if let Some(tag) = target {
                    if config.pointer("/routing/rules").is_none() {
                        out.changed |= patch(config, "/routing/rules", json!([])).is_ok();
                    }
                    if let Some(rules) = config
                        .pointer_mut("/routing/rules")
                        .and_then(Value::as_array_mut)
                    {
                        rules.push(json!({"type":"field","domain":[],"outboundTag":tag}));
                        out.changed = true;
                        state.focus = Some(format!("/routing/rules/{}/domain", rules.len() - 1));
                    } else {
                        state.notice = Some("路由规则字段不是数组，请使用原始 JSON 修正。".into());
                    }
                } else {
                    state.notice = Some("请先添加对应的代理节点、直连或阻止出站。".into());
                }
            }
        }
    });
    let mut action = None;
    if let Some(rules) = config
        .pointer_mut("/routing/rules")
        .and_then(Value::as_array_mut)
    {
        for (index, rule) in rules.iter_mut().enumerate() {
            ui.push_id(index, |ui| {
                ui.separator();
                let simple = simple_rule(rule);
                let original_kind = if rule.get("domain").is_some() { "domain" } else { "ip" };
                let mut match_kind = original_kind.to_owned();
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("规则 {}", index + 1)).strong());
                    if simple {
                        egui::ComboBox::from_id_salt("match-kind").selected_text(if match_kind == "domain" { "网站 / 域名" } else { "IP / CIDR" }).show_ui(ui, |ui| {
                            ui.selectable_value(&mut match_kind, "domain".into(), "网站 / 域名");
                            ui.selectable_value(&mut match_kind, "ip".into(), "IP / CIDR");
                        });
                    }
                    if ui.add_enabled(index > 0, egui::Button::new("上移")).clicked() { action = Some((index,0)); }
                    if ui.button("下移").clicked() { action = Some((index,1)); }
                    if ui.button("删除规则").clicked() { action = Some((index,2)); }
                });
                if simple {
                    if match_kind != original_kind {
                        let value = rule.as_object_mut().unwrap().remove(original_kind).unwrap();
                        rule[&match_kind] = if match_kind == "domain" {
                            domain_lines(&value.as_array().unwrap().iter().filter_map(Value::as_str).collect::<Vec<_>>().join("\n"))
                        } else { value };
                        out.changed = true;
                    }
                    let path = if match_kind == "domain" { "/domain" } else { "/ip" };
                    ui.label(if path == "/domain" { "匹配网站 / 域名（每行一项）" } else { "匹配 IP / CIDR（每行一项）" }).on_hover_text(if path == "/domain" { "输入 example.com 匹配此域及子域；精确匹配可填 full:example.com。也支持原生前缀；geosite / geoip 需要相应资源。" } else { "例如 192.0.2.0/24；也支持原生 geoip: 写法。" });
                    let mut text = rule.pointer(path).and_then(Value::as_array).unwrap().iter().filter_map(Value::as_str).map(|s| if path == "/domain" { s.strip_prefix("domain:").unwrap_or(s) } else { s }).collect::<Vec<_>>().join("\n");
                    let response = ui.add(egui::TextEdit::multiline(&mut text).desired_width(f32::INFINITY).desired_rows(3));
                    if focus.as_deref().is_some_and(|f| f.starts_with(&format!("/routing/rules/{index}/"))) { response.scroll_to_me(Some(egui::Align::Center)); response.request_focus(); }
                    if response.changed() {
                        rule[path.trim_start_matches('/')] = if path == "/domain" { domain_lines(&text) } else { Value::Array(text.split('\n').map(|line| Value::String(line.trim().to_owned())).collect()) };
                        out.changed = true;
                    }
                    if text.trim().is_empty() || text.lines().any(|s| s.trim().is_empty()) { ui.colored_label(ui.visuals().warn_fg_color, "请补充匹配项，移除空行后检查。"); }
                    out.changed |= choice(ui, rule, "/outboundTag", "处理方式", "", &targets.iter().map(|(tag,name)| (tag.as_str(),name.as_str())).collect::<Vec<_>>());
                } else {
                    if focus.as_deref().is_some_and(|f| f.starts_with(&format!("/routing/rules/{index}"))) { out.raw = true; }
                    ui.label("此规则包含组合条件、负载均衡或其他原生字段；内容与顺序保留，请使用高级编辑。");
                    if ui.button("编辑原生规则").clicked() { out.raw = true; }
                }
            });
        }
        if let Some((index, kind)) = action {
            match kind {
                0 => {
                    rules.swap(index, index - 1);
                    out.changed = true;
                }
                1 if index + 1 < rules.len() => {
                    rules.swap(index, index + 1);
                    out.changed = true;
                }
                2 => {
                    rules.remove(index);
                    out.changed = true;
                }
                _ => {}
            }
        }
        for index in 0..rules.len() {
            if simple_rule(&rules[index])
                && rules[..index].iter().any(|r| {
                    simple_rule(r)
                        && r.get("domain") == rules[index].get("domain")
                        && r.get("ip") == rules[index].get("ip")
                })
            {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "规则 {} 的匹配条件与前面相同，可能无法生效；请核对处理方式和顺序。",
                        index + 1
                    ),
                );
            }
        }
    } else if config.pointer("/routing/rules").is_some() {
        ui.label("路由规则不是数组，请使用原始 JSON 修正。");
    }
    out
}

pub fn summary(ui: &mut egui::Ui, config: &Value, names: &BTreeMap<String, String>) {
    let default = config
        .pointer("/outbounds/0")
        .map(|n| {
            format!(
                "{} · {}",
                node_name(n, names),
                n["protocol"].as_str().unwrap_or("未知协议")
            )
        })
        .unwrap_or_else(|| "未显式设置，请核对 Core 默认行为".into());
    ui.label(format!("默认访问：{default}"));
    let rules = config
        .pointer("/routing/rules")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    ui.label(format!("分流规则：{rules} 条，按顺序匹配"));
    if let Some(inbounds) = config.get("inbounds").and_then(Value::as_array) {
        for inbound in inbounds {
            ui.add(
                egui::Label::new(format!(
                    "本地入口：{} · {}:{}",
                    inbound["protocol"].as_str().unwrap_or("未知协议"),
                    inbound["listen"]
                        .as_str()
                        .unwrap_or("使用 Core 默认监听地址"),
                    inbound
                        .get("port")
                        .map(Value::to_string)
                        .unwrap_or_else(|| "待补充".into())
                ))
                .wrap(),
            );
        }
    }
}
