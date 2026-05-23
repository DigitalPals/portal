//! Persistent terminal attention notification history.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::ConfigError;

fn default_max_entries() -> usize {
    500
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentNotificationEntry {
    pub id: Uuid,
    pub session_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<Uuid>,
    pub host_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    pub received_at: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cleared_at: Option<DateTime<Utc>>,
}

impl AgentNotificationEntry {
    pub fn new(
        session_id: Uuid,
        host_id: Option<Uuid>,
        host_name: String,
        title: Option<String>,
        body: Option<String>,
        read: bool,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4(),
            session_id,
            host_id,
            host_name,
            title,
            body,
            received_at: now,
            read_at: read.then_some(now),
            cleared_at: None,
        }
    }

    pub fn display_title(&self) -> &str {
        self.title
            .as_deref()
            .filter(|title| !title.trim().is_empty())
            .unwrap_or(&self.host_name)
    }

    pub fn display_body(&self) -> &str {
        self.body
            .as_deref()
            .filter(|body| !body.trim().is_empty())
            .unwrap_or("Terminal needs attention")
    }

    pub fn is_unread(&self) -> bool {
        self.read_at.is_none() && self.cleared_at.is_none()
    }

    pub fn is_active(&self) -> bool {
        self.cleared_at.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNotificationsConfig {
    #[serde(default)]
    pub entries: Vec<AgentNotificationEntry>,
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
}

impl Default for AgentNotificationsConfig {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: default_max_entries(),
        }
    }
}

impl AgentNotificationsConfig {
    pub fn add_entry(&mut self, entry: AgentNotificationEntry) {
        self.entries.insert(0, entry);
        self.trim_to_max_entries();
    }

    pub fn add_attention(
        &mut self,
        session_id: Uuid,
        host_id: Option<Uuid>,
        host_name: String,
        title: Option<String>,
        body: Option<String>,
        read: bool,
    ) -> Uuid {
        let entry = AgentNotificationEntry::new(session_id, host_id, host_name, title, body, read);
        let id = entry.id;
        self.add_entry(entry);
        id
    }

    pub fn unread_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.is_unread())
            .count()
    }

    pub fn active_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.is_active())
            .count()
    }

    pub fn latest_unread(&self) -> Option<&AgentNotificationEntry> {
        self.entries.iter().find(|entry| entry.is_unread())
    }

    pub fn active_entries(&self) -> impl Iterator<Item = &AgentNotificationEntry> {
        self.entries.iter().filter(|entry| entry.is_active())
    }

    pub fn latest_active_by_session(&self) -> Vec<&AgentNotificationEntry> {
        let mut seen = HashSet::new();
        self.entries
            .iter()
            .filter(|entry| entry.is_active())
            .filter(move |entry| seen.insert(entry.session_id))
            .collect()
    }

    pub fn unread_by_session(&self) -> HashMap<Uuid, usize> {
        let mut counts = HashMap::new();
        for entry in self.entries.iter().filter(|entry| entry.is_unread()) {
            *counts.entry(entry.session_id).or_insert(0) += 1;
        }
        counts
    }

    pub fn find_entry(&self, id: Uuid) -> Option<&AgentNotificationEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn mark_read(&mut self, id: Uuid) -> bool {
        let now = Utc::now();
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        if entry.read_at.is_some() {
            return false;
        }
        entry.read_at = Some(now);
        true
    }

    pub fn mark_unread(&mut self, id: Uuid) -> bool {
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        if entry.read_at.is_none() && entry.cleared_at.is_none() {
            return false;
        }
        entry.read_at = None;
        entry.cleared_at = None;
        true
    }

    pub fn mark_session_read(&mut self, session_id: Uuid) -> bool {
        let now = Utc::now();
        let mut changed = false;
        for entry in self
            .entries
            .iter_mut()
            .filter(|entry| entry.session_id == session_id && entry.read_at.is_none())
        {
            entry.read_at = Some(now);
            changed = true;
        }
        changed
    }

    pub fn mark_all_read(&mut self) -> bool {
        let now = Utc::now();
        let mut changed = false;
        for entry in self
            .entries
            .iter_mut()
            .filter(|entry| entry.read_at.is_none())
        {
            entry.read_at = Some(now);
            changed = true;
        }
        changed
    }

    pub fn clear(&mut self, id: Uuid) -> bool {
        let now = Utc::now();
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        if entry.cleared_at.is_some() {
            return false;
        }
        entry.read_at.get_or_insert(now);
        entry.cleared_at = Some(now);
        true
    }

    pub fn clear_read(&mut self) -> bool {
        let now = Utc::now();
        let mut changed = false;
        for entry in self
            .entries
            .iter_mut()
            .filter(|entry| entry.read_at.is_some() && entry.cleared_at.is_none())
        {
            entry.cleared_at = Some(now);
            changed = true;
        }
        changed
    }

    pub fn clear_all(&mut self) -> bool {
        let now = Utc::now();
        let mut changed = false;
        for entry in self
            .entries
            .iter_mut()
            .filter(|entry| entry.cleared_at.is_none())
        {
            entry.read_at.get_or_insert(now);
            entry.cleared_at = Some(now);
            changed = true;
        }
        changed
    }

    fn trim_to_max_entries(&mut self) {
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }
    }

    pub fn load() -> Result<Self, ConfigError> {
        let path =
            super::paths::agent_notifications_file().ok_or_else(|| ConfigError::ReadFile {
                path: std::path::PathBuf::from("agent_notifications.toml"),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not determine agent notifications file path",
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
        config.trim_to_max_entries();
        Ok(config)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        super::paths::ensure_config_dir().map_err(ConfigError::CreateDir)?;

        let path =
            super::paths::agent_notifications_file().ok_or_else(|| ConfigError::WriteFile {
                path: std::path::PathBuf::from("agent_notifications.toml"),
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not determine agent notifications file path",
                ),
            })?;

        let content = toml::to_string_pretty(self).map_err(ConfigError::Serialize)?;
        super::write_atomic(&path, &content).map_err(|e| ConfigError::WriteFile { path, source: e })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add_test_entry(config: &mut AgentNotificationsConfig, session_id: Uuid) -> Uuid {
        config.add_attention(
            session_id,
            None,
            "devbox".to_string(),
            Some("Codex".to_string()),
            Some("Needs input".to_string()),
            false,
        )
    }

    #[test]
    fn add_attention_records_unread_entry() {
        let mut config = AgentNotificationsConfig::default();
        let session_id = Uuid::new_v4();

        let id = add_test_entry(&mut config, session_id);

        assert_eq!(config.entries.len(), 1);
        assert_eq!(config.unread_count(), 1);
        assert_eq!(config.latest_unread().map(|entry| entry.id), Some(id));
    }

    #[test]
    fn mark_session_read_marks_all_matching_entries() {
        let mut config = AgentNotificationsConfig::default();
        let session_id = Uuid::new_v4();
        add_test_entry(&mut config, session_id);
        add_test_entry(&mut config, session_id);
        add_test_entry(&mut config, Uuid::new_v4());

        assert!(config.mark_session_read(session_id));

        assert_eq!(config.unread_count(), 1);
        assert!(
            config
                .entries
                .iter()
                .filter(|entry| entry.session_id == session_id)
                .all(|entry| entry.read_at.is_some())
        );
    }

    #[test]
    fn clear_read_keeps_unread_entries_active() {
        let mut config = AgentNotificationsConfig::default();
        let read_id = add_test_entry(&mut config, Uuid::new_v4());
        let unread_id = add_test_entry(&mut config, Uuid::new_v4());
        assert!(config.mark_read(read_id));

        assert!(config.clear_read());

        assert!(!config.find_entry(read_id).unwrap().is_active());
        assert!(config.find_entry(unread_id).unwrap().is_active());
    }

    #[test]
    fn latest_active_by_session_returns_newest_per_session() {
        let mut config = AgentNotificationsConfig::default();
        let session_id = Uuid::new_v4();
        let older = add_test_entry(&mut config, session_id);
        let newer = add_test_entry(&mut config, session_id);

        let latest = config.latest_active_by_session();

        assert_eq!(latest.len(), 1);
        assert_eq!(latest[0].id, newer);
        assert_ne!(latest[0].id, older);
    }
}
