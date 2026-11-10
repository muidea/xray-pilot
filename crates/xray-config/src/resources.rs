use crate::{atomic_write, write_json};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path},
};
use xray_model::{Error, ResourceFile, Resources, Result, hash, id};

pub fn freeze(value: &Value, source: &Path, output: &Path) -> Result<Resources> {
    let source = fs::canonicalize(source)?;
    let root = output.join(id());
    fs::create_dir_all(&root)?;
    let mut names: BTreeSet<String> = BTreeSet::new();
    let notes = crate::coverage::uncovered(value);
    // Resource prefixes have meaning only in native dependency-bearing fields.
    if let Some(rules) = value.pointer("/routing/rules").and_then(Value::as_array) {
        for rule in rules {
            for field in ["domain", "ip", "source"] {
                for entry in rule
                    .get(field)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let Some(s) = entry.as_str() else { continue };
                    if field == "domain" && s.starts_with("geosite:") {
                        names.insert("geosite.dat".into());
                    }
                    if field != "domain" && s.starts_with("geoip:") {
                        names.insert("geoip.dat".into());
                    }
                    if let Some(rest) = s.strip_prefix("ext:").or_else(|| s.strip_prefix("ext-ip:"))
                        && let Some((file, _)) = rest.split_once(':')
                    {
                        names.insert(file.into());
                    }
                }
            }
        }
    }
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
    for name in names {
        let relative = Path::new(&name);
        if relative.is_absolute()
            || relative
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
        complete: notes.is_empty(),
        notes,
    };
    write_json(&resources.root.join("manifest.json"), &resources)?;
    Ok(resources)
}
pub fn verify(resources: &Resources) -> Result<()> {
    let disk: Resources = crate::read_json(&resources.root.join("manifest.json"))?;
    if xray_model::canonical_hash(&disk)? != xray_model::canonical_hash(resources)? {
        return Err(Error::new(
            "RESOURCE_INTEGRITY_FAILED",
            "Resource manifest changed",
        ));
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
