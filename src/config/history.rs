use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ConfigError;

/// Type of session for history entry
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SessionType {
    Ssh,
    Sftp,
    Local,
    Vnc,
}

impl SessionType {
    pub fn display_name(&self) -> &str {
        match self {
            SessionType::Ssh => "SSH",
            SessionType::Sftp => "SFTP",
            SessionType::Local => "Local",
            SessionType::Vnc => "VNC",
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

    /// Create a new history entry for a local terminal session
    pub fn new_local() -> Self {
        let hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("HOST"))
            .unwrap_or_else(|_| "localhost".to_string());
        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_default();

        Self {
            id: Uuid::new_v4(),
            host_id: Uuid::nil(), // Nil UUID indicates local session
            host_name: "Local Terminal".to_string(),
            hostname,
            username,
            connected_at: chrono::Utc::now(),
            disconnected_at: None,
            session_type: SessionType::Local,
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
        super::write_atomic(&path, &content).map_err(|e| ConfigError::WriteFile { path, source: e })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === SessionType tests ===

    #[test]
    fn session_type_ssh_display_name() {
        assert_eq!(SessionType::Ssh.display_name(), "SSH");
    }

    #[test]
    fn session_type_sftp_display_name() {
        assert_eq!(SessionType::Sftp.display_name(), "SFTP");
    }

    #[test]
    fn session_type_local_display_name() {
        assert_eq!(SessionType::Local.display_name(), "Local");
    }

    #[test]
    fn session_type_debug() {
        assert!(format!("{:?}", SessionType::Ssh).contains("Ssh"));
        assert!(format!("{:?}", SessionType::Sftp).contains("Sftp"));
        assert!(format!("{:?}", SessionType::Local).contains("Local"));
    }

    #[test]
    fn session_type_clone() {
        let original = SessionType::Ssh;
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn session_type_equality() {
        assert_eq!(SessionType::Ssh, SessionType::Ssh);
        assert_eq!(SessionType::Sftp, SessionType::Sftp);
        assert_eq!(SessionType::Local, SessionType::Local);
        assert_ne!(SessionType::Ssh, SessionType::Sftp);
        assert_ne!(SessionType::Ssh, SessionType::Local);
        assert_ne!(SessionType::Sftp, SessionType::Local);
    }

    #[test]
    fn session_type_serialize_lowercase() {
        // SessionType uses rename_all = "lowercase", verify via HistoryEntry serialization
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let toml_str = toml::to_string(&entry).expect("serialize");
        assert!(toml_str.contains("session_type = \"ssh\""));

        let entry_sftp = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Sftp,
        );
        let toml_str_sftp = toml::to_string(&entry_sftp).expect("serialize");
        assert!(toml_str_sftp.contains("session_type = \"sftp\""));

        let entry_local = HistoryEntry::new_local();
        let toml_str_local = toml::to_string(&entry_local).expect("serialize");
        assert!(toml_str_local.contains("session_type = \"local\""));
    }

    #[test]
    fn session_type_deserialize_lowercase() {
        // Test deserialization via full entry parsing
        let toml_ssh = r#"
            id = "550e8400-e29b-41d4-a716-446655440000"
            host_id = "550e8400-e29b-41d4-a716-446655440001"
            host_name = "Server"
            hostname = "example.com"
            username = "user"
            connected_at = "2024-01-01T12:00:00Z"
            session_type = "ssh"
        "#;
        let entry_ssh: HistoryEntry = toml::from_str(toml_ssh).expect("deserialize ssh");
        assert_eq!(entry_ssh.session_type, SessionType::Ssh);

        let toml_sftp = r#"
            id = "550e8400-e29b-41d4-a716-446655440000"
            host_id = "550e8400-e29b-41d4-a716-446655440001"
            host_name = "Server"
            hostname = "example.com"
            username = "user"
            connected_at = "2024-01-01T12:00:00Z"
            session_type = "sftp"
        "#;
        let entry_sftp: HistoryEntry = toml::from_str(toml_sftp).expect("deserialize sftp");
        assert_eq!(entry_sftp.session_type, SessionType::Sftp);

        let toml_local = r#"
            id = "550e8400-e29b-41d4-a716-446655440000"
            host_id = "550e8400-e29b-41d4-a716-446655440001"
            host_name = "Local"
            hostname = "localhost"
            username = "user"
            connected_at = "2024-01-01T12:00:00Z"
            session_type = "local"
        "#;
        let entry_local: HistoryEntry = toml::from_str(toml_local).expect("deserialize local");
        assert_eq!(entry_local.session_type, SessionType::Local);
    }

    // === HistoryEntry::new tests ===

    #[test]
    fn history_entry_new_sets_host_id() {
        let host_id = Uuid::new_v4();
        let entry = HistoryEntry::new(
            host_id,
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        assert_eq!(entry.host_id, host_id);
    }

    #[test]
    fn history_entry_new_sets_host_name() {
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "My Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        assert_eq!(entry.host_name, "My Server");
    }

    #[test]
    fn history_entry_new_sets_hostname() {
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "server.example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        assert_eq!(entry.hostname, "server.example.com");
    }

    #[test]
    fn history_entry_new_sets_username() {
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "admin".to_string(),
            SessionType::Ssh,
        );
        assert_eq!(entry.username, "admin");
    }

    #[test]
    fn history_entry_new_sets_session_type() {
        let ssh = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let sftp = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Sftp,
        );

        assert_eq!(ssh.session_type, SessionType::Ssh);
        assert_eq!(sftp.session_type, SessionType::Sftp);
    }

    #[test]
    fn history_entry_new_generates_unique_id() {
        let entry1 = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let entry2 = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        assert_ne!(entry1.id, entry2.id);
    }

    #[test]
    fn history_entry_new_sets_connected_at() {
        let before = chrono::Utc::now();
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let after = chrono::Utc::now();

        assert!(entry.connected_at >= before && entry.connected_at <= after);
    }

    #[test]
    fn history_entry_new_disconnected_at_is_none() {
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        assert!(entry.disconnected_at.is_none());
    }

    // === HistoryEntry::new_local tests ===

    #[test]
    fn history_entry_new_local_session_type() {
        let entry = HistoryEntry::new_local();
        assert_eq!(entry.session_type, SessionType::Local);
    }

    #[test]
    fn history_entry_new_local_host_name() {
        let entry = HistoryEntry::new_local();
        assert_eq!(entry.host_name, "Local Terminal");
    }

    #[test]
    fn history_entry_new_local_nil_host_id() {
        let entry = HistoryEntry::new_local();
        assert_eq!(entry.host_id, Uuid::nil());
    }

    #[test]
    fn history_entry_new_local_disconnected_at_is_none() {
        let entry = HistoryEntry::new_local();
        assert!(entry.disconnected_at.is_none());
    }

    #[test]
    fn history_entry_new_local_generates_unique_id() {
        let entry1 = HistoryEntry::new_local();
        let entry2 = HistoryEntry::new_local();
        assert_ne!(entry1.id, entry2.id);
    }

    // === HistoryEntry::duration tests ===

    #[test]
    fn history_entry_duration_active_session() {
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        // Active session should have non-negative duration
        assert!(entry.duration().num_seconds() >= 0);
    }

    #[test]
    fn history_entry_duration_with_disconnected() {
        let mut entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        // Set disconnected_at to 1 hour after connected_at
        entry.disconnected_at = Some(entry.connected_at + chrono::Duration::hours(1));

        assert_eq!(entry.duration().num_hours(), 1);
    }

    // === HistoryEntry::duration_string tests ===

    #[test]
    fn history_entry_duration_string_seconds() {
        let mut entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        entry.disconnected_at = Some(entry.connected_at + chrono::Duration::seconds(45));

        assert_eq!(entry.duration_string(), "45s");
    }

    #[test]
    fn history_entry_duration_string_minutes() {
        let mut entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        entry.disconnected_at = Some(entry.connected_at + chrono::Duration::minutes(15));

        assert_eq!(entry.duration_string(), "15m");
    }

    #[test]
    fn history_entry_duration_string_hours_and_minutes() {
        let mut entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        entry.disconnected_at =
            Some(entry.connected_at + chrono::Duration::hours(2) + chrono::Duration::minutes(30));

        assert_eq!(entry.duration_string(), "2h 30m");
    }

    #[test]
    fn history_entry_duration_string_zero() {
        let mut entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        entry.disconnected_at = Some(entry.connected_at);

        assert_eq!(entry.duration_string(), "0s");
    }

    #[test]
    fn history_entry_duration_string_exactly_one_hour() {
        let mut entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        entry.disconnected_at = Some(entry.connected_at + chrono::Duration::hours(1));

        assert_eq!(entry.duration_string(), "1h 0m");
    }

    #[test]
    fn history_entry_duration_string_exactly_one_minute() {
        let mut entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        entry.disconnected_at = Some(entry.connected_at + chrono::Duration::minutes(1));

        assert_eq!(entry.duration_string(), "1m");
    }

    // === HistoryEntry traits tests ===

    #[test]
    fn history_entry_debug() {
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let debug_str = format!("{:?}", entry);

        assert!(debug_str.contains("HistoryEntry"));
        assert!(debug_str.contains("Server"));
        assert!(debug_str.contains("example.com"));
    }

    #[test]
    fn history_entry_clone() {
        let original = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let cloned = original.clone();

        assert_eq!(original.id, cloned.id);
        assert_eq!(original.host_id, cloned.host_id);
        assert_eq!(original.host_name, cloned.host_name);
        assert_eq!(original.hostname, cloned.hostname);
        assert_eq!(original.username, cloned.username);
        assert_eq!(original.session_type, cloned.session_type);
    }

    // === HistoryEntry serialization tests ===

    #[test]
    fn history_entry_serialize_to_toml() {
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "admin".to_string(),
            SessionType::Ssh,
        );
        let toml_str = toml::to_string(&entry).expect("serialize");

        assert!(toml_str.contains("host_name = \"Server\""));
        assert!(toml_str.contains("hostname = \"example.com\""));
        assert!(toml_str.contains("username = \"admin\""));
        assert!(toml_str.contains("session_type = \"ssh\""));
    }

    #[test]
    fn history_entry_serialize_skips_none_disconnected() {
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let toml_str = toml::to_string(&entry).expect("serialize");

        assert!(!toml_str.contains("disconnected_at"));
    }

    #[test]
    fn history_entry_serialize_includes_disconnected() {
        let mut entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        entry.disconnected_at = Some(chrono::Utc::now());
        let toml_str = toml::to_string(&entry).expect("serialize");

        assert!(toml_str.contains("disconnected_at"));
    }

    #[test]
    fn history_entry_deserialize_from_toml() {
        let toml_str = r#"
            id = "550e8400-e29b-41d4-a716-446655440000"
            host_id = "550e8400-e29b-41d4-a716-446655440001"
            host_name = "My Server"
            hostname = "server.example.com"
            username = "root"
            connected_at = "2024-01-01T12:00:00Z"
            session_type = "sftp"
        "#;

        let entry: HistoryEntry = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(entry.host_name, "My Server");
        assert_eq!(entry.hostname, "server.example.com");
        assert_eq!(entry.username, "root");
        assert_eq!(entry.session_type, SessionType::Sftp);
        assert!(entry.disconnected_at.is_none());
    }

    #[test]
    fn history_entry_deserialize_with_disconnected() {
        let toml_str = r#"
            id = "550e8400-e29b-41d4-a716-446655440000"
            host_id = "550e8400-e29b-41d4-a716-446655440001"
            host_name = "Server"
            hostname = "example.com"
            username = "user"
            connected_at = "2024-01-01T12:00:00Z"
            disconnected_at = "2024-01-01T14:30:00Z"
            session_type = "ssh"
        "#;

        let entry: HistoryEntry = toml::from_str(toml_str).expect("deserialize");
        assert!(entry.disconnected_at.is_some());
    }

    #[test]
    fn history_entry_roundtrip() {
        let mut original = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Sftp,
        );
        original.disconnected_at = Some(chrono::Utc::now());

        let toml_str = toml::to_string(&original).expect("serialize");
        let deserialized: HistoryEntry = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(original.id, deserialized.id);
        assert_eq!(original.host_name, deserialized.host_name);
        assert_eq!(original.session_type, deserialized.session_type);
    }

    // === HistoryConfig tests ===

    #[test]
    fn history_config_default_empty_entries() {
        let config = HistoryConfig::default();
        assert!(config.entries.is_empty());
    }

    #[test]
    fn history_config_default_max_entries() {
        let config = HistoryConfig::default();
        assert_eq!(config.max_entries, 100);
    }

    #[test]
    fn history_config_debug() {
        let config = HistoryConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("HistoryConfig"));
    }

    #[test]
    fn history_config_clone() {
        let mut config = HistoryConfig::default();
        config.add_entry(HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        ));

        let cloned = config.clone();
        assert_eq!(config.entries.len(), cloned.entries.len());
        assert_eq!(config.max_entries, cloned.max_entries);
    }

    // === HistoryConfig::add_entry tests ===

    #[test]
    fn add_entry_inserts_at_front() {
        let mut config = HistoryConfig::default();

        let entry1 = HistoryEntry::new(
            Uuid::new_v4(),
            "First".to_string(),
            "first.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let entry2 = HistoryEntry::new(
            Uuid::new_v4(),
            "Second".to_string(),
            "second.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );

        config.add_entry(entry1);
        config.add_entry(entry2);

        assert_eq!(config.entries[0].host_name, "Second");
        assert_eq!(config.entries[1].host_name, "First");
    }

    #[test]
    fn add_entry_trims_to_max() {
        let mut config = HistoryConfig {
            max_entries: 3,
            ..Default::default()
        };

        for i in 0..5 {
            config.add_entry(HistoryEntry::new(
                Uuid::new_v4(),
                format!("Server{}", i),
                "example.com".to_string(),
                "user".to_string(),
                SessionType::Ssh,
            ));
        }

        assert_eq!(config.entries.len(), 3);
        // Most recent entries should be kept
        assert_eq!(config.entries[0].host_name, "Server4");
        assert_eq!(config.entries[1].host_name, "Server3");
        assert_eq!(config.entries[2].host_name, "Server2");
    }

    #[test]
    fn add_entry_max_entries_zero() {
        let mut config = HistoryConfig {
            max_entries: 0,
            ..Default::default()
        };

        config.add_entry(HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        ));

        assert!(config.entries.is_empty());
    }

    // === HistoryConfig::find_entry tests ===

    #[test]
    fn find_entry_returns_none_for_empty() {
        let config = HistoryConfig::default();
        assert!(config.find_entry(Uuid::new_v4()).is_none());
    }

    #[test]
    fn find_entry_returns_none_for_nonexistent() {
        let mut config = HistoryConfig::default();
        config.add_entry(HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        ));

        assert!(config.find_entry(Uuid::new_v4()).is_none());
    }

    #[test]
    fn find_entry_returns_correct_entry() {
        let mut config = HistoryConfig::default();
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Target".to_string(),
            "target.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let id = entry.id;

        config.add_entry(HistoryEntry::new(
            Uuid::new_v4(),
            "Other".to_string(),
            "other.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        ));
        config.add_entry(entry);

        let found = config.find_entry(id).expect("should find");
        assert_eq!(found.host_name, "Target");
    }

    // === HistoryConfig::find_entry_mut tests ===

    #[test]
    fn find_entry_mut_returns_none_for_empty() {
        let mut config = HistoryConfig::default();
        assert!(config.find_entry_mut(Uuid::new_v4()).is_none());
    }

    #[test]
    fn find_entry_mut_allows_modification() {
        let mut config = HistoryConfig::default();
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let id = entry.id;
        config.add_entry(entry);

        if let Some(e) = config.find_entry_mut(id) {
            e.host_name = "Modified".to_string();
        }

        assert_eq!(config.find_entry(id).unwrap().host_name, "Modified");
    }

    // === HistoryConfig::mark_disconnected tests ===

    #[test]
    fn mark_disconnected_sets_timestamp() {
        let mut config = HistoryConfig::default();
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let id = entry.id;
        config.add_entry(entry);

        assert!(config.find_entry(id).unwrap().disconnected_at.is_none());

        config.mark_disconnected(id);

        assert!(config.find_entry(id).unwrap().disconnected_at.is_some());
    }

    #[test]
    fn mark_disconnected_nonexistent_does_nothing() {
        let mut config = HistoryConfig::default();
        config.add_entry(HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        ));

        // Should not panic
        config.mark_disconnected(Uuid::new_v4());
    }

    // === HistoryConfig::clear tests ===

    #[test]
    fn clear_removes_all_entries() {
        let mut config = HistoryConfig::default();
        for i in 0..5 {
            config.add_entry(HistoryEntry::new(
                Uuid::new_v4(),
                format!("Server{}", i),
                "example.com".to_string(),
                "user".to_string(),
                SessionType::Ssh,
            ));
        }

        assert_eq!(config.entries.len(), 5);
        config.clear();
        assert!(config.entries.is_empty());
    }

    #[test]
    fn clear_on_empty_does_nothing() {
        let mut config = HistoryConfig::default();
        config.clear();
        assert!(config.entries.is_empty());
    }

    // === HistoryConfig serialization tests ===

    #[test]
    fn history_config_serialize_empty() {
        let config = HistoryConfig::default();
        let toml_str = toml::to_string(&config).expect("serialize");

        assert!(toml_str.contains("entries"));
        assert!(toml_str.contains("max_entries = 100"));
    }

    #[test]
    fn history_config_serialize_with_entries() {
        let mut config = HistoryConfig::default();
        config.add_entry(HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        ));

        let toml_str = toml::to_string(&config).expect("serialize");
        assert!(toml_str.contains("[[entries]]"));
        assert!(toml_str.contains("host_name = \"Server\""));
    }

    #[test]
    fn history_config_deserialize_empty() {
        let toml_str = r#"
            entries = []
            max_entries = 50
        "#;

        let config: HistoryConfig = toml::from_str(toml_str).expect("deserialize");
        assert!(config.entries.is_empty());
        assert_eq!(config.max_entries, 50);
    }

    #[test]
    fn history_config_deserialize_default_max_entries() {
        let toml_str = "entries = []";
        let config: HistoryConfig = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(config.max_entries, 100);
    }

    #[test]
    fn history_config_deserialize_missing_entries() {
        let toml_str = "max_entries = 200";
        let config: HistoryConfig = toml::from_str(toml_str).expect("deserialize");
        assert!(config.entries.is_empty());
        assert_eq!(config.max_entries, 200);
    }

    #[test]
    fn history_config_deserialize_with_entries() {
        let toml_str = r#"
            max_entries = 100

            [[entries]]
            id = "550e8400-e29b-41d4-a716-446655440000"
            host_id = "550e8400-e29b-41d4-a716-446655440001"
            host_name = "Server1"
            hostname = "server1.com"
            username = "user1"
            connected_at = "2024-01-01T12:00:00Z"
            session_type = "ssh"

            [[entries]]
            id = "550e8400-e29b-41d4-a716-446655440002"
            host_id = "550e8400-e29b-41d4-a716-446655440003"
            host_name = "Server2"
            hostname = "server2.com"
            username = "user2"
            connected_at = "2024-01-02T12:00:00Z"
            session_type = "sftp"
        "#;

        let config: HistoryConfig = toml::from_str(toml_str).expect("deserialize");
        assert_eq!(config.entries.len(), 2);
        assert_eq!(config.entries[0].host_name, "Server1");
        assert_eq!(config.entries[1].host_name, "Server2");
        assert_eq!(config.entries[1].session_type, SessionType::Sftp);
    }

    #[test]
    fn history_config_roundtrip() {
        let mut config = HistoryConfig {
            max_entries: 50,
            ..Default::default()
        };
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Server".to_string(),
            "example.com".to_string(),
            "user".to_string(),
            SessionType::Ssh,
        );
        let id = entry.id;
        config.add_entry(entry);

        let toml_str = toml::to_string(&config).expect("serialize");
        let deserialized: HistoryConfig = toml::from_str(&toml_str).expect("deserialize");

        assert_eq!(deserialized.max_entries, 50);
        assert_eq!(deserialized.entries.len(), 1);
        assert!(deserialized.find_entry(id).is_some());
    }

    // === default_max_entries test ===

    #[test]
    fn default_max_entries_is_100() {
        assert_eq!(default_max_entries(), 100);
    }
}
