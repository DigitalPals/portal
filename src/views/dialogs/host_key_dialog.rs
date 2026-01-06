//! Host key verification dialog for unknown or changed SSH host keys

use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Element, Length};
use tokio::sync::oneshot;

use crate::icons::{self, icon_with_color};
use crate::message::{DialogMessage, Message};
use crate::ssh::host_key_verification::{
    HostKeyInfo, HostKeyVerificationRequest, HostKeyVerificationResponse,
};
use crate::theme::{BORDER_RADIUS, Theme};

use super::common::{dialog_backdrop, primary_button_style, secondary_button_style};

/// State for the host key verification dialog
pub struct HostKeyDialogState {
    /// The host being connected to
    pub host: String,
    /// The port
    pub port: u16,
    /// The key type (e.g., "ssh-ed25519", "ssh-rsa")
    pub key_type: String,
    /// The new fingerprint to verify
    pub fingerprint: String,
    /// For ChangedHost: the old fingerprint
    pub old_fingerprint: Option<String>,
    /// Whether this is a changed key (requires stronger warning)
    pub is_changed_host: bool,
    /// The responder to send the user's decision
    pub responder: Option<oneshot::Sender<HostKeyVerificationResponse>>,
}

impl HostKeyDialogState {
    /// Create state for a new unknown host
    pub fn new_host(
        info: HostKeyInfo,
        responder: oneshot::Sender<HostKeyVerificationResponse>,
    ) -> Self {
        Self {
            host: info.host,
            port: info.port,
            key_type: info.key_type,
            fingerprint: info.fingerprint,
            old_fingerprint: None,
            is_changed_host: false,
            responder: Some(responder),
        }
    }

    /// Create state for a host with changed key
    pub fn changed_host(
        info: HostKeyInfo,
        old_fingerprint: String,
        responder: oneshot::Sender<HostKeyVerificationResponse>,
    ) -> Self {
        Self {
            host: info.host,
            port: info.port,
            key_type: info.key_type,
            fingerprint: info.fingerprint,
            old_fingerprint: Some(old_fingerprint),
            is_changed_host: true,
            responder: Some(responder),
        }
    }

    /// Create from a verification request
    pub fn from_request(request: HostKeyVerificationRequest) -> Self {
        match request {
            HostKeyVerificationRequest::NewHost { info, responder } => {
                Self::new_host(info, responder)
            }
            HostKeyVerificationRequest::ChangedHost {
                info,
                old_fingerprint,
                responder,
            } => Self::changed_host(info, old_fingerprint, responder),
        }
    }

    /// Send the response and consume the responder
    pub fn respond(&mut self, response: HostKeyVerificationResponse) {
        if let Some(responder) = self.responder.take() {
            let _ = responder.send(response);
        }
    }
}

/// Build the host key dialog view
pub fn host_key_dialog_view(state: &HostKeyDialogState, theme: Theme) -> Element<'static, Message> {
    if state.is_changed_host {
        changed_host_dialog_view(state, theme)
    } else {
        new_host_dialog_view(state, theme)
    }
}

/// Dialog for new unknown hosts
fn new_host_dialog_view(state: &HostKeyDialogState, theme: Theme) -> Element<'static, Message> {
    let key_icon = icon_with_color(icons::ui::SERVER, 28, theme.accent);

    let title = text("Unknown Host").size(20).color(theme.text_primary);

    let host_info = text(format!("{}:{}", state.host, state.port))
        .size(14)
        .color(theme.text_secondary);

    let message = text("The authenticity of this host cannot be established.")
        .size(14)
        .color(theme.text_secondary);

    let key_type_label = text(format!("{} key fingerprint:", state.key_type))
        .size(12)
        .color(theme.text_muted);

    let fingerprint_text = text(state.fingerprint.clone())
        .size(11)
        .color(theme.text_primary);

    let fingerprint_box = container(fingerprint_text)
        .padding(10)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.background.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        });

    let question = text("Are you sure you want to continue connecting?")
        .size(14)
        .color(theme.text_secondary);

    let reject_button = button(text("Reject").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(secondary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::HostKeyReject));

    let accept_button = button(text("Accept").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::HostKeyAccept));

    let button_row = row![
        Space::new().width(Length::Fill),
        reject_button,
        accept_button,
    ]
    .spacing(8);

    let content = column![
        row![key_icon, title].spacing(12).align_y(Alignment::Center),
        Space::new().height(8),
        host_info,
        Space::new().height(16),
        message,
        Space::new().height(12),
        key_type_label,
        fingerprint_box,
        Space::new().height(16),
        question,
        Space::new().height(24),
        button_row,
    ]
    .spacing(4)
    .padding(24)
    .width(Length::Fixed(480.0));

    dialog_backdrop(content, theme)
}

/// Dialog for hosts with changed keys (MITM warning)
fn changed_host_dialog_view(state: &HostKeyDialogState, theme: Theme) -> Element<'static, Message> {
    let warning_color = iced::Color::from_rgb8(220, 50, 50);

    let warning_icon = icon_with_color(icons::ui::ALERT_TRIANGLE, 32, warning_color);

    let title = text("WARNING: HOST KEY CHANGED!")
        .size(20)
        .color(warning_color);

    let host_info = text(format!("{}:{}", state.host, state.port))
        .size(14)
        .color(theme.text_secondary);

    let mitm_warning = text(
        "IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!\n\
        Someone could be eavesdropping on you right now (man-in-the-middle attack)!\n\
        It is also possible that a host key has just been changed.",
    )
    .size(13)
    .color(theme.text_primary);

    let old_fp_label = text("Previous fingerprint:")
        .size(12)
        .color(theme.text_muted);

    let old_fingerprint = state.old_fingerprint.as_deref().unwrap_or("unknown");
    let old_fp_text = text(old_fingerprint.to_string())
        .size(11)
        .color(theme.text_secondary);

    let new_fp_label = text(format!("New {} fingerprint:", state.key_type))
        .size(12)
        .color(theme.text_muted);

    let new_fp_text = text(state.fingerprint.clone())
        .size(11)
        .color(theme.text_primary);

    let fingerprint_box = container(
        column![
            old_fp_label,
            old_fp_text,
            Space::new().height(8),
            new_fp_label,
            new_fp_text,
        ]
        .spacing(4),
    )
    .padding(12)
    .width(Length::Fill)
    .style(move |_theme| container::Style {
        background: Some(theme.background.into()),
        border: iced::Border {
            color: iced::Color::from_rgb8(180, 40, 40),
            width: 1.0,
            radius: BORDER_RADIUS.into(),
        },
        ..Default::default()
    });

    // For changed host: "Accept Anyway" is dangerous (red), "Reject" is safe (primary)
    let accept_button = button(text("Accept Anyway").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => iced::Color::from_rgb8(180, 40, 40),
                _ => iced::Color::from_rgb8(140, 30, 30),
            };
            button::Style {
                background: Some(bg.into()),
                text_color: theme.text_primary,
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .on_press(Message::Dialog(DialogMessage::HostKeyAccept));

    let reject_button = button(text("Reject").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(primary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::HostKeyReject));

    let button_row = row![
        Space::new().width(Length::Fill),
        accept_button,
        reject_button,
    ]
    .spacing(8);

    let content = column![
        row![warning_icon, title]
            .spacing(12)
            .align_y(Alignment::Center),
        Space::new().height(8),
        host_info,
        Space::new().height(16),
        mitm_warning,
        Space::new().height(16),
        fingerprint_box,
        Space::new().height(24),
        button_row,
    ]
    .spacing(4)
    .padding(24)
    .width(Length::Fixed(520.0));

    dialog_backdrop(content, theme)
}
