use eframe::egui;
use serde::{Deserialize, Serialize};
use std::path::Path;
use xray_model::Result;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}
impl Appearance {
    pub fn apply(self, context: &egui::Context) {
        context.set_theme(match self {
            Self::System => egui::ThemePreference::System,
            Self::Light => egui::ThemePreference::Light,
            Self::Dark => egui::ThemePreference::Dark,
        });
    }
    pub fn load(root: &Path) -> Result<Self> {
        Ok(xray_config::optional_json(&root.join("config/appearance.json"))?.unwrap_or_default())
    }
    pub fn save(self, root: &Path) -> Result<()> {
        xray_config::write_json(&root.join("config/appearance.json"), &self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appearance_defaults_to_system_and_roundtrips_all_choices() {
        let data = tempfile::tempdir().unwrap();
        assert_eq!(Appearance::load(data.path()).unwrap(), Appearance::System);
        let context = egui::Context::default();
        for (appearance, expected) in [
            (Appearance::Light, egui::ThemePreference::Light),
            (Appearance::Dark, egui::ThemePreference::Dark),
            (Appearance::System, egui::ThemePreference::System),
        ] {
            appearance.apply(&context);
            assert_eq!(context.options(|o| o.theme_preference), expected);
            appearance.save(data.path()).unwrap();
            assert_eq!(Appearance::load(data.path()).unwrap(), appearance);
        }
    }
    #[test]
    fn invalid_appearance_is_reported_and_preserved() {
        let data = tempfile::tempdir().unwrap();
        let config = data.path().join("config");
        std::fs::create_dir(&config).unwrap();
        std::fs::write(config.join("appearance.json"), b"broken").unwrap();
        assert!(Appearance::load(data.path()).is_err());
        assert_eq!(
            std::fs::read(config.join("appearance.json")).unwrap(),
            b"broken"
        );
    }
}
