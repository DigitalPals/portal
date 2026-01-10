//! About dialog showing application info and credits

use iced::widget::{Column, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Font, Length};

use crate::message::{DialogMessage, Message};
use crate::theme::{
    FONT_SIZE_BODY, FONT_SIZE_BUTTON_SMALL, FONT_SIZE_LABEL, FONT_SIZE_MONO_TINY,
    FONT_SIZE_SECTION, Theme,
};

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
pub fn about_dialog_view(_state: &AboutDialogState, theme: Theme) -> Element<'static, Message> {
    let version = env!("CARGO_PKG_VERSION");

    // Full logo
    let full_logo = format!("{}\n{}", PORTAL_LOGO_TOP, PORTAL_LOGO_LAST_LINE);

    let logo_text = text(full_logo)
        .size(FONT_SIZE_MONO_TINY)
        .color(theme.text_secondary)
        .font(Font::MONOSPACE);

    let version_text = text(format!("Version {}", version))
        .size(FONT_SIZE_SECTION)
        .color(theme.text_primary);

    let tagline = text("A modern, fast SSH client for macOS and Linux")
        .size(FONT_SIZE_BODY)
        .color(theme.text_secondary);

    // Author section
    let author_text = text("Created by John Pals")
        .size(FONT_SIZE_BUTTON_SMALL)
        .color(theme.text_secondary);

    // Vibe coded note
    let vibe_text = row![
        text("Proudly vibe coded with ")
            .size(FONT_SIZE_LABEL)
            .color(theme.text_muted),
        text("Claude Code")
            .size(FONT_SIZE_LABEL)
            .color(theme.text_primary),
        text(" & ").size(FONT_SIZE_LABEL).color(theme.text_muted),
        text("Codex CLI")
            .size(FONT_SIZE_LABEL)
            .color(theme.text_primary),
    ];

    // Credits section
    let credits_title = text("Built with")
        .size(FONT_SIZE_BODY)
        .color(theme.text_primary);

    let credits_list = vec![
        ("Iced", "Cross-platform GUI framework"),
        ("Alacritty Terminal", "Terminal emulation"),
        ("Russh", "SSH protocol implementation"),
        ("Tokio", "Async runtime"),
    ];

    let credits_items: Vec<Element<'static, Message>> = credits_list
        .into_iter()
        .map(|(name, desc)| {
            row![
                text(name)
                    .size(FONT_SIZE_BUTTON_SMALL)
                    .color(theme.text_primary),
                text(" - ")
                    .size(FONT_SIZE_BUTTON_SMALL)
                    .color(theme.text_muted),
                text(desc)
                    .size(FONT_SIZE_BUTTON_SMALL)
                    .color(theme.text_secondary),
            ]
            .into()
        })
        .collect();

    let credits_column = Column::with_children(credits_items).spacing(6);

    // Close button
    let close_btn = button(text("Close").size(FONT_SIZE_BODY).color(theme.text_primary))
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
        Space::new().height(12),
        container(vibe_text)
            .width(Length::Fill)
            .align_x(Alignment::Center),
        Space::new().height(20),
        credits_title,
        Space::new().height(8),
        scrollable(credits_column).height(Length::Shrink),
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
