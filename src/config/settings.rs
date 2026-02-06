use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::error::ConfigError;
use crate::fonts::TerminalFont;
use crate::theme::ThemeId;
use crate::views::sftp::ColumnWidths;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VncEncodingPreference {
    #[default]
    Auto,
    Tight,
    Zrle,
    Raw,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VncScalingMode {
    #[default]
    Fit,
    Actual,
    Stretch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SessionLogFormat {
    Plain,
    #[default]
    Timestamped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VncSettings {
    /// Preferred encoding selection
    #[serde(default)]
    pub encoding: VncEncodingPreference,

    /// Preferred color depth (bits per pixel)
    #[serde(default = "default_vnc_color_depth")]
    pub color_depth: u8,

    /// Refresh rate for VNC updates (frames per second)
    #[serde(default = "default_vnc_refresh_fps")]
    pub refresh_fps: u32,

    /// Max number of VNC events processed per poll tick
    #[serde(default = "default_vnc_max_events_per_tick")]
    pub max_events_per_tick: usize,

    /// Minimum interval between pointer events (ms)
    #[serde(default = "default_vnc_pointer_interval_ms")]
    pub pointer_interval_ms: u64,

    /// Request remote desktop resize based on client window size
    #[serde(default = "default_vnc_remote_resize")]
    pub remote_resize: bool,

    /// Enable bidirectional clipboard sharing
    #[serde(default = "default_vnc_clipboard_sharing")]
    pub clipboard_sharing: bool,

    /// Scaling mode for VNC display
    #[serde(default)]
    pub scaling_mode: VncScalingMode,
}

impl Default for VncSettings {
    fn default() -> Self {
        Self {
            encoding: VncEncodingPreference::default(),
            color_depth: default_vnc_color_depth(),
            refresh_fps: default_vnc_refresh_fps(),
            max_events_per_tick: default_vnc_max_events_per_tick(),
            pointer_interval_ms: default_vnc_pointer_interval_ms(),
            remote_resize: default_vnc_remote_resize(),
            clipboard_sharing: default_vnc_clipboard_sharing(),
            scaling_mode: VncScalingMode::default(),
        }
    }
}

impl VncSettings {
    pub fn apply_env_overrides(mut self) -> Self {
        if let Ok(raw) = std::env::var("PORTAL_VNC_ENCODING") {
            let value = raw.trim().to_lowercase();
            self.encoding = match value.as_str() {
                "auto" => VncEncodingPreference::Auto,
                "tight" => VncEncodingPreference::Tight,
                "zrle" => VncEncodingPreference::Zrle,
                "raw" => VncEncodingPreference::Raw,
                _ => self.encoding,
            };
        }

        if let Ok(raw) = std::env::var("PORTAL_VNC_COLOR_DEPTH") {
            if let Ok(bits) = raw.trim().parse::<u8>() {
                if matches!(bits, 16 | 32) {
                    self.color_depth = bits;
                }
            }
        }

        if let Ok(raw) = std::env::var("PORTAL_VNC_REFRESH_FPS") {
            if let Ok(fps) = raw.trim().parse::<u32>() {
                self.refresh_fps = fps.clamp(1, 60);
            }
        }

        if let Ok(raw) = std::env::var("PORTAL_VNC_MAX_EVENTS_PER_TICK") {
            if let Ok(count) = raw.trim().parse::<usize>() {
                self.max_events_per_tick = count.clamp(1, 1024);
            }
        }

        if let Ok(raw) = std::env::var("PORTAL_VNC_POINTER_INTERVAL_MS") {
            if let Ok(ms) = raw.trim().parse::<u64>() {
                self.pointer_interval_ms = ms.min(1000);
            }
        }

        if let Ok(raw) = std::env::var("PORTAL_VNC_REMOTE_RESIZE") {
            let value = raw.trim().to_lowercase();
            self.remote_resize = matches!(value.as_str(), "1" | "true" | "yes" | "on");
        }

        self
    }
}

fn default_vnc_refresh_fps() -> u32 {
    10
}

fn default_vnc_color_depth() -> u8 {
    32
}

fn default_vnc_max_events_per_tick() -> usize {
    64
}

fn default_vnc_pointer_interval_ms() -> u64 {
    16
}

fn default_vnc_remote_resize() -> bool {
    false
}

fn default_vnc_clipboard_sharing() -> bool {
    true
}

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

    /// VNC settings
    #[serde(default)]
    pub vnc: VncSettings,

    /// Auto-reconnect for SSH sessions
    #[serde(default = "default_auto_reconnect")]
    pub auto_reconnect: bool,

    /// Maximum number of reconnect attempts
    #[serde(default = "default_reconnect_max_attempts")]
    pub reconnect_max_attempts: u32,

    /// Base reconnect delay in milliseconds
    #[serde(default = "default_reconnect_base_delay_ms")]
    pub reconnect_base_delay_ms: u64,

    /// Maximum reconnect delay in milliseconds
    #[serde(default = "default_reconnect_max_delay_ms")]
    pub reconnect_max_delay_ms: u64,

    /// Legacy dark_mode field for migration (read-only, not serialized)
    #[serde(default, skip_serializing)]
    dark_mode: Option<bool>,

    /// Enable logging terminal session output to disk
    #[serde(default = "default_session_logging_enabled")]
    pub session_logging_enabled: bool,

    /// Directory for session log files
    #[serde(
        default = "default_session_log_dir",
        skip_serializing_if = "Option::is_none"
    )]
    pub session_log_dir: Option<PathBuf>,

    /// Session log format
    #[serde(default)]
    pub session_log_format: SessionLogFormat,
}

fn default_terminal_font_size() -> f32 {
    9.0
}

fn default_auto_reconnect() -> bool {
    true
}

fn default_reconnect_max_attempts() -> u32 {
    5
}

fn default_reconnect_base_delay_ms() -> u64 {
    1000
}

fn default_reconnect_max_delay_ms() -> u64 {
    30_000
}

fn default_session_logging_enabled() -> bool {
    false
}

fn default_session_log_dir() -> Option<PathBuf> {
    crate::config::paths::config_dir().map(|dir| dir.join("logs").join("sessions"))
}

impl Default for SettingsConfig {
    fn default() -> Self {
        Self {
            terminal_font_size: default_terminal_font_size(),
            terminal_font: TerminalFont::default(),
            theme: ThemeId::default(),
            ui_scale: None,
            sftp_column_widths: ColumnWidths::default(),
            vnc: VncSettings::default(),
            auto_reconnect: default_auto_reconnect(),
            reconnect_max_attempts: default_reconnect_max_attempts(),
            reconnect_base_delay_ms: default_reconnect_base_delay_ms(),
            reconnect_max_delay_ms: default_reconnect_max_delay_ms(),
            dark_mode: None,
            session_logging_enabled: default_session_logging_enabled(),
            session_log_dir: default_session_log_dir(),
            session_log_format: SessionLogFormat::default(),
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
