//! Passphrase prompt dialog for encrypted SSH private keys

use iced::widget::{Space, button, column, container, row, text, text_input};
use iced::{Alignment, Element, Length};
use std::path::PathBuf;
use uuid::Uuid;

use crate::icons::{self, icon_with_color};
use crate::message::{DialogMessage, Message};
use crate::theme::{BORDER_RADIUS, Theme};

use super::common::{dialog_backdrop, dialog_input_style, primary_button_style, secondary_button_style};

/// State for the passphrase prompt dialog (for encrypted SSH keys)
#[derive(Debug, Clone)]
pub struct PassphraseDialogState {
    /// The host name being connected to
    pub host_name: String,
    /// The key file path
    pub key_path: PathBuf,
    /// The passphrase being entered
    pub passphrase: String,
    /// Error message to display if decryption failed
    pub error: Option<String>,
    /// The host ID for resuming the connection
    pub host_id: Uuid,
    /// Whether this is for SSH (true) or SFTP (false)
    pub is_ssh: bool,
    /// For SFTP: the tab and pane IDs
    pub sftp_context: Option<SftpPassphraseContext>,
}

/// Context for SFTP passphrase connections
#[derive(Debug, Clone)]
pub struct SftpPassphraseContext {
    pub tab_id: Uuid,
    pub pane_id: crate::views::sftp::PaneId,
}

impl PassphraseDialogState {
    /// Create a new passphrase dialog state for SSH connection
    pub fn new_ssh(
        host_name: String,
        key_path: PathBuf,
        host_id: Uuid,
    ) -> Self {
        Self {
            host_name,
            key_path,
            passphrase: String::new(),
            error: None,
            host_id,
            is_ssh: true,
            sftp_context: None,
        }
    }

    /// Create a new passphrase dialog state for SFTP connection
    pub fn new_sftp(
        host_name: String,
        key_path: PathBuf,
        host_id: Uuid,
        tab_id: Uuid,
        pane_id: crate::views::sftp::PaneId,
    ) -> Self {
        Self {
            host_name,
            key_path,
            passphrase: String::new(),
            error: None,
            host_id,
            is_ssh: false,
            sftp_context: Some(SftpPassphraseContext { tab_id, pane_id }),
        }
    }

    /// Set an error message
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Clear the passphrase (for security)
    pub fn clear_passphrase(&mut self) {
        self.passphrase.clear();
    }
}

/// Build the passphrase dialog view
pub fn passphrase_dialog_view(state: &PassphraseDialogState, theme: Theme) -> Element<'static, Message> {
    let key_icon = icon_with_color(icons::ui::SETTINGS, 28, theme.accent);

    let title = text("Key Passphrase Required").size(20).color(theme.text_primary);

    let key_path_str = state.key_path.display().to_string();
    let key_info = text(format!("Key: {}", key_path_str))
        .size(12)
        .color(theme.text_secondary);

    let host_name_text = text(format!("Connecting to {}", state.host_name))
        .size(14)
        .color(theme.text_secondary);

    let description = text("This SSH key is encrypted. Enter the passphrase to unlock it.")
        .size(13)
        .color(theme.text_muted);

    let passphrase_label = text("Passphrase").size(12).color(theme.text_muted);

    let passphrase_input = text_input("Enter passphrase...", &state.passphrase)
        .size(14)
        .padding(10)
        .width(Length::Fill)
        .secure(true)
        .style(dialog_input_style(theme))
        .on_input(|s| Message::Dialog(DialogMessage::PassphraseChanged(s)))
        .on_submit(Message::Dialog(DialogMessage::PassphraseSubmit));

    // Error message if present
    let error_element: Element<'static, Message> = if let Some(error) = &state.error {
        let error_color = iced::Color::from_rgb8(220, 80, 80);
        container(
            text(error.clone())
                .size(12)
                .color(error_color),
        )
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
        key_info.into(),
        Space::new().height(12).into(),
        description.into(),
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
        Space::new().height(24).into(),
        button_row.into(),
    ]);

    let content = column(content_items)
        .spacing(4)
        .padding(24)
        .width(Length::Fixed(450.0));

    dialog_backdrop(content, theme)
}
