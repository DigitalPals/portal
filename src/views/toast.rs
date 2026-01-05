//! Toast notification system for displaying temporary messages

use std::time::{Duration, Instant};

use iced::widget::{button, container, row, text, Column, Space};
use iced::{Alignment, Color, Element, Length, Padding};
use uuid::Uuid;

use crate::icons::{icon_with_color, ui};
use crate::message::Message;
use crate::theme::{BORDER_RADIUS, THEME};

/// Type of toast notification (determines color and icon)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastType {
    Error,
    Warning,
    Success,
}

impl ToastType {
    /// Get the color for this toast type
    pub fn color(&self) -> Color {
        match self {
            ToastType::Error => Color::from_rgb8(0xf4, 0x43, 0x36),   // #f44336 - red
            ToastType::Warning => Color::from_rgb8(0xff, 0x98, 0x00), // #ff9800 - orange
            ToastType::Success => Color::from_rgb8(0x4c, 0xaf, 0x50), // #4caf50 - green
        }
    }

    /// Get the icon for this toast type
    pub fn icon(&self) -> &'static [u8] {
        match self {
            ToastType::Error => ui::X,
            ToastType::Warning => ui::ALERT_TRIANGLE,
            ToastType::Success => ui::CHECK,
        }
    }
}

/// A single toast notification
#[derive(Debug, Clone)]
pub struct Toast {
    pub id: Uuid,
    pub message: String,
    pub toast_type: ToastType,
    pub created_at: Instant,
    pub duration: Duration,
}

impl Toast {
    /// Create a new toast with the specified type and message
    pub fn new(message: impl Into<String>, toast_type: ToastType) -> Self {
        Self {
            id: Uuid::new_v4(),
            message: message.into(),
            toast_type,
            created_at: Instant::now(),
            duration: Duration::from_secs(5),
        }
    }

    /// Create an error toast
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(message, ToastType::Error)
    }

    /// Create a warning toast
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(message, ToastType::Warning)
    }

    /// Create a success toast
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(message, ToastType::Success)
    }

    /// Check if this toast has expired
    pub fn is_expired(&self) -> bool {
        self.created_at.elapsed() >= self.duration
    }
}

/// Manager for multiple toast notifications
#[derive(Debug, Default)]
pub struct ToastManager {
    toasts: Vec<Toast>,
}

impl ToastManager {
    /// Create a new empty toast manager
    pub fn new() -> Self {
        Self { toasts: Vec::new() }
    }

    /// Add a toast notification
    pub fn push(&mut self, toast: Toast) {
        // Limit to 5 visible toasts
        if self.toasts.len() >= 5 {
            self.toasts.remove(0);
        }
        self.toasts.push(toast);
    }

    /// Remove a toast by ID (for manual dismissal)
    pub fn dismiss(&mut self, id: Uuid) {
        self.toasts.retain(|t| t.id != id);
    }

    /// Remove all expired toasts
    pub fn cleanup_expired(&mut self) {
        self.toasts.retain(|t| !t.is_expired());
    }

    /// Check if there are any active toasts
    pub fn has_toasts(&self) -> bool {
        !self.toasts.is_empty()
    }

    /// Get slice of toasts
    pub fn toasts(&self) -> &[Toast] {
        &self.toasts
    }
}

/// Render the toast overlay (positioned at bottom-right)
pub fn toast_overlay_view(manager: &ToastManager) -> Element<'static, Message> {
    if !manager.has_toasts() {
        return Space::new(0, 0).into();
    }

    let toast_list: Element<'_, Message> = Column::with_children(
        manager
            .toasts()
            .iter()
            .rev()
            .map(|toast| toast_item_view(toast)),
    )
    .spacing(8)
    .into();

    // Position at bottom-right with padding
    container(
        container(toast_list)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(Alignment::End)
            .align_y(Alignment::End)
            .padding(16),
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

/// Render a single toast notification
fn toast_item_view(toast: &Toast) -> Element<'static, Message> {
    let toast_id = toast.id;
    let accent_color = toast.toast_type.color();
    let message = toast.message.clone();

    // Type icon
    let type_icon = icon_with_color(toast.toast_type.icon(), 16, accent_color);

    // Dismiss button
    let dismiss_btn = button(icon_with_color(ui::X, 12, THEME.text_secondary))
        .padding(4)
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => Some(THEME.hover.into()),
                _ => None,
            };
            button::Style {
                background: bg,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .on_press(Message::ToastDismiss(toast_id));

    let content = row![
        container(type_icon).padding(Padding::from([0, 8])),
        text(message).size(13).color(THEME.text_primary),
        Space::with_width(Length::Fill),
        dismiss_btn,
    ]
    .align_y(Alignment::Center);

    container(content)
        .padding([10, 14])
        .width(Length::Fixed(360.0))
        .style(move |_theme| container::Style {
            background: Some(THEME.surface.into()),
            border: iced::Border {
                color: accent_color,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            shadow: iced::Shadow {
                color: Color::from_rgba8(0, 0, 0, 0.3),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        })
        .into()
}
