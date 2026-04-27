use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

use crate::error::ConfigError;

/// Connection protocol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Protocol {
    #[default]
    Ssh,
    Vnc,
}

/// SSH port forward type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PortForwardKind {
    Local,
    Remote,
    Dynamic,
}

impl PortForwardKind {
    pub const ALL: [PortForwardKind; 3] = [
        PortForwardKind::Local,
        PortForwardKind::Remote,
        PortForwardKind::Dynamic,
    ];
}

impl std::fmt::Display for PortForwardKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PortForwardKind::Local => write!(f, "Local"),
            PortForwardKind::Remote => write!(f, "Remote"),
            PortForwardKind::Dynamic => write!(f, "Dynamic"),
        }
    }
}

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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        vault_key_id: Option<Uuid>,
    },
    /// SSH Agent authentication
    #[default]
    Agent,
}

fn default_bind_host() -> String {
    "localhost".to_string()
}

fn default_port_forward_enabled() -> bool {
    true
}

/// SSH port forwarding configuration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PortForward {
    pub id: Uuid,
    pub kind: PortForwardKind,
    #[serde(default = "default_bind_host")]
    pub bind_host: String,
    pub bind_port: u16,
    pub target_host: String,
    pub target_port: u16,
    #[serde(default = "default_port_forward_enabled")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
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
        let mut id = None;
        let mut id_like = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if let Some(stripped) = line.strip_prefix("ID=") {
                id = Some(Self::from_os_release_id(stripped));
                continue;
            }
            if let Some(stripped) = line.strip_prefix("ID_LIKE=") {
                id_like = parse_os_release_value(stripped)
                    .split_whitespace()
                    .map(str::to_string)
                    .collect();
            }
        }

        let id_like_os = id_like
            .into_iter()
            .map(|id| Self::from_os_release_id(&id))
            .find(|os| *os != DetectedOs::Linux);

        match id {
            Some(DetectedOs::Linux) => id_like_os.or(Some(DetectedOs::Linux)),
            Some(os) => Some(os),
            None => id_like_os,
        }
    }

    fn from_os_release_id(raw: &str) -> Self {
        let id = parse_os_release_value(raw).to_lowercase();
        match id.as_str() {
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
            _ => DetectedOs::Linux,
        }
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

fn parse_os_release_value(raw: &str) -> String {
    let trimmed = raw.trim();
    let mut without_comment = String::with_capacity(trimmed.len());
    let mut quote = None;
    let mut escaped = false;

    for ch in trimmed.chars() {
        if escaped {
            without_comment.push(ch);
            escaped = false;
            continue;
        }

        match (ch, quote) {
            ('\\', Some(_)) => {
                escaped = true;
            }
            ('"' | '\'', None) => {
                quote = Some(ch);
                without_comment.push(ch);
            }
            (current, Some(active)) if current == active => {
                quote = None;
                without_comment.push(ch);
            }
            ('#', None) => break,
            _ => without_comment.push(ch),
        }
    }

    without_comment.trim().trim_matches(['"', '\'']).to_string()
}

fn default_port() -> u16 {
    22
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Single host configuration (SSH or VNC)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Host {
    pub id: Uuid,
    pub name: String,
    pub hostname: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub username: String,
    /// Connection protocol (SSH or VNC)
    #[serde(default)]
    pub protocol: Protocol,
    /// VNC port (defaults to 5900 when protocol is VNC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vnc_port: Option<u16>,
    /// Encrypted vault secret used as the default VNC password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vnc_password_id: Option<Uuid>,
    #[serde(default)]
    pub auth: AuthMethod,
    /// Enable SSH agent forwarding for this host
    #[serde(default)]
    pub agent_forwarding: bool,
    /// SSH port forwards (-L and -R)
    #[serde(default)]
    pub port_forwards: Vec<PortForward>,
    /// Route SSH terminal sessions for this host through Portal Hub.
    #[serde(default, skip_serializing_if = "is_false")]
    pub portal_hub_enabled: bool,
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

impl Host {
    /// Get the effective VNC port (vnc_port or default 5900)
    pub fn effective_vnc_port(&self) -> u16 {
        self.vnc_port.unwrap_or(5900)
    }

    /// Get the effective SSH username (host override or current user)
    pub fn effective_username(&self) -> String {
        let trimmed = self.username.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }

        default_username()
    }
}

pub(crate) fn default_username() -> String {
    first_non_empty_value(["USER", "USERNAME"].into_iter().filter_map(|key| {
        std::env::var(key)
            .ok()
            .map(|value| value.trim().to_string())
    }))
    .unwrap_or_else(|| "root".to_string())
}

fn first_non_empty_value<I>(values: I) -> Option<String>
where
    I: IntoIterator<Item = String>,
{
    values.into_iter().find(|value| !value.is_empty())
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

    /// Remove an existing host.
    pub fn remove_host(&mut self, id: Uuid) -> Result<Host, ConfigError> {
        let index = self
            .hosts
            .iter()
            .position(|host| host.id == id)
            .ok_or(ConfigError::HostNotFound(id))?;

        Ok(self.hosts.remove(index))
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

    /// Import hosts from the user's SSH config file.
    /// Returns the number of new hosts imported.
    pub fn import_from_ssh_config(&mut self) -> Result<usize, ConfigError> {
        let mut imported = 0usize;
        let existing_keys: std::collections::HashSet<(String, u16, String)> = self
            .hosts
            .iter()
            .map(|host| {
                (
                    host.hostname.to_ascii_lowercase(),
                    host.port,
                    host.effective_username().to_ascii_lowercase(),
                )
            })
            .collect();
        let mut seen = existing_keys;

        let ssh_hosts = super::ssh_config::load_hosts_from_ssh_config()?;
        for host in ssh_hosts {
            let key = (
                host.hostname.to_ascii_lowercase(),
                host.port,
                host.effective_username().to_ascii_lowercase(),
            );
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            self.hosts.push(host);
            imported += 1;
        }

        Ok(imported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_host(name: &str) -> Host {
        let now = chrono::Utc::now();
        Host {
            id: Uuid::new_v4(),
            name: name.to_string(),
            hostname: format!("{}.example.test", name.to_lowercase()),
            port: 22,
            username: "root".to_string(),
            protocol: Protocol::Ssh,
            vnc_port: None,
            vnc_password_id: None,
            auth: AuthMethod::Agent,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            portal_hub_enabled: false,
            group_id: None,
            notes: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        }
    }

    #[test]
    fn remove_host_removes_matching_host() {
        let kept = test_host("Kept");
        let deleted = test_host("Deleted");
        let deleted_id = deleted.id;
        let mut config = HostsConfig {
            hosts: vec![kept.clone(), deleted],
            groups: Vec::new(),
        };

        let removed = config.remove_host(deleted_id).unwrap();

        assert_eq!(removed.id, deleted_id);
        assert_eq!(config.hosts.len(), 1);
        assert_eq!(config.hosts[0].id, kept.id);
    }

    #[test]
    fn remove_host_returns_error_for_missing_host() {
        let missing_id = Uuid::new_v4();
        let mut config = HostsConfig::default();

        let error = config.remove_host(missing_id).unwrap_err();

        assert!(matches!(error, ConfigError::HostNotFound(id) if id == missing_id));
    }

    // === DetectedOs::from_uname tests ===

    #[test]
    fn from_uname_linux() {
        assert_eq!(DetectedOs::from_uname("Linux"), DetectedOs::Linux);
        assert_eq!(DetectedOs::from_uname("linux"), DetectedOs::Linux);
        assert_eq!(DetectedOs::from_uname("LINUX"), DetectedOs::Linux);
        assert_eq!(DetectedOs::from_uname("  Linux  \n"), DetectedOs::Linux);
    }

    #[test]
    fn from_uname_darwin() {
        assert_eq!(DetectedOs::from_uname("Darwin"), DetectedOs::MacOS);
        assert_eq!(DetectedOs::from_uname("darwin"), DetectedOs::MacOS);
        assert_eq!(DetectedOs::from_uname("DARWIN\n"), DetectedOs::MacOS);
    }

    #[test]
    fn from_uname_freebsd() {
        assert_eq!(DetectedOs::from_uname("FreeBSD"), DetectedOs::FreeBSD);
        assert_eq!(DetectedOs::from_uname("freebsd"), DetectedOs::FreeBSD);
    }

    #[test]
    fn from_uname_openbsd() {
        assert_eq!(DetectedOs::from_uname("OpenBSD"), DetectedOs::OpenBSD);
        assert_eq!(DetectedOs::from_uname("openbsd"), DetectedOs::OpenBSD);
    }

    #[test]
    fn from_uname_netbsd() {
        assert_eq!(DetectedOs::from_uname("NetBSD"), DetectedOs::NetBSD);
        assert_eq!(DetectedOs::from_uname("netbsd"), DetectedOs::NetBSD);
    }

    #[test]
    fn from_uname_windows_mingw() {
        assert_eq!(
            DetectedOs::from_uname("MINGW64_NT-10.0"),
            DetectedOs::Windows
        );
        assert_eq!(
            DetectedOs::from_uname("MINGW32_NT-6.2"),
            DetectedOs::Windows
        );
    }

    #[test]
    fn from_uname_windows_cygwin() {
        assert_eq!(
            DetectedOs::from_uname("CYGWIN_NT-10.0"),
            DetectedOs::Windows
        );
        assert_eq!(DetectedOs::from_uname("cygwin_nt-6.1"), DetectedOs::Windows);
    }

    #[test]
    fn from_uname_windows_msys() {
        assert_eq!(DetectedOs::from_uname("MSYS_NT-10.0"), DetectedOs::Windows);
        assert_eq!(DetectedOs::from_uname("msys_nt-6.3"), DetectedOs::Windows);
    }

    #[test]
    fn from_uname_unknown() {
        let os = DetectedOs::from_uname("SunOS");
        assert!(matches!(os, DetectedOs::Unknown(s) if s == "sunos"));

        let os2 = DetectedOs::from_uname("AIX");
        assert!(matches!(os2, DetectedOs::Unknown(s) if s == "aix"));
    }

    #[test]
    fn from_uname_empty() {
        let os = DetectedOs::from_uname("");
        assert!(matches!(os, DetectedOs::Unknown(s) if s.is_empty()));
    }

    #[test]
    fn from_uname_whitespace_only() {
        let os = DetectedOs::from_uname("   \n\t  ");
        assert!(matches!(os, DetectedOs::Unknown(s) if s.is_empty()));
    }

    #[test]
    fn host_portal_hub_enabled_defaults_false() {
        let host: Host = toml::from_str(
            r#"
id = "11111111-2222-4333-8444-555555555555"
name = "Test"
hostname = "example.com"
port = 22
username = "john"
protocol = "ssh"
agent_forwarding = false
port_forwards = []
tags = []
created_at = "2026-04-25T00:00:00Z"
updated_at = "2026-04-25T00:00:00Z"

[auth]
type = "agent"
"#,
        )
        .unwrap();

        assert!(!host.portal_hub_enabled);
    }

    #[test]
    fn default_username_helper_skips_empty_values() {
        let username =
            first_non_empty_value(["".to_string(), "  ".trim().to_string(), "alice".to_string()]);

        assert_eq!(username.as_deref(), Some("alice"));
    }

    // === DetectedOs::from_os_release tests ===

    #[test]
    fn from_os_release_ubuntu() {
        let content = r#"
NAME="Ubuntu"
VERSION="22.04.3 LTS (Jammy Jellyfish)"
ID=ubuntu
ID_LIKE=debian
PRETTY_NAME="Ubuntu 22.04.3 LTS"
VERSION_ID="22.04"
"#;
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Ubuntu)
        );
    }

    #[test]
    fn from_os_release_debian() {
        let content = r#"
PRETTY_NAME="Debian GNU/Linux 12 (bookworm)"
NAME="Debian GNU/Linux"
VERSION_ID="12"
ID=debian
"#;
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Debian)
        );
    }

    #[test]
    fn from_os_release_fedora() {
        let content = r#"
NAME="Fedora Linux"
VERSION="39 (Workstation Edition)"
ID=fedora
VERSION_ID=39
"#;
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Fedora)
        );
    }

    #[test]
    fn from_os_release_arch() {
        let content = "ID=arch\nNAME=\"Arch Linux\"\n";
        assert_eq!(DetectedOs::from_os_release(content), Some(DetectedOs::Arch));

        let content2 = "ID=archlinux\n";
        assert_eq!(
            DetectedOs::from_os_release(content2),
            Some(DetectedOs::Arch)
        );
    }

    #[test]
    fn from_os_release_centos() {
        let content = r#"
NAME="CentOS Stream"
VERSION="9"
ID="centos"
"#;
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::CentOS)
        );
    }

    #[test]
    fn from_os_release_redhat() {
        let content = "ID=rhel\nNAME=\"Red Hat Enterprise Linux\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::RedHat)
        );

        let content2 = "ID=redhat\n";
        assert_eq!(
            DetectedOs::from_os_release(content2),
            Some(DetectedOs::RedHat)
        );
    }

    #[test]
    fn from_os_release_opensuse() {
        let content = "ID=opensuse-leap\nNAME=\"openSUSE Leap\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::OpenSUSE)
        );

        let content2 = "ID=opensuse-tumbleweed\n";
        assert_eq!(
            DetectedOs::from_os_release(content2),
            Some(DetectedOs::OpenSUSE)
        );

        let content3 = "ID=opensuse\n";
        assert_eq!(
            DetectedOs::from_os_release(content3),
            Some(DetectedOs::OpenSUSE)
        );
    }

    #[test]
    fn from_os_release_nixos() {
        let content = r#"
ID=nixos
NAME=NixOS
VERSION="24.05 (Uakari)"
"#;
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::NixOS)
        );
    }

    #[test]
    fn from_os_release_manjaro() {
        let content = "ID=manjaro\nNAME=\"Manjaro Linux\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Manjaro)
        );
    }

    #[test]
    fn from_os_release_mint() {
        let content = "ID=linuxmint\nNAME=\"Linux Mint\"\n";
        assert_eq!(DetectedOs::from_os_release(content), Some(DetectedOs::Mint));
    }

    #[test]
    fn from_os_release_popos() {
        let content = "ID=pop\nNAME=\"Pop!_OS\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::PopOS)
        );
    }

    #[test]
    fn from_os_release_gentoo() {
        let content = "ID=gentoo\nNAME=\"Gentoo\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Gentoo)
        );
    }

    #[test]
    fn from_os_release_alpine() {
        let content = "ID=alpine\nNAME=\"Alpine Linux\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Alpine)
        );
    }

    #[test]
    fn from_os_release_kali() {
        let content = "ID=kali\nNAME=\"Kali GNU/Linux\"\n";
        assert_eq!(DetectedOs::from_os_release(content), Some(DetectedOs::Kali));
    }

    #[test]
    fn from_os_release_rocky() {
        let content = "ID=rocky\nNAME=\"Rocky Linux\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Rocky)
        );
    }

    #[test]
    fn from_os_release_alma() {
        let content = "ID=almalinux\nNAME=\"AlmaLinux\"\n";
        assert_eq!(DetectedOs::from_os_release(content), Some(DetectedOs::Alma));
    }

    #[test]
    fn from_os_release_unknown_distro() {
        let content = "ID=someunknowndistro\nNAME=\"Unknown\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Linux)
        );
    }

    #[test]
    fn from_os_release_quoted_id() {
        let content = "ID=\"ubuntu\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Ubuntu)
        );

        let content2 = "ID='debian'\n";
        assert_eq!(
            DetectedOs::from_os_release(content2),
            Some(DetectedOs::Debian)
        );
    }

    #[test]
    fn from_os_release_strips_inline_comment() {
        let content = "ID=ubuntu # comment\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Ubuntu)
        );
    }

    #[test]
    fn os_release_value_keeps_hash_inside_quotes() {
        assert_eq!(
            parse_os_release_value(r#""ubuntu#daily" # comment"#),
            "ubuntu#daily"
        );
        assert_eq!(
            parse_os_release_value(r#"'debian#stable'"#),
            "debian#stable"
        );
    }

    #[test]
    fn from_os_release_uses_id_like_when_id_missing() {
        let content = "NAME=\"Raspberry Pi OS\"\nID_LIKE=\"debian\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Debian)
        );
    }

    #[test]
    fn from_os_release_uses_id_like_when_id_is_unknown_linux() {
        let content = "ID=raspbian\nID_LIKE=\"debian\"\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Debian)
        );
    }

    #[test]
    fn from_os_release_no_id_field() {
        let content = "NAME=\"Some Linux\"\nVERSION=\"1.0\"\n";
        assert_eq!(DetectedOs::from_os_release(content), None);
    }

    #[test]
    fn from_os_release_empty() {
        assert_eq!(DetectedOs::from_os_release(""), None);
    }

    #[test]
    fn from_os_release_case_insensitive_id() {
        let content = "ID=UBUNTU\n";
        assert_eq!(
            DetectedOs::from_os_release(content),
            Some(DetectedOs::Ubuntu)
        );

        let content2 = "ID=FeDora\n";
        assert_eq!(
            DetectedOs::from_os_release(content2),
            Some(DetectedOs::Fedora)
        );
    }

    // === DetectedOs::is_linux tests ===

    #[test]
    fn is_linux_true_for_linux_variants() {
        assert!(DetectedOs::Linux.is_linux());
        assert!(DetectedOs::Ubuntu.is_linux());
        assert!(DetectedOs::Debian.is_linux());
        assert!(DetectedOs::Fedora.is_linux());
        assert!(DetectedOs::Arch.is_linux());
        assert!(DetectedOs::CentOS.is_linux());
        assert!(DetectedOs::RedHat.is_linux());
        assert!(DetectedOs::OpenSUSE.is_linux());
        assert!(DetectedOs::NixOS.is_linux());
        assert!(DetectedOs::Manjaro.is_linux());
        assert!(DetectedOs::Mint.is_linux());
        assert!(DetectedOs::PopOS.is_linux());
        assert!(DetectedOs::Gentoo.is_linux());
        assert!(DetectedOs::Alpine.is_linux());
        assert!(DetectedOs::Kali.is_linux());
        assert!(DetectedOs::Rocky.is_linux());
        assert!(DetectedOs::Alma.is_linux());
    }

    #[test]
    fn is_linux_false_for_non_linux() {
        assert!(!DetectedOs::MacOS.is_linux());
        assert!(!DetectedOs::FreeBSD.is_linux());
        assert!(!DetectedOs::OpenBSD.is_linux());
        assert!(!DetectedOs::NetBSD.is_linux());
        assert!(!DetectedOs::Windows.is_linux());
        assert!(!DetectedOs::Unknown("SunOS".to_string()).is_linux());
    }

    // === DetectedOs::display_name tests ===

    #[test]
    fn display_name_bsd() {
        assert_eq!(DetectedOs::FreeBSD.display_name(), "FreeBSD");
        assert_eq!(DetectedOs::OpenBSD.display_name(), "OpenBSD");
        assert_eq!(DetectedOs::NetBSD.display_name(), "NetBSD");
    }

    #[test]
    fn display_name_macos_windows() {
        assert_eq!(DetectedOs::MacOS.display_name(), "macOS");
        assert_eq!(DetectedOs::Windows.display_name(), "Windows");
    }

    #[test]
    fn display_name_linux_distros() {
        assert_eq!(DetectedOs::Ubuntu.display_name(), "Ubuntu");
        assert_eq!(DetectedOs::Debian.display_name(), "Debian");
        assert_eq!(DetectedOs::Fedora.display_name(), "Fedora");
        assert_eq!(DetectedOs::Arch.display_name(), "Arch Linux");
        assert_eq!(DetectedOs::CentOS.display_name(), "CentOS");
        assert_eq!(DetectedOs::RedHat.display_name(), "Red Hat");
        assert_eq!(DetectedOs::OpenSUSE.display_name(), "openSUSE");
        assert_eq!(DetectedOs::NixOS.display_name(), "NixOS");
        assert_eq!(DetectedOs::Manjaro.display_name(), "Manjaro");
        assert_eq!(DetectedOs::Mint.display_name(), "Linux Mint");
        assert_eq!(DetectedOs::PopOS.display_name(), "Pop!_OS");
        assert_eq!(DetectedOs::Gentoo.display_name(), "Gentoo");
        assert_eq!(DetectedOs::Alpine.display_name(), "Alpine");
        assert_eq!(DetectedOs::Kali.display_name(), "Kali Linux");
        assert_eq!(DetectedOs::Rocky.display_name(), "Rocky Linux");
        assert_eq!(DetectedOs::Alma.display_name(), "AlmaLinux");
        assert_eq!(DetectedOs::Linux.display_name(), "Linux");
    }

    #[test]
    fn display_name_unknown() {
        assert_eq!(
            DetectedOs::Unknown("SunOS".to_string()).display_name(),
            "SunOS"
        );
        assert_eq!(DetectedOs::Unknown("AIX".to_string()).display_name(), "AIX");
    }

    // === DetectedOs::icon_color tests ===

    #[test]
    fn icon_color_returns_valid_rgb() {
        // Test that all variants return valid RGB tuples
        let variants = [
            DetectedOs::FreeBSD,
            DetectedOs::OpenBSD,
            DetectedOs::NetBSD,
            DetectedOs::MacOS,
            DetectedOs::Windows,
            DetectedOs::Ubuntu,
            DetectedOs::Debian,
            DetectedOs::Fedora,
            DetectedOs::Arch,
            DetectedOs::CentOS,
            DetectedOs::RedHat,
            DetectedOs::OpenSUSE,
            DetectedOs::NixOS,
            DetectedOs::Manjaro,
            DetectedOs::Mint,
            DetectedOs::PopOS,
            DetectedOs::Gentoo,
            DetectedOs::Alpine,
            DetectedOs::Kali,
            DetectedOs::Rocky,
            DetectedOs::Alma,
            DetectedOs::Linux,
            DetectedOs::Unknown("test".to_string()),
        ];

        for variant in variants {
            let (r, g, b) = variant.icon_color();
            // All RGB values are valid u8, so just verify we get a tuple
            let _ = (r, g, b);
        }
    }

    #[test]
    fn icon_color_specific_values() {
        // Test a few specific color values
        assert_eq!(DetectedOs::Ubuntu.icon_color(), (0xE9, 0x54, 0x20));
        assert_eq!(DetectedOs::Fedora.icon_color(), (0x51, 0xA2, 0xDA));
        assert_eq!(DetectedOs::Windows.icon_color(), (0x00, 0x78, 0xD4));
        assert_eq!(
            DetectedOs::Unknown("x".to_string()).icon_color(),
            (0x70, 0x70, 0x70)
        );
    }

    // === DetectedOs trait tests ===

    #[test]
    fn detected_os_clone() {
        let os = DetectedOs::Ubuntu;
        let cloned = os.clone();
        assert_eq!(os, cloned);

        let unknown = DetectedOs::Unknown("Custom".to_string());
        let cloned_unknown = unknown.clone();
        assert_eq!(unknown, cloned_unknown);
    }

    #[test]
    fn detected_os_debug() {
        let debug_str = format!("{:?}", DetectedOs::Ubuntu);
        assert!(debug_str.contains("Ubuntu"));

        let debug_unknown = format!("{:?}", DetectedOs::Unknown("Test".to_string()));
        assert!(debug_unknown.contains("Unknown"));
        assert!(debug_unknown.contains("Test"));
    }

    #[test]
    fn detected_os_equality() {
        assert_eq!(DetectedOs::Ubuntu, DetectedOs::Ubuntu);
        assert_ne!(DetectedOs::Ubuntu, DetectedOs::Debian);
        assert_ne!(DetectedOs::Linux, DetectedOs::MacOS);

        let unknown1 = DetectedOs::Unknown("test".to_string());
        let unknown2 = DetectedOs::Unknown("test".to_string());
        let unknown3 = DetectedOs::Unknown("other".to_string());
        assert_eq!(unknown1, unknown2);
        assert_ne!(unknown1, unknown3);
    }

    // === AuthMethod tests ===

    #[test]
    fn auth_method_default_is_agent() {
        let auth = AuthMethod::default();
        assert_eq!(auth, AuthMethod::Agent);
    }

    #[test]
    fn auth_method_password() {
        let auth = AuthMethod::Password;
        assert_eq!(auth, AuthMethod::Password);
    }

    #[test]
    fn auth_method_public_key_with_path() {
        let auth = AuthMethod::PublicKey {
            key_path: Some(PathBuf::from("/home/user/.ssh/id_ed25519")),
            vault_key_id: None,
        };
        if let AuthMethod::PublicKey { key_path, .. } = auth {
            assert_eq!(key_path, Some(PathBuf::from("/home/user/.ssh/id_ed25519")));
        } else {
            panic!("Expected PublicKey variant");
        }
    }

    #[test]
    fn auth_method_public_key_without_path() {
        let auth = AuthMethod::PublicKey {
            key_path: None,
            vault_key_id: None,
        };
        if let AuthMethod::PublicKey { key_path, .. } = auth {
            assert_eq!(key_path, None);
        } else {
            panic!("Expected PublicKey variant");
        }
    }

    #[test]
    fn auth_method_public_key_with_vault_key() {
        let id = Uuid::new_v4();
        let auth = AuthMethod::PublicKey {
            key_path: None,
            vault_key_id: Some(id),
        };
        if let AuthMethod::PublicKey { vault_key_id, .. } = auth {
            assert_eq!(vault_key_id, Some(id));
        } else {
            panic!("Expected PublicKey variant");
        }
    }

    #[test]
    fn auth_method_equality() {
        assert_eq!(AuthMethod::Password, AuthMethod::Password);
        assert_eq!(AuthMethod::Agent, AuthMethod::Agent);
        assert_ne!(AuthMethod::Password, AuthMethod::Agent);

        let pk1 = AuthMethod::PublicKey {
            key_path: Some(PathBuf::from("/a")),
            vault_key_id: None,
        };
        let pk2 = AuthMethod::PublicKey {
            key_path: Some(PathBuf::from("/a")),
            vault_key_id: None,
        };
        let pk3 = AuthMethod::PublicKey {
            key_path: Some(PathBuf::from("/b")),
            vault_key_id: None,
        };
        assert_eq!(pk1, pk2);
        assert_ne!(pk1, pk3);
    }

    // === PortForward tests ===

    #[test]
    fn port_forward_defaults_bind_host_and_enabled() {
        let id = Uuid::new_v4();
        let toml_str = format!(
            r#"
id = "{}"
kind = "local"
bind_port = 8080
target_host = "127.0.0.1"
target_port = 80
"#,
            id
        );
        let forward: PortForward = toml::from_str(&toml_str).expect("PortForward parse failed");
        assert_eq!(forward.bind_host, "localhost");
        assert!(forward.enabled);
        assert!(forward.description.is_none());
    }

    #[test]
    fn port_forward_kind_display() {
        assert_eq!(PortForwardKind::Local.to_string(), "Local");
        assert_eq!(PortForwardKind::Remote.to_string(), "Remote");
        assert_eq!(PortForwardKind::Dynamic.to_string(), "Dynamic");
    }
}
