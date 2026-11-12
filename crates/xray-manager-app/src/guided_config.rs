//! Guided edits patch native documents. Unedited fields and array order stay intact.
use serde_json::{Value, json};
use std::collections::BTreeMap;

mod ui;
pub use ui::{State, edit_nodes, edit_routes, node_form, summary};
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scene {
    #[default]
    Single,
    Split,
    Direct,
}
impl Scene {
    pub const ALL: [Self; 3] = [Self::Single, Self::Split, Self::Direct];
    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "单节点代理",
            Self::Split => "按网站分流",
            Self::Direct => "本地直连诊断",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Template {
    #[default]
    VlessReality,
    VlessTls,
    Trojan,
    Shadowsocks,
    Socks,
    Http,
    Vmess,
}
impl Template {
    pub const ALL: [Self; 7] = [
        Self::VlessReality,
        Self::VlessTls,
        Self::Trojan,
        Self::Shadowsocks,
        Self::Socks,
        Self::Http,
        Self::Vmess,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::VlessReality => "VLESS + Reality",
            Self::VlessTls => "VLESS + TLS",
            Self::Trojan => "Trojan + TLS",
            Self::Shadowsocks => "Shadowsocks",
            Self::Socks => "SOCKS 上游",
            Self::Http => "HTTP 上游",
            Self::Vmess => "VMess（兼容）",
        }
    }
    pub fn node(self, tag: &str) -> Value {
        let protocol = match self {
            Self::VlessReality | Self::VlessTls => "vless",
            Self::Trojan => "trojan",
            Self::Shadowsocks => "shadowsocks",
            Self::Socks => "socks",
            Self::Http => "http",
            Self::Vmess => "vmess",
        };
        // No server address, port, credentials or service-specific TLS/Reality values invented.
        let mut node = json!({"tag": tag, "protocol": protocol, "settings": {}});
        node["settings"] = match self {
            Self::VlessReality | Self::VlessTls => {
                json!({"vnext":[{"users":[{"encryption":"none"}]}]})
            }
            Self::Vmess => json!({"vnext":[{"users":[{"security":"auto"}]}]}),
            _ => json!({"servers":[{}]}),
        };
        match self {
            Self::VlessReality => {
                node["streamSettings"] =
                    json!({"network":"tcp", "security":"reality", "realitySettings":{}})
            }
            Self::VlessTls | Self::Trojan => {
                node["streamSettings"] =
                    json!({"network":"tcp", "security":"tls", "tlsSettings":{}})
            }
            _ => {}
        }
        node
    }
}

pub fn scene(scene: Scene, template: Template) -> Value {
    let mut config: Value = serde_json::from_str(&crate::app::default_config()).unwrap();
    config["outbounds"]
        .as_array_mut()
        .unwrap()
        .push(json!({"tag":"blocked", "protocol":"blackhole", "settings":{}}));
    if scene != Scene::Direct {
        config["outbounds"]
            .as_array_mut()
            .unwrap()
            .insert(0, template.node("node-1"));
    }
    // Split starts without example rules that would accidentally affect real traffic.
    config["routing"] = json!({"rules":[]});
    config
}

pub fn node_name(node: &Value, names: &BTreeMap<String, String>) -> String {
    let tag = node.get("tag").and_then(Value::as_str).unwrap_or("未命名");
    names
        .get(tag)
        .filter(|n| !n.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| match node.get("protocol").and_then(Value::as_str) {
            Some("freedom") if tag == "direct" => "直连".into(),
            Some("blackhole") if matches!(tag, "blocked" | "block") => "阻止访问".into(),
            Some("freedom") => format!("直连（{tag}）"),
            Some("blackhole") => format!("阻止访问（{tag}）"),
            _ => tag.into(),
        })
}

pub fn next_tag(config: &Value, names: &BTreeMap<String, String>) -> String {
    for index in 1.. {
        let tag = format!("node-{index}");
        let used = config
            .get("outbounds")
            .and_then(Value::as_array)
            .is_some_and(|a| {
                a.iter()
                    .any(|n| n.get("tag").and_then(Value::as_str) == Some(&tag))
            })
            || config.pointer("/api/tag").and_then(Value::as_str) == Some(&tag)
            || config
                .pointer("/reverse/portals")
                .and_then(Value::as_array)
                .is_some_and(|a| {
                    a.iter()
                        .any(|n| n.get("tag").and_then(Value::as_str) == Some(&tag))
                });
        if !used && !names.contains_key(&tag) {
            return tag;
        }
    }
    unreachable!()
}

/// A missing path can be filled, but malformed ancestors are never overwritten.
/// Transactional: an error leaves the entire document unchanged.
pub fn patch(value: &mut Value, path: &str, replacement: Value) -> Result<(), &'static str> {
    fn put(value: &mut Value, parts: &[&str], replacement: Value) -> Result<(), &'static str> {
        let Some((part, rest)) = parts.split_first() else {
            *value = replacement;
            return Ok(());
        };
        if let Ok(index) = part.parse::<usize>() {
            let array = value
                .as_array_mut()
                .ok_or("父字段不是数组，请在原始 JSON 中修正")?;
            if index > array.len() {
                return Err("条目不存在，请重新选择");
            }
            if index == array.len() {
                array.push(
                    if rest.first().is_some_and(|s| s.parse::<usize>().is_ok()) {
                        json!([])
                    } else {
                        json!({})
                    },
                );
            }
            put(&mut array[index], rest, replacement)
        } else {
            let map = value
                .as_object_mut()
                .ok_or("父字段不是对象，请在原始 JSON 中修正")?;
            let child = map.entry((*part).to_string()).or_insert_with(|| {
                if rest.first().is_some_and(|s| s.parse::<usize>().is_ok()) {
                    json!([])
                } else {
                    json!({})
                }
            });
            put(child, rest, replacement)
        }
    }
    let mut edited = value.clone();
    put(
        &mut edited,
        &path.trim_start_matches('/').split('/').collect::<Vec<_>>(),
        replacement,
    )?;
    *value = edited;
    Ok(())
}

pub fn set_default(config: &mut Value, index: usize) -> bool {
    let Some(nodes) = config.get_mut("outbounds").and_then(Value::as_array_mut) else {
        return false;
    };
    if index == 0 || index >= nodes.len() {
        return false;
    }
    let node = nodes.remove(index);
    nodes.insert(0, node);
    true
}

pub fn references(config: &Value, tag: &str) -> bool {
    // Conservatively include native chaining/balancing/observatory and unknown extensions.
    // The node's own tag is excluded by the caller.
    match config {
        Value::String(s) => s == tag,
        Value::Object(map) => map.values().any(|v| references(v, tag)),
        Value::Array(a) => a.iter().any(|v| references(v, tag)),
        _ => false,
    }
}

pub fn remove_node(config: &mut Value, index: usize) -> Result<(), &'static str> {
    let Some(nodes) = config.get("outbounds").and_then(Value::as_array) else {
        return Err("出站列表不是数组");
    };
    let Some(node) = nodes.get(index) else {
        return Err("节点不存在");
    };
    if index == 0 {
        return Err("请先选择其他默认访问方式，再删除此项");
    }
    let mut remaining = config.clone();
    remaining["outbounds"].as_array_mut().unwrap().remove(index);
    if let Some(tag) = node.get("tag").and_then(Value::as_str)
        && !tag.is_empty()
        && references(&remaining, tag)
    {
        return Err("此节点仍被路由或其他字段引用，请先调整引用");
    }
    *config = remaining;
    Ok(())
}

/// Bare host names in the guided form mean this domain and its subdomains.
/// Native prefixed matchers retain their meaning; incomplete lines remain draft issues.
pub fn domain_lines(text: &str) -> Value {
    Value::Array(
        text.split('\n')
            .map(|line| {
                let line = line.trim();
                Value::String(if line.is_empty() || line.contains(':') {
                    line.into()
                } else {
                    format!("domain:{line}")
                })
            })
            .collect(),
    )
}

pub fn simple_rule(rule: &Value) -> bool {
    let Some(map) = rule.as_object() else {
        return false;
    };
    let domain = rule.get("domain").and_then(Value::as_array);
    let ip = rule.get("ip").and_then(Value::as_array);
    map.keys()
        .all(|k| matches!(k.as_str(), "type" | "domain" | "ip" | "outboundTag"))
        && rule.get("type").and_then(Value::as_str) == Some("field")
        && rule.get("outboundTag").is_some_and(Value::is_string)
        && (domain.is_some() ^ ip.is_some())
        && domain
            .or(ip)
            .is_some_and(|a| a.iter().all(Value::is_string))
        && domain.is_none_or(|a| {
            a.iter()
                .all(|v| v.as_str().is_some_and(|s| s.is_empty() || s.contains(':')))
        })
}
