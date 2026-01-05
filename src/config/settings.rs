use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsConfig {
    /// Terminal font size
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: f32,

    /// Dark mode enabled
    #[serde(default = "default_dark_mode")]
    pub dark_mode: bool,
}

fn default_terminal_font_size() -> f32 {
    9.0
}

fn default_dark_mode() -> bool {
    true
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            terminal_font_size: default_terminal_font_size(),
            dark_mode: default_dark_mode(),
        }
    }
}

impl SettingsConfig {
    /// Load from file, creating default if not exists
    pub fn load() -> Result<Self, ConfigError> {
        let path = super::paths::settings_file().ok_or_else(|| ConfigError::ReadFile {
            path: std::path::PathBuf::from("settings.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine settings file path",
            ),
        })?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path).map_err(|e| ConfigError::ReadFile {
            path: path.clone(),
            source: e,
        })?;

        toml::from_str(&content).map_err(ConfigError::Parse)
    }

    /// Save to file
    pub fn save(&self) -> Result<(), ConfigError> {
        super::paths::ensure_config_dir().map_err(ConfigError::CreateDir)?;

        let path = super::paths::settings_file().ok_or_else(|| ConfigError::WriteFile {
            path: std::path::PathBuf::from("settings.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine settings file path",
            ),
        })?;

        let content = toml::to_string_pretty(self).map_err(ConfigError::Serialize)?;
        std::fs::write(&path, content).map_err(|e| ConfigError::WriteFile { path, source: e })
    }
}
