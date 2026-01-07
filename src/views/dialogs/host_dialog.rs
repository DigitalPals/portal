use iced::widget::{Space, button, column, pick_list, row, text, text_input};
use iced::{Alignment, Element, Length};
use uuid::Uuid;

use crate::config::{AuthMethod, Host};
use crate::message::{DialogMessage, HostDialogField, Message};
use crate::theme::Theme;

use super::common::{
    dialog_backdrop, dialog_input_style, dialog_pick_list_menu_style, dialog_pick_list_style,
    primary_button_style, secondary_button_style,
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
    pub tags: String,
    pub notes: String,
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
            tags: String::new(),
            notes: String::new(),
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
            tags: host.tags.join(", "),
            notes: host.notes.clone().unwrap_or_default(),
        }
    }

    /// Convert dialog state to a Host struct
    pub fn to_host(&self) -> Option<Host> {
        // Validate required fields
        if self.name.trim().is_empty() || self.hostname.trim().is_empty() {
            return None;
        }

        let port: u16 = self.port.parse().unwrap_or(22);
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

        Some(Host {
            id,
            name: self.name.trim().to_string(),
            hostname: self.hostname.trim().to_string(),
            port,
            username,
            auth,
            group_id: None,
            notes,
            tags,
            created_at,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        })
    }

    /// Check if the form is valid
    pub fn is_valid(&self) -> bool {
        !self.name.trim().is_empty() && !self.hostname.trim().is_empty()
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
    let tags_value = state.tags.clone();
    let notes_value = state.notes.clone();
    let auth_method = state.auth_method;
    let is_valid = state.is_valid();
    let username_placeholder = std::env::var("USER").unwrap_or_default();

    // Form fields using owned values
    let name_input = column![
        text("Name").size(12).color(theme.text_secondary),
        text_input("my-server", &name_value)
            .id(host_dialog_field_id(0))
            .on_input(|s| Message::Dialog(DialogMessage::FieldChanged(HostDialogField::Name, s)))
            .on_submit(Message::Dialog(DialogMessage::Submit))
            .padding(8)
            .width(Length::Fill)
            .style(dialog_input_style(theme))
    ]
    .spacing(4);

    let hostname_input = column![
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
            .style(dialog_input_style(theme))
    ]
    .spacing(4);

    let port_input = column![
        text("Port").size(12).color(theme.text_secondary),
        text_input("22", &port_value)
            .id(host_dialog_field_id(2))
            .on_input(|s| Message::Dialog(DialogMessage::FieldChanged(HostDialogField::Port, s)))
            .on_submit(Message::Dialog(DialogMessage::Submit))
            .padding(8)
            .width(Length::Fill)
            .style(dialog_input_style(theme))
    ]
    .spacing(4);

    let username_input = column![
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
            .style(dialog_input_style(theme))
    ]
    .spacing(4);

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

    // Buttons
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

    let button_row = row![Space::new().width(Length::Fill), cancel_button, save_button,]
        .spacing(8)
        .align_y(Alignment::Center);

    // Form layout
    let form = column![
        text(title).size(20).color(theme.text_primary),
        Space::new().height(16),
        name_input,
        row![
            column![hostname_input].width(Length::FillPortion(3)),
            column![port_input].width(Length::FillPortion(1)),
        ]
        .spacing(12),
        username_input,
        auth_picker,
        key_path_section,
        tags_input,
        notes_input,
        Space::new().height(16),
        button_row,
    ]
    .spacing(12)
    .padding(24)
    .width(Length::Fixed(450.0));

    dialog_backdrop(form, theme)
}
