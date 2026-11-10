use serde::{Deserialize, Serialize};
use std::{path::Path, sync::mpsc};
use xray_model::{Error, Result};

pub const DEFAULT_VERSION: &str = "v26.11.9";

#[derive(Clone, Deserialize)]
pub struct Bundle {
    pub version: String,
    pub core_path: std::path::PathBuf,
    pub asset_root: std::path::PathBuf,
}
#[derive(Clone, Deserialize)]
pub struct Installation {
    pub version: String,
    pub install_id: String,
    pub valid: bool,
    pub error: Option<String>,
}
#[derive(Default, Deserialize)]
pub struct Report {
    pub selected: Option<Bundle>,
    pub installations: Vec<Installation>,
}
#[derive(Serialize)]
pub struct Job {
    pub action: &'static str,
    pub data_dir: String,
    pub version: String,
    pub install_id: String,
}
impl Job {
    pub fn new(action: &'static str, root: &Path, version: &str, install_id: &str) -> Self {
        Self {
            action,
            data_dir: root.display().to_string(),
            version: version.into(),
            install_id: install_id.into(),
        }
    }
}

pub fn spawn(job: Job, repaint: eframe::egui::Context) -> mpsc::Receiver<Result<Report>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(run(&job));
        repaint.request_repaint();
    });
    rx
}

#[cfg(windows)]
fn run(job: &Job) -> Result<Report> {
    use std::{io::Write, os::windows::process::CommandExt, process::Stdio};
    let windows = std::env::var_os("SystemRoot")
        .ok_or_else(|| Error::new("DEPENDENCY_INSTALL_FAILED", "SystemRoot is unavailable"))?;
    let script = format!(
        "& {{ {} }} -StdinJson",
        include_str!("../../../scripts/windows-dependencies.ps1").trim_start_matches('\u{feff}')
    );
    let mut child = std::process::Command::new(
        Path::new(&windows).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    )
    .args([
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &script,
    ])
    .creation_flags(0x08000000)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&serde_json::to_vec(job)?)?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(Error::new(
            "DEPENDENCY_INSTALL_FAILED",
            String::from_utf8_lossy(&output.stderr)
                .chars()
                .take(4000)
                .collect::<String>(),
        ));
    }
    serde_json::from_slice(&output.stdout).map_err(Into::into)
}

#[cfg(not(windows))]
fn run(_job: &Job) -> Result<Report> {
    Err(Error::new(
        "UNSUPPORTED_PLATFORM",
        "Managed dependency installation is available on Windows x64",
    ))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn embedded_installer_roundtrips_unicode_paths_without_network() {
        let data = tempfile::tempdir().unwrap();
        let root = data.path().join("data spaces 中文");
        std::fs::create_dir(&root).unwrap();
        let canonical = std::fs::canonicalize(&root).unwrap();
        let report = run(&Job::new("Status", &canonical, DEFAULT_VERSION, "")).unwrap();
        assert!(report.selected.is_none());
        assert!(report.installations.is_empty());
        assert!(root.join("dependencies/install.lock").exists());
        assert!(!root.join("config/manager.json").exists());
    }
}
