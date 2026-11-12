use super::*;
use serde_json::json;

fn endpoint() -> SocketAddr {
    "127.0.0.1:10809".parse().unwrap()
}

fn config() -> Value {
    json!({
        "outbounds": [
            {"tag": "proxy", "protocol": "vless"},
            {"tag": "direct", "protocol": "freedom"},
            {"tag": "blocked", "protocol": "blackhole"}
        ],
        "routing": {"rules": [
            {"type": "field", "domain": ["full:exact.example", "domain:corp.example"], "outboundTag": "direct"},
            {"type": "field", "domain": ["geosite:ads"], "outboundTag": "proxy"}
        ]}
    })
}

#[test]
fn generated_pac_directs_only_explicit_freedom_domains() {
    let generated = generate(&config(), endpoint());
    validate(&generated.script).unwrap();
    assert!(generated.script.contains("FindProxyForURL"));
    assert_eq!(
        evaluate(&generated.script, "http://exact.example/a")
            .unwrap()
            .kind,
        "DIRECT"
    );
    assert_eq!(
        evaluate(&generated.script, "https://a.corp.example/")
            .unwrap()
            .kind,
        "DIRECT"
    );
    let proxied = evaluate(&generated.script, "https://other.example/").unwrap();
    assert_eq!(proxied.kind, "PROXY");
    assert_eq!(proxied.proxy.as_deref(), Some("127.0.0.1:10809"));
}

#[test]
fn unsupported_preceding_proxy_rule_disables_later_direct_rules() {
    let mut config = config();
    config["routing"]["rules"] = json!([
        {"type": "field", "ip": ["10.0.0.0/8"], "outboundTag": "proxy"},
        {"type": "field", "domain": ["full:exact.example"], "outboundTag": "direct"}
    ]);
    let generated = generate(&config, endpoint());
    assert!(
        generated
            .notes
            .iter()
            .any(|n| n.contains("ip") || n.contains("无法"))
    );
    assert_eq!(
        evaluate(&generated.script, "http://exact.example/")
            .unwrap()
            .kind,
        "PROXY"
    );
}

#[test]
fn direct_default_still_proxies_explicit_proxy_rules() {
    let mut config = config();
    config["outbounds"] = json!([
        {"tag": "direct", "protocol": "freedom"},
        {"tag": "proxy", "protocol": "vless"}
    ]);
    config["routing"]["rules"] = json!([
        {"type": "field", "domain": ["domain:proxied.example"], "outboundTag": "proxy"}
    ]);
    let generated = generate(&config, endpoint());
    assert_eq!(
        evaluate(&generated.script, "http://proxied.example/")
            .unwrap()
            .kind,
        "PROXY"
    );
    assert_eq!(
        evaluate(&generated.script, "http://other.example/")
            .unwrap()
            .kind,
        "DIRECT"
    );
}

#[test]
fn custom_pac_can_be_validated_and_tested() {
    let script = r#"
function FindProxyForURL(url, host) {
  if (shExpMatch(host, "*.internal")) return "DIRECT";
  return "PROXY 127.0.0.1:10809";
}"#;
    validate(script).unwrap();
    assert_eq!(
        evaluate(script, "http://svc.internal/").unwrap().kind,
        "DIRECT"
    );
    assert_eq!(
        evaluate(script, "http://example.com/").unwrap().kind,
        "PROXY"
    );
}

#[test]
fn invalid_or_unsupported_scripts_fail_closed() {
    assert_eq!(
        validate("function FindProxyForURL( {").unwrap_err().code,
        "PAC_SCRIPT_INVALID"
    );
    assert_eq!(
        validate("var x = 1;").unwrap_err().code,
        "PAC_SCRIPT_INVALID"
    );
    let unsupported = "function FindProxyForURL(url, host) { return dnsResolve(host) ? \"DIRECT\" : \"PROXY 127.0.0.1:1\"; }";
    assert_eq!(
        evaluate(unsupported, "http://example.com/")
            .unwrap_err()
            .code,
        "PAC_UNSUPPORTED"
    );
    assert_eq!(
        evaluate("function FindProxyForURL(){return 'BOGUS';}", "http://a/")
            .unwrap_err()
            .code,
        "PAC_RESULT_UNSUPPORTED"
    );
}

#[test]
fn runaway_scripts_are_bounded() {
    let script = "function FindProxyForURL(){ while (true) {} }";
    let error = evaluate(script, "http://example.com/").unwrap_err();
    assert!(
        error.code == "PAC_SCRIPT_ERROR" || error.code == "PAC_SCRIPT_INVALID",
        "{error:?}"
    );
}

#[test]
fn helper_glob_is_case_insensitive_and_bounded() {
    assert!(glob_match("A.B", "*.b"));
    assert!(!glob_match("abc", "a?d"));
    assert!(glob_match("abc", "a*c"));
    assert!(!glob_match(&"a".repeat(2048), "*"));
}

#[test]
fn modern_syntax_produces_a_jscript_compatibility_warning() {
    assert!(compat_notes("function FindProxyForURL(){return 'DIRECT';}").is_empty());
    let notes = compat_notes("const f = (u, h) => 'DIRECT';");
    assert!(notes.iter().any(|n| n.contains("const")));
    assert!(notes.iter().any(|n| n.contains("箭头函数")));
}

#[test]
fn builtin_default_template_bypasses_local_and_lan_hosts() {
    let script = default_template(endpoint());
    validate(&script).unwrap();
    assert!(script.contains("PROXY 127.0.0.1:10809"));
    assert_eq!(
        evaluate(&script, "http://localhost/").unwrap().kind,
        "DIRECT"
    );
    assert_eq!(
        evaluate(&script, "http://printer.lan/").unwrap().kind,
        "DIRECT"
    );
    assert_eq!(
        evaluate(&script, "http://intranet/").unwrap().kind,
        "DIRECT"
    );
    assert_eq!(
        evaluate(&script, "https://example.com/").unwrap().kind,
        "PROXY"
    );
}

#[test]
fn template_domain_lists_manage_proxy_targets() {
    let endpoint = endpoint();
    let proxy = template(
        endpoint,
        xray_model::PacTemplateMode::ProxyList,
        &[
            "example.com".into(),
            "*.internal.test".into(),
            "full:exact.test".into(),
        ],
    )
    .unwrap();
    validate(&proxy).unwrap();
    assert_eq!(
        evaluate(&proxy, "https://example.com/").unwrap().kind,
        "PROXY"
    );
    assert_eq!(
        evaluate(&proxy, "https://a.example.com/").unwrap().kind,
        "PROXY"
    );
    assert_eq!(
        evaluate(&proxy, "https://internal.test/").unwrap().kind,
        "DIRECT"
    );
    assert_eq!(
        evaluate(&proxy, "https://a.internal.test/").unwrap().kind,
        "PROXY"
    );
    assert_eq!(
        evaluate(&proxy, "https://exact.test/").unwrap().kind,
        "PROXY"
    );
    assert_eq!(
        evaluate(&proxy, "https://other.test/").unwrap().kind,
        "DIRECT"
    );

    let direct = template(
        endpoint,
        xray_model::PacTemplateMode::DirectList,
        &["example.com".into()],
    )
    .unwrap();
    assert_eq!(
        evaluate(&direct, "https://example.com/").unwrap().kind,
        "DIRECT"
    );
    assert_eq!(
        evaluate(&direct, "https://other.test/").unwrap().kind,
        "PROXY"
    );
}

#[test]
fn template_domain_validation_rejects_unsafe_input() {
    assert_eq!(normalize_domain("Example.COM").unwrap(), "example.com");
    assert_eq!(normalize_domain("*.Example.COM").unwrap(), "*.example.com");
    assert_eq!(
        normalize_domain("full:Example.COM").unwrap(),
        "full:example.com"
    );
    assert!(normalize_domain("example.com/path").is_err());
    assert!(normalize_domain("bad\"name").is_err());
    assert!(normalize_domain("").is_err());
}

#[test]
fn empty_proxy_list_returns_direct_for_regular_hosts() {
    let script = template(endpoint(), xray_model::PacTemplateMode::ProxyList, &[]).unwrap();
    validate(&script).unwrap();
    assert_eq!(
        evaluate(&script, "https://example.com/").unwrap().kind,
        "DIRECT"
    );
    assert_eq!(
        evaluate(&script, "https://another.test/").unwrap().kind,
        "DIRECT"
    );
    assert_eq!(
        evaluate(&script, "https://localhost/").unwrap().kind,
        "DIRECT"
    );
}

#[test]
fn pac_test_url_accepts_host_without_scheme_and_rejects_relative_input() {
    assert_eq!(
        normalize_test_url("example.com/path").unwrap(),
        "https://example.com/path"
    );
    assert_eq!(
        normalize_test_url("http://example.com/").unwrap(),
        "http://example.com/"
    );
    assert!(normalize_test_url("").is_err());
    assert!(normalize_test_url("/relative").is_err());
    assert!(normalize_test_url("ftp://example.com/").is_err());
}
