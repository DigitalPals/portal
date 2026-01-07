//! Command snippets configuration

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ConfigError;

/// A single command snippet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snippet {
    /// Unique identifier
    pub id: Uuid,
    /// Display name
    pub name: String,
    /// Command to execute
    pub command: String,
    /// Optional description
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Optional tags for filtering
    #[serde(default)]
    pub tags: Vec<String>,
    /// Target host IDs for multi-host execution
    #[serde(default)]
    pub host_ids: Vec<Uuid>,
    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
    /// Last update timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Snippet {
    /// Create a new snippet
    pub fn new(name: String, command: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            command,
            description: None,
            tags: Vec::new(),
            host_ids: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }
}

/// Root configuration for snippets.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SnippetsConfig {
    /// All snippets
    #[serde(default)]
    pub snippets: Vec<Snippet>,
}

impl SnippetsConfig {
    /// Find a snippet by ID
    pub fn find_snippet(&self, id: Uuid) -> Option<&Snippet> {
        self.snippets.iter().find(|s| s.id == id)
    }

    /// Find a snippet by ID (mutable)
    pub fn find_snippet_mut(&mut self, id: Uuid) -> Option<&mut Snippet> {
        self.snippets.iter_mut().find(|s| s.id == id)
    }

    /// Add a new snippet
    pub fn add_snippet(&mut self, snippet: Snippet) {
        self.snippets.push(snippet);
    }

    /// Delete a snippet by ID
    pub fn delete_snippet(&mut self, id: Uuid) -> Result<Snippet, ConfigError> {
        let pos = self
            .snippets
            .iter()
            .position(|s| s.id == id)
            .ok_or(ConfigError::SnippetNotFound(id))?;
        Ok(self.snippets.remove(pos))
    }

    /// Load from file, creating default if not exists
    pub fn load() -> Result<Self, ConfigError> {
        let path = super::paths::snippets_file().ok_or_else(|| ConfigError::ReadFile {
            path: std::path::PathBuf::from("snippets.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine snippets file path",
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

        let path = super::paths::snippets_file().ok_or_else(|| ConfigError::WriteFile {
            path: std::path::PathBuf::from("snippets.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine snippets file path",
            ),
        })?;

        let content = toml::to_string_pretty(self).map_err(ConfigError::Serialize)?;
        super::write_atomic(&path, &content).map_err(|e| ConfigError::WriteFile { path, source: e })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_snippet() {
        let mut config = SnippetsConfig::default();
        let snippet = Snippet::new("test".to_string(), "echo hello".to_string());
        let id = snippet.id;

        config.add_snippet(snippet);

        assert_eq!(config.snippets.len(), 1);
        assert!(config.find_snippet(id).is_some());
    }

    #[test]
    fn test_delete_snippet() {
        let mut config = SnippetsConfig::default();
        let snippet = Snippet::new("test".to_string(), "echo hello".to_string());
        let id = snippet.id;

        config.add_snippet(snippet);
        assert!(config.find_snippet(id).is_some());

        let deleted = config.delete_snippet(id).unwrap();
        assert_eq!(deleted.id, id);
        assert!(config.find_snippet(id).is_none());
    }

    #[test]
    fn test_delete_snippet_not_found() {
        let mut config = SnippetsConfig::default();
        let random_id = Uuid::new_v4();

        let result = config.delete_snippet(random_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_snippet_mut() {
        let mut config = SnippetsConfig::default();
        let snippet = Snippet::new("original".to_string(), "echo 1".to_string());
        let id = snippet.id;

        config.add_snippet(snippet);

        if let Some(s) = config.find_snippet_mut(id) {
            s.name = "updated".to_string();
        }

        assert_eq!(config.find_snippet(id).unwrap().name, "updated");
    }
}
