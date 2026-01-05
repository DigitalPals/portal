use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ConfigError;

/// Type of session for history entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SessionType {
    Ssh,
    Sftp,
}

impl SessionType {
    pub fn display_name(&self) -> &str {
        match self {
            SessionType::Ssh => "SSH",
            SessionType::Sftp => "SFTP",
        }
    }
}

/// Single connection history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub host_id: Uuid,
    pub host_name: String,
    pub hostname: String,
    pub username: String,
    pub connected_at: chrono::DateTime<chrono::Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disconnected_at: Option<chrono::DateTime<chrono::Utc>>,
    pub session_type: SessionType,
}

impl HistoryEntry {
    /// Create a new history entry for a connection
    pub fn new(
        host_id: Uuid,
        host_name: String,
        hostname: String,
        username: String,
        session_type: SessionType,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            host_id,
            host_name,
            hostname,
            username,
            connected_at: chrono::Utc::now(),
            disconnected_at: None,
            session_type,
        }
    }

    /// Get duration of session (if disconnected) or time since connection
    pub fn duration(&self) -> chrono::Duration {
        let end = self.disconnected_at.unwrap_or_else(chrono::Utc::now);
        end - self.connected_at
    }

    /// Format duration as human-readable string
    pub fn duration_string(&self) -> String {
        let duration = self.duration();
        let secs = duration.num_seconds();

        if secs < 60 {
            format!("{}s", secs)
        } else if secs < 3600 {
            format!("{}m", secs / 60)
        } else {
            format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
        }
    }
}

fn default_max_entries() -> usize {
    100
}

/// Root configuration for history.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryConfig {
    #[serde(default)]
    pub entries: Vec<HistoryEntry>,
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: default_max_entries(),
        }
    }
}

impl HistoryConfig {
    /// Add a new history entry, trimming old entries if over limit
    pub fn add_entry(&mut self, entry: HistoryEntry) {
        self.entries.insert(0, entry);

        // Trim to max_entries
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }
    }

    /// Find entry by ID
    pub fn find_entry(&self, id: Uuid) -> Option<&HistoryEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Find entry by ID (mutable)
    pub fn find_entry_mut(&mut self, id: Uuid) -> Option<&mut HistoryEntry> {
        self.entries.iter_mut().find(|e| e.id == id)
    }

    /// Mark an entry as disconnected
    pub fn mark_disconnected(&mut self, id: Uuid) {
        if let Some(entry) = self.find_entry_mut(id) {
            entry.disconnected_at = Some(chrono::Utc::now());
        }
    }

    /// Clear all history
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Load from file, creating default if not exists
    pub fn load() -> Result<Self, ConfigError> {
        let path = super::paths::history_file().ok_or_else(|| ConfigError::ReadFile {
            path: std::path::PathBuf::from("history.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine history file path",
            ),
        })?;

        tracing::debug!("Loading history from: {:?}", path);

        if !path.exists() {
            tracing::debug!("History file does not exist: {:?}", path);
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

        let path = super::paths::history_file().ok_or_else(|| ConfigError::WriteFile {
            path: std::path::PathBuf::from("history.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine history file path",
            ),
        })?;

        let content = toml::to_string_pretty(self).map_err(ConfigError::Serialize)?;
        std::fs::write(&path, content).map_err(|e| ConfigError::WriteFile { path, source: e })
    }
}
