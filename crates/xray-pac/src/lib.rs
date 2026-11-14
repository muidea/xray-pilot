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
use xray_config::geodata::GeoData;
use xray_model::{Error, PacDraft, PacMode, Result};

pub const DEFAULT_PAC_PORT: u16 = 18080;
/// Threshold above which Windows / Chromium PAC parsers may show noticeable startup latency.
pub const PAC_SIZE_WARNING_BYTES: usize = 512 * 1024;

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

/// Result of extracting plain/wildcard/full domains and `geosite:` tags from native routing rules.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExtractedConfigDomains {
    pub domains: Vec<String>,
    pub geosites: Vec<String>,
    pub skipped_regexp: usize,
    pub skipped_other: usize,
}

/// Extract template-compatible domain entries and `geosite:`/`ext:` tags from `/routing/rules[*]/domain`.
/// Rules relying on `regexp:` or unrecognized inputs are counted as skipped.
pub fn extract_domains_from_config(config: &Value) -> ExtractedConfigDomains {
    let mut out = ExtractedConfigDomains::default();
    let mut seen_domains = HashSet::new();
    let mut seen_geosites = HashSet::new();
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
            if lower.starts_with("geosite:") || lower.starts_with("ext:") {
                if let Some(tag) = normalize_geosite_spec(trimmed)
                    && seen_geosites.insert(tag.to_ascii_lowercase())
                {
                    out.geosites.push(tag);
                }
                continue;
            }
            if lower.starts_with("regexp:") {
                out.skipped_regexp += 1;
                continue;
            }
            if lower.starts_with("ext-ip:") {
                out.skipped_other += 1;
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
                    if seen_domains.insert(domain.to_ascii_lowercase()) {
                        out.domains.push(domain);
                    }
                }
                Err(_) => out.skipped_other += 1,
            }
        }
    }
    out
}

/// Validate and normalize a `geosite:<cat>[@attr]` or `ext:<file>:<tag>[@attr]` specification.
pub fn normalize_geosite_spec(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("geosite:") {
        let cat = rest.trim();
        if cat.is_empty() {
            return None;
        }
        return Some(format!("geosite:{cat}"));
    }
    if let Some(rest) = lower.strip_prefix("ext:") {
        let (file, tag) = rest.split_once(':')?;
        if file.trim().is_empty() || tag.trim().is_empty() {
            return None;
        }
        // Preserve original file case from `input` in case of case-sensitive filesystems.
        let orig_rest = trimmed.get(4..)?;
        let (orig_file, orig_tag) = orig_rest.split_once(':')?;
        return Some(format!(
            "ext:{}:{}",
            orig_file.trim(),
            orig_tag.trim().to_ascii_lowercase()
        ));
    }
    None
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

/// Compilation statistics for UI feedback and size diagnostics.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PacCompileStats {
    pub domain_count: usize,
    pub full_count: usize,
    pub subdomain_count: usize,
    pub keyword_count: usize,
    pub folded_count: usize,
    pub skipped_regex_count: usize,
    pub resolved_geosites: Vec<String>,
    pub missing_geosites: Vec<String>,
    pub script_bytes: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneratedPac {
    pub script: String,
    pub notes: Vec<String>,
    #[serde(default)]
    pub stats: PacCompileStats,
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

#[derive(Default)]
struct DomainMatchSets {
    exact: BTreeSet<String>,
    sub: BTreeSet<String>,
    suffix: BTreeSet<String>,
    keywords: BTreeSet<String>,
}

impl DomainMatchSets {
    fn is_empty(&self) -> bool {
        self.exact.is_empty()
            && self.sub.is_empty()
            && self.suffix.is_empty()
            && self.keywords.is_empty()
    }

    fn insert_normalized_domain(&mut self, normalized: &str) {
        if let Some(full) = normalized.strip_prefix("full:") {
            self.exact.insert(full.to_owned());
        } else if let Some(wild) = normalized.strip_prefix("*.") {
            self.sub.insert(wild.to_owned());
        } else {
            self.suffix.insert(normalized.to_owned());
        }
    }

    fn insert_resolved_geosite(&mut self, resolved: &xray_config::geodata::ResolvedGeoSite) {
        for entry in &resolved.domains {
            if let Some(full) = entry.strip_prefix("full:") {
                self.exact.insert(full.to_owned());
            } else {
                self.suffix.insert(entry.to_owned());
            }
        }
        for kw in &resolved.plain_keywords {
            let trimmed = kw.trim().to_ascii_lowercase();
            if !trimmed.is_empty() {
                self.keywords.insert(trimmed);
            }
        }
    }

    /// Remove exact, subdomain, and suffix entries already covered by a broader parent suffix/wildcard.
    fn fold(&mut self) -> usize {
        let mut folded = 0;
        let suffix_lookup: HashSet<String> = self.suffix.iter().cloned().collect();
        let sub_lookup: HashSet<String> = self.sub.iter().cloned().collect();

        let has_ancestor = |host: &str| {
            let mut rest = host;
            while let Some((_, parent)) = rest.split_once('.') {
                if suffix_lookup.contains(parent) || sub_lookup.contains(parent) {
                    return true;
                }
                rest = parent;
            }
            false
        };

        let before_suffix = self.suffix.len();
        self.suffix.retain(|d| !has_ancestor(d));
        folded += before_suffix - self.suffix.len();

        let before_sub = self.sub.len();
        self.sub
            .retain(|d| !(suffix_lookup.contains(d) || has_ancestor(d)));
        folded += before_sub - self.sub.len();

        let before_exact = self.exact.len();
        self.exact
            .retain(|d| !(suffix_lookup.contains(d) || has_ancestor(d)));
        folded += before_exact - self.exact.len();

        folded
    }

    fn add_to_stats(&self, stats: &mut PacCompileStats, folded: usize) {
        stats.domain_count += self.suffix.len();
        stats.full_count += self.exact.len();
        stats.subdomain_count += self.sub.len();
        stats.keyword_count += self.keywords.len();
        stats.folded_count += folded;
    }

    fn js_args_suffix(&self, prefix: &str, decls: &mut Vec<String>) -> String {
        let mut bind = |name: &str, json: Option<String>| -> String {
            match json {
                Some(literal) => {
                    let var_name = format!("{prefix}_{name}");
                    decls.push(format!("var {var_name} = {literal};"));
                    var_name
                }
                None => "null".to_owned(),
            }
        };
        let exact_arg = bind(
            "EXACT",
            (!self.exact.is_empty()).then(|| js_domain_set(&self.exact)),
        );
        let sub_arg = bind(
            "SUB",
            (!self.sub.is_empty()).then(|| js_domain_set(&self.sub)),
        );
        let suffix_arg = bind(
            "SUFFIX",
            (!self.suffix.is_empty()).then(|| js_domain_set(&self.suffix)),
        );
        let kw_arg = bind(
            "KW",
            (!self.keywords.is_empty()).then(|| js_keyword_array(&self.keywords)),
        );
        format!("{exact_arg}, {sub_arg}, {suffix_arg}, {kw_arg}")
    }
}

fn js_keyword_array(entries: &BTreeSet<String>) -> String {
    let mut out = String::from("[");
    for (idx, item) in entries.iter().enumerate() {
        if idx > 0 {
            out.push(',');
        }
        out.push_str(&string_literal(item));
    }
    out.push(']');
    out
}

const PAC_MATCH_HELPER: &str = r#"var __PAC_OWN = Object.prototype.hasOwnProperty;
function __pacMatchHost(host, exact, sub, suffix, keywords) {
    if (exact && __PAC_OWN.call(exact, host)) {
        return true;
    }
    if (suffix && __PAC_OWN.call(suffix, host)) {
        return true;
    }
    if (sub || suffix) {
        var pos = host.indexOf(".");
        while (pos !== -1) {
            var parent = host.substring(pos + 1);
            if (sub && __PAC_OWN.call(sub, parent)) {
                return true;
            }
            if (suffix && __PAC_OWN.call(suffix, parent)) {
                return true;
            }
            pos = host.indexOf(".", pos + 1);
        }
    }
    if (keywords) {
        for (var i = 0; i < keywords.length; i++) {
            if (host.indexOf(keywords[i]) !== -1) {
                return true;
            }
        }
    }
    return false;
}
"#;

const PAC_LAN_BYPASS: &str = "    if (host === \"localhost\" || host === \"127.0.0.1\" || host === \"::1\" ||\n        isPlainHostName(host) || dnsDomainIs(host, \".lan\") || dnsDomainIs(host, \".local\")) {\n        return \"DIRECT\";\n    }\n";

fn compile_list_template(
    endpoint: SocketAddr,
    mode: PacMode,
    geosites: &[String],
    domains: &[String],
    geo: Option<&GeoData>,
) -> Result<GeneratedPac> {
    let mut sets = DomainMatchSets::default();
    let mut stats = PacCompileStats::default();
    let mut notes = Vec::new();

    for spec in geosites {
        let Some(norm_spec) = normalize_geosite_spec(spec) else {
            return Err(Error::new(
                "PAC_GEOSITE_INVALID",
                format!("无效的 GeoSite 规则：{spec}"),
            ));
        };
        match geo.and_then(|g| g.resolve_domain_spec(&norm_spec)) {
            Some(resolved) => {
                sets.insert_resolved_geosite(&resolved);
                if resolved.regex_count > 0 {
                    stats.skipped_regex_count += resolved.regex_count;
                    notes.push(format!(
                        "{norm_spec} 包含 {} 条正则条目，PAC 已忽略正则并由 Xray 路由兜底",
                        resolved.regex_count
                    ));
                }
                stats.resolved_geosites.push(norm_spec);
            }
            None => {
                stats.missing_geosites.push(norm_spec.clone());
            }
        }
    }

    if !stats.missing_geosites.is_empty() {
        notes.push(format!(
            "以下分类在当前 geosite.dat 中未找到（或资源未加载）：{}",
            stats.missing_geosites.join("、")
        ));
    }

    for domain in domains {
        let value = normalize_domain(domain)?;
        sets.insert_normalized_domain(&value);
    }

    let folded = sets.fold();
    sets.add_to_stats(&mut stats, folded);

    let script = if mode == PacMode::All || sets.is_empty() {
        let branch = match mode {
            PacMode::All | PacMode::DirectList => format!("return \"PROXY {endpoint}\";"),
            PacMode::ProxyList => "return \"DIRECT\";".to_owned(),
            PacMode::AutoRules | PacMode::Custom => unreachable!(),
        };
        format!(
            "function FindProxyForURL(url, host) {{\n    host = host.toLowerCase();\n{PAC_LAN_BYPASS}    {branch}\n}}\n"
        )
    } else {
        let mut decls = Vec::new();
        let args = sets.js_args_suffix("__PAC", &mut decls);
        let decl_block = decls.join("\n");
        let (hit, miss) = match mode {
            PacMode::ProxyList => (format!("\"PROXY {endpoint}\""), "\"DIRECT\"".to_owned()),
            PacMode::DirectList => ("\"DIRECT\"".to_owned(), format!("\"PROXY {endpoint}\"")),
            PacMode::All | PacMode::AutoRules | PacMode::Custom => unreachable!(),
        };
        let decl_section = if decl_block.is_empty() {
            String::new()
        } else {
            format!("{decl_block}\n")
        };
        format!(
            "{decl_section}{PAC_MATCH_HELPER}function FindProxyForURL(url, host) {{\n    host = host.toLowerCase();\n{PAC_LAN_BYPASS}    if (__pacMatchHost(host, {args})) {{\n        return {hit};\n    }}\n    return {miss};\n}}\n"
        )
    };

    stats.script_bytes = script.len();
    if stats.script_bytes > PAC_SIZE_WARNING_BYTES {
        notes.push(format!(
            "生成的 PAC 脚本体积为 {:.0} KiB（超过 512 KiB），建议改用「按路由规则自动生成」或精简分组",
            stats.script_bytes as f64 / 1024.0
        ));
    }

    Ok(GeneratedPac {
        script,
        notes,
        stats,
    })
}

/// Build a built-in domain-list or proxy-all template without GeoData expansion.
pub fn template(endpoint: SocketAddr, mode: PacMode, domains: &[String]) -> Result<String> {
    if matches!(mode, PacMode::AutoRules | PacMode::Custom) {
        return Err(Error::new(
            "PAC_MODE_UNSUPPORTED",
            "该 PAC 模式需要通过完整编译器生成",
        ));
    }
    compile_list_template(endpoint, mode, &[], domains, None).map(|g| g.script)
}

/// Built-in PAC template with the default "proxy everything except LAN" policy.
pub fn default_template(endpoint: SocketAddr) -> String {
    compile_list_template(endpoint, PacMode::All, &[], &[], None)
        .expect("default PAC template is valid")
        .script
}

/// Compile a user's `PacDraft` against the active config and loaded `GeoData`.
pub fn compile_draft(
    config: &Value,
    endpoint: SocketAddr,
    draft: &PacDraft,
    geo: Option<&GeoData>,
) -> Result<GeneratedPac> {
    match draft.mode {
        PacMode::AutoRules => Ok(generate_with_geo(config, endpoint, geo)),
        PacMode::All | PacMode::DirectList | PacMode::ProxyList => {
            compile_list_template(endpoint, draft.mode, &draft.geosites, &draft.domains, geo)
        }
        PacMode::Custom => {
            if draft.script.trim().is_empty() {
                return Err(Error::new(
                    "PAC_SCRIPT_REQUIRED",
                    "请先输入自定义 PAC 脚本内容",
                ));
            }
            validate(&draft.script)?;
            let mut notes = compat_notes(&draft.script);
            let script_bytes = draft.script.len();
            if script_bytes > PAC_SIZE_WARNING_BYTES {
                notes.push(format!(
                    "自定义 PAC 脚本体积为 {:.0} KiB（超过 512 KiB）",
                    script_bytes as f64 / 1024.0
                ));
            }
            Ok(GeneratedPac {
                script: draft.script.clone(),
                notes,
                stats: PacCompileStats {
                    script_bytes,
                    ..Default::default()
                },
            })
        }
    }
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

fn compile_domain_patterns(
    patterns: &[String],
    geo: Option<&GeoData>,
    stats: &mut PacCompileStats,
    notes: &mut Vec<String>,
) -> Option<(DomainMatchSets, bool)> {
    let mut sets = DomainMatchSets::default();
    let mut partial_regex = false;
    for raw in patterns {
        let pattern = raw.trim();
        if pattern.is_empty() {
            return None;
        }
        let lower = pattern.to_ascii_lowercase();
        if let Some(full) = lower.strip_prefix("full:") {
            let host = full.trim_start_matches('.').trim_end_matches('.');
            if host.is_empty() {
                return None;
            }
            sets.exact.insert(host.to_owned());
        } else if let Some(suffix) = lower.strip_prefix("domain:") {
            let suffix = suffix.trim_start_matches('.').trim_end_matches('.');
            if suffix.is_empty() {
                return None;
            }
            sets.suffix.insert(suffix.to_owned());
        } else if let Some(kw) = lower.strip_prefix("keyword:") {
            let kw = kw.trim();
            if kw.is_empty() {
                return None;
            }
            sets.keywords.insert(kw.to_owned());
        } else if lower.starts_with("geosite:") || lower.starts_with("ext:") {
            let norm = normalize_geosite_spec(pattern)?;
            match geo.and_then(|g| g.resolve_domain_spec(&norm)) {
                Some(resolved) => {
                    sets.insert_resolved_geosite(&resolved);
                    if resolved.regex_count > 0 {
                        partial_regex = true;
                        stats.skipped_regex_count += resolved.regex_count;
                    }
                    if !stats.resolved_geosites.contains(&norm) {
                        stats.resolved_geosites.push(norm);
                    }
                }
                None => {
                    if !stats.missing_geosites.contains(&norm) {
                        stats.missing_geosites.push(norm.clone());
                        notes.push(format!(
                            "规则中的 {norm} 无法展开（geosite.dat 未加载或缺少该分组）"
                        ));
                    }
                    return None;
                }
            }
        } else if lower.contains(':') {
            // regexp: and other native prefixes cannot be expressed reliably in PAC.
            return None;
        } else {
            sets.keywords.insert(lower);
        }
    }
    let folded = sets.fold();
    sets.add_to_stats(stats, folded);
    if sets.is_empty() {
        return None;
    }
    Some((sets, partial_regex))
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
    let action = if balancer.is_some() {
        Action::Proxy
    } else {
        Action::of(None)
    };
    Some((patterns, action))
}

/// Generate a conservative PAC script from native routing rules without GeoData.
pub fn generate(config: &Value, endpoint: SocketAddr) -> GeneratedPac {
    generate_with_geo(config, endpoint, None)
}

/// Generate a conservative PAC script from native routing rules with optional GeoData expansion.
///
/// Only rules whose complete match condition is a domain list are compiled.
/// A direct decision is emitted only while every preceding rule is also
/// safely expressible; otherwise traffic falls through to PROXY and Xray applies
/// the full native rule set. This may under-direct, but never leaks traffic
/// that a native proxy/block rule would have captured.
pub fn generate_with_geo(
    config: &Value,
    endpoint: SocketAddr,
    geo: Option<&GeoData>,
) -> GeneratedPac {
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
    let mut stats = PacCompileStats::default();
    let initial_direct = default_tag
        .and_then(|tag| protocols.get(tag).copied())
        .is_some_and(|protocol| protocol == "freedom");
    let rules: Vec<Value> = config
        .pointer("/routing/rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    struct Rule {
        index: usize,
        sets: Option<DomainMatchSets>,
        action: Action,
        expressible: bool,
        has_unexpressible_regex: bool,
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

        let (sets, expressible, has_unexpressible_regex) = match domain_rule(rule) {
            Some((patterns, _)) if !catch_all => {
                match compile_domain_patterns(&patterns, geo, &mut stats, &mut notes) {
                    Some((sets, partial_regex)) => {
                        if partial_regex {
                            notes.push(format!(
                                "规则 {} 的 geosite 包含正则条目，PAC 忽略正则并由 Xray 路由兜底",
                                index + 1
                            ));
                        }
                        (Some(sets), true, partial_regex)
                    }
                    None => (None, false, false),
                }
            }
            _ => (None, false, false),
        };

        resolved.push(Rule {
            index,
            sets,
            action,
            expressible,
            has_unexpressible_regex,
            catch_all,
        });
    }

    // A direct default is only safe when no preceding proxy/block rule has unexpressible parts.
    let mut default_direct = initial_direct;
    if default_direct
        && resolved.iter().any(|rule| {
            (!rule.expressible || rule.has_unexpressible_regex) && rule.action == Action::Proxy
        })
    {
        default_direct = false;
        notes.push(
            "存在无法用 PAC 完整表达的前序代理/阻断规则，默认改为 PROXY，由 Xray 保留原生路由"
                .into(),
        );
    }

    let mut decls = Vec::new();
    let mut blocks: Vec<(String, &'static str)> = Vec::new();
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

        let sets = rule.sets.as_ref().unwrap();
        let make_condition = |decls: &mut Vec<String>, rule_idx: usize| {
            let args = sets.js_args_suffix(&format!("__PAC_R{rule_idx}"), decls);
            format!("__pacMatchHost(host, {args})")
        };

        match (default_direct, rule.action) {
            (false, Action::Direct) => {
                if direct_allowed {
                    let cond = make_condition(&mut decls, rule.index);
                    blocks.push((cond, "DIRECT"));
                } else {
                    notes.push(format!(
                        "规则 {} 的直连位于无法用 PAC 保留的代理/阻断规则之后，保留 PROXY",
                        rule.index + 1
                    ));
                }
            }
            (false, Action::Proxy) => {
                // Default is already PROXY; this rule blocks later DIRECT entries because
                // later rules in Xray cannot override an earlier proxy/block match.
                direct_allowed = false;
            }
            (true, Action::Proxy) => {
                let cond = make_condition(&mut decls, rule.index);
                blocks.push((cond, "PROXY"));
                if rule.has_unexpressible_regex {
                    direct_allowed = false;
                }
            }
            (true, Action::Direct) => {}
        }
    }

    let mut script = String::new();
    if !decls.is_empty() {
        for decl in decls {
            script.push_str(&decl);
            script.push('\n');
        }
    }
    if !blocks.is_empty() {
        script.push_str(PAC_MATCH_HELPER);
    }
    script.push_str("function FindProxyForURL(url, host) {\n");
    script.push_str("    host = host.toLowerCase();\n");
    script.push_str(
        "    if (host === \"localhost\" || host === \"127.0.0.1\" || host === \"::1\") {\n",
    );
    script.push_str("        return \"DIRECT\";\n");
    script.push_str("    }\n");
    for (condition, result) in &blocks {
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
    stats.script_bytes = script.len();
    if stats.script_bytes > PAC_SIZE_WARNING_BYTES {
        notes.push(format!(
            "生成的 PAC 脚本体积为 {:.0} KiB（超过 512 KiB）",
            stats.script_bytes as f64 / 1024.0
        ));
    }
    GeneratedPac {
        script,
        notes,
        stats,
    }
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
