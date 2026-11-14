//! Read-only decoder and query index for Xray `geosite.dat` and `geoip.dat` protobuf files.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::{Path, PathBuf},
};
use xray_model::{Error, Result};

/// Match type inside an Xray `GeoSite` entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum GeoDomainType {
    Plain,
    Regex,
    Domain,
    Full,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeoDomain {
    pub kind: GeoDomainType,
    pub value: String,
    pub attrs: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeoSiteCategorySummary {
    pub name: String,
    pub domain_count: usize,
    pub full_count: usize,
    pub plain_count: usize,
    pub regex_count: usize,
    pub attributes: Vec<String>,
}

impl GeoSiteCategorySummary {
    pub fn total(&self) -> usize {
        self.domain_count + self.full_count + self.plain_count + self.regex_count
    }

    pub fn pac_compatible(&self) -> usize {
        self.domain_count + self.full_count
    }
}

#[derive(Clone, Debug, Default)]
pub struct GeoSiteIndex {
    entries: BTreeMap<String, Vec<GeoDomain>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeoCidr {
    V4 { network: u32, mask: u32 },
    V6 { network: u128, mask: u128 },
}

impl GeoCidr {
    fn contains(self, ip: IpAddr) -> bool {
        match (self, ip) {
            (Self::V4 { network, mask }, IpAddr::V4(addr)) => u32::from(addr) & mask == network,
            (Self::V6 { network, mask }, IpAddr::V6(addr)) => u128::from(addr) & mask == network,
            _ => false,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct GeoIpEntry {
    cidrs: Vec<GeoCidr>,
    reverse_match: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GeoIpIndex {
    entries: BTreeMap<String, GeoIpEntry>,
}

/// Loaded `geosite.dat`, `geoip.dat`, and optional `ext:*.dat` files from an asset root.
#[derive(Clone, Debug, Default)]
pub struct GeoData {
    pub asset_root: Option<PathBuf>,
    pub geosite: Option<GeoSiteIndex>,
    pub geoip: Option<GeoIpIndex>,
    ext_sites: BTreeMap<String, GeoSiteIndex>,
    ext_ips: BTreeMap<String, GeoIpIndex>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedGeoSite {
    pub category: String,
    pub domains: Vec<String>,
    pub plain_keywords: Vec<String>,
    pub regex_count: usize,
}

impl GeoData {
    pub fn load_from_asset_root(root: &Path) -> Result<Self> {
        let mut data = Self {
            asset_root: Some(root.to_path_buf()),
            ..Self::default()
        };
        let geosite_path = root.join("geosite.dat");
        if geosite_path.is_file() {
            data.geosite = Some(GeoSiteIndex::load(&geosite_path)?);
        }
        let geoip_path = root.join("geoip.dat");
        if geoip_path.is_file() {
            data.geoip = Some(GeoIpIndex::load(&geoip_path)?);
        }
        Ok(data)
    }

    pub fn from_indexes(geosite: Option<GeoSiteIndex>, geoip: Option<GeoIpIndex>) -> Self {
        Self {
            asset_root: None,
            geosite,
            geoip,
            ext_sites: BTreeMap::new(),
            ext_ips: BTreeMap::new(),
        }
    }

    pub fn with_ext_site(mut self, file: impl Into<String>, index: GeoSiteIndex) -> Self {
        self.ext_sites.insert(file.into(), index);
        self
    }

    pub fn with_ext_ip(mut self, file: impl Into<String>, index: GeoIpIndex) -> Self {
        self.ext_ips.insert(file.into(), index);
        self
    }

    pub fn preload_ext_from_config(&mut self, config: &serde_json::Value) {
        let Some(root) = self.asset_root.clone() else {
            return;
        };
        let names = crate::resources::collect_referenced_geo_files(config);
        for name in names {
            if !name.to_ascii_lowercase().ends_with(".dat") {
                continue;
            }
            let path = root.join(&name);
            if !path.is_file() {
                continue;
            }
            if !self.ext_sites.contains_key(&name)
                && let Ok(index) = GeoSiteIndex::load(&path)
                && !index.entries.is_empty()
            {
                self.ext_sites.insert(name.clone(), index);
            }
            if !self.ext_ips.contains_key(&name)
                && let Ok(index) = GeoIpIndex::load(&path)
                && !index.entries.is_empty()
            {
                self.ext_ips.insert(name, index);
            }
        }
    }

    pub fn categories(&self) -> Vec<GeoSiteCategorySummary> {
        self.geosite
            .as_ref()
            .map(GeoSiteIndex::categories)
            .unwrap_or_default()
    }

    pub fn resolve_domain_spec(&self, spec: &str) -> Option<ResolvedGeoSite> {
        let trimmed = spec.trim();
        if let Some(rest) = strip_prefix_ignore_case(trimmed, "ext:") {
            let (file, tag) = rest.split_once(':')?;
            let index = self.ext_sites.get(file)?;
            return index.resolve(tag);
        }
        let tag = strip_prefix_ignore_case(trimmed, "geosite:").unwrap_or(trimmed);
        self.geosite.as_ref()?.resolve(tag)
    }

    pub fn matches_domain_spec(&self, spec: &str, host: &str) -> Option<bool> {
        let host = host.trim().to_ascii_lowercase();
        if host.is_empty() {
            return None;
        }
        let resolved = self.resolve_domain_spec(spec)?;
        for entry in &resolved.domains {
            if let Some(full) = entry.strip_prefix("full:") {
                if host == full {
                    return Some(true);
                }
            } else if host == *entry || host.ends_with(&format!(".{entry}")) {
                return Some(true);
            }
        }
        for keyword in &resolved.plain_keywords {
            if host.contains(keyword) {
                return Some(true);
            }
        }
        if resolved.regex_count > 0 {
            None
        } else {
            Some(false)
        }
    }

    pub fn matches_ip_spec(&self, spec: &str, ip: IpAddr) -> Option<bool> {
        let trimmed = spec.trim();
        if let Some(rest) = strip_prefix_ignore_case(trimmed, "ext:") {
            let (file, tag) = rest.split_once(':')?;
            return self.ext_ips.get(file)?.matches(tag, ip);
        }
        let tag = strip_prefix_ignore_case(trimmed, "geoip:").unwrap_or(trimmed);
        self.geoip.as_ref()?.matches(tag, ip)
    }
}

impl GeoSiteIndex {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut entries = BTreeMap::new();
        let mut cursor = Cursor::new(bytes);
        while !cursor.is_empty() {
            let (field, wire) = cursor.read_tag()?;
            if field == 1 && wire == 2 {
                let payload = cursor.read_bytes()?;
                if let Some((category, domains)) = parse_geosite_entry(payload)? {
                    entries.insert(category, domains);
                }
            } else {
                cursor.skip(wire)?;
            }
        }
        Ok(Self { entries })
    }

    pub fn categories(&self) -> Vec<GeoSiteCategorySummary> {
        self.entries
            .iter()
            .map(|(name, domains)| {
                let mut summary = GeoSiteCategorySummary {
                    name: name.clone(),
                    ..GeoSiteCategorySummary::default()
                };
                let mut attrs = BTreeSet::new();
                for domain in domains {
                    match domain.kind {
                        GeoDomainType::Domain => summary.domain_count += 1,
                        GeoDomainType::Full => summary.full_count += 1,
                        GeoDomainType::Plain => summary.plain_count += 1,
                        GeoDomainType::Regex => summary.regex_count += 1,
                    }
                    for attr in &domain.attrs {
                        attrs.insert(attr.clone());
                    }
                }
                summary.attributes = attrs.into_iter().collect();
                summary
            })
            .collect()
    }

    pub fn resolve(&self, spec: &str) -> Option<ResolvedGeoSite> {
        let (raw_category, attr_filters) = parse_spec_and_attrs(spec)?;
        let entries = self.entries.get(&raw_category)?;
        let mut out = ResolvedGeoSite {
            category: raw_category,
            ..ResolvedGeoSite::default()
        };
        for item in entries {
            if !matches_attrs(&item.attrs, &attr_filters) {
                continue;
            }
            match item.kind {
                GeoDomainType::Domain => {
                    let domain = item
                        .value
                        .trim()
                        .trim_start_matches('.')
                        .to_ascii_lowercase();
                    if !domain.is_empty() {
                        out.domains.push(domain);
                    }
                }
                GeoDomainType::Full => {
                    let full = item.value.trim().to_ascii_lowercase();
                    if !full.is_empty() {
                        out.domains.push(format!("full:{full}"));
                    }
                }
                GeoDomainType::Plain => {
                    let keyword = item.value.trim().to_ascii_lowercase();
                    if !keyword.is_empty() {
                        out.plain_keywords.push(keyword);
                    }
                }
                GeoDomainType::Regex => out.regex_count += 1,
            }
        }
        Some(out)
    }
}

impl GeoIpIndex {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = fs::read(path)?;
        Self::parse(&bytes)
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let mut entries = BTreeMap::new();
        let mut cursor = Cursor::new(bytes);
        while !cursor.is_empty() {
            let (field, wire) = cursor.read_tag()?;
            if field == 1 && wire == 2 {
                let payload = cursor.read_bytes()?;
                if let Some((category, entry)) = parse_geoip_entry(payload)? {
                    entries.insert(category, entry);
                }
            } else {
                cursor.skip(wire)?;
            }
        }
        Ok(Self { entries })
    }

    pub fn categories(&self) -> Vec<(String, usize)> {
        self.entries
            .iter()
            .map(|(name, entry)| (name.clone(), entry.cidrs.len()))
            .collect()
    }

    pub fn matches(&self, tag: &str, ip: IpAddr) -> Option<bool> {
        let key = tag.trim().to_ascii_lowercase();
        let entry = self.entries.get(&key)?;
        let contained = entry.cidrs.iter().any(|cidr| cidr.contains(ip));
        Some(if entry.reverse_match {
            !contained
        } else {
            contained
        })
    }
}

enum AttrFilter {
    Include(String),
    Exclude(String),
}

fn parse_spec_and_attrs(spec: &str) -> Option<(String, Vec<AttrFilter>)> {
    let mut parts = spec.trim().split('@');
    let category = parts.next()?.trim().to_ascii_lowercase();
    if category.is_empty() {
        return None;
    }
    let mut filters = Vec::new();
    for raw in parts {
        let attr = raw.trim().to_ascii_lowercase();
        if attr.is_empty() {
            continue;
        }
        if let Some(neg) = attr.strip_prefix('!') {
            if !neg.is_empty() {
                filters.push(AttrFilter::Exclude(neg.to_owned()));
            }
        } else {
            filters.push(AttrFilter::Include(attr));
        }
    }
    Some((category, filters))
}

fn matches_attrs(attrs: &[String], filters: &[AttrFilter]) -> bool {
    filters.iter().all(|filter| match filter {
        AttrFilter::Include(req) => attrs.iter().any(|a| a == req),
        AttrFilter::Exclude(exc) => attrs.iter().all(|a| a != exc),
    })
}

fn strip_prefix_ignore_case<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    input
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
        .then(|| &input[prefix.len()..])
}

fn parse_geosite_entry(bytes: &[u8]) -> Result<Option<(String, Vec<GeoDomain>)>> {
    let mut category = String::new();
    let mut domains = Vec::new();
    let mut cursor = Cursor::new(bytes);
    while !cursor.is_empty() {
        let (field, wire) = cursor.read_tag()?;
        match (field, wire) {
            (1, 2) => category = cursor.read_string()?.trim().to_ascii_lowercase(),
            (2, 2) => {
                if let Some(domain) = parse_domain(cursor.read_bytes()?)? {
                    domains.push(domain);
                }
            }
            _ => cursor.skip(wire)?,
        }
    }
    if category.is_empty() {
        return Ok(None);
    }
    Ok(Some((category, domains)))
}

fn parse_domain(bytes: &[u8]) -> Result<Option<GeoDomain>> {
    let mut kind = GeoDomainType::Plain;
    let mut value = String::new();
    let mut attrs = Vec::new();
    let mut cursor = Cursor::new(bytes);
    while !cursor.is_empty() {
        let (field, wire) = cursor.read_tag()?;
        match (field, wire) {
            (1, 0) => {
                kind = match cursor.read_varint()? {
                    0 => GeoDomainType::Plain,
                    1 => GeoDomainType::Regex,
                    2 => GeoDomainType::Domain,
                    3 => GeoDomainType::Full,
                    _ => return Ok(None),
                };
            }
            (2, 2) => value = cursor.read_string()?.trim().to_ascii_lowercase(),
            (3, 2) => {
                if let Some(attr) = parse_attribute(cursor.read_bytes()?)? {
                    attrs.push(attr);
                }
            }
            _ => cursor.skip(wire)?,
        }
    }
    if value.is_empty() {
        return Ok(None);
    }
    Ok(Some(GeoDomain { kind, value, attrs }))
}

fn parse_attribute(bytes: &[u8]) -> Result<Option<String>> {
    let mut key = String::new();
    let mut enabled = true;
    let mut cursor = Cursor::new(bytes);
    while !cursor.is_empty() {
        let (field, wire) = cursor.read_tag()?;
        match (field, wire) {
            (1, 2) => key = cursor.read_string()?.trim().to_ascii_lowercase(),
            (2, 0) => enabled = cursor.read_varint()? != 0,
            (3, 0) => enabled = cursor.read_varint()? != 0,
            _ => cursor.skip(wire)?,
        }
    }
    if key.is_empty() || !enabled {
        return Ok(None);
    }
    Ok(Some(key))
}

fn parse_geoip_entry(bytes: &[u8]) -> Result<Option<(String, GeoIpEntry)>> {
    let mut category = String::new();
    let mut entry = GeoIpEntry::default();
    let mut cursor = Cursor::new(bytes);
    while !cursor.is_empty() {
        let (field, wire) = cursor.read_tag()?;
        match (field, wire) {
            (1, 2) => category = cursor.read_string()?.trim().to_ascii_lowercase(),
            (2, 2) => {
                if let Some(cidr) = parse_cidr(cursor.read_bytes()?)? {
                    entry.cidrs.push(cidr);
                }
            }
            (3, 0) => entry.reverse_match = cursor.read_varint()? != 0,
            _ => cursor.skip(wire)?,
        }
    }
    if category.is_empty() {
        return Ok(None);
    }
    Ok(Some((category, entry)))
}

fn parse_cidr(bytes: &[u8]) -> Result<Option<GeoCidr>> {
    let mut ip_bytes: &[u8] = &[];
    let mut prefix: Option<u32> = None;
    let mut cursor = Cursor::new(bytes);
    while !cursor.is_empty() {
        let (field, wire) = cursor.read_tag()?;
        match (field, wire) {
            (1, 2) => ip_bytes = cursor.read_bytes()?,
            (2, 0) => prefix = Some(cursor.read_varint()? as u32),
            _ => cursor.skip(wire)?,
        }
    }
    match ip_bytes.len() {
        4 => {
            let bits = prefix.unwrap_or(32).min(32);
            let mask = if bits == 0 {
                0
            } else {
                u32::MAX << (32 - bits)
            };
            let addr = u32::from(Ipv4Addr::new(
                ip_bytes[0],
                ip_bytes[1],
                ip_bytes[2],
                ip_bytes[3],
            ));
            Ok(Some(GeoCidr::V4 {
                network: addr & mask,
                mask,
            }))
        }
        16 => {
            let bits = prefix.unwrap_or(128).min(128);
            let mask = if bits == 0 {
                0
            } else {
                u128::MAX << (128 - bits)
            };
            let octets: [u8; 16] = ip_bytes.try_into().unwrap();
            let addr = u128::from(Ipv6Addr::from(octets));
            Ok(Some(GeoCidr::V6 {
                network: addr & mask,
                mask,
            }))
        }
        _ => Ok(None),
    }
}

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn is_empty(&self) -> bool {
        self.pos >= self.data.len()
    }

    fn read_varint(&mut self) -> Result<u64> {
        let mut out = 0u64;
        let mut shift = 0u32;
        while shift < 64 {
            let byte = *self
                .data
                .get(self.pos)
                .ok_or_else(|| Error::new("INVALID_GEODATA", "Unexpected EOF in geodata"))?;
            self.pos += 1;
            out |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(out);
            }
            shift += 7;
        }
        Err(Error::new("INVALID_GEODATA", "Malformed protobuf varint"))
    }

    fn read_tag(&mut self) -> Result<(u32, u8)> {
        let tag = self.read_varint()?;
        let field = u32::try_from(tag >> 3)
            .map_err(|_| Error::new("INVALID_GEODATA", "Invalid protobuf field number"))?;
        let wire = (tag & 0x07) as u8;
        if field == 0 {
            return Err(Error::new("INVALID_GEODATA", "Zero protobuf field number"));
        }
        Ok((field, wire))
    }

    fn read_bytes(&mut self) -> Result<&'a [u8]> {
        let len = usize::try_from(self.read_varint()?)
            .map_err(|_| Error::new("INVALID_GEODATA", "Invalid length in geodata"))?;
        let end = self
            .pos
            .checked_add(len)
            .ok_or_else(|| Error::new("INVALID_GEODATA", "Overflow in geodata length"))?;
        let slice = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| Error::new("INVALID_GEODATA", "Truncated geodata record"))?;
        self.pos = end;
        Ok(slice)
    }

    fn read_string(&mut self) -> Result<String> {
        let bytes = self.read_bytes()?;
        std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| Error::new("INVALID_GEODATA", "Invalid UTF-8 string in geodata"))
    }

    fn skip(&mut self, wire: u8) -> Result<()> {
        match wire {
            0 => {
                self.read_varint()?;
            }
            1 => {
                let end = self
                    .pos
                    .checked_add(8)
                    .filter(|&e| e <= self.data.len())
                    .ok_or_else(|| Error::new("INVALID_GEODATA", "Truncated 64-bit geodata"))?;
                self.pos = end;
            }
            2 => {
                self.read_bytes()?;
            }
            5 => {
                let end = self
                    .pos
                    .checked_add(4)
                    .filter(|&e| e <= self.data.len())
                    .ok_or_else(|| Error::new("INVALID_GEODATA", "Truncated 32-bit geodata"))?;
                self.pos = end;
            }
            _ => {
                return Err(Error::new(
                    "INVALID_GEODATA",
                    "Unsupported protobuf wire type",
                ));
            }
        }
        Ok(())
    }
}

#[doc(hidden)]
pub mod test_encoder {
    pub fn push_varint(buf: &mut Vec<u8>, mut val: u64) {
        loop {
            let byte = (val & 0x7f) as u8;
            val >>= 7;
            if val == 0 {
                buf.push(byte);
                break;
            }
            buf.push(byte | 0x80);
        }
    }

    pub fn push_bytes_field(buf: &mut Vec<u8>, field: u32, payload: &[u8]) {
        push_varint(buf, u64::from(field) << 3 | 2);
        push_varint(buf, payload.len() as u64);
        buf.extend_from_slice(payload);
    }

    pub fn push_varint_field(buf: &mut Vec<u8>, field: u32, val: u64) {
        push_varint(buf, u64::from(field) << 3);
        push_varint(buf, val);
    }

    pub type GeoSiteTestDomain<'a> = (u64, &'a str, &'a [&'a str]);
    pub type GeoSiteTestCategory<'a> = (&'a str, &'a [GeoSiteTestDomain<'a>]);
    pub type GeoIpTestCidr<'a> = (&'a [u8], u32);
    pub type GeoIpTestCategory<'a> = (&'a str, &'a [GeoIpTestCidr<'a>], bool);

    pub fn encode_geosite_list(entries: &[GeoSiteTestCategory<'_>]) -> Vec<u8> {
        let mut root = Vec::new();
        for (category, domains) in entries {
            let mut site = Vec::new();
            push_bytes_field(&mut site, 1, category.as_bytes());
            for (kind, value, attrs) in *domains {
                let mut domain = Vec::new();
                push_varint_field(&mut domain, 1, *kind);
                push_bytes_field(&mut domain, 2, value.as_bytes());
                for attr_key in *attrs {
                    let mut attr = Vec::new();
                    push_bytes_field(&mut attr, 1, attr_key.as_bytes());
                    push_varint_field(&mut attr, 2, 1);
                    push_bytes_field(&mut domain, 3, &attr);
                }
                push_bytes_field(&mut site, 2, &domain);
            }
            push_bytes_field(&mut root, 1, &site);
        }
        root
    }

    pub fn encode_geoip_list(entries: &[GeoIpTestCategory<'_>]) -> Vec<u8> {
        let mut root = Vec::new();
        for (category, cidrs, reverse) in entries {
            let mut geoip = Vec::new();
            push_bytes_field(&mut geoip, 1, category.as_bytes());
            for (ip, prefix) in *cidrs {
                let mut cidr = Vec::new();
                push_bytes_field(&mut cidr, 1, ip);
                push_varint_field(&mut cidr, 2, u64::from(*prefix));
                push_bytes_field(&mut geoip, 2, &cidr);
            }
            if *reverse {
                push_varint_field(&mut geoip, 3, 1);
            }
            push_bytes_field(&mut root, 1, &geoip);
        }
        root
    }
}

#[cfg(test)]
mod tests {
    use super::test_encoder::{encode_geoip_list, encode_geosite_list};
    use super::*;

    #[test]
    fn parses_geosite_attributes_and_geoip_cidrs() {
        let site_bytes = encode_geosite_list(&[
            (
                "CN",
                &[
                    (2, "baidu.com", &[]),
                    (3, "api.qq.com", &["ads"]),
                    (0, "taobao", &[]),
                    (1, r"^cdn\d+\.cn$", &[]),
                ],
            ),
            ("PRIVATE", &[(2, "lan", &[]), (3, "localhost", &[])]),
        ]);
        let ip_bytes = encode_geoip_list(&[(
            "CN",
            &[
                (&[223, 5, 5, 0], 24),
                (
                    &[0x24, 0x00, 0xda, 0x00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                    32,
                ),
            ],
            false,
        )]);

        let geosite = GeoSiteIndex::parse(&site_bytes).unwrap();
        let geoip = GeoIpIndex::parse(&ip_bytes).unwrap();
        let data = GeoData::from_indexes(Some(geosite), Some(geoip));

        let categories = data.categories();
        assert_eq!(categories.len(), 2);
        assert_eq!(categories[0].name, "cn");
        assert_eq!(categories[0].pac_compatible(), 2);
        assert_eq!(categories[0].attributes, vec!["ads"]);

        let cn_no_ads = data.resolve_domain_spec("geosite:cn@!ads").unwrap();
        assert_eq!(cn_no_ads.domains, vec!["baidu.com"]);
        assert_eq!(cn_no_ads.plain_keywords, vec!["taobao"]);
        assert_eq!(cn_no_ads.regex_count, 1);

        assert_eq!(
            data.matches_domain_spec("geosite:cn", "www.baidu.com"),
            Some(true)
        );
        assert_eq!(
            data.matches_domain_spec("geosite:cn@!ads", "api.qq.com"),
            None // contains regex_count == 1, so non-matching host stays Unknown
        );
        assert_eq!(
            data.matches_domain_spec("geosite:private", "router.lan"),
            Some(true)
        );
        assert_eq!(
            data.matches_domain_spec("geosite:private", "example.com"),
            Some(false)
        );
        assert_eq!(
            data.matches_ip_spec("geoip:cn", "223.5.5.5".parse().unwrap()),
            Some(true)
        );
        assert_eq!(
            data.matches_ip_spec("geoip:cn", "8.8.8.8".parse().unwrap()),
            Some(false)
        );
    }

    #[test]
    fn rejects_truncated_or_malformed_geodata_without_panicking() {
        assert!(GeoSiteIndex::parse(&[0x0a, 0xff, 0xff, 0x01]).is_err());
        assert!(GeoIpIndex::parse(&[0x08]).is_err());
    }
}
