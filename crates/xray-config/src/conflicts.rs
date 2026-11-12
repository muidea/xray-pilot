//! Conservative static conflict analysis for native routing rules.
//!
//! The analyzer never changes rule order. It reports cases where a later rule
//! cannot be proven reachable or where a DIRECT decision may be shadowed by an
//! earlier native rule. Unknown patterns are treated as possibly overlapping.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteConflict {
    /// 1-based rule number in the native list.
    pub index: usize,
    pub kind: String,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Direct,
    Proxy,
    Block,
}

fn action(rule: &Value, protocols: &std::collections::BTreeMap<String, String>) -> Action {
    if rule.get("balancerTag").is_some() {
        return Action::Proxy;
    }
    let tag = rule.get("outboundTag").and_then(Value::as_str);
    match tag.and_then(|tag| protocols.get(tag)).map(String::as_str) {
        Some("freedom") => Action::Direct,
        Some("blackhole") => Action::Block,
        _ => Action::Proxy,
    }
}

fn has_matcher(rule: &Value) -> bool {
    [
        "domain",
        "ip",
        "port",
        "sourcePort",
        "network",
        "source",
        "user",
        "inboundTag",
        "protocol",
        "attrs",
    ]
    .iter()
    .any(|key| rule.get(*key).is_some())
}

fn only_domain(rule: &Value) -> bool {
    rule.get("domain").is_some()
        && [
            "ip",
            "port",
            "sourcePort",
            "network",
            "source",
            "user",
            "inboundTag",
            "protocol",
            "attrs",
        ]
        .iter()
        .all(|key| rule.get(*key).is_none())
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Pattern {
    Full(String),
    Suffix(String),
    Substring(String),
    Unknown,
}

fn pattern(value: &str) -> Pattern {
    let value = value.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Pattern::Unknown;
    }
    if let Some(value) = value.strip_prefix("full:") {
        return Pattern::Full(value.into());
    }
    if let Some(value) = value.strip_prefix("domain:") {
        return Pattern::Suffix(value.trim_start_matches('.').into());
    }
    if value.contains(':') {
        return Pattern::Unknown;
    }
    Pattern::Substring(value)
}

fn overlaps(a: &Pattern, b: &Pattern) -> bool {
    use Pattern::*;
    match (a, b) {
        (Unknown, _) | (_, Unknown) => true,
        (Full(a), Full(b)) => a == b,
        (Full(a), Suffix(b)) | (Suffix(b), Full(a)) => a == b || a.ends_with(&format!(".{b}")),
        (Suffix(a), Suffix(b)) => {
            a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}"))
        }
        (Substring(a), Substring(b)) => a.contains(b) || b.contains(a),
        (Substring(a), Full(b)) | (Full(b), Substring(a)) => b.contains(a),
        (Substring(a), Suffix(b)) | (Suffix(b), Substring(a)) => b.contains(a),
    }
}

fn domains(rule: &Value) -> Vec<Pattern> {
    rule.get("domain")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(pattern)
        .collect()
}

fn any_overlap(a: &[Pattern], b: &[Pattern]) -> bool {
    a.iter().any(|a| b.iter().any(|b| overlaps(a, b)))
}

/// Return static conflicts without claiming that a rule actually matches.
pub fn conflicts(config: &Value) -> Vec<RouteConflict> {
    let protocols: std::collections::BTreeMap<String, String> = config
        .get("outbounds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|outbound| {
            Some((
                outbound.get("tag")?.as_str()?.to_owned(),
                outbound.get("protocol")?.as_str()?.to_owned(),
            ))
        })
        .collect();
    let rules = config
        .pointer("/routing/rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut found = vec![];
    let mut catch_all: Option<usize> = None;
    for (index, rule) in rules.iter().enumerate() {
        if let Some(catch) = catch_all {
            found.push(RouteConflict {
                index: index + 1,
                kind: "unreachable".into(),
                message: format!(
                    "规则 {} 位于规则 {} 的全量匹配之后，静态上不可达；请确认顺序",
                    index + 1,
                    catch + 1
                ),
            });
            continue;
        }
        let current_action = action(rule, &protocols);
        let current_domains = domains(rule);
        for (earlier, other) in rules[..index].iter().enumerate() {
            let other_action = action(other, &protocols);
            if current_action != Action::Direct || other_action == Action::Direct {
                continue;
            }
            let other_domains = domains(other);
            let shadowed = if other_domains.is_empty() {
                // An earlier non-domain proxy/block rule may match the same host.
                !only_domain(other)
            } else {
                any_overlap(&current_domains, &other_domains)
            };
            if shadowed {
                found.push(RouteConflict {
                    index: index + 1,
                    kind: "direct-shadowed".into(),
                    message: format!(
                        "规则 {} 的直连可能被规则 {} 的代理/阻断条件遮蔽；静态无法证明不相交",
                        index + 1,
                        earlier + 1
                    ),
                });
                break;
            }
        }
        if !has_matcher(rule) {
            catch_all = Some(index);
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn direct_after_overlapping_proxy_or_non_domain_rule_is_reported() {
        let config = json!({
            "outbounds": [
                {"tag":"proxy","protocol":"vless"},
                {"tag":"direct","protocol":"freedom"}
            ],
            "routing":{"rules":[
                {"type":"field","domain":["domain:example.com"],"outboundTag":"proxy"},
                {"type":"field","domain":["full:a.example.com"],"outboundTag":"direct"},
                {"type":"field","ip":["10.0.0.0/8"],"outboundTag":"proxy"},
                {"type":"field","domain":["full:other.example"],"outboundTag":"direct"}
            ]}
        });
        let found = conflicts(&config);
        assert!(found.iter().any(|c| c.index == 2));
        assert!(found.iter().any(|c| c.index == 4));
    }
    #[test]
    fn catch_all_makes_later_rules_unreachable_but_disjoint_rules_do_not_conflict() {
        let config = json!({
            "outbounds":[{"tag":"proxy","protocol":"vless"},{"tag":"direct","protocol":"freedom"}],
            "routing":{"rules":[
                {"type":"field","domain":["full:one.example"],"outboundTag":"direct"},
                {"type":"field","domain":["full:two.example"],"outboundTag":"proxy"},
                {"type":"field","outboundTag":"proxy"},
                {"type":"field","domain":["full:three.example"],"outboundTag":"direct"}
            ]}
        });
        let found = conflicts(&config);
        assert!(
            found
                .iter()
                .any(|c| c.index == 4 && c.kind == "unreachable")
        );
        assert!(!found.iter().any(|c| c.index == 2));
    }
}
