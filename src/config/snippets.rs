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

    // === Existing tests ===

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

    // === Snippet::new tests ===

    #[test]
    fn snippet_new_sets_name() {
        let snippet = Snippet::new("my-snippet".to_string(), "ls -la".to_string());
        assert_eq!(snippet.name, "my-snippet");
    }

    #[test]
    fn snippet_new_sets_command() {
        let snippet = Snippet::new("test".to_string(), "echo hello world".to_string());
        assert_eq!(snippet.command, "echo hello world");
    }

    #[test]
    fn snippet_new_generates_unique_id() {
        let snippet1 = Snippet::new("s1".to_string(), "cmd1".to_string());
        let snippet2 = Snippet::new("s2".to_string(), "cmd2".to_string());
        assert_ne!(snippet1.id, snippet2.id);
    }

    #[test]
    fn snippet_new_description_is_none() {
        let snippet = Snippet::new("test".to_string(), "cmd".to_string());
        assert!(snippet.description.is_none());
    }

    #[test]
    fn snippet_new_tags_is_empty() {
        let snippet = Snippet::new("test".to_string(), "cmd".to_string());
        assert!(snippet.tags.is_empty());
    }

    #[test]
    fn snippet_new_host_ids_is_empty() {
        let snippet = Snippet::new("test".to_string(), "cmd".to_string());
        assert!(snippet.host_ids.is_empty());
    }

    #[test]
    fn snippet_new_sets_timestamps() {
        let before = chrono::Utc::now();
        let snippet = Snippet::new("test".to_string(), "cmd".to_string());
        let after = chrono::Utc::now();

        assert!(snippet.created_at >= before && snippet.created_at <= after);
        assert!(snippet.updated_at >= before && snippet.updated_at <= after);
        assert_eq!(snippet.created_at, snippet.updated_at);
    }

    #[test]
    fn snippet_new_with_empty_name() {
        let snippet = Snippet::new(String::new(), "cmd".to_string());
        assert!(snippet.name.is_empty());
    }

    #[test]
    fn snippet_new_with_empty_command() {
        let snippet = Snippet::new("test".to_string(), String::new());
        assert!(snippet.command.is_empty());
    }

    #[test]
    fn snippet_new_with_multiline_command() {
        let cmd = "echo line1\necho line2\necho line3".to_string();
        let snippet = Snippet::new("multi".to_string(), cmd.clone());
        assert_eq!(snippet.command, cmd);
    }

    #[test]
    fn snippet_new_with_unicode() {
        let snippet = Snippet::new("日本語".to_string(), "echo こんにちは".to_string());
        assert_eq!(snippet.name, "日本語");
        assert_eq!(snippet.command, "echo こんにちは");
    }

    // === Snippet traits tests ===

    #[test]
    fn snippet_debug() {
        let snippet = Snippet::new("test".to_string(), "echo hi".to_string());
        let debug_str = format!("{:?}", snippet);
        assert!(debug_str.contains("Snippet"));
        assert!(debug_str.contains("test"));
        assert!(debug_str.contains("echo hi"));
    }

    #[test]
    fn snippet_clone() {
        let original = Snippet::new("original".to_string(), "cmd".to_string());
        let cloned = original.clone();

        assert_eq!(original.id, cloned.id);
        assert_eq!(original.name, cloned.name);
        assert_eq!(original.command, cloned.command);
        assert_eq!(original.created_at, cloned.created_at);
    }

    #[test]
    fn snippet_clone_is_independent() {
        let original = Snippet::new("original".to_string(), "cmd".to_string());
        let mut cloned = original.clone();
        cloned.name = "modified".to_string();

        assert_eq!(original.name, "original");
        assert_eq!(cloned.name, "modified");
    }

    // === Snippet serialization tests ===

    #[test]
    fn snippet_serialize_to_toml() {
        let snippet = Snippet::new("test".to_string(), "echo hello".to_string());
        let toml_str = toml::to_string(&snippet).expect("serialize");

        assert!(toml_str.contains("name = \"test\""));
        assert!(toml_str.contains("command = \"echo hello\""));
        assert!(toml_str.contains("id = "));
    }

    #[test]
    fn snippet_serialize_skips_none_description() {
        let snippet = Snippet::new("test".to_string(), "cmd".to_string());
        let toml_str = toml::to_string(&snippet).expect("serialize");

        assert!(!toml_str.contains("description"));
    }

    #[test]
    fn snippet_serialize_includes_some_description() {
        let mut snippet = Snippet::new("test".to_string(), "cmd".to_string());
        snippet.description = Some("A helpful description".to_string());
        let toml_str = toml::to_string(&snippet).expect("serialize");

        assert!(toml_str.contains("description = \"A helpful description\""));
    }

    #[test]
    fn snippet_serialize_includes_tags() {
        let mut snippet = Snippet::new("test".to_string(), "cmd".to_string());
        snippet.tags = vec!["linux".to_string(), "network".to_string()];
        let toml_str = toml::to_string(&snippet).expect("serialize");

        assert!(toml_str.contains("tags"));
        assert!(toml_str.contains("linux"));
        assert!(toml_str.contains("network"));
    }

    #[test]
    fn snippet_deserialize_from_toml() {
        let toml_str = r#"
            id = "550e8400-e29b-41d4-a716-446655440000"
            name = "list files"
            command = "ls -la"
            tags = ["basic"]
            host_ids = []
            created_at = "2024-01-01T00:00:00Z"
            updated_at = "2024-01-01T00:00:00Z"
        "#;

        let snippet: Snippet = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(snippet.name, "list files");
        assert_eq!(snippet.command, "ls -la");
        assert_eq!(snippet.tags, vec!["basic"]);
    }

    #[test]
    fn snippet_deserialize_with_description() {
        let toml_str = r#"
            id = "550e8400-e29b-41d4-a716-446655440000"
            name = "test"
            command = "cmd"
            description = "My description"
            tags = []
            host_ids = []
            created_at = "2024-01-01T00:00:00Z"
            updated_at = "2024-01-01T00:00:00Z"
        "#;

        let snippet: Snippet = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(snippet.description, Some("My description".to_string()));
    }

    #[test]
    fn snippet_deserialize_without_optional_fields() {
        let toml_str = r#"
            id = "550e8400-e29b-41d4-a716-446655440000"
            name = "minimal"
            command = "echo"
            created_at = "2024-01-01T00:00:00Z"
            updated_at = "2024-01-01T00:00:00Z"
        "#;

        let snippet: Snippet = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(snippet.name, "minimal");
        assert!(snippet.description.is_none());
        assert!(snippet.tags.is_empty());
        assert!(snippet.host_ids.is_empty());
    }

    #[test]
    fn snippet_roundtrip_serialization() {
        let mut original = Snippet::new("roundtrip".to_string(), "echo test".to_string());
        original.description = Some("desc".to_string());
        original.tags = vec!["tag1".to_string(), "tag2".to_string()];

        let toml_str = toml::to_string(&original).expect("serialize");
        let deserialized: Snippet = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(original.id, deserialized.id);
        assert_eq!(original.name, deserialized.name);
        assert_eq!(original.command, deserialized.command);
        assert_eq!(original.description, deserialized.description);
        assert_eq!(original.tags, deserialized.tags);
    }

    // === SnippetsConfig tests ===

    #[test]
    fn snippets_config_default_is_empty() {
        let config = SnippetsConfig::default();
        assert!(config.snippets.is_empty());
    }

    #[test]
    fn snippets_config_debug() {
        let config = SnippetsConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("SnippetsConfig"));
    }

    #[test]
    fn snippets_config_clone() {
        let mut config = SnippetsConfig::default();
        config.add_snippet(Snippet::new("test".to_string(), "cmd".to_string()));

        let cloned = config.clone();
        assert_eq!(config.snippets.len(), cloned.snippets.len());
    }

    #[test]
    fn find_snippet_returns_none_for_empty_config() {
        let config = SnippetsConfig::default();
        assert!(config.find_snippet(Uuid::new_v4()).is_none());
    }

    #[test]
    fn find_snippet_returns_none_for_nonexistent_id() {
        let mut config = SnippetsConfig::default();
        config.add_snippet(Snippet::new("test".to_string(), "cmd".to_string()));

        assert!(config.find_snippet(Uuid::new_v4()).is_none());
    }

    #[test]
    fn find_snippet_returns_correct_snippet() {
        let mut config = SnippetsConfig::default();
        let snippet1 = Snippet::new("first".to_string(), "cmd1".to_string());
        let snippet2 = Snippet::new("second".to_string(), "cmd2".to_string());
        let id2 = snippet2.id;

        config.add_snippet(snippet1);
        config.add_snippet(snippet2);

        let found = config.find_snippet(id2).expect("should find");
        assert_eq!(found.name, "second");
    }

    #[test]
    fn find_snippet_mut_returns_none_for_empty_config() {
        let mut config = SnippetsConfig::default();
        assert!(config.find_snippet_mut(Uuid::new_v4()).is_none());
    }

    #[test]
    fn find_snippet_mut_allows_modification() {
        let mut config = SnippetsConfig::default();
        let snippet = Snippet::new("test".to_string(), "old".to_string());
        let id = snippet.id;
        config.add_snippet(snippet);

        if let Some(s) = config.find_snippet_mut(id) {
            s.command = "new".to_string();
            s.description = Some("added".to_string());
            s.tags.push("modified".to_string());
        }

        let found = config.find_snippet(id).unwrap();
        assert_eq!(found.command, "new");
        assert_eq!(found.description, Some("added".to_string()));
        assert_eq!(found.tags, vec!["modified"]);
    }

    #[test]
    fn add_snippet_multiple() {
        let mut config = SnippetsConfig::default();

        for i in 0..5 {
            config.add_snippet(Snippet::new(format!("snippet{}", i), format!("cmd{}", i)));
        }

        assert_eq!(config.snippets.len(), 5);
    }

    #[test]
    fn add_snippet_preserves_order() {
        let mut config = SnippetsConfig::default();
        config.add_snippet(Snippet::new("first".to_string(), "cmd1".to_string()));
        config.add_snippet(Snippet::new("second".to_string(), "cmd2".to_string()));
        config.add_snippet(Snippet::new("third".to_string(), "cmd3".to_string()));

        assert_eq!(config.snippets[0].name, "first");
        assert_eq!(config.snippets[1].name, "second");
        assert_eq!(config.snippets[2].name, "third");
    }

    #[test]
    fn delete_snippet_returns_deleted() {
        let mut config = SnippetsConfig::default();
        let snippet = Snippet::new("to-delete".to_string(), "cmd".to_string());
        let id = snippet.id;
        config.add_snippet(snippet);

        let deleted = config.delete_snippet(id).unwrap();
        assert_eq!(deleted.name, "to-delete");
        assert_eq!(deleted.id, id);
    }

    #[test]
    fn delete_snippet_removes_from_list() {
        let mut config = SnippetsConfig::default();
        let snippet = Snippet::new("test".to_string(), "cmd".to_string());
        let id = snippet.id;
        config.add_snippet(snippet);

        assert_eq!(config.snippets.len(), 1);
        config.delete_snippet(id).unwrap();
        assert_eq!(config.snippets.len(), 0);
    }

    #[test]
    fn delete_snippet_preserves_others() {
        let mut config = SnippetsConfig::default();
        let snippet1 = Snippet::new("keep1".to_string(), "cmd1".to_string());
        let snippet2 = Snippet::new("delete".to_string(), "cmd2".to_string());
        let snippet3 = Snippet::new("keep2".to_string(), "cmd3".to_string());
        let id1 = snippet1.id;
        let id2 = snippet2.id;
        let id3 = snippet3.id;

        config.add_snippet(snippet1);
        config.add_snippet(snippet2);
        config.add_snippet(snippet3);

        config.delete_snippet(id2).unwrap();

        assert_eq!(config.snippets.len(), 2);
        assert!(config.find_snippet(id1).is_some());
        assert!(config.find_snippet(id2).is_none());
        assert!(config.find_snippet(id3).is_some());
    }

    // === SnippetsConfig serialization tests ===

    #[test]
    fn snippets_config_serialize_empty() {
        let config = SnippetsConfig::default();
        let toml_str = toml::to_string(&config).expect("serialize");
        assert!(toml_str.contains("snippets"));
    }

    #[test]
    fn snippets_config_serialize_with_snippets() {
        let mut config = SnippetsConfig::default();
        config.add_snippet(Snippet::new("test".to_string(), "echo hi".to_string()));

        let toml_str = toml::to_string(&config).expect("serialize");
        assert!(toml_str.contains("[[snippets]]"));
        assert!(toml_str.contains("name = \"test\""));
    }

    #[test]
    fn snippets_config_deserialize_empty() {
        let toml_str = "snippets = []";
        let config: SnippetsConfig = toml::from_str(toml_str).expect("deserialize");
        assert!(config.snippets.is_empty());
    }

    #[test]
    fn snippets_config_deserialize_missing_snippets_field() {
        let toml_str = "";
        let config: SnippetsConfig = toml::from_str(toml_str).expect("deserialize");
        assert!(config.snippets.is_empty());
    }

    #[test]
    fn snippets_config_deserialize_with_snippets() {
        let toml_str = r#"
            [[snippets]]
            id = "550e8400-e29b-41d4-a716-446655440000"
            name = "list"
            command = "ls"
            tags = []
            host_ids = []
            created_at = "2024-01-01T00:00:00Z"
            updated_at = "2024-01-01T00:00:00Z"

            [[snippets]]
            id = "550e8400-e29b-41d4-a716-446655440001"
            name = "disk"
            command = "df -h"
            tags = ["system"]
            host_ids = []
            created_at = "2024-01-01T00:00:00Z"
            updated_at = "2024-01-01T00:00:00Z"
        "#;

        let config: SnippetsConfig = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(config.snippets.len(), 2);
        assert_eq!(config.snippets[0].name, "list");
        assert_eq!(config.snippets[1].name, "disk");
        assert_eq!(config.snippets[1].tags, vec!["system"]);
    }

    #[test]
    fn snippets_config_roundtrip() {
        let mut config = SnippetsConfig::default();
        let mut snippet = Snippet::new("roundtrip".to_string(), "echo test".to_string());
        snippet.description = Some("desc".to_string());
        snippet.tags = vec!["tag1".to_string()];
        let id = snippet.id;
        config.add_snippet(snippet);

        let toml_str = toml::to_string(&config).expect("serialize");
        let deserialized: SnippetsConfig = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(deserialized.snippets.len(), 1);
        let found = deserialized.find_snippet(id).expect("find");
        assert_eq!(found.name, "roundtrip");
        assert_eq!(found.description, Some("desc".to_string()));
    }

    // === Edge cases ===

    #[test]
    fn snippet_with_special_characters_in_command() {
        let cmd = r#"echo "hello $USER" && ls | grep 'test' > /tmp/out"#;
        let snippet = Snippet::new("special".to_string(), cmd.to_string());

        let toml_str = toml::to_string(&snippet).expect("serialize");
        let deserialized: Snippet = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(deserialized.command, cmd);
    }

    #[test]
    fn snippet_with_host_ids() {
        let mut snippet = Snippet::new("multi-host".to_string(), "uptime".to_string());
        let host1 = Uuid::new_v4();
        let host2 = Uuid::new_v4();
        snippet.host_ids = vec![host1, host2];

        let toml_str = toml::to_string(&snippet).expect("serialize");
        let deserialized: Snippet = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(deserialized.host_ids.len(), 2);
        assert!(deserialized.host_ids.contains(&host1));
        assert!(deserialized.host_ids.contains(&host2));
    }

    #[test]
    fn snippet_timestamps_preserved() {
        let snippet = Snippet::new("test".to_string(), "cmd".to_string());
        let created = snippet.created_at;
        let updated = snippet.updated_at;

        let toml_str = toml::to_string(&snippet).expect("serialize");
        let deserialized: Snippet = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(deserialized.created_at, created);
        assert_eq!(deserialized.updated_at, updated);
    }
}
