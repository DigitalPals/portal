//! About dialog showing application info

use iced::widget::{Space, button, column, container, text};
use iced::{Alignment, Element, Font, Length};

use crate::message::{DialogMessage, Message};
use crate::theme::{ScaledFonts, Theme};

use super::common::{dialog_backdrop, secondary_button_style};

/// ASCII logo for Portal (moved from host_grid.rs)
const PORTAL_LOGO_TOP: &str = r#"                                  .             oooo
                                .o8             `888
oo.ooooo.   .ooooo.  oooo d8b .o888oo  .oooo.    888
 888' `88b d88' `88b `888""8P   888   `P  )88b   888
 888   888 888   888  888       888    .oP"888   888
 888   888 888   888  888       888 . d8(  888   888
 888bod8P' `Y8bod8P' d888b      "888" `Y888""8o o888o
 888"#;

const PORTAL_LOGO_LAST_LINE: &str = "o888o";

/// State for the About dialog (minimal - just a marker)
#[derive(Debug, Clone, Default)]
pub struct AboutDialogState;

impl AboutDialogState {
    pub fn new() -> Self {
        Self
    }
}

/// Build the About dialog view
pub fn about_dialog_view(
    _state: &AboutDialogState,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let version = env!("CARGO_PKG_VERSION");

    // Full logo
    let full_logo = format!("{}\n{}", PORTAL_LOGO_TOP, PORTAL_LOGO_LAST_LINE);

    let logo_text = text(full_logo)
        .size(fonts.mono_tiny)
        .color(theme.text_secondary)
        .font(Font::MONOSPACE);

    let version_text = text(format!("Version {}", version))
        .size(fonts.section)
        .color(theme.text_primary);

    let tagline = text("A modern, fast SSH client for macOS and Linux")
        .size(fonts.body)
        .color(theme.text_secondary);

    // Author section
    let author_text = text("Created by John Pals")
        .size(fonts.button_small)
        .color(theme.text_secondary);

    // Close button
    let close_btn = button(text("Close").size(fonts.body).color(theme.text_primary))
        .style(secondary_button_style(theme))
        .padding([8, 20])
        .on_press(Message::Dialog(DialogMessage::Close));

    let content = column![
        container(logo_text)
            .width(Length::Fill)
            .align_x(Alignment::Center),
        Space::new().height(16),
        container(version_text)
            .width(Length::Fill)
            .align_x(Alignment::Center),
        container(tagline)
            .width(Length::Fill)
            .align_x(Alignment::Center),
        Space::new().height(16),
        container(author_text)
            .width(Length::Fill)
            .align_x(Alignment::Center),
        Space::new().height(20),
        container(close_btn)
            .width(Length::Fill)
            .align_x(Alignment::Center),
    ]
    .spacing(4)
    .padding(24)
    .width(Length::Fixed(480.0));

    dialog_backdrop(content, theme)
}
