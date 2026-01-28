use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::fonts::TerminalFont;
use crate::theme::ThemeId;
use crate::views::sftp::ColumnWidths;

/// Application settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsConfig {
    /// Terminal font size
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: f32,

    /// Terminal font family
    #[serde(default)]
    pub terminal_font: TerminalFont,

    /// Selected theme
    #[serde(default)]
    pub theme: ThemeId,

    /// UI scale override (None = use system default)
    /// Range: 0.8 to 1.5 (80% to 150%)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_scale: Option<f32>,

    /// SFTP file list column widths
    #[serde(default)]
    pub sftp_column_widths: ColumnWidths,

    /// Legacy dark_mode field for migration (read-only, not serialized)
    #[serde(default, skip_serializing)]
    dark_mode: Option<bool>,
}

fn default_terminal_font_size() -> f32 {
    9.0
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            terminal_font_size: default_terminal_font_size(),
            terminal_font: TerminalFont::default(),
            theme: ThemeId::default(),
            ui_scale: None,
            sftp_column_widths: ColumnWidths::default(),
            dark_mode: None,
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

        let mut config: Self = toml::from_str(&content).map_err(ConfigError::Parse)?;
        let mut needs_save = false;

        // Migration: convert old dark_mode to new theme field
        if let Some(dark_mode) = config.dark_mode.take() {
            config.theme = if dark_mode {
                ThemeId::PortalDefault
            } else {
                ThemeId::CatppuccinLatte
            };
            needs_save = true;
        }

        // Migration: convert old proportion-based column widths to pixel-based
        // Old values were typically 4-20, new values should be 60+ pixels
        if config.sftp_column_widths.name < 50.0
            || config.sftp_column_widths.date_modified < 50.0
            || config.sftp_column_widths.size < 50.0
            || config.sftp_column_widths.kind < 50.0
        {
            config.sftp_column_widths = ColumnWidths::default();
            needs_save = true;
        }

        // Save migrated config to persist the changes
        if needs_save {
            if let Err(e) = config.save() {
                tracing::warn!("Failed to save migrated settings: {}", e);
            }
        }

        Ok(config)
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
        super::write_atomic(&path, &content).map_err(|e| ConfigError::WriteFile { path, source: e })
    }
}
