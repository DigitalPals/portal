//! Connecting dialog shown while establishing SSH/VNC connections

use iced::widget::{column, container, text};
use iced::{Alignment, Element, Length};

use crate::message::Message;
use crate::theme::Theme;

use super::common::dialog_backdrop;

/// State for the connecting dialog
#[derive(Debug, Clone)]
pub struct ConnectingDialogState {
    pub host_name: String,
    pub protocol: String,
}

impl ConnectingDialogState {
    pub fn new(host_name: String, protocol: &str) -> Self {
        Self {
            host_name,
            protocol: protocol.to_string(),
        }
    }
}

/// Build the connecting dialog view
pub fn connecting_dialog_view(
    state: &ConnectingDialogState,
    theme: Theme,
) -> Element<'static, Message> {
    let content = column![
        text(format!("Connecting to {}...", state.host_name))
            .size(16)
            .color(theme.text_primary),
        text(format!("Establishing {} connection", state.protocol))
            .size(13)
            .color(theme.text_secondary),
    ]
    .spacing(8)
    .align_x(Alignment::Center);

    let padded = container(content)
        .padding(30)
        .width(Length::Shrink)
        .center_x(Length::Shrink);

    dialog_backdrop(padded, theme)
}
