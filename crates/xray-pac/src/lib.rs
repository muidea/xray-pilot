//! PAC script generation, validation and isolated `FindProxyForURL` evaluation.
//!
//! The generated script only maps native *domain* routing rules to DIRECT/PROXY.
//! It deliberately never bypasses a preceding rule that it cannot express: the
//! default is PROXY (enter Xray) whenever such a rule could hide a proxy action.
use boa_engine::{Context, JsNativeError, JsValue, Source, js_string};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashSet};
use std::net::SocketAddr;
use url::Url;
use xray_model::{Error, PacTemplateMode, Result};

pub const DEFAULT_PAC_PORT: u16 = 18080;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PacDomainKind {
    /// Matches only the exact hostname (`full:example.com`).
    Exact,
    /// Matches only subdomains (`*.example.com`).
    Subdomain,
    /// Matches the domain and all of its subdomains (`example.com`).
    DomainAndSub,
}

/// Classify a normalized or raw PAC template domain rule.
pub fn domain_kind(domain: &str) -> PacDomainKind {
    let trimmed = domain.trim();
    if trimmed
        .get(..5)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("full:"))
    {
        PacDomainKind::Exact
    } else if trimmed.starts_with("*.") {
        PacDomainKind::Subdomain
    } else {
        PacDomainKind::DomainAndSub
    }
}

/// Result of parsing a bulk domain list against an existing list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BatchImportResult {
    pub added: Vec<String>,
    pub duplicates: usize,
    pub invalid: Vec<(usize, String, String)>,
}

/// Parse multiline or delimiter-separated domains, ignoring blank lines and `#` / `//` comments.
pub fn parse_domain_batch(input: &str, existing: &[String]) -> BatchImportResult {
    let mut seen: HashSet<String> = existing.iter().map(|d| d.to_ascii_lowercase()).collect();
    let mut result = BatchImportResult::default();
    for (idx, raw_line) in input.lines().enumerate() {
        let line_no = idx + 1;
        let line = strip_line_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }
        for token in line
            .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';'))
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match normalize_domain(token) {
                Ok(domain) => {
                    let key = domain.to_ascii_lowercase();
                    if seen.insert(key) {
                        result.added.push(domain);
                    } else {
                        result.duplicates += 1;
                    }
                }
                Err(error) => {
                    result
                        .invalid
                        .push((line_no, token.to_owned(), error.message));
                }
            }
        }
    }
    result
}

fn strip_line_comment(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || trimmed.starts_with("//") {
        return "";
    }
    let without_hash = line.split(" #").next().unwrap_or(line);
    without_hash.split(" //").next().unwrap_or(without_hash)
}

/// Format domains as a plain newline-separated list for editing or export.
pub fn format_domain_list(domains: &[String]) -> String {
    domains.join("\n")
}

/// Result of extracting plain/wildcard/full domains from native routing rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtractedConfigDomains {
    pub domains: Vec<String>,
    pub skipped_geosite: usize,
    pub skipped_regexp: usize,
    pub skipped_ext: usize,
    pub skipped_other: usize,
}

/// Extract template-compatible domain entries from `/routing/rules[*]/domain`.
/// Rules relying on `geosite:`, `regexp:`, `ext:`, or keyword substrings cannot
/// be represented directly by the PAC domain template and are counted as skipped.
pub fn extract_domains_from_config(config: &Value) -> ExtractedConfigDomains {
    let mut out = ExtractedConfigDomains::default();
    let mut seen = HashSet::new();
    let Some(rules) = config.pointer("/routing/rules").and_then(Value::as_array) else {
        return out;
    };
    for rule in rules {
        let Some(entries) = rule.get("domain").and_then(Value::as_array) else {
            continue;
        };
        for entry in entries.iter().filter_map(Value::as_str) {
            let trimmed = entry.trim();
            let lower = trimmed.to_ascii_lowercase();
            if lower.starts_with("geosite:") {
                out.skipped_geosite += 1;
                continue;
            }
            if lower.starts_with("regexp:") {
                out.skipped_regexp += 1;
                continue;
            }
            if lower.starts_with("ext:") || lower.starts_with("ext-ip:") {
                out.skipped_ext += 1;
                continue;
            }
            // Bare entries without a prefix are keyword substrings in Xray native routing,
            // unless they look like a domain name (contain a dot).
            if !(lower.starts_with("domain:")
                || lower.starts_with("full:")
                || (!lower.contains(':') && trimmed.contains('.')))
            {
                out.skipped_other += 1;
                continue;
            }
            match normalize_domain(trimmed) {
                Ok(domain) => {
                    if seen.insert(domain.to_ascii_lowercase()) {
                        out.domains.push(domain);
                    }
                }
                Err(_) => out.skipped_other += 1,
            }
        }
    }
    out
}

/// Validate and normalize a domain used by the built-in PAC template.
/// Accepts plain domains, `*.example.com`, `full:example.com` and
/// `domain:example.com`; prefixes are preserved for display.
pub fn normalize_domain(input: &str) -> Result<String> {
    let mut value = input.trim().to_ascii_lowercase();
    if value.is_empty() {
        return Err(Error::new("PAC_DOMAIN_INVALID", "Domain must not be empty"));
    }
    let wildcard = value.starts_with("*.");
    if wildcard {
        value = format!("domain:{}", value.trim_start_matches("*."));
    }
    let (kind, body) = if let Some(body) = value.strip_prefix("full:") {
        ("full", body)
    } else if let Some(body) = value.strip_prefix("domain:") {
        ("domain", body)
    } else {
        ("domain", value.as_str())
    };
    let body = body.trim_start_matches('.').trim_end_matches('.');
    if body.is_empty()
        || body.len() > 253
        || body.contains("..")
        || !body
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
    {
        return Err(Error::new(
            "PAC_DOMAIN_INVALID",
            format!("Invalid PAC domain: {input}"),
        ));
    }
    if kind == "full" {
        Ok(format!("full:{body}"))
    } else if wildcard {
        Ok(format!("*.{body}"))
    } else {
        Ok(body.to_owned())
    }
}

fn js_domain_set(entries: &BTreeSet<String>) -> String {
    let mut out = String::from("{");
    for (idx, item) in entries.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&string_literal(item));
        out.push_str(":1");
    }
    out.push('}');
    out
}

/// Build a built-in template with an optional domain list.
///
/// Uses ES3-compatible object maps and dot-segment lookup so large domain lists
/// stay compact, avoid deep `||` expression trees, and evaluate in O(labels).
pub fn template(endpoint: SocketAddr, mode: PacTemplateMode, domains: &[String]) -> Result<String> {
    let mut exact = BTreeSet::new();
    let mut sub = BTreeSet::new();
    let mut suffix = BTreeSet::new();
    for domain in domains {
        let value = normalize_domain(domain)?;
        if let Some(full) = value.strip_prefix("full:") {
            exact.insert(full.to_owned());
        } else if let Some(wild) = value.strip_prefix("*.") {
            sub.insert(wild.to_owned());
        } else {
            suffix.insert(value);
        }
    }

    let empty = exact.is_empty() && sub.is_empty() && suffix.is_empty();
    if mode == PacTemplateMode::All || empty {
        let branch = match mode {
            PacTemplateMode::All | PacTemplateMode::DirectList => {
                format!("return \"PROXY {endpoint}\";")
            }
            PacTemplateMode::ProxyList => "return \"DIRECT\";".to_owned(),
        };
        return Ok(format!(
            r#"function FindProxyForURL(url, host) {{
    host = host.toLowerCase();
    if (host === "localhost" || host === "127.0.0.1" || host === "::1" ||
        isPlainHostName(host) || dnsDomainIs(host, ".lan") || dnsDomainIs(host, ".local")) {{
        return "DIRECT";
    }}
    {branch}
}}
"#
        ));
    }

    let mut globals = String::new();
    let mut direct_checks = Vec::new();
    let mut parent_checks = Vec::new();
    if !exact.is_empty() {
        globals.push_str(&format!("var __PAC_EXACT = {};\n", js_domain_set(&exact)));
        direct_checks.push("__PAC_OWN.call(__PAC_EXACT, host)");
    }
    if !sub.is_empty() {
        globals.push_str(&format!("var __PAC_SUB = {};\n", js_domain_set(&sub)));
        parent_checks.push("__PAC_OWN.call(__PAC_SUB, parent)");
    }
    if !suffix.is_empty() {
        globals.push_str(&format!("var __PAC_SUFFIX = {};\n", js_domain_set(&suffix)));
        direct_checks.push("__PAC_OWN.call(__PAC_SUFFIX, host)");
        parent_checks.push("__PAC_OWN.call(__PAC_SUFFIX, parent)");
    }

    let direct_expr = if direct_checks.is_empty() {
        "false".to_owned()
    } else {
        direct_checks.join(" || ")
    };
    let parent_loop = if parent_checks.is_empty() {
        String::new()
    } else {
        let cond = parent_checks.join(" || ");
        format!(
            r#"    if (!matched) {{
        var pos = host.indexOf(".");
        while (pos !== -1) {{
            var parent = host.substring(pos + 1);
            if ({cond}) {{
                matched = true;
                break;
            }}
            pos = host.indexOf(".", pos + 1);
        }}
    }}
"#
        )
    };
    let (hit, miss) = match mode {
        PacTemplateMode::ProxyList => (format!("\"PROXY {endpoint}\""), "\"DIRECT\"".to_owned()),
        PacTemplateMode::DirectList => ("\"DIRECT\"".to_owned(), format!("\"PROXY {endpoint}\"")),
        PacTemplateMode::All => unreachable!(),
    };

    Ok(format!(
        r#"{globals}var __PAC_OWN = Object.prototype.hasOwnProperty;
function __pacMatch(host) {{
    var matched = {direct_expr};
{parent_loop}    return matched;
}}
function FindProxyForURL(url, host) {{
    host = host.toLowerCase();
    if (host === "localhost" || host === "127.0.0.1" || host === "::1" ||
        isPlainHostName(host) || dnsDomainIs(host, ".lan") || dnsDomainIs(host, ".local")) {{
        return "DIRECT";
    }}
    if (__pacMatch(host)) {{
        return {hit};
    }}
    return {miss};
}}
"#
    ))
}

/// Built-in PAC template with the default "proxy everything" policy.
pub fn default_template(endpoint: SocketAddr) -> String {
    template(endpoint, PacTemplateMode::All, &[]).expect("default PAC template is valid")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedPac {
    pub script: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacDecision {
    /// First directive kind: DIRECT, PROXY, SOCKS, HTTP, HTTPS or UNKNOWN.
    pub kind: String,
    /// First proxy endpoint for PROXY/SOCKS/HTTP/HTTPS directives.
    pub proxy: Option<String>,
    /// Original return value, bounded for transport.
    pub raw: String,
    /// All directives in order, normalized to `KIND [host:port]`.
    pub directives: Vec<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Direct,
    Proxy,
}

impl Action {
    fn of(protocol: Option<&str>) -> Self {
        match protocol {
            Some("freedom") => Self::Direct,
            // blackhole still enters Xray; Xray performs the block.
            _ => Self::Proxy,
        }
    }
}

fn string_literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

fn domain_condition(pattern: &str) -> Option<String> {
    let pattern = pattern.trim().to_ascii_lowercase();
    if pattern.is_empty() {
        return None;
    }
    if let Some(full) = pattern.strip_prefix("full:") {
        return Some(format!("host === {}", string_literal(full)));
    }
    if let Some(suffix) = pattern.strip_prefix("domain:") {
        let suffix = suffix.trim_start_matches('.');
        return Some(format!(
            "(host === {p} || dnsDomainIs(host, {d}))",
            p = string_literal(suffix),
            d = string_literal(&format!(".{suffix}"))
        ));
    }
    if pattern.contains(':') {
        // regexp:, geosite:, ext: and other native prefixes need data or a regex engine.
        return None;
    }
    Some(format!(
        "(host.indexOf({}) !== -1)",
        string_literal(&pattern)
    ))
}

fn domain_rule(rule: &Value) -> Option<(Vec<String>, Action)> {
    let map = rule.as_object()?;
    if map
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|t| t != "field")
    {
        return None;
    }
    let allowed = ["type", "ruleTag", "domain", "outboundTag", "balancerTag"];
    if map.keys().any(|key| !allowed.contains(&key.as_str())) {
        return None;
    }
    let outbound = map.get("outboundTag").and_then(Value::as_str);
    let balancer = map.get("balancerTag").and_then(Value::as_str);
    if outbound.is_none() && balancer.is_none() {
        return None;
    }
    let patterns: Vec<String> = map
        .get("domain")
        .and_then(Value::as_array)?
        .iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    if patterns.is_empty() {
        return None;
    }
    // The outbound protocol is resolved by the caller; a balancer always proxies.
    let action = if balancer.is_some() {
        Action::Proxy
    } else {
        Action::of(None)
    };
    Some((patterns, action))
}

/// Generate a conservative PAC script from the native routing rules.
///
/// Only rules whose complete match condition is a domain list are compiled.
/// A direct decision is emitted only while every preceding rule is also
/// expressible; otherwise traffic falls through to PROXY and Xray applies the
/// full native rule set. This may under-direct, but it never leaks traffic that
/// a native proxy/block rule would have captured.
pub fn generate(config: &Value, endpoint: SocketAddr) -> GeneratedPac {
    let proxy = format!("PROXY {endpoint}");
    let protocols: std::collections::BTreeMap<&str, &str> = config
        .get("outbounds")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|outbound| {
            Some((
                outbound.get("tag")?.as_str()?,
                outbound.get("protocol")?.as_str()?,
            ))
        })
        .collect();
    let default_tag = config.pointer("/outbounds/0/tag").and_then(Value::as_str);
    let mut notes = vec![];
    let initial_direct = default_tag
        .and_then(|tag| protocols.get(tag).copied())
        .is_some_and(|protocol| protocol == "freedom");
    let rules: Vec<Value> = config
        .pointer("/routing/rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    // Resolve rule action and expressibility up front.
    struct Rule {
        index: usize,
        patterns: Vec<String>,
        conditions: Option<Vec<String>>,
        action: Action,
        expressible: bool,
        catch_all: bool,
    }
    let mut resolved = vec![];
    for (index, rule) in rules.iter().enumerate() {
        let map = match rule.as_object() {
            Some(map) => map,
            None => {
                notes.push(format!("规则 {} 不是对象，已跳过", index + 1));
                continue;
            }
        };
        let outbound = map.get("outboundTag").and_then(Value::as_str);
        let balancer = map.get("balancerTag").and_then(Value::as_str);
        let action = if balancer.is_some() {
            Action::Proxy
        } else {
            Action::of(outbound.and_then(|tag| protocols.get(tag).copied()))
        };
        let catch_all = !map.contains_key("domain")
            && !map.contains_key("ip")
            && !map.contains_key("port")
            && !map.contains_key("sourcePort")
            && !map.contains_key("network")
            && !map.contains_key("source")
            && !map.contains_key("user")
            && !map.contains_key("inboundTag")
            && !map.contains_key("protocol")
            && !map.contains_key("attrs");
        let (patterns, conditions, expressible) = match domain_rule(rule) {
            Some((patterns, _)) if catch_all => (patterns, Some(vec![]), false),
            Some((patterns, _)) => {
                let conditions: Option<Vec<String>> =
                    patterns.iter().map(|p| domain_condition(p)).collect();
                (patterns, conditions, true)
            }
            None => (vec![], None, false),
        };
        let conditions = if catch_all { Some(vec![]) } else { conditions };
        let expressible = expressible && conditions.is_some();
        resolved.push(Rule {
            index,
            patterns,
            conditions,
            action,
            expressible,
            catch_all,
        });
    }

    // A direct default is only safe when no preceding proxy/block rule is lost.
    let mut default_direct = initial_direct;
    if default_direct
        && resolved
            .iter()
            .any(|rule| !rule.expressible && rule.action == Action::Proxy)
    {
        default_direct = false;
        notes.push(
            "存在无法用 PAC 表达的前序代理/阻断规则，默认改为 PROXY，由 Xray 保留原生路由".into(),
        );
    }

    let mut blocks = vec![];
    // When the default is DIRECT, a proxy/block rule must be emitted. When the
    // default is PROXY, a later DIRECT rule is only safe while every preceding
    // rule is also a DIRECT decision; otherwise the earlier rule takes precedence
    // in Xray and PAC must not bypass it.
    let mut direct_allowed = true;
    let mut catch_all_action = if default_direct {
        Action::Direct
    } else {
        Action::Proxy
    };
    for rule in &resolved {
        if rule.catch_all {
            if rule.action == Action::Direct && direct_allowed {
                catch_all_action = Action::Direct;
            } else {
                if rule.action == Action::Direct {
                    notes.push(format!(
                        "规则 {} 的全量直连位于代理/无法表达的规则之后，保留 PROXY",
                        rule.index + 1
                    ));
                }
                catch_all_action = Action::Proxy;
            }
            break;
        }
        if !rule.expressible {
            if rule.action == Action::Direct {
                if direct_allowed {
                    notes.push(format!(
                        "规则 {} 的直连条件无法用 PAC 表达，后续直连规则被抑制，流量进入 Xray",
                        rule.index + 1
                    ));
                }
                direct_allowed = false;
            } else {
                notes.push(format!(
                    "规则 {} 的代理/阻断条件无法用 PAC 表达；默认 PROXY 仍由 Xray 覆盖，后续直连规则被抑制",
                    rule.index + 1
                ));
                direct_allowed = false;
            }
            continue;
        }
        let conditions = rule.conditions.as_ref().unwrap();
        let condition = if conditions.is_empty() {
            "true".to_owned()
        } else {
            format!("({})", conditions.join(" || "))
        };
        match (default_direct, rule.action) {
            (false, Action::Direct) => {
                if direct_allowed {
                    blocks.push((condition, "DIRECT", rule.patterns.clone()));
                } else {
                    notes.push(format!(
                        "规则 {} 的直连位于无法用 PAC 保留的代理/阻断规则之后，保留 PROXY",
                        rule.index + 1
                    ));
                }
            }
            (false, Action::Proxy) => {
                // Default is already PROXY; this rule also blocks later DIRECT entries.
                direct_allowed = false;
            }
            (true, Action::Proxy) => {
                blocks.push((condition, "PROXY", rule.patterns.clone()));
                direct_allowed = false;
            }
            (true, Action::Direct) => {}
        }
    }

    let mut script = String::new();
    script.push_str("function FindProxyForURL(url, host) {\n");
    script.push_str("    host = host.toLowerCase();\n");
    script.push_str(
        "    if (host === \"localhost\" || host === \"127.0.0.1\" || host === \"::1\") {\n",
    );
    script.push_str("        return \"DIRECT\";\n");
    script.push_str("    }\n");
    for (condition, result, _) in &blocks {
        let value = match *result {
            "DIRECT" => "DIRECT".to_owned(),
            _ => proxy.clone(),
        };
        script.push_str(&format!(
            "    if ({condition}) {{\n        return {};\n    }}\n",
            string_literal(&value)
        ));
    }
    let fallback = if catch_all_action == Action::Direct {
        "DIRECT".to_owned()
    } else {
        proxy
    };
    script.push_str(&format!("    return {};\n}}\n", string_literal(&fallback)));
    GeneratedPac { script, notes }
}

fn native_unsupported(name: &str) -> boa_engine::JsError {
    JsNativeError::typ()
        .with_message(format!(
            "PAC_UNSUPPORTED: {name} requires execution context not provided"
        ))
        .into()
}

fn arg(args: &[JsValue], index: usize, context: &mut Context) -> boa_engine::JsResult<String> {
    args.get(index)
        .cloned()
        .unwrap_or_else(JsValue::undefined)
        .to_string(context)
        .map(|s| s.to_std_string_escaped())
}

fn install_helpers(context: &mut Context) -> boa_engine::JsResult<()> {
    use boa_engine::NativeFunction;
    let mut register = |name: &str, length: usize, f: NativeFunction| -> boa_engine::JsResult<()> {
        context.register_global_builtin_callable(js_string!(name), length, f)?;
        Ok(())
    };
    register(
        "isPlainHostName",
        1,
        NativeFunction::from_fn_ptr(|_, args, context| {
            let host = arg(args, 0, context)?;
            Ok(JsValue::from(!host.contains('.')))
        }),
    )?;
    register(
        "dnsDomainIs",
        2,
        NativeFunction::from_fn_ptr(|_, args, context| {
            let host = arg(args, 0, context)?.to_ascii_lowercase();
            let domain = arg(args, 1, context)?.to_ascii_lowercase();
            Ok(JsValue::from(host.ends_with(&domain)))
        }),
    )?;
    register(
        "localHostOrDomainIs",
        2,
        NativeFunction::from_fn_ptr(|_, args, context| {
            let host = arg(args, 0, context)?.to_ascii_lowercase();
            let hostdom = arg(args, 1, context)?.to_ascii_lowercase();
            let matched = host == hostdom
                || hostdom
                    .strip_prefix(host.as_str())
                    .is_some_and(|rest| rest.starts_with('.'));
            Ok(JsValue::from(matched))
        }),
    )?;
    register(
        "dnsDomainLevels",
        1,
        NativeFunction::from_fn_ptr(|_, args, context| {
            let host = arg(args, 0, context)?;
            Ok(JsValue::from(host.matches('.').count() as f64))
        }),
    )?;
    register(
        "shExpMatch",
        2,
        NativeFunction::from_fn_ptr(|_, args, context| {
            let text = arg(args, 0, context)?;
            let pattern = arg(args, 1, context)?;
            Ok(JsValue::from(glob_match(&text, &pattern)))
        }),
    )?;
    for unsupported in [
        "dnsResolve",
        "dnsResolveEx",
        "myIpAddress",
        "myIpAddressEx",
        "isInNet",
        "isInNetEx",
        "sortIpAddressList",
        "getClientVersion",
        "weekdayRange",
        "dateRange",
        "timeRange",
        "convert_addr",
    ] {
        register(
            unsupported,
            1,
            NativeFunction::from_fn_ptr(|_, _, _| Err(native_unsupported("PAC network helper"))),
        )?;
    }
    context.register_global_builtin_callable(
        js_string!("alert"),
        1,
        NativeFunction::from_fn_ptr(|_, _, _| Ok(JsValue::undefined())),
    )?;
    Ok(())
}

fn glob_match(text: &str, pattern: &str) -> bool {
    // PAC shExpMatch uses shell-style globs. Keep this iterative and bounded:
    // native recursion is not covered by Boa's loop limit.
    if text.len() > 1024 || pattern.len() > 1024 {
        return false;
    }
    let text = text.as_bytes();
    let pattern = pattern.as_bytes();
    let (mut text_index, mut pattern_index) = (0, 0);
    let mut star: Option<usize> = None;
    let mut star_text = 0;
    while text_index < text.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?'
                || pattern[pattern_index].eq_ignore_ascii_case(&text[text_index]))
        {
            text_index += 1;
            pattern_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star = Some(pattern_index);
            star_text = text_index;
            pattern_index += 1;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            star_text += 1;
            text_index = star_text;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn context() -> Context {
    let mut context = Context::default();
    // Bounded evaluation: no host I/O is registered, and runaway loops throw.
    context
        .runtime_limits_mut()
        .set_loop_iteration_limit(2_000_000);
    context.runtime_limits_mut().set_recursion_limit(64);
    context.runtime_limits_mut().set_stack_size_limit(1024);
    context
}

fn eval_script(script: &str) -> Result<Context> {
    if script.len() > 512 * 1024 {
        return Err(Error::new(
            "PAC_SCRIPT_TOO_LARGE",
            "PAC script exceeds 512 KiB",
        ));
    }
    let mut context = context();
    install_helpers(&mut context).map_err(|e| {
        Error::new(
            "PAC_SCRIPT_INVALID",
            format!("Cannot install PAC helpers: {e}"),
        )
    })?;
    context
        .eval(Source::from_bytes(script))
        .map_err(|e| Error::new("PAC_SCRIPT_INVALID", format!("PAC script error: {e}")))?;
    let function = context
        .global_object()
        .get(js_string!("FindProxyForURL"), &mut context)
        .map_err(|e| Error::new("PAC_SCRIPT_INVALID", format!("FindProxyForURL: {e}")))?;
    if !function.is_callable() {
        return Err(Error::new(
            "PAC_SCRIPT_INVALID",
            "PAC must define a callable FindProxyForURL",
        ));
    }
    Ok(context)
}

/// Validate syntax, top-level execution and the presence of FindProxyForURL.
pub fn validate(script: &str) -> Result<()> {
    let mut context = eval_script(script)?;
    // Also require a usable function object in the same context.
    let function = context
        .global_object()
        .get(js_string!("FindProxyForURL"), &mut context)
        .map_err(|e| Error::new("PAC_SCRIPT_INVALID", format!("FindProxyForURL: {e}")))?;
    if !function.is_callable() {
        return Err(Error::new(
            "PAC_SCRIPT_INVALID",
            "PAC must define a callable FindProxyForURL",
        ));
    }
    Ok(())
}

/// Normalize a user-entered PAC test URL. A missing scheme is treated as
/// HTTPS; only HTTP(S) URLs with a host are accepted.
pub fn normalize_test_url(input: &str) -> Result<String> {
    let value = input.trim();
    if value.is_empty() {
        return Err(Error::new(
            "PAC_URL_INVALID",
            "请输入测试 URL，例如 https://example.com/",
        ));
    }
    if value.starts_with(['/', '.', '?', '#']) {
        return Err(Error::new(
            "PAC_URL_INVALID",
            "PAC test URL must be an absolute http:// or https:// URL",
        ));
    }
    let value = if value.contains("://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    let parsed = Url::parse(&value)
        .map_err(|error| Error::new("PAC_URL_INVALID", format!("Invalid PAC test URL: {error}")))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
        return Err(Error::new(
            "PAC_URL_INVALID",
            "PAC test URL must be an http:// or https:// URL with a host",
        ));
    }
    Ok(value)
}

/// Evaluate FindProxyForURL in an isolated context with a bounded runtime.
pub fn evaluate(script: &str, url: &str) -> Result<PacDecision> {
    let parsed = Url::parse(url)
        .map_err(|e| Error::new("PAC_URL_INVALID", format!("Invalid PAC test URL: {e}")))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| Error::new("PAC_URL_INVALID", "PAC test URL has no host"))?;
    let mut context = eval_script(script)?;
    let function = context
        .global_object()
        .get(js_string!("FindProxyForURL"), &mut context)
        .map_err(|e| Error::new("PAC_SCRIPT_INVALID", format!("FindProxyForURL: {e}")))?;
    let result = function
        .as_object()
        .ok_or_else(|| Error::new("PAC_SCRIPT_INVALID", "FindProxyForURL is not callable"))?
        .call(
            &JsValue::undefined(),
            &[
                JsValue::from(js_string!(url)),
                JsValue::from(js_string!(host)),
            ],
            &mut context,
        )
        .map_err(|e| {
            let message = format!("{e}");
            if message.contains("PAC_UNSUPPORTED") {
                Error::new("PAC_UNSUPPORTED", message)
            } else {
                Error::new("PAC_SCRIPT_ERROR", format!("FindProxyForURL failed: {e}"))
            }
        })?;
    let raw = result
        .to_string(&mut context)
        .map_err(|e| {
            Error::new(
                "PAC_SCRIPT_ERROR",
                format!("PAC result is not a string: {e}"),
            )
        })?
        .to_std_string_escaped();
    let raw = raw.chars().take(16 * 1024).collect::<String>();
    let directives: Vec<String> = raw
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_owned)
        .collect();
    let first = directives.first().cloned().unwrap_or_default();
    let (kind, proxy) = parse_directive(&first);
    if kind == "UNKNOWN" {
        return Err(Error::new(
            "PAC_RESULT_UNSUPPORTED",
            format!("Unsupported PAC directive: {first}"),
        ));
    }
    Ok(PacDecision {
        kind,
        proxy,
        raw,
        directives,
    })
}

fn parse_directive(value: &str) -> (String, Option<String>) {
    let mut parts = value.split_whitespace();
    let kind = parts.next().unwrap_or_default().to_ascii_uppercase();
    match kind.as_str() {
        "DIRECT" => ("DIRECT".into(), None),
        "PROXY" | "SOCKS" | "HTTP" | "HTTPS" => {
            let proxy = parts.next().map(str::to_owned);
            if proxy.is_none() {
                ("UNKNOWN".into(), None)
            } else {
                (kind, proxy)
            }
        }
        _ => ("UNKNOWN".into(), None),
    }
}

/// Warn about syntax that Boa accepts but older Windows JScript engines may
/// reject. This is advisory; the selected Windows client remains authoritative.
pub fn compat_notes(script: &str) -> Vec<String> {
    let lower = script.to_ascii_lowercase();
    let mut notes = vec![];
    for (token, label) in [
        ("let ", "let"),
        ("const ", "const"),
        ("=>", "箭头函数"),
        ("class ", "class"),
        ("`", "模板字符串"),
        ("async ", "async"),
    ] {
        if lower.contains(token) {
            notes.push(format!(
                "PAC 使用了 {label} 语法；Windows WinINet/JScript 可能不支持，建议改用 var/function"
            ));
        }
    }
    notes
}

/// Validate a response returned by a PAC test, including a warning when it
/// contains a fallback to DIRECT (strict no-leak setups must not use it).
pub fn check_result(decision: &PacDecision) -> Vec<String> {
    let mut notes = vec![];
    if decision
        .directives
        .iter()
        .any(|part| part.eq_ignore_ascii_case("DIRECT"))
        && decision.directives.len() > 1
    {
        notes.push("PAC 返回值包含 DIRECT 回退，代理失败时可能直连".into());
    }
    notes
}

#[cfg(test)]
mod tests;
