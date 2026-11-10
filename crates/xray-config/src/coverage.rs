//! Conservative dependency coverage for automatic recovery. Manual Core validation remains broader.
use serde_json::Value;

pub fn uncovered(value: &Value) -> Vec<String> {
    fn walk(value: &Value, path: &str, notes: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                let keys: &[&str] = match path {
                    "" => &["log", "inbounds", "outbounds", "routing"],
                    "/log" => &["loglevel", "dnsLog", "maskAddress"],
                    "/inbounds/[]" => {
                        &["tag", "listen", "port", "protocol", "settings", "sniffing"]
                    }
                    "/inbounds/[]/settings" => &[
                        "auth",
                        "accounts",
                        "udp",
                        "ip",
                        "userLevel",
                        "allowTransparent",
                    ],
                    "/inbounds/[]/settings/accounts/[]" => &["user", "pass"],
                    "/inbounds/[]/sniffing" => &[
                        "enabled",
                        "destOverride",
                        "metadataOnly",
                        "routeOnly",
                        "domainsExcluded",
                    ],
                    "/outbounds/[]" => &[
                        "tag",
                        "protocol",
                        "settings",
                        "streamSettings",
                        "sendThrough",
                        "proxySettings",
                        "mux",
                    ],
                    "/outbounds/[]/settings" => &[
                        "domainStrategy",
                        "redirect",
                        "userLevel",
                        "response",
                        "servers",
                        "vnext",
                    ],
                    "/outbounds/[]/settings/response" => &["type"],
                    "/outbounds/[]/settings/servers/[]" => &[
                        "address", "port", "password", "method", "ota", "email", "level", "users",
                    ],
                    "/outbounds/[]/settings/vnext/[]" => &["address", "port", "users"],
                    "/outbounds/[]/settings/servers/[]/users/[]"
                    | "/outbounds/[]/settings/vnext/[]/users/[]" => &[
                        "id",
                        "email",
                        "level",
                        "security",
                        "encryption",
                        "flow",
                        "alterId",
                        "user",
                        "pass",
                    ],
                    "/outbounds/[]/streamSettings" => &[
                        "network",
                        "security",
                        "tlsSettings",
                        "realitySettings",
                        "tcpSettings",
                        "wsSettings",
                        "grpcSettings",
                        "httpupgradeSettings",
                    ],
                    "/outbounds/[]/streamSettings/tlsSettings" => &[
                        "serverName",
                        "allowInsecure",
                        "alpn",
                        "fingerprint",
                        "certificates",
                        "disableSystemRoot",
                        "pinnedPeerCertificateChainSha256",
                        "verifyPeerCertInNames",
                    ],
                    "/outbounds/[]/streamSettings/tlsSettings/certificates/[]" => &[
                        "certificateFile",
                        "keyFile",
                        "certificate",
                        "key",
                        "usage",
                        "oneTimeLoading",
                        "buildChain",
                    ],
                    "/outbounds/[]/streamSettings/realitySettings" => &[
                        "show",
                        "fingerprint",
                        "serverName",
                        "publicKey",
                        "password",
                        "shortId",
                        "spiderX",
                    ],
                    "/outbounds/[]/streamSettings/tcpSettings" => {
                        &["header", "acceptProxyProtocol"]
                    }
                    "/outbounds/[]/streamSettings/tcpSettings/header" => &["type"],
                    "/outbounds/[]/streamSettings/wsSettings" => {
                        &["path", "host", "headers", "acceptProxyProtocol"]
                    }
                    "/outbounds/[]/streamSettings/wsSettings/headers" => &["Host"],
                    "/outbounds/[]/streamSettings/grpcSettings" => &[
                        "serviceName",
                        "authority",
                        "multiMode",
                        "idle_timeout",
                        "health_check_timeout",
                        "permit_without_stream",
                        "initial_windows_size",
                    ],
                    "/outbounds/[]/streamSettings/httpupgradeSettings" => {
                        &["host", "path", "acceptProxyProtocol"]
                    }
                    "/outbounds/[]/proxySettings" => &["tag", "transportLayer"],
                    "/outbounds/[]/mux" => &[
                        "enabled",
                        "concurrency",
                        "xudpConcurrency",
                        "xudpProxyUDP443",
                    ],
                    "/routing" => &["domainStrategy", "domainMatcher", "rules"],
                    "/routing/rules/[]" => &[
                        "type",
                        "ruleTag",
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
                        "outboundTag",
                    ],
                    _ => &[],
                };
                for (key, child) in map {
                    let child_path = format!("{path}/{key}");
                    if !keys.contains(&key.as_str()) {
                        notes.push(format!("Uncovered dependency capability {child_path}; automatic recovery disabled"));
                    } else {
                        walk(child, &child_path, notes);
                    }
                }
                if path == "/inbounds/[]"
                    && !matches!(
                        map.get("protocol").and_then(Value::as_str),
                        Some("http" | "socks")
                    )
                {
                    notes.push("Automatic recovery covers HTTP/SOCKS inbounds only".into());
                }
                if path == "/outbounds/[]"
                    && !matches!(
                        map.get("protocol").and_then(Value::as_str),
                        Some(
                            "freedom"
                                | "blackhole"
                                | "http"
                                | "socks"
                                | "vless"
                                | "vmess"
                                | "trojan"
                                | "shadowsocks"
                        )
                    )
                {
                    notes.push("Outbound protocol dependency coverage is incomplete".into());
                }
            }
            Value::Array(values) => {
                for value in values {
                    walk(value, &format!("{path}/[]"), notes);
                }
            }
            _ => {}
        }
    }
    let mut notes = vec![];
    walk(value, "", &mut notes);
    notes.sort();
    notes.dedup();
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_nested_inputs_and_protocols_disable_automatic_recovery() {
        assert!(uncovered(&serde_json::json!({"inbounds":[{"protocol":"http","settings":{}}],"outbounds":[{"protocol":"freedom","settings":{}}]})).is_empty());
        assert!(!uncovered(&serde_json::json!({"outbounds":[{"protocol":"freedom","settings":{"vendorResource":"mutable.dat"}}]})).is_empty());
        assert!(!uncovered(&serde_json::json!({"outbounds":[{"protocol":"future"}]})).is_empty());
    }
}
