//! Conservative dependency coverage for automatic recovery.
//!
//! A field is listed here only when the manager either freezes its file
//! dependencies or knows that it carries no file dependency. Unknown fields
//! still reduce coverage and disable unattended recovery.
use serde_json::Value;

fn allowed(path: &str) -> &'static [&'static str] {
    match path {
        "" => &[
            "log",
            "inbounds",
            "outbounds",
            "routing",
            "dns",
            "api",
            "reverse",
            "policy",
            "stats",
            "metrics",
            "observatory",
            "burstObservatory",
            "fakedns",
            "browserForwarder",
        ],
        "/log" => &["loglevel", "access", "error", "dnsLog", "maskAddress"],
        "/dns" => &[
            "hosts",
            "servers",
            "clientIp",
            "queryStrategy",
            "disableCache",
            "disableFallback",
            "disableFallbackIfMatch",
            "tag",
            "enableParallelQuery",
            "serveStale",
            "serveExpiredTTL",
            "useSystemHosts",
            "respectSearchDomain",
        ],
        "/dns/servers/[]" => &[
            "address",
            "port",
            "domains",
            "expectIPs",
            "skipFallback",
            "clientIp",
            "queryStrategy",
            "timeoutMs",
            "tag",
            "disableCache",
            "finalQuery",
            "serveStale",
            "serveExpiredTTL",
        ],
        "/inbounds/[]" => &[
            "tag",
            "listen",
            "port",
            "protocol",
            "settings",
            "sniffing",
            "streamSettings",
            "allocate",
        ],
        "/inbounds/[]/settings" => &[
            "auth",
            "accounts",
            "udp",
            "ip",
            "userLevel",
            "allowTransparent",
            "address",
            "network",
            "followRedirect",
            "timeout",
            "domainOverride",
            "rewriteAddress",
            "clients",
            "decryption",
            "fallbacks",
            "acceptProxyProtocol",
        ],
        "/inbounds/[]/settings/accounts/[]" => &["user", "pass"],
        "/inbounds/[]/settings/clients/[]" => &["id", "email", "flow", "level", "alterId"],
        "/inbounds/[]/settings/fallbacks/[]" => &["name", "alpn", "path", "dest", "xver"],
        "/inbounds/[]/sniffing" => &[
            "enabled",
            "destOverride",
            "metadataOnly",
            "routeOnly",
            "domainsExcluded",
        ],
        "/inbounds/[]/allocate" => &["strategy", "refresh", "concurrency"],
        "/outbounds/[]" => &[
            "tag",
            "protocol",
            "settings",
            "streamSettings",
            "sendThrough",
            "proxySettings",
            "mux",
            "targetStrategy",
            "email",
            "level",
        ],
        "/outbounds/[]/settings" => &[
            "domainStrategy",
            "redirect",
            "userLevel",
            "response",
            "servers",
            "vnext",
            "fragment",
            "noises",
            "secret",
            "address",
            "port",
            "password",
            "method",
            "ota",
            "email",
            "level",
            "users",
            "network",
            "security",
        ],
        "/outbounds/[]/settings/response" => &["type"],
        "/outbounds/[]/settings/fragment" => &["packets", "length", "interval", "maxSplit"],
        "/outbounds/[]/settings/noises/[]" => &["type", "packet", "delay", "applyTo"],
        "/outbounds/[]/settings/servers/[]" | "/outbounds/[]/settings/vnext/[]" => &[
            "address", "port", "users", "password", "method", "ota", "email", "level",
        ],
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
            "rawSettings",
            "wsSettings",
            "grpcSettings",
            "httpupgradeSettings",
            "kcpSettings",
            "quicSettings",
            "httpSettings",
            "splithttpSettings",
            "xhttpSettings",
            "sockopt",
        ],
        "/outbounds/[]/streamSettings/tlsSettings" | "/inbounds/[]/streamSettings/tlsSettings" => {
            &[
                "serverName",
                "allowInsecure",
                "alpn",
                "fingerprint",
                "certificates",
                "disableSystemRoot",
                "pinnedPeerCertificateChainSha256",
                "verifyPeerCertInNames",
                "minVersion",
                "maxVersion",
                "cipherSuites",
                "curvePreferences",
                "rejectUnknownSni",
                "enableSessionResumption",
            ]
        }
        "/outbounds/[]/streamSettings/tlsSettings/certificates/[]"
        | "/inbounds/[]/streamSettings/tlsSettings/certificates/[]" => &[
            "certificateFile",
            "keyFile",
            "certificate",
            "key",
            "usage",
            "oneTimeLoading",
            "buildChain",
        ],
        "/outbounds/[]/streamSettings/realitySettings"
        | "/inbounds/[]/streamSettings/realitySettings" => &[
            "show",
            "fingerprint",
            "serverName",
            "publicKey",
            "password",
            "shortId",
            "spiderX",
            "target",
            "xver",
            "privateKey",
            "settings",
        ],
        "/outbounds/[]/streamSettings/tcpSettings"
        | "/outbounds/[]/streamSettings/rawSettings"
        | "/inbounds/[]/streamSettings/tcpSettings"
        | "/inbounds/[]/streamSettings/rawSettings" => &["header", "acceptProxyProtocol"],
        "/outbounds/[]/streamSettings/tcpSettings/header"
        | "/outbounds/[]/streamSettings/rawSettings/header"
        | "/inbounds/[]/streamSettings/tcpSettings/header"
        | "/inbounds/[]/streamSettings/rawSettings/header" => &["type", "request", "response"],
        "/outbounds/[]/streamSettings/wsSettings" | "/inbounds/[]/streamSettings/wsSettings" => &[
            "path",
            "host",
            "headers",
            "acceptProxyProtocol",
            "maxEarlyData",
            "earlyDataHeaderName",
            "useBrowserForwarding",
        ],
        "/outbounds/[]/streamSettings/grpcSettings"
        | "/inbounds/[]/streamSettings/grpcSettings" => &[
            "serviceName",
            "authority",
            "multiMode",
            "idle_timeout",
            "health_check_timeout",
            "permit_without_stream",
            "initial_windows_size",
        ],
        "/outbounds/[]/streamSettings/httpupgradeSettings"
        | "/inbounds/[]/streamSettings/httpupgradeSettings" => {
            &["host", "path", "headers", "acceptProxyProtocol"]
        }
        "/outbounds/[]/streamSettings/kcpSettings" | "/inbounds/[]/streamSettings/kcpSettings" => {
            &[
                "mtu",
                "tti",
                "uplinkCapacity",
                "downlinkCapacity",
                "congestion",
                "readBufferSize",
                "writeBufferSize",
                "header",
                "seed",
                "alternative",
            ]
        }
        "/outbounds/[]/streamSettings/quicSettings"
        | "/inbounds/[]/streamSettings/quicSettings" => &[
            "security",
            "key",
            "header",
            "congestion",
            "tcpCongestion",
            "keepAlive",
            "maxIdleTimeout",
        ],
        "/outbounds/[]/streamSettings/httpSettings"
        | "/inbounds/[]/streamSettings/httpSettings" => &[
            "host",
            "path",
            "read_idle_timeout",
            "health_check_timeout",
            "method",
            "headers",
        ],
        "/outbounds/[]/streamSettings/splithttpSettings"
        | "/inbounds/[]/streamSettings/splithttpSettings"
        | "/outbounds/[]/streamSettings/xhttpSettings"
        | "/inbounds/[]/streamSettings/xhttpSettings" => &[
            "path",
            "host",
            "headers",
            "mode",
            "noSSEHeader",
            "scMaxEachPostBytes",
            "scMinPostsIntervalMs",
            "xmux",
        ],
        "/outbounds/[]/streamSettings/sockopt" | "/inbounds/[]/streamSettings/sockopt" => &[
            "mark",
            "tcpFastOpen",
            "tproxy",
            "acceptProxyProtocol",
            "domainStrategy",
            "dialerProxy",
            "tcpKeepAliveIdle",
            "tcpKeepAliveInterval",
            "tcpUserTimeout",
            "tcpcongestion",
            "tcpMptcp",
            "penetrate",
            "V6Only",
            "interface",
        ],
        "/outbounds/[]/proxySettings" => &["tag", "transportLayer"],
        "/outbounds/[]/mux" => &[
            "enabled",
            "concurrency",
            "xudpConcurrency",
            "xudpProxyUDP443",
        ],
        "/routing" => &["domainStrategy", "domainMatcher", "rules", "balancers"],
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
            "balancerTag",
        ],
        "/routing/balancers/[]" => &["tag", "selector", "strategy", "fallbackTag"],
        "/api" => &["tag", "services"],
        "/reverse" => &["bridges", "portals"],
        "/reverse/portals/[]" => &["tag", "domain"],
        "/reverse/bridges/[]" => &["tag", "domain"],
        "/policy" => &["levels", "system"],
        "/metrics" => &["tag", "listen"],
        "/observatory" => &[
            "subjectSelector",
            "probeUrl",
            "probeInterval",
            "enableConcurrency",
        ],
        "/burstObservatory" => &["subjectSelector", "pingConfig"],
        "/fakedns" => &["ipPool", "poolSize", "ranges", "pools"],
        "/browserForwarder" => &["listenAddr", "listenPort"],
        _ => &[],
    }
}

pub fn uncovered(value: &Value) -> Vec<String> {
    fn walk(value: &Value, path: &str, notes: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                // Host/attribute names are data, not configuration capability.
                if path.ends_with("/headers") || path.ends_with("/attrs") {
                    return;
                }
                if path == "/dns/hosts" {
                    for (key, value) in map {
                        if !value.is_string()
                            && !value
                                .as_array()
                                .is_some_and(|a| a.iter().all(Value::is_string))
                        {
                            notes.push(format!(
                                "Uncovered dependency capability /dns/hosts/{key}; automatic recovery disabled"
                            ));
                        }
                    }
                    return;
                }
                let keys = allowed(path);
                for (key, child) in map {
                    let child_path = format!("{path}/{key}");
                    if !keys.contains(&key.as_str()) {
                        notes.push(format!(
                            "Uncovered dependency capability {child_path}; automatic recovery disabled"
                        ));
                    } else {
                        walk(child, &child_path, notes);
                    }
                }
                if path == "/inbounds/[]"
                    && !matches!(
                        map.get("protocol").and_then(Value::as_str),
                        Some("http" | "socks" | "mixed")
                    )
                {
                    notes.push("Automatic recovery covers HTTP/SOCKS/Mixed inbounds only".into());
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
                                | "wireguard"
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
    #[test]
    fn common_transports_and_native_sections_do_not_false_positive() {
        let value = serde_json::json!({
            "log": {"loglevel": "warning", "access": "none"},
            "dns": {"servers": [{"address": "1.1.1.1", "domains": ["geosite:cn"], "expectIPs": ["geoip:cn"]}]},
            "inbounds": [{"tag": "mixed-in", "protocol": "mixed", "listen": "127.0.0.1", "port": 1080, "settings": {"auth": "noauth", "udp": true}}],
            "outbounds": [{"tag": "vless", "protocol": "vless", "settings": {"vnext": [{"address": "a", "port": 443, "users": [{"id": "u"}]}]}, "streamSettings": {
                "network": "ws", "security": "tls",
                "tlsSettings": {"serverName": "a", "minVersion": "1.2", "certificates": [{"certificateFile": "cert.pem"}]},
                "wsSettings": {"path": "/", "headers": {"Host": "a", "User-Agent": "x"}},
                "sockopt": {"tcpFastOpen": true}
            }}],
            "routing": {"domainStrategy": "AsIs", "rules": [{"type": "field", "domain": ["geosite:cn"], "outboundTag": "vless"}], "balancers": [{"tag": "b", "selector": ["vless"]}]},
            "api": {"tag": "api", "services": ["HandlerService"]},
            "metrics": {"tag": "metrics", "listen": "127.0.0.1:1111"}
        });
        let notes = uncovered(&value);
        assert!(
            notes
                .iter()
                .all(|n| !n.contains("Uncovered dependency capability")),
            "{notes:?}"
        );
    }
}
