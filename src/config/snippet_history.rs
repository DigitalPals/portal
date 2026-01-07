//! Snippet execution history configuration
//!
//! Stores persistent history of snippet executions, including
//! per-host results, timing, and output.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ConfigError;

/// Result of executing a command on a single host (for history)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalHostResult {
    pub host_id: Uuid,
    pub host_name: String,
    pub success: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stdout: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stderr: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Single snippet execution entry in history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetExecutionEntry {
    pub id: Uuid,
    pub snippet_id: Uuid,
    pub snippet_name: String,
    pub command: String,
    pub executed_at: chrono::DateTime<chrono::Utc>,
    pub host_results: Vec<HistoricalHostResult>,
    pub success_count: usize,
    pub failure_count: usize,
    pub total_duration_ms: u64,
}

impl SnippetExecutionEntry {
    /// Create a new execution entry
    pub fn new(
        snippet_id: Uuid,
        snippet_name: String,
        command: String,
        host_results: Vec<HistoricalHostResult>,
    ) -> Self {
        let success_count = host_results.iter().filter(|r| r.success).count();
        let failure_count = host_results.len() - success_count;
        let total_duration_ms = host_results
            .iter()
            .map(|r| r.duration_ms)
            .max()
            .unwrap_or(0);

        Self {
            id: Uuid::new_v4(),
            snippet_id,
            snippet_name,
            command,
            executed_at: chrono::Utc::now(),
            host_results,
            success_count,
            failure_count,
            total_duration_ms,
        }
    }

    /// Format the execution time as a human-readable string
    pub fn time_ago(&self) -> String {
        let now = chrono::Utc::now();
        let duration = now - self.executed_at;
        let secs = duration.num_seconds();

        if secs < 60 {
            "just now".to_string()
        } else if secs < 3600 {
            format!("{}m ago", secs / 60)
        } else if secs < 86400 {
            format!("{}h ago", secs / 3600)
        } else {
            format!("{}d ago", secs / 86400)
        }
    }
}

fn default_max_entries() -> usize {
    50
}

/// Root configuration for snippet execution history
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnippetHistoryConfig {
    #[serde(default)]
    pub entries: Vec<SnippetExecutionEntry>,
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
}

impl Default for SnippetHistoryConfig {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: default_max_entries(),
        }
    }
}

impl SnippetHistoryConfig {
    /// Add a new execution entry, trimming old entries if over limit
    pub fn add_entry(&mut self, entry: SnippetExecutionEntry) {
        self.entries.insert(0, entry);

        // Trim to max_entries
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }
    }

    /// Find entry by ID
    pub fn find_entry(&self, id: Uuid) -> Option<&SnippetExecutionEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Get entries for a specific snippet
    pub fn entries_for_snippet(&self, snippet_id: Uuid) -> Vec<&SnippetExecutionEntry> {
        self.entries
            .iter()
            .filter(|e| e.snippet_id == snippet_id)
            .collect()
    }

    /// Load from file, creating default if not exists
    pub fn load() -> Result<Self, ConfigError> {
        let path = super::paths::snippet_history_file().ok_or_else(|| ConfigError::ReadFile {
            path: std::path::PathBuf::from("snippet_history.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine snippet history file path",
            ),
        })?;

        tracing::debug!("Loading snippet history from: {:?}", path);

        if !path.exists() {
            tracing::debug!("Snippet history file does not exist: {:?}", path);
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

        let path = super::paths::snippet_history_file().ok_or_else(|| ConfigError::WriteFile {
            path: std::path::PathBuf::from("snippet_history.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine snippet history file path",
            ),
        })?;

        let content = toml::to_string_pretty(self).map_err(ConfigError::Serialize)?;
        super::write_atomic(&path, &content).map_err(|e| ConfigError::WriteFile { path, source: e })
    }
}
