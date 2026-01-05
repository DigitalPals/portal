use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

/// Application-wide settings stored in config.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    #[serde(default)]
    pub ui: UiConfig,
    #[serde(default)]
    pub ssh: SshDefaults,
}

impl AppConfig {
    /// Load from file, creating default if not exists
    pub fn load() -> Result<Self, ConfigError> {
        let path = super::paths::config_file().ok_or_else(|| ConfigError::ReadFile {
            path: std::path::PathBuf::from("config.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine config file path",
            ),
        })?;

        if !path.exists() {
            let config = Self::default();
            config.save()?;
            return Ok(config);
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

        let path = super::paths::config_file().ok_or_else(|| ConfigError::WriteFile {
            path: std::path::PathBuf::from("config.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine config file path",
            ),
        })?;

        let content = toml::to_string_pretty(self).map_err(ConfigError::Serialize)?;
        std::fs::write(&path, content).map_err(|e| ConfigError::WriteFile { path, source: e })
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_true")]
    pub show_status_bar: bool,
    #[serde(default)]
    pub theme: Theme,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_status_bar: true,
            theme: Theme::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

fn default_timeout() -> u64 {
    30
}

fn default_keepalive() -> u64 {
    60
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshDefaults {
    #[serde(default = "default_timeout")]
    pub connection_timeout_secs: u64,
    #[serde(default = "default_keepalive")]
    pub keepalive_interval_secs: u64,
    #[serde(default)]
    pub default_username: Option<String>,
    #[serde(default)]
    pub session_logging: SessionLoggingConfig,
}

impl Default for SshDefaults {
    fn default() -> Self {
        Self {
            connection_timeout_secs: 30,
            keepalive_interval_secs: 60,
            default_username: None,
            session_logging: SessionLoggingConfig::default(),
        }
    }
}

/// Session logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionLoggingConfig {
    /// Whether session logging is enabled
    #[serde(default)]
    pub enabled: bool,
    /// Directory to store log files (defaults to ~/.config/portal/logs)
    #[serde(default)]
    pub log_directory: Option<String>,
    /// Include timestamps in log output
    #[serde(default = "default_true")]
    pub include_timestamps: bool,
}

impl Default for SessionLoggingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            log_directory: None,
            include_timestamps: true,
        }
    }
}
