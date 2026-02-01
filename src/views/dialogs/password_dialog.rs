//! Password prompt dialog for SSH password authentication

use iced::widget::{Space, button, column, container, row, text, text_input};
use iced::{Alignment, Element, Length};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::icons::{self, icon_with_color};
use crate::message::{DialogMessage, Message};
use crate::theme::{BORDER_RADIUS, Theme};

use super::common::{
    dialog_backdrop, dialog_input_style, primary_button_style, secondary_button_style,
};

/// What kind of connection this password dialog is for
#[derive(Debug, Clone, PartialEq)]
pub enum PasswordConnectionKind {
    Ssh,
    Sftp,
    Vnc,
}

/// State for the password prompt dialog
#[derive(Debug, Clone)]
pub struct PasswordDialogState {
    /// The host name being connected to
    pub host_name: String,
    /// The hostname/IP
    pub hostname: String,
    /// The port
    pub port: u16,
    /// The username
    pub username: String,
    /// The password being entered (sensitive - should be cleared after use)
    pub password: SecretString,
    /// Error message to display if authentication failed
    pub error: Option<String>,
    /// The host ID for resuming the connection
    pub host_id: Uuid,
    /// Whether this is for SSH (true) or SFTP (false)
    pub is_ssh: bool,
    /// The connection kind
    pub connection_kind: PasswordConnectionKind,
    /// For SFTP: the tab and pane IDs
    pub sftp_context: Option<SftpConnectionContext>,
}

/// Context for SFTP password connections
#[derive(Debug, Clone)]
pub struct SftpConnectionContext {
    pub tab_id: Uuid,
    pub pane_id: crate::views::sftp::PaneId,
}

impl PasswordDialogState {
    /// Create a new password dialog state for SSH connection
    pub fn new_ssh(
        host_name: String,
        hostname: String,
        port: u16,
        username: String,
        host_id: Uuid,
    ) -> Self {
        Self {
            host_name,
            hostname,
            port,
            username,
            password: SecretString::from(String::new()),
            error: None,
            host_id,
            is_ssh: true,
            connection_kind: PasswordConnectionKind::Ssh,
            sftp_context: None,
        }
    }

    /// Create a new password dialog state for SFTP connection
    pub fn new_sftp(
        host_name: String,
        hostname: String,
        port: u16,
        username: String,
        host_id: Uuid,
        tab_id: Uuid,
        pane_id: crate::views::sftp::PaneId,
    ) -> Self {
        Self {
            host_name,
            hostname,
            port,
            username,
            password: SecretString::from(String::new()),
            error: None,
            host_id,
            is_ssh: false,
            connection_kind: PasswordConnectionKind::Sftp,
            sftp_context: Some(SftpConnectionContext { tab_id, pane_id }),
        }
    }

    /// Create a new password dialog state for VNC connection
    pub fn new_vnc(
        host_name: String,
        hostname: String,
        port: u16,
        username: String,
        host_id: Uuid,
    ) -> Self {
        Self {
            host_name,
            hostname,
            port,
            username,
            password: SecretString::from(String::new()),
            error: None,
            host_id,
            is_ssh: false,
            connection_kind: PasswordConnectionKind::Vnc,
            sftp_context: None,
        }
    }

    /// Clear the password (for security)
    pub fn clear_password(&mut self) {
        self.password = SecretString::from(String::new());
    }
}

/// Build the password dialog view
pub fn password_dialog_view(
    state: &PasswordDialogState,
    theme: Theme,
) -> Element<'static, Message> {
    let key_icon = icon_with_color(icons::ui::SERVER, 28, theme.accent);

    let title = text("Password Required").size(20).color(theme.text_primary);

    let connection_info = text(format!(
        "{}@{}:{}",
        state.username, state.hostname, state.port
    ))
    .size(14)
    .color(theme.text_secondary);

    let host_name_text = text(format!("Connecting to {}", state.host_name))
        .size(14)
        .color(theme.text_secondary);

    let password_label = text("Password").size(12).color(theme.text_muted);

    let password_input = text_input("Enter password...", state.password.expose_secret())
        .size(14)
        .padding(10)
        .width(Length::Fill)
        .secure(true)
        .style(dialog_input_style(theme))
        .on_input(|s| Message::Dialog(DialogMessage::PasswordChanged(SecretString::from(s))))
        .on_submit(Message::Dialog(DialogMessage::PasswordSubmit));

    // Error message if present
    let error_element: Element<'static, Message> = if let Some(error) = &state.error {
        let error_color = iced::Color::from_rgb8(220, 80, 80);
        container(text(error.clone()).size(12).color(error_color))
            .padding([8, 12])
            .width(Length::Fill)
            .style(move |_theme| container::Style {
                background: Some(iced::Color::from_rgba8(220, 80, 80, 0.1).into()),
                border: iced::Border {
                    color: error_color,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            })
            .into()
    } else {
        Space::new().height(0).into()
    };

    let cancel_button = button(text("Cancel").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(secondary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::PasswordCancel));

    let connect_button = button(text("Connect").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::PasswordSubmit));

    let button_row = row![
        Space::new().width(Length::Fill),
        cancel_button,
        connect_button,
    ]
    .spacing(8);

    let mut content_items: Vec<Element<'static, Message>> = vec![
        row![key_icon, title]
            .spacing(12)
            .align_y(Alignment::Center)
            .into(),
        Space::new().height(8).into(),
        host_name_text.into(),
        connection_info.into(),
        Space::new().height(16).into(),
    ];

    // Add error if present
    if state.error.is_some() {
        content_items.push(error_element);
        content_items.push(Space::new().height(8).into());
    }

    // Show username field for VNC connections
    if state.connection_kind == PasswordConnectionKind::Vnc {
        let username_label = text("Username").size(12).color(theme.text_muted);
        let username_input = text_input("Enter username...", &state.username)
            .size(14)
            .padding(10)
            .width(Length::Fill)
            .style(dialog_input_style(theme))
            .on_input(|s| Message::Dialog(DialogMessage::PasswordUsernameChanged(s)));
        content_items.push(username_label.into());
        content_items.push(Space::new().height(4).into());
        content_items.push(username_input.into());
        content_items.push(Space::new().height(8).into());
    }

    content_items.extend([
        password_label.into(),
        Space::new().height(4).into(),
        password_input.into(),
        Space::new().height(24).into(),
        button_row.into(),
    ]);

    let content = column(content_items)
        .spacing(4)
        .padding(24)
        .width(Length::Fixed(400.0));

    dialog_backdrop(content, theme)
}
