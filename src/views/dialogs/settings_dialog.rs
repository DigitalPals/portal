//! Settings dialog

use iced::widget::{button, column, container, row, text, toggler, Space};
use iced::{Alignment, Element, Length};

use crate::message::Message;
use crate::theme::{BORDER_RADIUS, THEME};

/// State for the settings dialog
#[derive(Debug, Clone)]
pub struct SettingsDialogState {
    pub dark_mode: bool,
}

impl Default for SettingsDialogState {
    fn default() -> Self {
        Self { dark_mode: true }
    }
}

/// Build the settings dialog view
pub fn settings_dialog_view(state: &SettingsDialogState) -> Element<'static, Message> {
    let title = text("Settings").size(20).color(THEME.text_primary);

    // Theme toggle
    let theme_row = row![
        text("Dark Mode").size(14).color(THEME.text_primary),
        Space::with_width(Length::Fill),
        toggler(state.dark_mode)
            .on_toggle(Message::SettingsThemeToggle)
            .size(20),
    ]
    .align_y(Alignment::Center)
    .spacing(12);

    let theme_section = column![
        text("Appearance").size(12).color(THEME.text_muted),
        Space::with_height(8),
        theme_row,
    ]
    .spacing(0);

    // About section
    let about_section = column![
        text("About").size(12).color(THEME.text_muted),
        Space::with_height(8),
        text("Portal SSH Client v0.1.0").size(14).color(THEME.text_secondary),
        text("A modern SSH client built with Rust and iced").size(12).color(THEME.text_muted),
    ]
    .spacing(4);

    // Close button
    let close_button = button(text("Close").size(14).color(THEME.text_primary))
        .padding([8, 16])
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => THEME.accent,
                _ => THEME.surface,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: THEME.text_primary,
                border: iced::Border {
                    color: THEME.border,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            }
        })
        .on_press(Message::DialogClose);

    let button_row = row![Space::with_width(Length::Fill), close_button];

    let form = column![
        title,
        Space::with_height(24),
        theme_section,
        Space::with_height(24),
        about_section,
        Space::with_height(24),
        button_row,
    ]
    .spacing(0)
    .padding(24)
    .width(Length::Fixed(400.0));

    dialog_backdrop(form)
}

/// Helper to wrap dialog content in a backdrop
fn dialog_backdrop(content: impl Into<Element<'static, Message>>) -> Element<'static, Message> {
    let dialog_box = container(content)
        .style(|_theme| container::Style {
            background: Some(THEME.surface.into()),
            border: iced::Border {
                color: THEME.border,
                width: 1.0,
                radius: (BORDER_RADIUS * 2.0).into(),
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 0, 0, 0.5),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        });

    container(
        container(dialog_box)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(iced::Color::from_rgba8(0, 0, 0, 0.7).into()),
        ..Default::default()
    })
    .into()
}
