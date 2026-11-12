use serde_json::Value;
use xray_model::{RouteContext, RouteResult};

#[derive(Clone, Copy, PartialEq)]
enum Match {
    Yes,
    No,
    Unknown,
}
fn and(a: Match, b: Match) -> Match {
    if a == Match::No || b == Match::No {
        Match::No
    } else if a == Match::Unknown || b == Match::Unknown {
        Match::Unknown
    } else {
        Match::Yes
    }
}
fn domain(pattern: &str, host: &str) -> Match {
    let host = host.to_ascii_lowercase();
    let p = pattern.to_ascii_lowercase();
    if host.is_empty() {
        return Match::Unknown;
    }
    if let Some(p) = p.strip_prefix("full:") {
        return if host == p { Match::Yes } else { Match::No };
    }
    if let Some(p) = p.strip_prefix("domain:") {
        return if host == p || host.ends_with(&format!(".{p}")) {
            Match::Yes
        } else {
            Match::No
        };
    }
    if p.contains(':') {
        return Match::Unknown;
    }
    if host.contains(&p) {
        Match::Yes
    } else {
        Match::No
    }
}
fn any(values: &Value, mut check: impl FnMut(&str) -> Match) -> Match {
    let Some(values) = values.as_array() else {
        return Match::Unknown;
    };
    let mut result = Match::No;
    for v in values {
        let m = v.as_str().map(&mut check).unwrap_or(Match::Unknown);
        if m == Match::Yes {
            return m;
        }
        if m == Match::Unknown {
            result = m;
        }
    }
    result
}
fn ip_match(pattern: &str, target: Option<&str>) -> Match {
    let Some(ip) = target.and_then(|s| s.parse::<std::net::IpAddr>().ok()) else {
        return Match::Unknown;
    };
    let (address, bits) = if let Some((address, prefix)) = pattern.split_once('/') {
        let Ok(bits) = prefix.parse::<u32>() else {
            return Match::Unknown;
        };
        (address, Some(bits))
    } else {
        (pattern, None)
    };
    let Ok(network) = address.parse::<std::net::IpAddr>() else {
        return Match::Unknown;
    };
    let matched = match (ip, network) {
        (std::net::IpAddr::V4(a), std::net::IpAddr::V4(b)) => {
            let bits = bits.unwrap_or(32);
            if bits > 32 {
                return Match::Unknown;
            }
            let mask = u32::MAX.checked_shl(32 - bits).unwrap_or(0);
            u32::from(a) & mask == u32::from(b) & mask
        }
        (std::net::IpAddr::V6(a), std::net::IpAddr::V6(b)) => {
            let bits = bits.unwrap_or(128);
            if bits > 128 {
                return Match::Unknown;
            }
            let mask = u128::MAX.checked_shl(128 - bits).unwrap_or(0);
            u128::from(a) & mask == u128::from(b) & mask
        }
        _ => false,
    };
    if matched { Match::Yes } else { Match::No }
}
fn port_match(value: &Value, port: u16) -> Match {
    let text = value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string());
    let mut unknown = false;
    for spec in text.split(',') {
        let bounds = spec
            .trim()
            .split_once('-')
            .map(|(a, b)| (a.parse::<u16>(), b.parse::<u16>()))
            .unwrap_or_else(|| (spec.trim().parse(), spec.trim().parse()));
        match bounds {
            (Ok(a), Ok(b)) if a <= b => {
                if (a..=b).contains(&port) {
                    return Match::Yes;
                }
            }
            _ => unknown = true,
        }
    }
    if unknown { Match::Unknown } else { Match::No }
}
pub fn simulate(config: &Value, context: &RouteContext) -> RouteResult {
    let result = |state: &str, index, outbound, reason: String| RouteResult {
        kind: "StaticSimulation".into(),
        match_state: state.into(),
        rule_index: index,
        expected_outbound: outbound,
        actual_outbound: "Unverified".into(),
        reason,
    };
    if config
        .get("inbounds")
        .and_then(Value::as_array)
        .is_some_and(|inbounds| {
            inbounds.iter().any(|inbound| {
                inbound
                    .pointer("/sniffing/enabled")
                    .and_then(Value::as_bool)
                    == Some(true)
                    && context
                        .inbound_tag
                        .as_deref()
                        .is_none_or(|tag| inbound.get("tag").and_then(Value::as_str) == Some(tag))
            })
        })
    {
        return result(
            "Unknown",
            None,
            None,
            "Sniffing may change the routing destination; sniffed context is unavailable".into(),
        );
    }
    if let Some(rules) = config.pointer("/routing/rules").and_then(Value::as_array) {
        for (index, rule) in rules.iter().enumerate() {
            let Some(map) = rule.as_object() else {
                return result("Unknown", Some(index), None, "Invalid rule".into());
            };
            let mut matched = Match::Yes;
            for (key, value) in map {
                let m = match key.as_str() {
                    "type" if value.as_str() == Some("field") => Match::Yes,
                    "outboundTag" | "ruleTag" => Match::Yes,
                    "domain" => any(value, |s| domain(s, &context.host)),
                    "inboundTag" => context
                        .inbound_tag
                        .as_deref()
                        .map(|tag| any(value, |s| if s == tag { Match::Yes } else { Match::No }))
                        .unwrap_or(Match::Unknown),
                    "network" => context
                        .network
                        .as_deref()
                        .zip(value.as_str())
                        .map(|(network, s)| {
                            if s.split(',').any(|v| v.trim() == network) {
                                Match::Yes
                            } else {
                                Match::No
                            }
                        })
                        .unwrap_or(Match::Unknown),
                    "port" => context
                        .port
                        .map(|port| port_match(value, port))
                        .unwrap_or(Match::Unknown),
                    "ip" => any(value, |s| ip_match(s, context.target_ip.as_deref())),
                    "source" => context
                        .source_ip
                        .as_deref()
                        .map(|source| any(value, |s| ip_match(s, Some(source))))
                        .unwrap_or(Match::Unknown),
                    "sourcePort" => context
                        .source_port
                        .map(|port| port_match(value, port))
                        .unwrap_or(Match::Unknown),
                    "user" => context
                        .user
                        .as_deref()
                        .map(|user| any(value, |s| if s == user { Match::Yes } else { Match::No }))
                        .unwrap_or(Match::Unknown),
                    "protocol" => context
                        .protocol
                        .as_deref()
                        .map(|protocol| {
                            any(value, |s| {
                                if s.eq_ignore_ascii_case(protocol) {
                                    Match::Yes
                                } else {
                                    Match::No
                                }
                            })
                        })
                        .unwrap_or(Match::Unknown),
                    "attrs" => context
                        .attrs
                        .as_ref()
                        .map(|attrs| match value.as_object() {
                            Some(expected) => {
                                expected.iter().fold(Match::Yes, |state, (key, value)| {
                                    let Some(value) = value.as_str() else {
                                        return Match::Unknown;
                                    };
                                    and(
                                        state,
                                        match attrs.get(key) {
                                            Some(actual) if actual == value => Match::Yes,
                                            Some(_) => Match::No,
                                            None => Match::Unknown,
                                        },
                                    )
                                })
                            }
                            None => Match::Unknown,
                        })
                        .unwrap_or(Match::Unknown),
                    _ => Match::Unknown,
                };
                matched = and(matched, m);
            }
            if matched == Match::No {
                continue;
            }
            if matched == Match::Unknown {
                return result(
                    "Unknown",
                    Some(index),
                    None,
                    "Earlier rule has unsupported fields or missing context".into(),
                );
            }
            let outbound = rule
                .get("outboundTag")
                .and_then(Value::as_str)
                .map(str::to_owned);
            return result(
                if outbound.is_some() {
                    "MatchKnown"
                } else {
                    "Unknown"
                },
                Some(index),
                outbound,
                "Static prediction only".into(),
            );
        }
    }
    let outbound = config
        .pointer("/outbounds/0/tag")
        .and_then(Value::as_str)
        .map(str::to_owned);
    result(
        if outbound.is_some() {
            "DefaultKnown"
        } else {
            "Unknown"
        },
        None,
        outbound,
        "First native outbound is the default; actual exit is unverified".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_earlier_rule_blocks_default_prediction() {
        let config = serde_json::json!({"outbounds":[{"tag":"a"}],"routing":{"rules":[{"inboundTag":["private"],"outboundTag":"b"},{"domain":["full:example.com"],"outboundTag":"a"}]}});
        let context = RouteContext {
            host: "example.com".into(),
            ..Default::default()
        };
        assert_eq!(simulate(&config, &context).match_state, "Unknown");
        let context = RouteContext {
            inbound_tag: Some("public".into()),
            ..context
        };
        assert_eq!(
            simulate(&config, &context).expected_outbound.as_deref(),
            Some("a")
        );
    }
    #[test]
    fn malformed_cidr_and_sniffing_do_not_claim_a_known_route() {
        let mut config = serde_json::json!({"outbounds":[{"tag":"default"}],"routing":{"rules":[{"ip":["127.0.0.1/bad"],"outboundTag":"other"}]}});
        let context = RouteContext {
            target_ip: Some("127.0.0.1".into()),
            ..Default::default()
        };
        assert_eq!(simulate(&config, &context).match_state, "Unknown");
        config["routing"]["rules"] = serde_json::json!([]);
        config["inbounds"] = serde_json::json!([{"tag":"http-in","sniffing":{"enabled":true}}]);
        assert_eq!(simulate(&config, &context).match_state, "Unknown");
    }
}

#[cfg(test)]
mod extended_tests {
    use super::*;
    fn matches(rule: serde_json::Value, context: RouteContext) -> Option<String> {
        let config = serde_json::json!({"outbounds":[{"tag":"a"}],"routing":{"rules":[rule]}});
        simulate(&config, &context).expected_outbound
    }
    #[test]
    fn source_user_protocol_and_attrs_are_matched_or_unknown() {
        let base = RouteContext {
            host: "example.com".into(),
            ..Default::default()
        };
        assert_eq!(simulate(&serde_json::json!({"outbounds":[{"tag":"a"}],"routing":{"rules":[{"type":"field","source":["10.0.0.0/8"],"outboundTag":"a"}]}}), &base).match_state, "Unknown");
        assert_eq!(
            matches(
                serde_json::json!({"type":"field","source":["10.0.0.0/8"],"outboundTag":"a"}),
                RouteContext {
                    source_ip: Some("10.1.2.3".into()),
                    ..base.clone()
                }
            ),
            Some("a".into())
        );
        assert_eq!(
            matches(
                serde_json::json!({"type":"field","sourcePort":"1000-2000","outboundTag":"a"}),
                RouteContext {
                    source_port: Some(1500),
                    ..base.clone()
                }
            ),
            Some("a".into())
        );
        assert_eq!(
            matches(
                serde_json::json!({"type":"field","user":["alice"],"outboundTag":"a"}),
                RouteContext {
                    user: Some("alice".into()),
                    ..base.clone()
                }
            ),
            Some("a".into())
        );
        assert_eq!(
            matches(
                serde_json::json!({"type":"field","protocol":["http"],"outboundTag":"a"}),
                RouteContext {
                    protocol: Some("HTTP".into()),
                    ..base.clone()
                }
            ),
            Some("a".into())
        );
        assert_eq!(
            matches(
                serde_json::json!({"type":"field","attrs":{"v":"1"},"outboundTag":"a"}),
                RouteContext {
                    attrs: Some(std::collections::BTreeMap::from([("v".into(), "1".into())])),
                    ..base
                }
            ),
            Some("a".into())
        );
    }
}
