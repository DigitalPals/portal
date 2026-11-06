//! Toast notifications, including persistent connection failures.

use std::time::{Duration, Instant};

use iced::widget::{Column, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Color, Element, Length, Padding};
use uuid::Uuid;

use crate::config::Host;
use crate::icons::{icon_with_color, ui};
use crate::message::{Message, UiMessage};
use crate::theme::{BORDER_RADIUS, ScaledFonts, Theme};
use crate::views::dialogs::common::secondary_button_style;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastAction {
    OpenVaultApprovals,
    RetryConnection,
    EditConnection,
}

/// Type of toast notification (determines color and icon)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastType {
    Error,
    Warning,
    Success,
}

impl ToastType {
    /// Get the color for this toast type
    pub fn color(&self, theme: Theme) -> Color {
        match self {
            ToastType::Error => theme.terminal.ansi[1],
            ToastType::Warning => theme.terminal.ansi[3],
            ToastType::Success => theme.terminal.ansi[2],
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

#[derive(Debug, Clone)]
pub struct ConnectionFailure {
    pub host: Option<Host>,
    pub detail: String,
    pub show_details: bool,
}

/// A single toast notification
#[derive(Debug, Clone)]
pub struct Toast {
    pub id: Uuid,
    pub message: String,
    pub toast_type: ToastType,
    pub created_at: Instant,
    pub duration: Option<Duration>,
    pub action: Option<ToastAction>,
    pub connection: Option<ConnectionFailure>,
}

impl Toast {
    /// Create a new toast with the specified type and message
    pub fn new(message: impl Into<String>, toast_type: ToastType) -> Self {
        Self {
            id: Uuid::new_v4(),
            message: message.into(),
            toast_type,
            created_at: Instant::now(),
            duration: Some(Duration::from_secs(5)),
            action: None,
            connection: None,
        }
    }

    /// Create an error toast
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(message, ToastType::Error)
    }

    pub fn connection_error(host: Option<Host>, detail: impl Into<String>) -> Self {
        let detail = detail.into();
        let summary = connection_error_summary(&detail);
        let message = host.as_ref().map_or_else(
            || summary.to_string(),
            |host| format!("{}: {}", host.name, summary),
        );
        let mut toast = Self::error(message).persistent();
        toast.connection = Some(ConnectionFailure {
            host,
            detail,
            show_details: false,
        });
        toast
    }

    /// Create a warning toast
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(message, ToastType::Warning)
    }

    /// Create a success toast
    pub fn success(message: impl Into<String>) -> Self {
        Self::new(message, ToastType::Success)
    }

    pub fn persistent(mut self) -> Self {
        self.duration = None;
        self
    }

    pub fn action(mut self, action: ToastAction) -> Self {
        self.action = Some(action);
        self
    }

    /// Check if this toast has expired
    pub fn is_expired(&self) -> bool {
        self.duration
            .is_some_and(|duration| self.created_at.elapsed() >= duration)
    }
}

/// Backends preserve their diagnostic strings; only the visible summary is simplified.
fn connection_error_summary(detail: &str) -> &'static str {
    let detail = detail.to_ascii_lowercase();
    if detail.contains("connection refused") || detail.contains("actively refused") {
        "Connection refused"
    } else if ["timed out", "timeout", "deadline has elapsed"]
        .iter()
        .any(|word| detail.contains(word))
    {
        "Connection timed out"
    } else if [
        "host not found",
        "no such host",
        "name or service not known",
        "nodename nor servname",
        "failed to lookup address",
        "name resolution",
        "dns error",
    ]
    .iter()
    .any(|word| detail.contains(word))
    {
        "Host not found"
    } else if [
        "authentication failed",
        "authentication failure",
        "authentication error",
        "authentication rejected",
        "wrong password",
        "no password provided",
        "permission denied",
        "no authentication method",
        "no auth method",
        "authfailure",
    ]
    .iter()
    .any(|word| detail.contains(word))
    {
        "Authentication failed"
    } else {
        "Could not connect"
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
        // Trim temporary notifications without discarding undismissed errors.
        if self.toasts.len() >= 5
            && let Some(index) = self
                .toasts
                .iter()
                .position(|toast| toast.duration.is_some())
        {
            self.toasts.remove(index);
        }
        self.toasts.push(toast);
    }

    /// Add a toast, or refresh an existing one with the same message and type.
    /// If a matching toast exists, its `created_at` is reset to extend its display time.
    pub fn push_or_refresh(&mut self, toast: Toast) {
        if let Some(existing) = self
            .toasts
            .iter_mut()
            .find(|t| t.message == toast.message && t.toast_type == toast.toast_type)
        {
            existing.created_at = Instant::now();
            existing.duration = toast.duration;
            existing.action = toast.action;
            existing.connection = toast.connection;
            return;
        }
        self.push(toast);
    }

    pub fn dismiss_action(&mut self, action: ToastAction) {
        self.toasts.retain(|toast| toast.action != Some(action));
    }

    /// Remove a toast by ID (for manual dismissal)
    pub fn dismiss(&mut self, id: Uuid) {
        self.toasts.retain(|t| t.id != id);
    }

    pub fn get(&self, id: Uuid) -> Option<&Toast> {
        self.toasts.iter().find(|toast| toast.id == id)
    }

    pub fn toggle_details(&mut self, id: Uuid) {
        if let Some(connection) = self
            .toasts
            .iter_mut()
            .find(|toast| toast.id == id)
            .and_then(|toast| toast.connection.as_mut())
        {
            connection.show_details = !connection.show_details;
        }
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
pub fn toast_overlay_view(
    manager: &ToastManager,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    if !manager.has_toasts() {
        return Space::new().into();
    }

    let toast_list: Element<'_, Message> = Column::with_children(
        manager
            .toasts()
            .iter()
            .rev()
            .map(|toast| toast_item_view(toast, theme, fonts)),
    )
    .spacing(8)
    .width(Length::Fill)
    .max_width(520)
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
fn toast_item_view(toast: &Toast, theme: Theme, fonts: ScaledFonts) -> Element<'static, Message> {
    let toast_id = toast.id;
    let accent_color = toast.toast_type.color(theme);
    let message = toast.message.clone();
    let action = toast.action;

    // Type icon
    let type_icon = icon_with_color(toast.toast_type.icon(), 16, accent_color);

    // Dismiss button
    let dismiss_btn = button(icon_with_color(ui::X, 12, theme.text_secondary))
        .padding(4)
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => Some(theme.hover.into()),
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
        .on_press(Message::Ui(UiMessage::ToastDismiss(toast_id)));

    let message_content = row![
        container(type_icon).padding(Padding::from([0, 8])),
        text(message)
            .size(fonts.button_small)
            .color(theme.text_primary)
            .width(Length::Fill),
    ]
    .align_y(Alignment::Center);

    let message_element: Element<'static, Message> = if let Some(action) = action {
        button(message_content)
            .padding(0)
            .width(Length::Fill)
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered => Some(theme.hover.into()),
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
            .on_press(Message::Ui(UiMessage::ToastAction(toast_id, action)))
            .into()
    } else {
        container(message_content).width(Length::Fill).into()
    };

    let mut content =
        column![row![message_element, dismiss_btn].align_y(Alignment::Start)].spacing(10);

    if let Some(connection) = &toast.connection {
        let mut actions = row![].spacing(8).align_y(Alignment::Center);
        if connection.host.is_some() {
            for (label, action) in [
                ("Retry", ToastAction::RetryConnection),
                ("Edit host", ToastAction::EditConnection),
            ] {
                actions = actions.push(
                    button(text(label).size(fonts.button_small))
                        .padding([6, 12])
                        .style(secondary_button_style(theme))
                        .on_press(Message::Ui(UiMessage::ToastAction(toast_id, action))),
                );
            }
        }
        actions = actions.push(
            button(
                text(if connection.show_details {
                    "Hide details"
                } else {
                    "Details"
                })
                .size(fonts.button_small),
            )
            .padding([6, 12])
            .style(secondary_button_style(theme))
            .on_press(Message::Ui(UiMessage::ToastDetailsToggle(toast_id))),
        );
        content = content.push(actions);
        if connection.show_details {
            content = content.push(
                container(
                    scrollable(
                        text(connection.detail.clone())
                            .size(fonts.label)
                            .color(theme.text_secondary)
                            .width(Length::Fill),
                    )
                    .height(Length::Shrink)
                    .width(Length::Fill),
                )
                .max_height(160),
            );
        }
    }

    container(content)
        .padding([10, 14])
        .width(Length::Fill)
        .max_width(520)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: accent_color,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            shadow: iced::Shadow {
                color: theme.text_primary.scale_alpha(0.18),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_connection_errors_have_short_summaries_and_preserve_details() {
        for (detail, summary) in [
            (
                "Connection failed: Connection failed to 127.0.0.1:1: Connection refused (os error 111)",
                "Connection refused",
            ),
            (
                "No connection could be made because the target machine actively refused it. (os error 10061)",
                "Connection refused",
            ),
            (
                "Timeout connecting to example.com:22",
                "Connection timed out",
            ),
            ("Connection timed out (os error 60)", "Connection timed out"),
            (
                "failed to lookup address information: Name or service not known",
                "Host not found",
            ),
            (
                "nodename nor servname provided, or not known",
                "Host not found",
            ),
            ("No such host is known. (os error 11001)", "Host not found"),
            (
                "Authentication failed: Permission denied (publickey)",
                "Authentication failed",
            ),
            ("Wrong password", "Authentication failed"),
            (
                "Portal Hub connection failed: websocket protocol error",
                "Could not connect",
            ),
        ] {
            let toast = Toast::connection_error(None, detail);
            assert_eq!(toast.message, summary);
            let connection = toast.connection.unwrap();
            assert_eq!(connection.detail, detail);
            assert!(!connection.show_details);
        }
    }

    #[test]
    fn connection_failure_survives_expiry_and_temporary_notifications() {
        let mut manager = ToastManager::new();
        let mut failure = Toast::connection_error(None, "Connection refused");
        failure.created_at = Instant::now() - Duration::from_secs(60);
        let id = failure.id;
        manager.push(failure);
        for _ in 0..8 {
            let mut info = Toast::success("Saved");
            assert_eq!(info.duration, Some(Duration::from_secs(5)));
            info.created_at = Instant::now() - Duration::from_secs(6);
            manager.push(info);
        }
        manager.cleanup_expired();
        assert_eq!(manager.toasts().len(), 1);
        assert_eq!(manager.toasts()[0].id, id);
        manager.toggle_details(id);
        assert!(
            manager
                .get(id)
                .unwrap()
                .connection
                .as_ref()
                .unwrap()
                .show_details
        );
        manager.dismiss(id);
        assert!(!manager.has_toasts());
    }
}
