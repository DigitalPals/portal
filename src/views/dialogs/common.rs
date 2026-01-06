//! Common dialog components and utilities
//!
//! This module provides shared UI components used across all dialogs:
//! - `dialog_backdrop` - Modal backdrop with centered content
//! - `primary_button_style` - Accent-colored action button
//! - `secondary_button_style` - Outlined cancel/secondary button

use iced::widget::{button, container};
use iced::{Alignment, Element, Length};

use crate::message::Message;
use crate::theme::{Theme, BORDER_RADIUS};

/// Wrap dialog content in a centered backdrop with modal styling.
///
/// Creates a semi-transparent overlay with the dialog box centered on screen.
/// The dialog box has a shadow and rounded corners.
pub fn dialog_backdrop(
    content: impl Into<Element<'static, Message>>,
    theme: Theme,
) -> Element<'static, Message> {
    let dialog_box = container(content).style(move |_theme| container::Style {
        background: Some(theme.surface.into()),
        border: iced::Border {
            color: theme.border,
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
    .style(move |_theme| container::Style {
        background: Some(iced::Color::from_rgba8(0, 0, 0, 0.7).into()),
        ..Default::default()
    })
    .into()
}

/// Primary button style - accent-colored for main actions (Save, Submit, etc.)
///
/// Returns a closure suitable for use with `button.style(...)`.
pub fn primary_button_style(
    theme: Theme,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_iced_theme, status| {
        let (background, text_color) = match status {
            button::Status::Disabled => (theme.surface, theme.text_muted),
            _ => (theme.accent, theme.text_primary),
        };
        button::Style {
            background: Some(background.into()),
            text_color,
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }
}

/// Secondary button style - outlined for cancel/secondary actions
///
/// Returns a closure suitable for use with `button.style(...)`.
pub fn secondary_button_style(
    theme: Theme,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |_iced_theme, status| {
        let bg = match status {
            button::Status::Hovered => theme.hover,
            _ => theme.surface,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        }
    }
}
