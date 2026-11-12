//! Offline CLI must not require a running IPC server.
use std::process::Command;

#[test]
fn maintenance_cli_defaults_to_preview_and_rejects_invalid_flags() {
    let tmp = tempfile::tempdir().unwrap();
    xray_config::Store::open(tmp.path()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xrayctl"))
        .arg("--data-dir")
        .arg(tmp.path())
        .arg("maintenance")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["applied"], false);
    assert_eq!(report["retain_days"], 7);
    assert!(report["operations"].as_array().unwrap().is_empty());
    for flag in ["--invalid", "--retain-days"] {
        let invalid = Command::new(env!("CARGO_BIN_EXE_xrayctl"))
            .arg("maintenance")
            .arg("--data-dir")
            .arg(tmp.path())
            .arg(flag)
            .output()
            .unwrap();
        assert!(!invalid.status.success());
        assert!(String::from_utf8_lossy(&invalid.stderr).contains("INVALID_ARGUMENT"));
    }
}
