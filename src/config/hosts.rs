use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::error::ConfigError;

/// Authentication method for SSH connection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Password authentication
    Password,
    /// Public key authentication
    PublicKey {
        #[serde(default)]
        key_path: Option<PathBuf>,
    },
    /// SSH Agent authentication
    #[default]
    Agent,
}


fn default_port() -> u16 {
    22
}

/// Single SSH host configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    pub id: Uuid,
    pub name: String,
    pub hostname: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub auth: AuthMethod,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}


/// Group/folder for organizing hosts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostGroup {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<Uuid>,
    #[serde(default)]
    pub collapsed: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}


/// Root configuration for hosts.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HostsConfig {
    #[serde(default)]
    pub hosts: Vec<Host>,
    #[serde(default)]
    pub groups: Vec<HostGroup>,
}

impl HostsConfig {
    /// Find host by ID
    pub fn find_host(&self, id: Uuid) -> Option<&Host> {
        self.hosts.iter().find(|h| h.id == id)
    }

    /// Find group by ID
    pub fn find_group(&self, id: Uuid) -> Option<&HostGroup> {
        self.groups.iter().find(|g| g.id == id)
    }

    /// Find group by ID (mutable)
    pub fn find_group_mut(&mut self, id: Uuid) -> Option<&mut HostGroup> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    /// Add a new host
    pub fn add_host(&mut self, host: Host) {
        self.hosts.push(host);
    }

    /// Update an existing host
    pub fn update_host(&mut self, host: Host) -> Result<(), ConfigError> {
        let existing = self
            .hosts
            .iter_mut()
            .find(|h| h.id == host.id)
            .ok_or(ConfigError::HostNotFound(host.id))?;
        *existing = host;
        Ok(())
    }

    /// Load from file, creating default if not exists
    pub fn load() -> Result<Self, ConfigError> {
        let path = super::paths::hosts_file().ok_or_else(|| ConfigError::ReadFile {
            path: std::path::PathBuf::from("hosts.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine hosts file path",
            ),
        })?;

        tracing::debug!("Loading hosts from: {:?}", path);

        if !path.exists() {
            tracing::warn!("Hosts file does not exist: {:?}", path);
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

        let path = super::paths::hosts_file().ok_or_else(|| ConfigError::WriteFile {
            path: std::path::PathBuf::from("hosts.toml"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine hosts file path",
            ),
        })?;

        let content = toml::to_string_pretty(self).map_err(ConfigError::Serialize)?;
        std::fs::write(&path, content).map_err(|e| ConfigError::WriteFile { path, source: e })
    }
}
