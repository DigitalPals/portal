//! Connecting dialog shown while establishing SSH/VNC connections

use std::time::Instant;

use iced::widget::{button, column, container, text};
use iced::{Alignment, Element, Length};

use crate::message::{DialogMessage, Message};
use crate::theme::{ScaledFonts, Theme};

use super::common::{dialog_backdrop, secondary_button_style};

/// State for the connecting dialog
#[derive(Debug, Clone)]
pub struct ConnectingDialogState {
    pub host_name: String,
    pub protocol: String,
    pub step: &'static str,
    pub started_at: Instant,
}

impl ConnectingDialogState {
    pub fn new(host_name: String, protocol: &str) -> Self {
        Self {
            host_name,
            protocol: protocol.to_string(),
            step: "Connecting",
            started_at: Instant::now(),
        }
    }
}

/// Build the connecting dialog view
pub fn connecting_dialog_view(
    state: &ConnectingDialogState,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let content = column![
        text(format!("Connecting to {}", state.host_name))
            .size(fonts.section)
            .color(theme.text_primary),
        text(format!(
            "{} · {} · {}s",
            state.protocol,
            state.step,
            state.started_at.elapsed().as_secs()
        ))
        .size(fonts.small)
        .color(theme.text_secondary),
        button(text("Cancel").size(fonts.body))
            .padding([8, 20])
            .style(secondary_button_style(theme))
            .on_press(Message::Dialog(DialogMessage::ConnectingCancel)),
    ]
    .spacing(12)
    .align_x(Alignment::Center);

    let padded = container(content)
        .padding(30)
        .width(Length::Fixed(420.0))
        .align_x(Alignment::Center);

    dialog_backdrop(padded, theme)
}
