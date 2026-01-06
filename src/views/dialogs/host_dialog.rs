use iced::widget::{button, column, pick_list, row, text, text_input, Space};
use iced::{Alignment, Element, Length};
use uuid::Uuid;

use crate::config::{AuthMethod, Host, HostGroup};
use crate::message::{HostDialogField, Message};
use crate::theme::Theme;

use super::common::{dialog_backdrop, primary_button_style, secondary_button_style};

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
    pub group_id: Option<Uuid>,
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
            group_id: None,
            tags: String::new(),
            notes: String::new(),
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
            group_id: self.group_id,
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

/// Group choice for the dropdown
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupChoice {
    pub id: Option<Uuid>,
    pub name: String,
}

impl std::fmt::Display for GroupChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// Build the host dialog view
pub fn host_dialog_view(
    state: &HostDialogState,
    groups: &[HostGroup],
    theme: Theme,
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
    let tags_value = state.tags.clone();
    let notes_value = state.notes.clone();
    let auth_method = state.auth_method;
    let is_valid = state.is_valid();
    let username_placeholder = std::env::var("USER").unwrap_or_default();

    // Build group choices
    let mut group_choices = vec![GroupChoice {
        id: None,
        name: "(No folder)".to_string(),
    }];
    for group in groups {
        group_choices.push(GroupChoice {
            id: Some(group.id),
            name: group.name.clone(),
        });
    }
    let selected_group = group_choices
        .iter()
        .find(|g| g.id == state.group_id)
        .cloned();

    // Form fields using owned values
    let name_input = column![
        text("Name").size(12).color(theme.text_secondary),
        text_input("my-server", &name_value)
            .on_input(|s| Message::DialogFieldChanged(HostDialogField::Name, s))
            .padding(8)
            .width(Length::Fill)
    ]
    .spacing(4);

    let hostname_input = column![
        text("Hostname / IP").size(12).color(theme.text_secondary),
        text_input("192.168.1.100", &hostname_value)
            .on_input(|s| Message::DialogFieldChanged(HostDialogField::Hostname, s))
            .padding(8)
            .width(Length::Fill)
    ]
    .spacing(4);

    let port_input = column![
        text("Port").size(12).color(theme.text_secondary),
        text_input("22", &port_value)
            .on_input(|s| Message::DialogFieldChanged(HostDialogField::Port, s))
            .padding(8)
            .width(Length::Fill)
    ]
    .spacing(4);

    let username_input = column![
        text("Username").size(12).color(theme.text_secondary),
        text_input(&username_placeholder, &username_value)
            .on_input(|s| Message::DialogFieldChanged(HostDialogField::Username, s))
            .padding(8)
            .width(Length::Fill)
    ]
    .spacing(4);

    // Auth method picker
    let auth_picker = column![
        text("Authentication").size(12).color(theme.text_secondary),
        pick_list(
            AuthMethodChoice::ALL.as_slice(),
            Some(auth_method),
            |choice| Message::DialogFieldChanged(HostDialogField::AuthMethod, format!("{:?}", choice))
        )
        .width(Length::Fill)
        .padding(8)
    ]
    .spacing(4);

    // Key path (only shown for PublicKey auth)
    let key_path_section: Element<'static, Message> = if auth_method == AuthMethodChoice::PublicKey {
        column![
            text("Key Path").size(12).color(theme.text_secondary),
            text_input("~/.ssh/id_ed25519", &key_path_value)
                .on_input(|s| Message::DialogFieldChanged(HostDialogField::KeyPath, s))
                .padding(8)
                .width(Length::Fill)
        ]
        .spacing(4)
        .into()
    } else {
        column![].into()
    };

    // Group picker
    let group_picker = column![
        text("Folder").size(12).color(theme.text_secondary),
        pick_list(
            group_choices.clone(),
            selected_group,
            |choice| Message::DialogFieldChanged(HostDialogField::GroupId,
                choice.id.map(|id| id.to_string()).unwrap_or_default())
        )
        .width(Length::Fill)
        .padding(8)
    ]
    .spacing(4);

    let tags_input = column![
        text("Tags").size(12).color(theme.text_secondary),
        text_input("web, production", &tags_value)
            .on_input(|s| Message::DialogFieldChanged(HostDialogField::Tags, s))
            .padding(8)
            .width(Length::Fill)
    ]
    .spacing(4);

    let notes_input = column![
        text("Notes").size(12).color(theme.text_secondary),
        text_input("Optional notes...", &notes_value)
            .on_input(|s| Message::DialogFieldChanged(HostDialogField::Notes, s))
            .padding(8)
            .width(Length::Fill)
    ]
    .spacing(4);

    // Buttons
    let cancel_button = button(text("Cancel").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(secondary_button_style(theme))
        .on_press(Message::DialogClose);

    let save_button = button(text("Save").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press_maybe(if is_valid {
            Some(Message::DialogSubmit)
        } else {
            None
        });

    let button_row = row![
        Space::with_width(Length::Fill),
        cancel_button,
        save_button,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // Form layout
    let form = column![
        text(title).size(20).color(theme.text_primary),
        Space::with_height(16),
        name_input,
        row![
            column![hostname_input].width(Length::FillPortion(3)),
            column![port_input].width(Length::FillPortion(1)),
        ]
        .spacing(12),
        username_input,
        auth_picker,
        key_path_section,
        group_picker,
        tags_input,
        notes_input,
        Space::with_height(16),
        button_row,
    ]
    .spacing(12)
    .padding(24)
    .width(Length::Fixed(450.0));

    dialog_backdrop(form, theme)
}
