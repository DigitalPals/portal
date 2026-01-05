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

impl AuthMethod {
    pub fn display_name(&self) -> &'static str {
        match self {
            AuthMethod::Password => "Password",
            AuthMethod::PublicKey { .. } => "Public Key",
            AuthMethod::Agent => "SSH Agent",
        }
    }
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

impl Host {
    pub fn new(name: String, hostname: String, username: String) -> Self {
        let now = chrono::Utc::now();
        Self {
            id: Uuid::new_v4(),
            name,
            hostname,
            port: 22,
            username,
            auth: AuthMethod::default(),
            group_id: None,
            notes: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
        }
    }

    /// Returns connection string for display: user@host:port
    pub fn connection_string(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.username, self.hostname)
        } else {
            format!("{}@{}:{}", self.username, self.hostname, self.port)
        }
    }

    /// Update the updated_at timestamp
    pub fn touch(&mut self) {
        self.updated_at = chrono::Utc::now();
    }
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

impl HostGroup {
    pub fn new(name: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            parent_id: None,
            collapsed: false,
            created_at: chrono::Utc::now(),
        }
    }
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
    /// Get hosts in a specific group (or root if group_id is None)
    pub fn hosts_in_group(&self, group_id: Option<Uuid>) -> Vec<&Host> {
        self.hosts
            .iter()
            .filter(|h| h.group_id == group_id)
            .collect()
    }

    /// Get child groups of a parent
    pub fn child_groups(&self, parent_id: Option<Uuid>) -> Vec<&HostGroup> {
        self.groups
            .iter()
            .filter(|g| g.parent_id == parent_id)
            .collect()
    }

    /// Find host by ID
    pub fn find_host(&self, id: Uuid) -> Option<&Host> {
        self.hosts.iter().find(|h| h.id == id)
    }

    /// Find host by ID (mutable)
    pub fn find_host_mut(&mut self, id: Uuid) -> Option<&mut Host> {
        self.hosts.iter_mut().find(|h| h.id == id)
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

    /// Delete a host by ID
    pub fn delete_host(&mut self, id: Uuid) -> Result<Host, ConfigError> {
        let pos = self
            .hosts
            .iter()
            .position(|h| h.id == id)
            .ok_or(ConfigError::HostNotFound(id))?;
        Ok(self.hosts.remove(pos))
    }

    /// Add a new group
    pub fn add_group(&mut self, group: HostGroup) {
        self.groups.push(group);
    }

    /// Delete a group by ID (moves hosts to parent group)
    pub fn delete_group(&mut self, id: Uuid) -> Result<HostGroup, ConfigError> {
        let group = self
            .groups
            .iter()
            .find(|g| g.id == id)
            .ok_or(ConfigError::GroupNotFound(id))?;
        let parent_id = group.parent_id;

        // Move hosts to parent group
        for host in &mut self.hosts {
            if host.group_id == Some(id) {
                host.group_id = parent_id;
            }
        }

        // Move child groups to parent
        for child in &mut self.groups {
            if child.parent_id == Some(id) {
                child.parent_id = parent_id;
            }
        }

        let pos = self.groups.iter().position(|g| g.id == id).unwrap();
        Ok(self.groups.remove(pos))
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
