use std::collections::HashMap;

use iced::widget::{
    Row, Space, button, checkbox, column, container, pick_list, row, scrollable, text, text_input,
};
use iced::{Alignment, Element, Length};
use uuid::Uuid;

use crate::config::hosts::{HubRouting, default_username};
use crate::config::{AuthMethod, Host, PortForward, PortForwardKind, Protocol};
use crate::hub::vault::{VaultKey, VaultSecret};
use crate::message::{DialogMessage, HostDialogField, Message};
use crate::theme::{BORDER_RADIUS, ScaledFonts, Theme};
use crate::validation::{validate_hostname, validate_port, validate_username};
use crate::views::components::toggle_group;

use super::common::{
    ERROR_COLOR, destructive_button_style, dialog_input_style, dialog_input_style_with_error,
    dialog_pick_list_menu_style, dialog_pick_list_style, primary_button_style,
    secondary_button_style,
};

/// Widget ID for host dialog fields (for keyboard navigation)
pub fn host_dialog_field_id(index: usize) -> iced::widget::Id {
    match index {
        0 => iced::widget::Id::new("host_dialog_field_0"),
        1 => iced::widget::Id::new("host_dialog_field_1"),
        2 => iced::widget::Id::new("host_dialog_field_2"),
        3 => iced::widget::Id::new("host_dialog_field_3"),
        5 => iced::widget::Id::new("host_dialog_field_5"),
        6 => iced::widget::Id::new("host_dialog_field_6"),
        7 => iced::widget::Id::new("host_dialog_field_7"),
        _ => iced::widget::Id::new("host_dialog_field_0"),
    }
}

/// State for the host dialog (add or edit)
#[derive(Debug, Clone)]
pub struct HostDialogState {
    /// None for new host, Some(id) for editing existing
    pub editing_id: Option<Uuid>,
    /// Form fields
    pub name: String,
    pub hostname: String,
    pub port: String,
    pub username: String,
    pub auth_method: AuthMethodChoice,
    pub key_source: KeySourceChoice,
    pub key_path: String,
    pub vault_key_id: Option<Uuid>,
    pub vnc_password_id: Option<Uuid>,
    /// SSH host to tunnel VNC traffic through
    pub vnc_via_ssh_host_id: Option<Uuid>,
    pub agent_forwarding: bool,
    pub hub_routing: HubRouting,
    /// Jump (bastion) host to tunnel through
    pub jump_host_id: Option<Uuid>,
    pub tags: String,
    pub notes: String,
    /// Connection protocol
    pub protocol: ProtocolChoice,
    /// Port forwards for SSH
    pub port_forwards: Vec<PortForward>,
    pub port_forwards_expanded: bool,
    pub port_forward_editor: Option<PortForwardEditorState>,
    pub delete_requested: bool,
    /// Validation errors by field name
    pub validation_errors: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultKeyOption {
    pub id: Uuid,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VncPasswordOption {
    pub id: Option<Uuid>,
    pub label: String,
}

impl VncPasswordOption {
    fn ask_every_time() -> Self {
        Self {
            id: None,
            label: "Ask every time".to_string(),
        }
    }
}

impl From<&VaultSecret> for VncPasswordOption {
    fn from(secret: &VaultSecret) -> Self {
        Self {
            id: Some(secret.id),
            label: secret.name.clone(),
        }
    }
}

impl std::fmt::Display for VncPasswordOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

impl From<&VaultKey> for VaultKeyOption {
    fn from(key: &VaultKey) -> Self {
        let suffix = key
            .fingerprint
            .as_deref()
            .map(|fingerprint| {
                let short = fingerprint
                    .rsplit_once(':')
                    .map(|(_, value)| value)
                    .unwrap_or(fingerprint);
                format!(" ({})", short.chars().take(12).collect::<String>())
            })
            .unwrap_or_default();
        Self {
            id: key.id,
            label: format!("{}{}", key.name, suffix),
        }
    }
}

impl std::fmt::Display for VaultKeyOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

/// Simplified protocol for the dropdown
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProtocolChoice {
    #[default]
    Ssh,
    Vnc,
}

impl std::fmt::Display for ProtocolChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolChoice::Ssh => write!(f, "SSH"),
            ProtocolChoice::Vnc => write!(f, "VNC"),
        }
    }
}

impl ProtocolChoice {
    pub const ALL: [ProtocolChoice; 2] = [ProtocolChoice::Ssh, ProtocolChoice::Vnc];
}

/// Simplified auth method for the dropdown
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuthMethodChoice {
    #[default]
    Agent,
    Password,
    PublicKey,
    KeyboardInteractive,
}

impl std::fmt::Display for AuthMethodChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthMethodChoice::Agent => write!(f, "SSH Agent"),
            AuthMethodChoice::Password => write!(f, "Password"),
            AuthMethodChoice::PublicKey => write!(f, "Public Key"),
            AuthMethodChoice::KeyboardInteractive => write!(f, "Keyboard Interactive"),
        }
    }
}

impl AuthMethodChoice {
    pub const ALL: [AuthMethodChoice; 4] = [
        AuthMethodChoice::Agent,
        AuthMethodChoice::Password,
        AuthMethodChoice::PublicKey,
        AuthMethodChoice::KeyboardInteractive,
    ];
}

/// Option entry for the jump host picker ("None" or another SSH host)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JumpHostOption {
    pub id: Option<Uuid>,
    pub label: String,
}

impl JumpHostOption {
    pub fn none() -> Self {
        Self {
            id: None,
            label: "None (direct)".to_string(),
        }
    }
}

impl From<&Host> for JumpHostOption {
    fn from(host: &Host) -> Self {
        Self {
            id: Some(host.id),
            label: host.name.clone(),
        }
    }
}

impl std::fmt::Display for JumpHostOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeySourceChoice {
    #[default]
    Local,
    Vault,
}

impl std::fmt::Display for KeySourceChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeySourceChoice::Local => write!(f, "Local key"),
            KeySourceChoice::Vault => write!(f, "Vault key"),
        }
    }
}

impl KeySourceChoice {
    pub const ALL: [KeySourceChoice; 2] = [KeySourceChoice::Local, KeySourceChoice::Vault];
}

/// Port forward editor state within the host dialog
#[derive(Debug, Clone)]
pub struct PortForwardEditorState {
    pub id: Uuid,
    pub kind: PortForwardKind,
    pub bind_host: String,
    pub bind_port: String,
    pub target_host: String,
    pub target_port: String,
    pub enabled: bool,
    pub description: String,
    pub validation_error: Option<String>,
}

impl Default for PortForwardEditorState {
    fn default() -> Self {
        Self::new()
    }
}

impl PortForwardEditorState {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            kind: PortForwardKind::Local,
            bind_host: "localhost".to_string(),
            bind_port: String::new(),
            target_host: String::new(),
            target_port: String::new(),
            enabled: true,
            description: String::new(),
            validation_error: None,
        }
    }

    pub fn from_forward(forward: &PortForward) -> Self {
        Self {
            id: forward.id,
            kind: forward.kind,
            bind_host: forward.bind_host.clone(),
            bind_port: forward.bind_port.to_string(),
            target_host: forward.target_host.clone(),
            target_port: forward.target_port.to_string(),
            enabled: forward.enabled,
            description: forward.description.clone().unwrap_or_default(),
            validation_error: None,
        }
    }

    pub fn build(&self) -> Result<PortForward, String> {
        let bind_host = if self.bind_host.trim().is_empty() {
            "localhost".to_string()
        } else {
            self.bind_host.trim().to_string()
        };

        if let Err(e) = validate_hostname(&bind_host) {
            return Err(format!("Bind host: {}", e.message));
        }

        let bind_port = validate_forward_bind_port(&self.bind_port)?;

        // Dynamic forwarding is a local SOCKS5 proxy (-D). Target host/port come from the
        // client's SOCKS CONNECT requests; the config's target_* fields are ignored.
        let (target_host, target_port) = if self.kind == PortForwardKind::Dynamic {
            ("socks".to_string(), 0u16)
        } else {
            if let Err(e) = validate_hostname(&self.target_host) {
                return Err(format!("Target host: {}", e.message));
            }
            let target_port = validate_port(&self.target_port).map_err(|e| e.message)?;
            (self.target_host.trim().to_string(), target_port)
        };

        let description = if self.description.trim().is_empty() {
            None
        } else {
            Some(self.description.trim().to_string())
        };

        Ok(PortForward {
            id: self.id,
            kind: self.kind,
            bind_host,
            bind_port,
            target_host,
            target_port,
            enabled: self.enabled,
            description,
        })
    }
}

fn validate_forward_bind_port(port_str: &str) -> Result<u16, String> {
    let port_str = port_str.trim();

    if port_str.is_empty() {
        return Err("Bind port is required".to_string());
    }

    match port_str.parse::<u16>() {
        Ok(port) => Ok(port), // Allow 0 for "auto-assign"
        Err(_) => Err(format!("Invalid bind port number: '{}'", port_str)),
    }
}

impl HostDialogState {
    /// Create a new empty dialog for adding a host
    pub fn new_host() -> Self {
        Self::new_host_with_defaults(false, None)
    }

    /// Create a new host dialog honoring the global Portal Hub defaults:
    /// routing starts at Auto, and when "Prefer Vault keys" is on the key
    /// source starts at Vault with the default key preselected.
    pub fn new_host_with_defaults(prefer_vault: bool, default_vault_key: Option<Uuid>) -> Self {
        Self {
            editing_id: None,
            name: String::new(),
            hostname: String::new(),
            port: "22".to_string(),
            username: String::new(),
            auth_method: AuthMethodChoice::Agent,
            key_source: if prefer_vault && default_vault_key.is_some() {
                KeySourceChoice::Vault
            } else {
                KeySourceChoice::Local
            },
            key_path: String::new(),
            vault_key_id: default_vault_key.filter(|_| prefer_vault),
            vnc_password_id: None,
            vnc_via_ssh_host_id: None,
            agent_forwarding: false,
            hub_routing: HubRouting::Auto,
            jump_host_id: None,
            tags: String::new(),
            notes: String::new(),
            protocol: ProtocolChoice::Ssh,
            port_forwards: Vec::new(),
            port_forwards_expanded: false,
            port_forward_editor: None,
            delete_requested: false,
            validation_errors: HashMap::new(),
        }
    }

    /// Create dialog state from an existing host for editing
    pub fn from_host(host: &Host) -> Self {
        Self {
            editing_id: Some(host.id),
            name: host.name.clone(),
            hostname: host.hostname.clone(),
            port: host.port.to_string(),
            username: host.username.clone(),
            auth_method: match &host.auth {
                AuthMethod::Agent => AuthMethodChoice::Agent,
                AuthMethod::Password => AuthMethodChoice::Password,
                AuthMethod::PublicKey { .. } => AuthMethodChoice::PublicKey,
                AuthMethod::KeyboardInteractive => AuthMethodChoice::KeyboardInteractive,
            },
            key_source: match &host.auth {
                AuthMethod::PublicKey {
                    vault_key_id: Some(_),
                    ..
                } => KeySourceChoice::Vault,
                _ => KeySourceChoice::Local,
            },
            key_path: match &host.auth {
                AuthMethod::PublicKey { key_path, .. } => key_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            },
            vault_key_id: match &host.auth {
                AuthMethod::PublicKey { vault_key_id, .. } => *vault_key_id,
                _ => None,
            },
            vnc_password_id: host.vnc_password_id,
            vnc_via_ssh_host_id: host.vnc_via_ssh_host_id,
            agent_forwarding: host.agent_forwarding,
            hub_routing: host.hub_routing,
            jump_host_id: host.jump_host_id,
            tags: host.tags.join(", "),
            notes: host.notes.clone().unwrap_or_default(),
            protocol: match host.protocol {
                Protocol::Ssh => ProtocolChoice::Ssh,
                Protocol::Vnc => ProtocolChoice::Vnc,
            },
            port_forwards: host.port_forwards.clone(),
            port_forwards_expanded: false,
            port_forward_editor: None,
            delete_requested: false,
            validation_errors: HashMap::new(),
        }
    }

    /// Validate all fields and return errors.
    /// Also updates self.validation_errors with results.
    pub fn validate(&mut self) -> bool {
        self.validation_errors.clear();

        // Name is required
        if self.name.trim().is_empty() {
            self.validation_errors
                .insert("name".to_string(), "Name is required".to_string());
        }

        // Validate hostname
        if let Err(e) = validate_hostname(&self.hostname) {
            self.validation_errors
                .insert("hostname".to_string(), e.message);
        }

        // Validate port
        if let Err(e) = validate_port(&self.port) {
            self.validation_errors.insert("port".to_string(), e.message);
        }

        // Validate username (empty is allowed)
        if let Err(e) = validate_username(&self.username) {
            self.validation_errors
                .insert("username".to_string(), e.message);
        }

        if self.auth_method == AuthMethodChoice::PublicKey
            && self.key_source == KeySourceChoice::Vault
            && self.vault_key_id.is_none()
        {
            self.validation_errors
                .insert("vault_key".to_string(), "Select a vault key".to_string());
        }

        self.validation_errors.is_empty()
    }

    /// Convert dialog state to a Host struct.
    /// Returns None if validation fails.
    #[allow(clippy::wrong_self_convention)]
    pub fn to_host(&mut self) -> Option<Host> {
        // Run validation
        if !self.validate() {
            return None;
        }

        // Port is validated, safe to parse
        let port: u16 = validate_port(&self.port).unwrap_or_else(|_| {
            tracing::warn!("Invalid port, using default 22");
            22
        });

        let username = if self.username.trim().is_empty() {
            default_username()
        } else {
            self.username.trim().to_string()
        };

        let auth = match self.auth_method {
            AuthMethodChoice::Agent => AuthMethod::Agent,
            AuthMethodChoice::Password => AuthMethod::Password,
            AuthMethodChoice::KeyboardInteractive => AuthMethod::KeyboardInteractive,
            AuthMethodChoice::PublicKey => AuthMethod::PublicKey {
                key_path: if self.key_source == KeySourceChoice::Local
                    && !self.key_path.trim().is_empty()
                {
                    Some(self.key_path.trim().into())
                } else {
                    None
                },
                vault_key_id: if self.key_source == KeySourceChoice::Vault {
                    self.vault_key_id
                } else {
                    None
                },
            },
        };

        let tags: Vec<String> = self
            .tags
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let notes = if self.notes.trim().is_empty() {
            None
        } else {
            Some(self.notes.trim().to_string())
        };

        let now = chrono::Utc::now();
        let (id, created_at) = if let Some(existing_id) = self.editing_id {
            (existing_id, now) // We'll preserve created_at in the update
        } else {
            (Uuid::new_v4(), now)
        };

        let protocol = match self.protocol {
            ProtocolChoice::Ssh => Protocol::Ssh,
            ProtocolChoice::Vnc => Protocol::Vnc,
        };

        let agent_forwarding = if protocol == Protocol::Ssh {
            self.agent_forwarding
        } else {
            false
        };

        let hub_routing = if protocol == Protocol::Ssh
            && !matches!(auth, AuthMethod::Password | AuthMethod::KeyboardInteractive)
        {
            self.hub_routing
        } else {
            HubRouting::Auto
        };

        let jump_host_id = if protocol == Protocol::Ssh {
            self.jump_host_id.filter(|id| Some(*id) != self.editing_id)
        } else {
            None
        };

        let vnc_port = if protocol == Protocol::Vnc && port != 5900 {
            Some(port)
        } else {
            None
        };
        let vnc_password_id = if protocol == Protocol::Vnc {
            self.vnc_password_id
        } else {
            None
        };
        let vnc_via_ssh_host_id = if protocol == Protocol::Vnc {
            self.vnc_via_ssh_host_id
        } else {
            None
        };

        let port_forwards = if protocol == Protocol::Ssh {
            self.port_forwards.clone()
        } else {
            Vec::new()
        };

        Some(Host {
            id,
            name: self.name.trim().to_string(),
            hostname: self.hostname.trim().to_string(),
            port,
            username,
            protocol,
            vnc_port,
            vnc_password_id,
            vnc_via_ssh_host_id,
            allow_cleartext_vnc: false,
            auth,
            agent_forwarding,
            port_forwards,
            hub_routing,
            jump_host_id,
            group_id: None,
            notes,
            tags,
            created_at,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        })
    }

    /// Check if the form has no validation errors.
    /// Does not run validation; use validate() first for accurate results.
    pub fn is_valid(&self) -> bool {
        // Quick check - name and hostname must not be empty
        if self.name.trim().is_empty() || self.hostname.trim().is_empty() {
            return false;
        }
        // If we've run validation, check for errors
        self.validation_errors.is_empty()
    }

    /// Get validation error for a specific field
    pub fn get_error(&self, field: &str) -> Option<&String> {
        self.validation_errors.get(field)
    }
}

/// Build the host dialog view
#[allow(clippy::too_many_arguments)]
pub fn host_dialog_view(
    state: &HostDialogState,
    theme: Theme,
    fonts: ScaledFonts,
    vault_keys: Vec<VaultKeyOption>,
    vault_vnc_passwords: Vec<VncPasswordOption>,
    jump_host_options: Vec<JumpHostOption>,
    hub_configured: bool,
    hub_default_on: bool,
) -> Element<'static, Message> {
    let title = if state.editing_id.is_some() {
        "Edit Host"
    } else {
        "Add Host"
    };

    // Clone values to make them owned
    let name_value = state.name.clone();
    let hostname_value = state.hostname.clone();
    let port_value = state.port.clone();
    let username_value = state.username.clone();
    let key_path_value = state.key_path.clone();
    let key_source = state.key_source;
    let selected_vault_key_id = state.vault_key_id;
    let selected_vnc_password_id = state.vnc_password_id;
    let agent_forwarding = state.agent_forwarding;
    let hub_routing = state.hub_routing;
    let tags_value = state.tags.clone();
    let notes_value = state.notes.clone();
    let auth_method = state.auth_method;
    let protocol = state.protocol;
    let is_vnc = protocol == ProtocolChoice::Vnc;
    let is_valid = state.is_valid();
    let username_placeholder = std::env::var("USER").unwrap_or_default();
    let mut vnc_password_options = Vec::with_capacity(vault_vnc_passwords.len() + 1);
    vnc_password_options.push(VncPasswordOption::ask_every_time());
    vnc_password_options.extend(vault_vnc_passwords);

    // Get validation errors
    let name_error = state.get_error("name").cloned();
    let hostname_error = state.get_error("hostname").cloned();
    let port_error = state.get_error("port").cloned();
    let username_error = state.get_error("username").cloned();
    let vault_key_error = state.get_error("vault_key").cloned();

    // Form fields using owned values with validation error display
    let name_input = {
        let has_error = name_error.is_some();
        let mut col = column![
            text("Name").size(fonts.label).color(theme.text_secondary),
            text_input("my-server", &name_value)
                .id(host_dialog_field_id(0))
                .on_input(|s| Message::Dialog(DialogMessage::FieldChanged(
                    HostDialogField::Name,
                    s
                )))
                .on_submit(Message::Dialog(DialogMessage::Submit))
                .padding(8)
                .width(Length::Fill)
                .style(dialog_input_style_with_error(theme, has_error))
        ]
        .spacing(4);
        if let Some(err) = name_error {
            col = col.push(text(err).size(fonts.small).color(ERROR_COLOR));
        }
        col
    };

    let hostname_input = {
        let has_error = hostname_error.is_some();
        let mut col = column![
            text("Hostname / IP")
                .size(fonts.label)
                .color(theme.text_secondary),
            text_input("192.168.1.100", &hostname_value)
                .id(host_dialog_field_id(1))
                .on_input(|s| Message::Dialog(DialogMessage::FieldChanged(
                    HostDialogField::Hostname,
                    s
                )))
                .on_submit(Message::Dialog(DialogMessage::Submit))
                .padding(8)
                .width(Length::Fill)
                .style(dialog_input_style_with_error(theme, has_error))
        ]
        .spacing(4);
        if let Some(err) = hostname_error {
            col = col.push(text(err).size(fonts.small).color(ERROR_COLOR));
        }
        col
    };

    let port_input = {
        let has_error = port_error.is_some();
        let mut col = column![
            text("Port").size(fonts.label).color(theme.text_secondary),
            text_input("22", &port_value)
                .id(host_dialog_field_id(2))
                .on_input(|s| Message::Dialog(DialogMessage::FieldChanged(
                    HostDialogField::Port,
                    s
                )))
                .on_submit(Message::Dialog(DialogMessage::Submit))
                .padding(8)
                .width(Length::Fill)
                .style(dialog_input_style_with_error(theme, has_error))
        ]
        .spacing(4);
        if let Some(err) = port_error {
            col = col.push(text(err).size(fonts.small).color(ERROR_COLOR));
        }
        col
    };

    let username_input = {
        let has_error = username_error.is_some();
        let mut col = column![
            text("Username")
                .size(fonts.label)
                .color(theme.text_secondary),
            text_input(&username_placeholder, &username_value)
                .id(host_dialog_field_id(3))
                .on_input(|s| Message::Dialog(DialogMessage::FieldChanged(
                    HostDialogField::Username,
                    s
                )))
                .on_submit(Message::Dialog(DialogMessage::Submit))
                .padding(8)
                .width(Length::Fill)
                .style(dialog_input_style_with_error(theme, has_error))
        ]
        .spacing(4);
        if let Some(err) = username_error {
            col = col.push(text(err).size(fonts.small).color(ERROR_COLOR));
        }
        col
    };

    // Auth method picker
    let auth_picker = column![
        text("Authentication")
            .size(fonts.label)
            .color(theme.text_secondary),
        pick_list(
            AuthMethodChoice::ALL.as_slice(),
            Some(auth_method),
            |choice| Message::Dialog(DialogMessage::FieldChanged(
                HostDialogField::AuthMethod,
                format!("{:?}", choice)
            ))
        )
        .width(Length::Fill)
        .padding(8)
        .style(dialog_pick_list_style(theme))
        .menu_style(dialog_pick_list_menu_style(theme))
    ]
    .spacing(4);

    // Key path (only shown for PublicKey auth)
    let key_path_section: Element<'static, Message> = if auth_method == AuthMethodChoice::PublicKey
    {
        let source_picker = pick_list(
            KeySourceChoice::ALL.as_slice(),
            Some(key_source),
            |choice| {
                Message::Dialog(DialogMessage::FieldChanged(
                    HostDialogField::KeySource,
                    format!("{:?}", choice),
                ))
            },
        )
        .width(Length::Fill)
        .padding(8)
        .style(dialog_pick_list_style(theme))
        .menu_style(dialog_pick_list_menu_style(theme));

        if key_source == KeySourceChoice::Vault {
            let selected = selected_vault_key_id
                .and_then(|id| vault_keys.iter().find(|key| key.id == id).cloned());
            let mut col = column![
                text("Key Source")
                    .size(fonts.label)
                    .color(theme.text_secondary),
                source_picker,
                text("Vault Key")
                    .size(fonts.label)
                    .color(theme.text_secondary),
                pick_list(vault_keys, selected, |choice| {
                    Message::Dialog(DialogMessage::FieldChanged(
                        HostDialogField::VaultKeyId,
                        choice.id.to_string(),
                    ))
                })
                .width(Length::Fill)
                .padding(8)
                .style(dialog_pick_list_style(theme))
                .menu_style(dialog_pick_list_menu_style(theme))
            ]
            .spacing(4);
            if let Some(error) = vault_key_error {
                col = col.push(text(error).size(fonts.small).color(ERROR_COLOR));
            }
            col.into()
        } else {
            column![
                text("Key Source")
                    .size(fonts.label)
                    .color(theme.text_secondary),
                source_picker,
                text("Key Path")
                    .size(fonts.label)
                    .color(theme.text_secondary),
                text_input("~/.ssh/id_ed25519", &key_path_value)
                    .id(host_dialog_field_id(5))
                    .on_input(|s| Message::Dialog(DialogMessage::FieldChanged(
                        HostDialogField::KeyPath,
                        s
                    )))
                    .on_submit(Message::Dialog(DialogMessage::Submit))
                    .padding(8)
                    .width(Length::Fill)
                    .style(dialog_input_style(theme))
            ]
            .spacing(4)
            .into()
        }
    } else {
        column![].into()
    };

    // Routing group: Auto / Direct / Always Hub segmented control with a
    // live explanation line. Transport routing is separated from security
    // decisions (agent forwarding) below.
    // Jump host picker: connect through another SSH host first (ProxyJump)
    let jump_host_section: Element<'static, Message> = if !is_vnc {
        let selected_jump_id = state.jump_host_id;
        let mut options = Vec::with_capacity(jump_host_options.len() + 1);
        options.push(JumpHostOption::none());
        options.extend(jump_host_options.iter().cloned());
        let selected = options
            .iter()
            .find(|option| option.id == selected_jump_id)
            .cloned()
            .unwrap_or_else(JumpHostOption::none);

        column![
            text("Jump Host")
                .size(fonts.label)
                .color(theme.text_secondary),
            pick_list(options, Some(selected), |choice| {
                Message::Dialog(DialogMessage::FieldChanged(
                    HostDialogField::JumpHostId,
                    choice.id.map(|id| id.to_string()).unwrap_or_default(),
                ))
            })
            .width(Length::Fill)
            .padding(8)
            .style(dialog_pick_list_style(theme))
            .menu_style(dialog_pick_list_menu_style(theme)),
            text("Tunnel this connection through another SSH host (ProxyJump).")
                .size(fonts.small)
                .color(theme.text_tertiary),
        ]
        .spacing(4)
        .into()
    } else {
        column![].into()
    };

    let routing_section: Element<'static, Message> = if !is_vnc {
        if matches!(
            auth_method,
            AuthMethodChoice::Password | AuthMethodChoice::KeyboardInteractive
        ) {
            column![
                text("Routing")
                    .size(fonts.label)
                    .color(theme.text_secondary),
                text("Portal Hub routing requires SSH Agent or Public Key authentication.")
                    .size(fonts.small)
                    .color(theme.text_tertiary)
            ]
            .spacing(4)
            .into()
        } else {
            let caption = match hub_routing {
                HubRouting::Auto => {
                    if !hub_configured {
                        "Auto — connects directly until Portal Hub is set up."
                    } else if hub_default_on {
                        "Auto — follows your default: connects via Portal Hub, session persists."
                    } else {
                        "Auto — follows your default: connects directly (Hub default is off)."
                    }
                }
                HubRouting::Direct => "Always connects directly, even while Hub is the default.",
                HubRouting::Hub => "Always routes through Portal Hub, regardless of the default.",
            };
            let options: Vec<(HubRouting, &'static str)> = HubRouting::ALL
                .iter()
                .map(|routing| (*routing, routing.label()))
                .collect();
            // Key-disclosure warning: Hub-routed sessions for key-file and
            // vault-key hosts send the private key to the Hub at session
            // start (see proxy_private_key). Agent auth sends no key.
            let hub_may_route = match hub_routing {
                HubRouting::Hub => true,
                HubRouting::Auto => hub_configured && hub_default_on,
                HubRouting::Direct => false,
            };
            let key_warning = (hub_may_route && matches!(auth_method, AuthMethodChoice::PublicKey))
                .then(|| {
                    text(
                        "Hub sessions send this host's private key to your Portal Hub \
                     over TLS at session start. Use SSH Agent auth to keep keys local.",
                    )
                    .size(fonts.small)
                    .color(theme.text_tertiary)
                });
            let mut section = column![
                text("Routing")
                    .size(fonts.label)
                    .color(theme.text_secondary),
                toggle_group(
                    hub_routing,
                    &options,
                    |routing| Message::Dialog(DialogMessage::FieldChanged(
                        HostDialogField::HubRouting,
                        format!("{:?}", routing),
                    )),
                    theme,
                    fonts,
                ),
                text(caption).size(fonts.small).color(theme.text_tertiary),
            ]
            .spacing(6);
            if let Some(warning) = key_warning {
                section = section.push(warning);
            }
            section.into()
        }
    } else {
        column![].into()
    };

    // Security group: agent forwarding is a security decision and gets its
    // own section with caution copy instead of sitting beside routing.
    let security_section: Element<'static, Message> = if !is_vnc {
        column![
            text("Security")
                .size(fonts.label)
                .color(theme.text_secondary),
            checkbox(agent_forwarding)
                .label("Enable SSH Agent Forwarding")
                .on_toggle(|value| {
                    Message::Dialog(DialogMessage::FieldChanged(
                        HostDialogField::AgentForwarding,
                        value.to_string(),
                    ))
                })
                .spacing(8),
            text("Forwards your local SSH agent to this host. Only enable for trusted systems.")
                .size(fonts.small)
                .color(theme.text_tertiary),
        ]
        .spacing(6)
        .into()
    } else {
        column![].into()
    };

    let vnc_password_section = {
        let selected = vnc_password_options
            .iter()
            .find(|option| option.id == selected_vnc_password_id)
            .cloned()
            .unwrap_or_else(VncPasswordOption::ask_every_time);

        // SSH tunnel picker: carry VNC traffic over an encrypted SSH channel.
        // Reuses the jump-host candidate list (other SSH hosts only).
        let selected_tunnel_id = state.vnc_via_ssh_host_id;
        let mut tunnel_options = Vec::with_capacity(jump_host_options.len() + 1);
        tunnel_options.push(JumpHostOption {
            id: None,
            label: "None (direct, unencrypted)".to_string(),
        });
        tunnel_options.extend(jump_host_options.iter().cloned());
        let selected_tunnel = tunnel_options
            .iter()
            .find(|option| option.id == selected_tunnel_id)
            .cloned()
            .unwrap_or_else(|| tunnel_options[0].clone());
        let vnc_tunnel_section = column![
            text("SSH tunnel")
                .size(fonts.label)
                .color(theme.text_secondary),
            pick_list(tunnel_options, Some(selected_tunnel), |choice| {
                Message::Dialog(DialogMessage::FieldChanged(
                    HostDialogField::VncViaSshHostId,
                    choice.id.map(|id| id.to_string()).unwrap_or_default(),
                ))
            })
            .width(Length::Fill)
            .padding(8)
            .style(dialog_pick_list_style(theme))
            .menu_style(dialog_pick_list_menu_style(theme)),
            text("Tunnel VNC traffic through an SSH host to encrypt it.")
                .size(fonts.small)
                .color(theme.text_tertiary),
        ]
        .spacing(4);

        column![
            section_heading("VNC", theme, fonts),
            text("Default password")
                .size(fonts.label)
                .color(theme.text_secondary),
            pick_list(vnc_password_options, Some(selected), |choice| {
                Message::Dialog(DialogMessage::FieldChanged(
                    HostDialogField::VncPasswordId,
                    choice.id.map(|id| id.to_string()).unwrap_or_default(),
                ))
            })
            .width(Length::Fill)
            .padding(8)
            .style(dialog_pick_list_style(theme))
            .menu_style(dialog_pick_list_menu_style(theme)),
            vnc_tunnel_section,
        ]
        .spacing(6)
        .width(Length::FillPortion(1))
    };

    let tags_input = column![
        text("Tags").size(fonts.label).color(theme.text_secondary),
        text_input("web, production", &tags_value)
            .id(host_dialog_field_id(6))
            .on_input(|s| Message::Dialog(DialogMessage::FieldChanged(HostDialogField::Tags, s)))
            .on_submit(Message::Dialog(DialogMessage::Submit))
            .padding(8)
            .width(Length::Fill)
            .style(dialog_input_style(theme))
    ]
    .spacing(4);

    let notes_input = column![
        text("Notes").size(fonts.label).color(theme.text_secondary),
        text_input("Optional notes...", &notes_value)
            .id(host_dialog_field_id(7))
            .on_input(|s| Message::Dialog(DialogMessage::FieldChanged(HostDialogField::Notes, s)))
            .on_submit(Message::Dialog(DialogMessage::Submit))
            .padding(8)
            .width(Length::Fill)
            .style(dialog_input_style(theme))
    ]
    .spacing(4);

    // Clone port forward data to avoid lifetime issues
    let port_forwards = state.port_forwards.clone();
    let port_forwards_expanded = state.port_forwards_expanded;
    let port_forward_editor = state.port_forward_editor.clone();

    let port_forwards_section: Element<'static, Message> = if !is_vnc {
        let expanded = port_forwards_expanded;
        let mut section = column![
            row![
                text("Port Forwards")
                    .size(fonts.section)
                    .color(theme.text_primary),
                Space::new().width(Length::Fill),
                button(text(if expanded { "Hide" } else { "Show" }).size(fonts.label))
                    .padding([4, 10])
                    .style(secondary_button_style(theme))
                    .on_press(Message::Dialog(DialogMessage::PortForwardSectionToggled))
            ]
            .align_y(Alignment::Center)
        ]
        .spacing(8);

        if expanded {
            if port_forwards.is_empty() {
                section = section.push(
                    text("No port forwards configured.")
                        .size(fonts.small)
                        .color(theme.text_secondary),
                );
            } else {
                for forward in &port_forwards {
                    let forward_id = forward.id;
                    let summary = format!(
                        "{} {}:{} -> {}:{}",
                        forward.kind,
                        forward.bind_host,
                        forward.bind_port,
                        forward.target_host,
                        forward.target_port
                    );
                    let description_text = forward.description.clone();
                    let mut row_content = row![
                        checkbox(forward.enabled)
                            .label("Enabled")
                            .on_toggle(move |value| {
                                Message::Dialog(DialogMessage::PortForwardToggleEnabled(
                                    forward_id, value,
                                ))
                            })
                            .spacing(8),
                        text(summary).size(fonts.label).color(theme.text_primary),
                        Space::new().width(Length::Fill),
                        button(text("Edit").size(fonts.label))
                            .padding([4, 10])
                            .style(secondary_button_style(theme))
                            .on_press(Message::Dialog(DialogMessage::PortForwardEdit(forward_id))),
                        button(text("Remove").size(fonts.label))
                            .padding([4, 10])
                            .style(secondary_button_style(theme))
                            .on_press(Message::Dialog(DialogMessage::PortForwardRemove(
                                forward_id,
                            ))),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center);

                    if let Some(description) = description_text {
                        row_content = row_content.push(
                            text(description)
                                .size(fonts.small)
                                .color(theme.text_secondary)
                                .width(Length::FillPortion(2)),
                        );
                    }

                    section = section.push(row_content);
                }
            }

            section = section.push(
                button(text("Add Forward").size(fonts.label))
                    .padding([6, 12])
                    .style(secondary_button_style(theme))
                    .on_press(Message::Dialog(DialogMessage::PortForwardAdd)),
            );

            if let Some(editor) = &port_forward_editor {
                let bind_host_value = editor.bind_host.clone();
                let bind_port_value = editor.bind_port.clone();
                let description_value = editor.description.clone();
                let kind = editor.kind;

                let mut editor_view = column![
                    text("Edit Forward")
                        .size(fonts.label)
                        .color(theme.text_secondary),
                    row![
                        column![
                            text("Type").size(fonts.small).color(theme.text_secondary),
                            pick_list(PortForwardKind::ALL.as_slice(), Some(kind), |choice| {
                                Message::Dialog(DialogMessage::PortForwardFieldChanged(
                                    crate::message::PortForwardField::Kind,
                                    format!("{:?}", choice),
                                ))
                            })
                            .padding(8)
                            .width(Length::Fill)
                            .style(dialog_pick_list_style(theme))
                            .menu_style(dialog_pick_list_menu_style(theme))
                        ]
                        .width(Length::FillPortion(1)),
                        column![
                            text("Bind Host")
                                .size(fonts.small)
                                .color(theme.text_secondary),
                            text_input("localhost", &bind_host_value)
                                .on_input(|s| {
                                    Message::Dialog(DialogMessage::PortForwardFieldChanged(
                                        crate::message::PortForwardField::BindHost,
                                        s,
                                    ))
                                })
                                .padding(8)
                                .width(Length::Fill)
                                .style(dialog_input_style(theme))
                        ]
                        .width(Length::FillPortion(2)),
                        column![
                            text("Bind Port")
                                .size(fonts.small)
                                .color(theme.text_secondary),
                            text_input("8080", &bind_port_value)
                                .on_input(|s| {
                                    Message::Dialog(DialogMessage::PortForwardFieldChanged(
                                        crate::message::PortForwardField::BindPort,
                                        s,
                                    ))
                                })
                                .padding(8)
                                .width(Length::Fill)
                                .style(dialog_input_style(theme))
                        ]
                        .width(Length::FillPortion(1)),
                    ]
                    .spacing(12),
                    column![
                        text("Description")
                            .size(fonts.small)
                            .color(theme.text_secondary),
                        text_input("Optional description", &description_value)
                            .on_input(|s| {
                                Message::Dialog(DialogMessage::PortForwardFieldChanged(
                                    crate::message::PortForwardField::Description,
                                    s,
                                ))
                            })
                            .padding(8)
                            .width(Length::Fill)
                            .style(dialog_input_style(theme))
                    ]
                    .spacing(4),
                    checkbox(editor.enabled)
                        .label("Enabled")
                        .on_toggle(|value| {
                            Message::Dialog(DialogMessage::PortForwardFieldChanged(
                                crate::message::PortForwardField::Enabled,
                                value.to_string(),
                            ))
                        })
                        .spacing(8),
                ]
                .spacing(8);

                if kind == PortForwardKind::Dynamic {
                    editor_view = editor_view.push(
                        text("Dynamic (-D) forwards create a local SOCKS5 proxy.")
                            .size(fonts.small)
                            .color(theme.text_secondary),
                    );
                } else {
                    let target_host_value = editor.target_host.clone();
                    let target_port_value = editor.target_port.clone();
                    editor_view = editor_view.push(
                        row![
                            column![
                                text("Target Host")
                                    .size(fonts.small)
                                    .color(theme.text_secondary),
                                text_input("127.0.0.1", &target_host_value)
                                    .on_input(|s| {
                                        Message::Dialog(DialogMessage::PortForwardFieldChanged(
                                            crate::message::PortForwardField::TargetHost,
                                            s,
                                        ))
                                    })
                                    .padding(8)
                                    .width(Length::Fill)
                                    .style(dialog_input_style(theme))
                            ]
                            .width(Length::FillPortion(3)),
                            column![
                                text("Target Port")
                                    .size(fonts.small)
                                    .color(theme.text_secondary),
                                text_input("80", &target_port_value)
                                    .on_input(|s| {
                                        Message::Dialog(DialogMessage::PortForwardFieldChanged(
                                            crate::message::PortForwardField::TargetPort,
                                            s,
                                        ))
                                    })
                                    .padding(8)
                                    .width(Length::Fill)
                                    .style(dialog_input_style(theme))
                            ]
                            .width(Length::FillPortion(1)),
                        ]
                        .spacing(12),
                    );
                }

                if let Some(err) = editor.validation_error.clone() {
                    editor_view = editor_view.push(text(err).size(fonts.small).color(ERROR_COLOR));
                }

                let editor_view = editor_view.push(
                    row![
                        button(text("Cancel").size(fonts.label))
                            .padding([6, 12])
                            .style(secondary_button_style(theme))
                            .on_press(Message::Dialog(DialogMessage::PortForwardCancel)),
                        button(text("Save").size(fonts.label))
                            .padding([6, 12])
                            .style(primary_button_style(theme))
                            .on_press(Message::Dialog(DialogMessage::PortForwardSave)),
                    ]
                    .spacing(8),
                );

                section = section.push(editor_view);
            }
        }

        section.into()
    } else {
        column![].into()
    };

    // Buttons
    let import_button = button(
        text("Import from SSH Config")
            .size(fonts.button_small)
            .color(theme.text_primary),
    )
    .padding([8, 16])
    .style(secondary_button_style(theme))
    .on_press(Message::Dialog(DialogMessage::ImportFromSshConfig));

    let cancel_button = button(
        text("Cancel")
            .size(fonts.button_small)
            .color(theme.text_primary),
    )
    .padding([8, 16])
    .style(secondary_button_style(theme))
    .on_press(Message::Dialog(DialogMessage::Close));

    let save_button = button(text("Save").size(fonts.button_small))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press_maybe(if is_valid {
            Some(Message::Dialog(DialogMessage::Submit))
        } else {
            None
        });

    let mut button_row = Row::new()
        .spacing(8)
        .align_y(Alignment::Center)
        .push(import_button);

    if state.editing_id.is_some() {
        let delete_label = if state.delete_requested {
            "Confirm Delete"
        } else {
            "Delete"
        };
        let delete_message = if state.delete_requested {
            DialogMessage::HostDeleteConfirm
        } else {
            DialogMessage::HostDeleteRequested
        };
        let delete_button = button(text(delete_label).size(fonts.button_small))
            .padding([8, 16])
            .style(destructive_button_style(theme))
            .on_press(Message::Dialog(delete_message));
        button_row = button_row.push(delete_button);
    }

    let button_row = button_row
        .push(Space::new().width(Length::Fill))
        .push(cancel_button)
        .push(save_button);

    // Protocol picker
    let protocol_picker = column![
        text("Protocol")
            .size(fonts.label)
            .color(theme.text_secondary),
        pick_list(ProtocolChoice::ALL.as_slice(), Some(protocol), |choice| {
            Message::Dialog(DialogMessage::FieldChanged(
                HostDialogField::Protocol,
                format!("{:?}", choice),
            ))
        })
        .width(Length::Fill)
        .padding(8)
        .style(dialog_pick_list_style(theme))
        .menu_style(dialog_pick_list_menu_style(theme))
    ]
    .spacing(4);

    let connection_section = column![
        section_heading("Connection", theme, fonts),
        name_input,
        row![
            column![protocol_picker].width(Length::FillPortion(1)),
            column![username_input].width(Length::FillPortion(2)),
        ]
        .spacing(12),
        row![
            column![hostname_input].width(Length::FillPortion(3)),
            column![port_input].width(Length::FillPortion(1)),
        ]
        .spacing(12),
        tags_input,
        notes_input,
    ]
    .spacing(10)
    .width(Length::FillPortion(1));

    let ssh_section = column![
        section_heading("SSH", theme, fonts),
        auth_picker,
        key_path_section,
        jump_host_section,
        routing_section,
        security_section,
    ]
    .spacing(10)
    .width(Length::FillPortion(1));

    let top_sections: Element<'static, Message> = if is_vnc {
        row![connection_section, vnc_password_section]
            .spacing(20)
            .align_y(Alignment::Start)
            .into()
    } else {
        row![connection_section, ssh_section]
            .spacing(20)
            .align_y(Alignment::Start)
            .into()
    };

    let mut body = column![top_sections].spacing(18);
    if !is_vnc {
        body = body.push(port_forwards_section);
    }

    let header = container(text(title).size(fonts.heading).color(theme.text_primary))
        .padding([18, 24])
        .width(Length::Fill);

    let footer = container(button_row)
        .padding([14, 24])
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        });

    let form = column![
        header,
        scrollable(container(body).padding([18, 24]).width(Length::Fill)).height(Length::Fill),
        footer,
    ]
    .width(Length::Fill)
    .max_width(760)
    .height(Length::Fill);

    host_dialog_backdrop(form, theme)
}

fn section_heading(
    label: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    text(label)
        .size(fonts.section)
        .color(theme.text_primary)
        .into()
}

fn host_dialog_backdrop(
    content: impl Into<Element<'static, Message>>,
    theme: Theme,
) -> Element<'static, Message> {
    let dialog_box = container(content)
        .width(Length::Fill)
        .max_width(760)
        .height(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: (BORDER_RADIUS * 2.0).into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 0, 0, 0.5),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        });

    let backdrop = container(
        container(dialog_box)
            .padding([24, 16])
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(iced::Color::from_rgba8(0, 0, 0, 0.7).into()),
        ..Default::default()
    });

    iced::widget::mouse_area(backdrop)
        .on_press(Message::Noop)
        .on_release(Message::Noop)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_key_host_can_use_vault_key() {
        let key_id = Uuid::new_v4();
        let mut state = HostDialogState::new_host();
        state.name = "prod".to_string();
        state.hostname = "prod.example.test".to_string();
        state.auth_method = AuthMethodChoice::PublicKey;
        state.key_source = KeySourceChoice::Vault;
        state.vault_key_id = Some(key_id);

        let host = state.to_host().expect("host");
        match host.auth {
            AuthMethod::PublicKey {
                key_path,
                vault_key_id,
            } => {
                assert_eq!(key_path, None);
                assert_eq!(vault_key_id, Some(key_id));
            }
            other => panic!("unexpected auth method: {:?}", other),
        }
    }

    #[test]
    fn vault_key_source_requires_selection() {
        let mut state = HostDialogState::new_host();
        state.name = "prod".to_string();
        state.hostname = "prod.example.test".to_string();
        state.auth_method = AuthMethodChoice::PublicKey;
        state.key_source = KeySourceChoice::Vault;

        assert!(state.to_host().is_none());
        assert_eq!(
            state.get_error("vault_key"),
            Some(&"Select a vault key".to_string())
        );
    }
}
