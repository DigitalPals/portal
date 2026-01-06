//! Settings dialog

use iced::widget::{button, column, row, slider, text, toggler, Space};
use iced::{Alignment, Element, Length};

use crate::message::Message;
use crate::theme::Theme;

use super::common::{dialog_backdrop, secondary_button_style};

/// State for the settings dialog
#[derive(Debug, Clone)]
pub struct SettingsDialogState {
    pub dark_mode: bool,
    pub terminal_font_size: f32,
}

impl Default for SettingsDialogState {
    fn default() -> Self {
        Self {
            dark_mode: true,
            terminal_font_size: 9.0,
        }
    }
}

/// Build the settings dialog view
pub fn settings_dialog_view(state: &SettingsDialogState, theme: Theme) -> Element<'static, Message> {
    let title = text("Settings").size(20).color(theme.text_primary);

    // Theme toggle
    let theme_row = row![
        text("Dark Mode").size(14).color(theme.text_primary),
        Space::with_width(Length::Fill),
        toggler(state.dark_mode)
            .on_toggle(Message::SettingsThemeToggle)
            .size(20),
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    let theme_section = column![
        text("Appearance").size(12).color(theme.text_muted),
        Space::with_height(8),
        theme_row,
    ]
    .spacing(0);

    // Terminal section
    let font_size = state.terminal_font_size;
    let font_size_row = row![
        text("Font Size").size(14).color(theme.text_primary),
        Space::with_width(Length::Fill),
        slider(6.0..=20.0, font_size, Message::SettingsFontSizeChange)
            .step(1.0)
            .width(120),
        Space::with_width(8),
        text(format!("{:.0}", font_size))
            .size(14)
            .color(theme.text_secondary)
            .width(Length::Fixed(24.0)),
    ]
    .align_y(Alignment::Center)
    .spacing(8);

    let terminal_section = column![
        text("Terminal").size(12).color(theme.text_muted),
        Space::with_height(8),
        font_size_row,
    ]
    .spacing(0);

    // About section
    let about_section = column![
        text("About").size(12).color(theme.text_muted),
        Space::with_height(8),
        text(format!("Portal SSH Client v{}", env!("CARGO_PKG_VERSION"))).size(14).color(theme.text_secondary),
        text("A modern SSH client built with Rust and iced").size(12).color(theme.text_muted),
    ]
    .spacing(4);

    // Close button using common style
    let close_button = button(text("Close").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(secondary_button_style(theme))
        .on_press(Message::DialogClose);

    let button_row = row![Space::with_width(Length::Fill), close_button];

    let form = column![
        title,
        Space::with_height(24),
        theme_section,
        Space::with_height(24),
        terminal_section,
        Space::with_height(24),
        about_section,
        Space::with_height(24),
        button_row,
    ]
    .spacing(0)
    .padding(24)
    .width(Length::Fixed(400.0));

    dialog_backdrop(form, theme)
}
