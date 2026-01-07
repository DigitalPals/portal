//! Common dialog components and utilities
//!
//! This module provides shared UI components used across all dialogs:
//! - `dialog_backdrop` - Modal backdrop with centered content
//! - `primary_button_style` - Accent-colored action button
//! - `secondary_button_style` - Outlined cancel/secondary button
//! - `dialog_input_style` - Styled text input for dialogs
//! - `dialog_pick_list_style` - Styled pick list for dialogs

use iced::widget::{button, container, mouse_area, pick_list, text_input};
use iced::{Alignment, Element, Length};

use crate::message::Message;
use crate::theme::{BORDER_RADIUS, Theme};

/// Wrap dialog content in a centered backdrop with modal styling.
///
/// Creates a semi-transparent overlay with the dialog box centered on screen.
/// The dialog box has a shadow and rounded corners.
/// The backdrop captures all mouse events to prevent interaction with elements behind it.
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

    let backdrop = container(
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
    });

    // Wrap in mouse_area to capture all mouse events and block clicks from passing through
    mouse_area(backdrop)
        .on_press(Message::Noop)
        .on_release(Message::Noop)
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

/// Styled text input for dialogs - rounded with border that highlights on focus
///
/// Returns a closure suitable for use with `text_input.style(...)`.
pub fn dialog_input_style(
    theme: Theme,
) -> impl Fn(&iced::Theme, text_input::Status) -> text_input::Style {
    move |_iced_theme, status| {
        let border_color = match status {
            text_input::Status::Focused { .. } => theme.accent,
            text_input::Status::Hovered => theme.text_muted,
            _ => theme.border,
        };
        text_input::Style {
            background: theme.surface.into(),
            border: iced::Border {
                color: border_color,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            icon: theme.text_muted,
            placeholder: theme.text_muted,
            value: theme.text_primary,
            selection: theme.selected,
        }
    }
}

/// Styled pick list for dialogs - rounded with border that highlights on hover/open
///
/// Returns a closure suitable for use with `pick_list.style(...)`.
pub fn dialog_pick_list_style(
    theme: Theme,
) -> impl Fn(&iced::Theme, pick_list::Status) -> pick_list::Style {
    move |_iced_theme, status| {
        let border_color = match status {
            pick_list::Status::Opened { .. } => theme.accent,
            pick_list::Status::Hovered => theme.text_muted,
            _ => theme.border,
        };
        pick_list::Style {
            background: theme.surface.into(),
            border: iced::Border {
                color: border_color,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            text_color: theme.text_primary,
            placeholder_color: theme.text_muted,
            handle_color: theme.text_secondary,
        }
    }
}

/// Styled menu for pick list dropdown
///
/// Returns a closure suitable for use with `pick_list.menu_style(...)`.
pub fn dialog_pick_list_menu_style(
    theme: Theme,
) -> impl Fn(&iced::Theme) -> iced::overlay::menu::Style {
    move |_iced_theme| iced::overlay::menu::Style {
        background: theme.surface.into(),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: BORDER_RADIUS.into(),
        },
        text_color: theme.text_primary,
        selected_background: theme.selected.into(),
        selected_text_color: theme.text_primary,
        shadow: iced::Shadow::default(),
    }
}
