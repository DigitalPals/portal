//! Passphrase prompt dialog for SSH key authentication

use std::path::PathBuf;

use iced::widget::{Space, button, checkbox, column, container, row, text, text_input};
use iced::{Alignment, Element, Length};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::icons::{self, icon_with_color};
use crate::message::{DialogMessage, Message, PassphraseRequest, PassphraseSftpContext};
use crate::theme::{BORDER_RADIUS, Theme};

use super::common::{
    dialog_backdrop, dialog_input_style, primary_button_style, secondary_button_style,
};

/// State for the passphrase prompt dialog
#[derive(Debug, Clone)]
pub struct PassphraseDialogState {
    /// The host name being connected to
    pub host_name: String,
    /// The hostname/IP
    pub hostname: String,
    /// The port
    pub port: u16,
    /// The username
    pub username: String,
    /// The key path that needs a passphrase
    pub key_path: PathBuf,
    /// The passphrase being entered (sensitive - should be cleared after use)
    pub passphrase: SecretString,
    /// Whether to cache this passphrase for the session (in-memory only).
    pub remember_for_session: bool,
    /// Error message to display if passphrase was invalid
    pub error: Option<String>,
    /// The host ID for resuming the connection
    pub host_id: Uuid,
    /// Whether this is for SSH (true) or SFTP (false)
    pub is_ssh: bool,
    /// Session ID for SSH connections
    pub session_id: Option<Uuid>,
    /// Whether to detect OS on connect for SSH
    pub should_detect_os: bool,
    /// For SFTP: the tab and pane IDs
    pub sftp_context: Option<PassphraseSftpContext>,
}

impl PassphraseDialogState {
    pub fn from_request(request: PassphraseRequest, remember_for_session: bool) -> Self {
        Self {
            host_name: request.host_name,
            hostname: request.hostname,
            port: request.port,
            username: request.username,
            key_path: request.key_path,
            passphrase: SecretString::from(String::new()),
            remember_for_session,
            error: request.error,
            host_id: request.host_id,
            is_ssh: request.is_ssh,
            session_id: request.session_id,
            should_detect_os: request.should_detect_os,
            sftp_context: request.sftp_context,
        }
    }

    /// Clear the passphrase (for security)
    pub fn clear_passphrase(&mut self) {
        self.passphrase = SecretString::from(String::new());
    }
}

/// Build the passphrase dialog view
pub fn passphrase_dialog_view(
    state: &PassphraseDialogState,
    theme: Theme,
) -> Element<'static, Message> {
    let key_icon = icon_with_color(icons::ui::SERVER, 28, theme.accent);

    let title = text("Passphrase Required")
        .size(20)
        .color(theme.text_primary);

    let connection_info = text(format!(
        "{}@{}:{}",
        state.username, state.hostname, state.port
    ))
    .size(14)
    .color(theme.text_secondary);

    let host_name_text = text(format!("Connecting to {}", state.host_name))
        .size(14)
        .color(theme.text_secondary);

    let key_path_text = text(format!("Key: {}", state.key_path.display()))
        .size(12)
        .color(theme.text_muted);

    let passphrase_label = text("Passphrase").size(12).color(theme.text_muted);

    let passphrase_input = text_input("Enter passphrase...", state.passphrase.expose_secret())
        .size(14)
        .padding(10)
        .width(Length::Fill)
        .secure(true)
        .style(dialog_input_style(theme))
        .on_input(|s| Message::Dialog(DialogMessage::PassphraseChanged(SecretString::from(s))))
        .on_submit(Message::Dialog(DialogMessage::PassphraseSubmit));

    let remember_checkbox = checkbox(state.remember_for_session)
        .label("Remember for session")
        .on_toggle(|v| Message::Dialog(DialogMessage::PassphraseRememberToggled(v)))
        .size(13)
        .spacing(8);

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
        .on_press(Message::Dialog(DialogMessage::PassphraseCancel));

    let unlock_button = button(text("Unlock").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::PassphraseSubmit));

    let button_row = row![
        Space::new().width(Length::Fill),
        cancel_button,
        unlock_button,
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
        key_path_text.into(),
        Space::new().height(16).into(),
    ];

    // Add error if present
    if state.error.is_some() {
        content_items.push(error_element);
        content_items.push(Space::new().height(8).into());
    }

    content_items.extend([
        passphrase_label.into(),
        Space::new().height(4).into(),
        passphrase_input.into(),
        Space::new().height(10).into(),
        remember_checkbox.into(),
        Space::new().height(20).into(),
        button_row.into(),
    ]);

    let content = column(content_items)
        .spacing(4)
        .padding(24)
        .width(Length::Fixed(420.0));

    dialog_backdrop(content, theme)
}
