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

/// Detected operating system from SSH connection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum DetectedOs {
    // BSD family
    FreeBSD,
    OpenBSD,
    NetBSD,
    // macOS
    #[serde(rename = "macos")]
    MacOS,
    // Windows
    Windows,
    // Linux distributions
    Ubuntu,
    Debian,
    Fedora,
    #[serde(rename = "arch")]
    Arch,
    CentOS,
    #[serde(rename = "redhat")]
    RedHat,
    #[serde(rename = "opensuse")]
    OpenSUSE,
    NixOS,
    Manjaro,
    Mint,
    #[serde(rename = "popos")]
    PopOS,
    Gentoo,
    Alpine,
    Kali,
    Rocky,
    Alma,
    // Generic Linux (fallback for unknown distros)
    Linux,
    // Unknown OS
    #[serde(untagged)]
    Unknown(String),
}

impl DetectedOs {
    /// Parse from uname -s output (determines OS family)
    pub fn from_uname(output: &str) -> Self {
        let normalized = output.trim().to_lowercase();
        match normalized.as_str() {
            "linux" => DetectedOs::Linux, // Will be refined by from_os_release
            "darwin" => DetectedOs::MacOS,
            "freebsd" => DetectedOs::FreeBSD,
            "openbsd" => DetectedOs::OpenBSD,
            "netbsd" => DetectedOs::NetBSD,
            s if s.contains("mingw") || s.contains("cygwin") || s.contains("msys") => {
                DetectedOs::Windows
            }
            other => DetectedOs::Unknown(other.to_string()),
        }
    }

    /// Parse from /etc/os-release content to identify specific Linux distro
    pub fn from_os_release(content: &str) -> Option<Self> {
        // Parse ID= field from os-release
        for line in content.lines() {
            let line = line.trim();
            if let Some(stripped) = line.strip_prefix("ID=") {
                let id = stripped.trim_matches('"').trim_matches('\'').to_lowercase();
                return Some(match id.as_str() {
                    "ubuntu" => DetectedOs::Ubuntu,
                    "debian" => DetectedOs::Debian,
                    "fedora" => DetectedOs::Fedora,
                    "arch" | "archlinux" => DetectedOs::Arch,
                    "centos" => DetectedOs::CentOS,
                    "rhel" | "redhat" => DetectedOs::RedHat,
                    "opensuse" | "opensuse-leap" | "opensuse-tumbleweed" => DetectedOs::OpenSUSE,
                    "nixos" => DetectedOs::NixOS,
                    "manjaro" => DetectedOs::Manjaro,
                    "linuxmint" => DetectedOs::Mint,
                    "pop" => DetectedOs::PopOS,
                    "gentoo" => DetectedOs::Gentoo,
                    "alpine" => DetectedOs::Alpine,
                    "kali" => DetectedOs::Kali,
                    "rocky" => DetectedOs::Rocky,
                    "almalinux" => DetectedOs::Alma,
                    _ => DetectedOs::Linux, // Unknown distro, use generic Linux
                });
            }
        }
        None
    }

    /// Check if this is a Linux variant
    pub fn is_linux(&self) -> bool {
        matches!(
            self,
            DetectedOs::Linux
                | DetectedOs::Ubuntu
                | DetectedOs::Debian
                | DetectedOs::Fedora
                | DetectedOs::Arch
                | DetectedOs::CentOS
                | DetectedOs::RedHat
                | DetectedOs::OpenSUSE
                | DetectedOs::NixOS
                | DetectedOs::Manjaro
                | DetectedOs::Mint
                | DetectedOs::PopOS
                | DetectedOs::Gentoo
                | DetectedOs::Alpine
                | DetectedOs::Kali
                | DetectedOs::Rocky
                | DetectedOs::Alma
        )
    }

    /// Get display name for UI
    pub fn display_name(&self) -> &str {
        match self {
            DetectedOs::FreeBSD => "FreeBSD",
            DetectedOs::OpenBSD => "OpenBSD",
            DetectedOs::NetBSD => "NetBSD",
            DetectedOs::MacOS => "macOS",
            DetectedOs::Windows => "Windows",
            DetectedOs::Ubuntu => "Ubuntu",
            DetectedOs::Debian => "Debian",
            DetectedOs::Fedora => "Fedora",
            DetectedOs::Arch => "Arch Linux",
            DetectedOs::CentOS => "CentOS",
            DetectedOs::RedHat => "Red Hat",
            DetectedOs::OpenSUSE => "openSUSE",
            DetectedOs::NixOS => "NixOS",
            DetectedOs::Manjaro => "Manjaro",
            DetectedOs::Mint => "Linux Mint",
            DetectedOs::PopOS => "Pop!_OS",
            DetectedOs::Gentoo => "Gentoo",
            DetectedOs::Alpine => "Alpine",
            DetectedOs::Kali => "Kali Linux",
            DetectedOs::Rocky => "Rocky Linux",
            DetectedOs::Alma => "AlmaLinux",
            DetectedOs::Linux => "Linux",
            DetectedOs::Unknown(s) => s,
        }
    }

    /// Get icon color for theming (returns RGB tuple)
    pub fn icon_color(&self) -> (u8, u8, u8) {
        match self {
            DetectedOs::FreeBSD => (0xAB, 0x22, 0x28),    // Red
            DetectedOs::OpenBSD => (0xF2, 0xCA, 0x30),    // Yellow
            DetectedOs::NetBSD => (0xF0, 0x80, 0x00),     // Orange
            DetectedOs::MacOS => (0xA0, 0xA0, 0xA0),      // Gray
            DetectedOs::Windows => (0x00, 0x78, 0xD4),    // Blue
            DetectedOs::Ubuntu => (0xE9, 0x54, 0x20),     // Ubuntu orange
            DetectedOs::Debian => (0xA8, 0x00, 0x30),     // Debian red
            DetectedOs::Fedora => (0x51, 0xA2, 0xDA),     // Fedora blue
            DetectedOs::Arch => (0x17, 0x93, 0xD1),       // Arch blue
            DetectedOs::CentOS => (0x93, 0x2E, 0x7D),     // CentOS purple
            DetectedOs::RedHat => (0xEE, 0x00, 0x00),     // Red Hat red
            DetectedOs::OpenSUSE => (0x73, 0xBA, 0x25),   // openSUSE green
            DetectedOs::NixOS => (0x7E, 0xBF, 0xFE),      // NixOS blue
            DetectedOs::Manjaro => (0x35, 0xBF, 0x5C),    // Manjaro green
            DetectedOs::Mint => (0x87, 0xCF, 0x3E),       // Mint green
            DetectedOs::PopOS => (0x48, 0xB9, 0xC7),      // Pop cyan
            DetectedOs::Gentoo => (0xBB, 0xBB, 0xD1),     // Gentoo lavender
            DetectedOs::Alpine => (0x0D, 0x59, 0x7F),     // Alpine blue
            DetectedOs::Kali => (0x55, 0x7C, 0x94),       // Kali blue-gray
            DetectedOs::Rocky => (0x10, 0xB9, 0x81),      // Rocky green
            DetectedOs::Alma => (0x0F, 0x43, 0x28),       // Alma dark green
            DetectedOs::Linux => (0xE9, 0x5A, 0x20),      // Generic orange
            DetectedOs::Unknown(_) => (0x70, 0x70, 0x70), // Muted gray
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
    /// Detected operating system (populated on first successful connection)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_os: Option<DetectedOs>,
    /// Last successful connection timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_connected: Option<chrono::DateTime<chrono::Utc>>,
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

    /// Find host by ID (mutable)
    pub fn find_host_mut(&mut self, id: Uuid) -> Option<&mut Host> {
        self.hosts.iter_mut().find(|h| h.id == id)
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
        super::write_atomic(&path, &content).map_err(|e| ConfigError::WriteFile { path, source: e })
    }
}
