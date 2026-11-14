use crate::{atomic_write, write_json};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};
use xray_model::{DynamicResource, Error, ResourceFile, Resources, Result, hash, id};

pub fn freeze(value: &Value, source: &Path, output: &Path) -> Result<Resources> {
    freeze_with(value, source, output, false)
}

pub fn freeze_with(
    value: &Value,
    source: &Path,
    output: &Path,
    allow_dynamic: bool,
) -> Result<Resources> {
    let source = fs::canonicalize(source)?;
    let root = output.join(id());
    fs::create_dir_all(&root)?;
    let mut names = collect_referenced_geo_files(value);
    let notes = crate::coverage::uncovered(value);
    for direction in ["inbounds", "outbounds"] {
        for endpoint in value
            .get(direction)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for cert in endpoint
                .pointer("/streamSettings/tlsSettings/certificates")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                for field in ["certificateFile", "keyFile"] {
                    if let Some(name) = cert.get(field).and_then(Value::as_str) {
                        names.insert(name.into());
                    }
                }
            }
        }
    }
    let mut files = vec![];
    let mut dynamic = vec![];
    let mut notes = notes;
    for name in names {
        let relative = Path::new(&name);
        if relative.is_absolute() {
            if !allow_dynamic {
                return Err(Error::new(
                    "RESOURCE_PATH_REQUIRES_EDIT",
                    format!(
                        "Use a relative resource path inside the asset directory, or explicitly allow dynamic inputs: {name}"
                    ),
                ));
            }
            let original = fs::canonicalize(relative)?;
            let bytes = fs::read(&original)?;
            dynamic.push(DynamicResource {
                source: original,
                sha256: hash(&bytes),
                observed_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            });
            notes.push(format!(
                "Dynamic external resource {name}; automatic recovery is disabled and the file is rechecked before start"
            ));
            continue;
        }
        if relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(Error::new(
                "RESOURCE_PATH_REQUIRES_EDIT",
                format!("Use a relative resource path inside the asset directory: {name}"),
            ));
        }
        let original = fs::canonicalize(source.join(relative))?;
        if !original.starts_with(&source) {
            return Err(Error::new(
                "RESOURCE_PATH_REQUIRES_EDIT",
                "Resource escapes asset directory",
            ));
        }
        let bytes = fs::read(&original)?;
        let digest = hash(&bytes);
        atomic_write(&root.join(relative), &bytes)?;
        if hash(&fs::read(&original)?) != digest {
            return Err(Error::new(
                "RESOURCE_REVALIDATION_REQUIRED",
                "Resource changed while freezing",
            ));
        }
        files.push(ResourceFile {
            source: original,
            relative_path: relative.into(),
            sha256: digest,
        });
    }
    let resources = Resources {
        id: root.file_name().unwrap().to_string_lossy().into(),
        root,
        files,
        complete: notes.is_empty() && dynamic.is_empty(),
        notes,
        dynamic,
    };
    write_json(&resources.root.join("manifest.json"), &resources)?;
    Ok(resources)
}
/// Collect `.dat` geo resource filenames referenced in routing and DNS sections.
pub(crate) fn collect_referenced_geo_files(value: &Value) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    // Resource prefixes have meaning only in native dependency-bearing fields.
    if let Some(rules) = value.pointer("/routing/rules").and_then(Value::as_array) {
        for rule in rules {
            for field in ["domain", "ip", "source"] {
                for entry in rule
                    .get(field)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    collect_geo(entry, field == "domain", &mut names);
                }
            }
        }
    }
    // Native DNS geo references participate in the same immutable snapshot as routing.
    if let Some(servers) = value.pointer("/dns/servers").and_then(Value::as_array) {
        for server in servers {
            for (field, domain) in [("domains", true), ("expectIPs", false)] {
                for entry in server
                    .get(field)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    collect_geo(entry, domain, &mut names);
                }
            }
        }
    }
    if let Some(hosts) = value.pointer("/dns/hosts").and_then(Value::as_object) {
        for key in hosts.keys() {
            collect_geo(key, true, &mut names);
        }
    }
    names
}

fn collect_geo(entry: &str, domain: bool, names: &mut BTreeSet<String>) {
    if domain && entry.starts_with("geosite:") {
        names.insert("geosite.dat".into());
    }
    if !domain && entry.starts_with("geoip:") {
        names.insert("geoip.dat".into());
    }
    if let Some(rest) = entry
        .strip_prefix("ext:")
        .or_else(|| entry.strip_prefix("ext-ip:"))
        && let Some((file, _)) = rest.split_once(':')
    {
        names.insert(file.into());
    }
}
pub fn verify(resources: &Resources) -> Result<()> {
    let disk: Resources = crate::read_json(&resources.root.join("manifest.json"))?;
    if xray_model::canonical_hash(&disk)? != xray_model::canonical_hash(resources)? {
        return Err(Error::new(
            "RESOURCE_INTEGRITY_FAILED",
            "Resource manifest changed",
        ));
    }
    for dynamic in &resources.dynamic {
        let bytes = fs::read(&dynamic.source).map_err(|_| {
            Error::new(
                "RESOURCE_REVALIDATION_REQUIRED",
                format!("Dynamic resource is missing: {}", dynamic.source.display()),
            )
        })?;
        if hash(&bytes) != dynamic.sha256 {
            return Err(Error::new(
                "RESOURCE_REVALIDATION_REQUIRED",
                format!(
                    "Dynamic resource changed and must be revalidated: {}",
                    dynamic.source.display()
                ),
            ));
        }
    }
    for file in &resources.files {
        let path = resources.root.join(&file.relative_path);
        let bytes = fs::read(&path).map_err(|_| {
            Error::new(
                "RESOURCE_INTEGRITY_FAILED",
                format!("Missing frozen resource: {}", file.relative_path.display()),
            )
        })?;
        if hash(&bytes) != file.sha256 {
            return Err(Error::new(
                "RESOURCE_INTEGRITY_FAILED",
                "Frozen resource changed",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn labels_passwords_and_unknown_fields_are_not_resource_references() {
        let tmp = tempfile::tempdir().unwrap();
        let value = serde_json::json!({
            "metadata":{"note":"geosite:label", "nested":{"certificateFile":"missing.pem"}},
            "outbounds":[{"tag":"geoip:tag","protocol":"trojan","settings":{
                "servers":[{"address":"localhost","port":443,"password":"ext:secret.dat:password"}]
            }}]
        });
        let r = freeze(&value, tmp.path(), &tmp.path().join("frozen")).unwrap();
        assert!(r.files.is_empty());
        assert!(!r.complete);
        assert!(r.notes.iter().any(|n| n.contains("/metadata")));
    }
    #[test]
    fn native_geo_ext_and_certificate_fields_are_frozen() {
        let tmp = tempfile::tempdir().unwrap();
        for name in [
            "geoip.dat",
            "geosite.dat",
            "custom.dat",
            "cert.pem",
            "key.pem",
        ] {
            fs::write(tmp.path().join(name), name).unwrap();
        }
        let r = freeze(&serde_json::json!({
            "routing":{"rules":[{"domain":["geosite:cn", "ext:custom.dat:ads"],"ip":["geoip:private"]}]},
            "outbounds":[{"protocol":"freedom","streamSettings":{"tlsSettings":{"certificates":[{
                "certificateFile":"cert.pem", "keyFile":"key.pem"
            }]}}}]
        }), tmp.path(), &tmp.path().join("frozen")).unwrap();
        assert_eq!(r.files.len(), 5);
        verify(&r).unwrap();
    }
    #[test]
    fn dns_hosts_and_server_geo_inputs_are_frozen_and_unknown_extensions_stay_restricted() {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["geosite.dat", "geoip.dat", "custom.dat"] {
            fs::write(tmp.path().join(name), name).unwrap();
        }
        let mut value = serde_json::json!({"dns":{"hosts":{"geosite:ads":"127.0.0.1"},"servers":[{"address":"https://example.com/dns-query","domains":["ext:custom.dat:cn"],"expectIPs":["geoip:cn"]}]},"outbounds":[{"protocol":"freedom"}]});
        let r = freeze(&value, tmp.path(), &tmp.path().join("frozen")).unwrap();
        assert!(r.complete, "{:?}", r.notes);
        assert_eq!(r.files.len(), 3);
        fs::write(tmp.path().join("geoip.dat"), "updated").unwrap();
        verify(&r).unwrap();
        fs::remove_file(r.root.join("custom.dat")).unwrap();
        assert!(verify(&r).is_err());
        value["dns"]["servers"][0]["vendorFile"] = serde_json::json!("missing");
        assert!(
            !freeze(&value, tmp.path(), &tmp.path().join("other"))
                .unwrap()
                .complete
        );
        value["dns"]["servers"][0]["domains"] = serde_json::json!(["ext:../outside:cn"]);
        assert_eq!(
            freeze(&value, tmp.path(), &tmp.path().join("escape"))
                .unwrap_err()
                .code,
            "RESOURCE_PATH_REQUIRES_EDIT"
        );
    }
    #[test]
    fn source_updates_do_not_change_frozen_resources() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir(&src).unwrap();
        fs::write(src.join("geosite.dat"), "original").unwrap();
        let r = freeze(
            &serde_json::json!({"routing":{"rules":[{"domain":["geosite:cn"]}]}}),
            &src,
            &tmp.path().join("resources"),
        )
        .unwrap();
        fs::write(src.join("geosite.dat"), "updated").unwrap();
        verify(&r).unwrap();
        fs::write(r.root.join("geosite.dat"), "corrupt").unwrap();
        assert_eq!(verify(&r).unwrap_err().code, "RESOURCE_INTEGRITY_FAILED");
    }
}

#[cfg(test)]
mod dynamic_tests {
    use super::*;
    #[test]
    fn absolute_dynamic_inputs_require_explicit_opt_in_and_are_revalidated() {
        let tmp = tempfile::tempdir().unwrap();
        let source = tmp.path().join("assets");
        fs::create_dir(&source).unwrap();
        let external = tmp.path().join("external-cert.pem");
        fs::write(&external, "original").unwrap();
        let config = serde_json::json!({
            "outbounds":[{"protocol":"freedom","streamSettings":{"tlsSettings":{"certificates":[{
                "certificateFile": external.to_string_lossy()
            }]}}}]
        });
        assert_eq!(
            freeze(&config, &source, &tmp.path().join("a"))
                .unwrap_err()
                .code,
            "RESOURCE_PATH_REQUIRES_EDIT"
        );
        let resources = freeze_with(&config, &source, &tmp.path().join("b"), true).unwrap();
        assert!(!resources.complete);
        assert_eq!(resources.dynamic.len(), 1);
        verify(&resources).unwrap();
        fs::write(&external, "changed").unwrap();
        assert_eq!(
            verify(&resources).unwrap_err().code,
            "RESOURCE_REVALIDATION_REQUIRED"
        );
    }
}
