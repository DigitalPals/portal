use std::collections::HashMap;

use iced::widget::{Space, button, checkbox, column, pick_list, row, text, text_input, tooltip};
use iced::{Alignment, Element, Length};
use uuid::Uuid;

use crate::config::{AuthMethod, Host, PortForward, PortForwardKind, Protocol};
use crate::message::{DialogMessage, HostDialogField, Message};
use crate::theme::Theme;
use crate::validation::{validate_hostname, validate_port, validate_username};

use super::common::{
    ERROR_COLOR, dialog_backdrop, dialog_input_style, dialog_input_style_with_error,
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
    pub key_path: String,
    pub agent_forwarding: bool,
    pub tags: String,
    pub notes: String,
    /// Connection protocol
    pub protocol: ProtocolChoice,
    /// Port forwards for SSH
    pub port_forwards: Vec<PortForward>,
    pub port_forwards_expanded: bool,
    pub port_forward_editor: Option<PortForwardEditorState>,
    /// Validation errors by field name
    pub validation_errors: HashMap<String, String>,
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
}

impl std::fmt::Display for AuthMethodChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthMethodChoice::Agent => write!(f, "SSH Agent"),
            AuthMethodChoice::Password => write!(f, "Password"),
            AuthMethodChoice::PublicKey => write!(f, "Public Key"),
        }
    }
}

impl AuthMethodChoice {
    pub const ALL: [AuthMethodChoice; 3] = [
        AuthMethodChoice::Agent,
        AuthMethodChoice::Password,
        AuthMethodChoice::PublicKey,
    ];
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
        Self {
            editing_id: None,
            name: String::new(),
            hostname: String::new(),
            port: "22".to_string(),
            username: String::new(),
            auth_method: AuthMethodChoice::Agent,
            key_path: String::new(),
            agent_forwarding: false,
            tags: String::new(),
            notes: String::new(),
            protocol: ProtocolChoice::Ssh,
            port_forwards: Vec::new(),
            port_forwards_expanded: false,
            port_forward_editor: None,
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
            },
            key_path: match &host.auth {
                AuthMethod::PublicKey { key_path } => key_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_default(),
                _ => String::new(),
            },
            agent_forwarding: host.agent_forwarding,
            tags: host.tags.join(", "),
            notes: host.notes.clone().unwrap_or_default(),
            protocol: match host.protocol {
                Protocol::Ssh => ProtocolChoice::Ssh,
                Protocol::Vnc => ProtocolChoice::Vnc,
            },
            port_forwards: host.port_forwards.clone(),
            port_forwards_expanded: false,
            port_forward_editor: None,
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
            std::env::var("USER").unwrap_or_else(|_| "root".to_string())
        } else {
            self.username.trim().to_string()
        };

        let auth = match self.auth_method {
            AuthMethodChoice::Agent => AuthMethod::Agent,
            AuthMethodChoice::Password => AuthMethod::Password,
            AuthMethodChoice::PublicKey => AuthMethod::PublicKey {
                key_path: if self.key_path.trim().is_empty() {
                    None
                } else {
                    Some(self.key_path.trim().into())
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

        let vnc_port = if protocol == Protocol::Vnc && port != 5900 {
            Some(port)
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
            auth,
            agent_forwarding,
            port_forwards,
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
pub fn host_dialog_view(state: &HostDialogState, theme: Theme) -> Element<'static, Message> {
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
    let agent_forwarding = state.agent_forwarding;
    let tags_value = state.tags.clone();
    let notes_value = state.notes.clone();
    let auth_method = state.auth_method;
    let protocol = state.protocol;
    let is_vnc = protocol == ProtocolChoice::Vnc;
    let is_valid = state.is_valid();
    let username_placeholder = std::env::var("USER").unwrap_or_default();

    // Get validation errors
    let name_error = state.get_error("name").cloned();
    let hostname_error = state.get_error("hostname").cloned();
    let port_error = state.get_error("port").cloned();
    let username_error = state.get_error("username").cloned();

    // Form fields using owned values with validation error display
    let name_input = {
        let has_error = name_error.is_some();
        let mut col = column![
            text("Name").size(12).color(theme.text_secondary),
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
            col = col.push(text(err).size(11).color(ERROR_COLOR));
        }
        col
    };

    let hostname_input = {
        let has_error = hostname_error.is_some();
        let mut col = column![
            text("Hostname / IP").size(12).color(theme.text_secondary),
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
            col = col.push(text(err).size(11).color(ERROR_COLOR));
        }
        col
    };

    let port_input = {
        let has_error = port_error.is_some();
        let mut col = column![
            text("Port").size(12).color(theme.text_secondary),
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
            col = col.push(text(err).size(11).color(ERROR_COLOR));
        }
        col
    };

    let username_input = {
        let has_error = username_error.is_some();
        let mut col = column![
            text("Username").size(12).color(theme.text_secondary),
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
            col = col.push(text(err).size(11).color(ERROR_COLOR));
        }
        col
    };

    // Auth method picker
    let auth_picker = column![
        text("Authentication").size(12).color(theme.text_secondary),
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
        column![
            text("Key Path").size(12).color(theme.text_secondary),
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
    } else {
        column![].into()
    };

    let agent_forwarding_section: Element<'static, Message> = if !is_vnc {
        let checkbox_control = checkbox(agent_forwarding)
            .label("Enable SSH Agent Forwarding")
            .on_toggle(|value| {
                Message::Dialog(DialogMessage::FieldChanged(
                    HostDialogField::AgentForwarding,
                    value.to_string(),
                ))
            })
            .spacing(8);

        let tooltip_text =
            text("Forwards your local SSH agent to this host. Only enable for trusted systems.")
                .size(11)
                .color(theme.text_secondary);

        tooltip(checkbox_control, tooltip_text, tooltip::Position::Top)
            .style(move |_theme| iced::widget::container::Style {
                background: Some(theme.surface.into()),
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: 4.0.into(),
                },
                ..Default::default()
            })
            .padding(8)
            .into()
    } else {
        column![].into()
    };

    let tags_input = column![
        text("Tags").size(12).color(theme.text_secondary),
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
        text("Notes").size(12).color(theme.text_secondary),
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
                text("Port Forwards").size(13).color(theme.text_primary),
                Space::new().width(Length::Fill),
                button(text(if expanded { "Hide" } else { "Show" }).size(12))
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
                        .size(11)
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
                        text(summary).size(12).color(theme.text_primary),
                        Space::new().width(Length::Fill),
                        button(text("Edit").size(12))
                            .padding([4, 10])
                            .style(secondary_button_style(theme))
                            .on_press(Message::Dialog(DialogMessage::PortForwardEdit(forward_id))),
                        button(text("Remove").size(12))
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
                                .size(11)
                                .color(theme.text_secondary)
                                .width(Length::FillPortion(2)),
                        );
                    }

                    section = section.push(row_content);
                }
            }

            section = section.push(
                button(text("Add Forward").size(12))
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
                    text("Edit Forward").size(12).color(theme.text_secondary),
                    row![
                        column![
                            text("Type").size(11).color(theme.text_secondary),
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
                            text("Bind Host").size(11).color(theme.text_secondary),
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
                            text("Bind Port").size(11).color(theme.text_secondary),
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
                        text("Description").size(11).color(theme.text_secondary),
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
                            .size(11)
                            .color(theme.text_secondary),
                    );
                } else {
                    let target_host_value = editor.target_host.clone();
                    let target_port_value = editor.target_port.clone();
                    editor_view = editor_view.push(
                        row![
                            column![
                                text("Target Host").size(11).color(theme.text_secondary),
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
                                text("Target Port").size(11).color(theme.text_secondary),
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
                    editor_view = editor_view.push(text(err).size(11).color(ERROR_COLOR));
                }

                let editor_view = editor_view.push(
                    row![
                        button(text("Cancel").size(12))
                            .padding([6, 12])
                            .style(secondary_button_style(theme))
                            .on_press(Message::Dialog(DialogMessage::PortForwardCancel)),
                        button(text("Save").size(12))
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
            .size(14)
            .color(theme.text_primary),
    )
    .padding([8, 16])
    .style(secondary_button_style(theme))
    .on_press(Message::Dialog(DialogMessage::ImportFromSshConfig));

    let cancel_button = button(text("Cancel").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(secondary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::Close));

    let save_button = button(text("Save").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press_maybe(if is_valid {
            Some(Message::Dialog(DialogMessage::Submit))
        } else {
            None
        });

    let button_row = row![
        import_button,
        Space::new().width(Length::Fill),
        cancel_button,
        save_button,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Protocol picker
    let protocol_picker = column![
        text("Protocol").size(12).color(theme.text_secondary),
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

    // Form layout - conditionally show SSH-specific fields
    let mut form = column![
        text(title).size(20).color(theme.text_primary),
        Space::new().height(16),
        name_input,
        protocol_picker,
        row![
            column![hostname_input].width(Length::FillPortion(3)),
            column![port_input].width(Length::FillPortion(1)),
        ]
        .spacing(12),
    ]
    .spacing(12)
    .padding(24)
    .width(Length::Fixed(450.0));

    // Username is shown for both SSH and VNC; auth fields are SSH-only
    form = form.push(username_input);
    if !is_vnc {
        form = form.push(auth_picker);
        form = form.push(key_path_section);
        form = form.push(agent_forwarding_section);
        form = form.push(port_forwards_section);
    }

    form = form.push(tags_input);
    form = form.push(notes_input);
    form = form.push(Space::new().height(16));
    form = form.push(button_row);

    dialog_backdrop(form, theme)
}
