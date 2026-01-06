//! Terminal status bar component
//!
//! Displays connection duration, hostname, and keyboard shortcut hints
//! at the bottom of the terminal view.

use std::time::Instant;

use iced::widget::{container, row, text, Space};
use iced::{Alignment, Element, Length};

use crate::message::Message;
use crate::theme::Theme;

/// Format duration as MM:SS or HH:MM:SS
fn format_duration(start: Instant) -> String {
    let elapsed = start.elapsed();
    let total_secs = elapsed.as_secs();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, mins, secs)
    } else {
        format!("{:02}:{:02}", mins, secs)
    }
}

/// Build the terminal status bar element
pub fn terminal_status_bar<'a>(
    theme: Theme,
    host_name: &'a str,
    session_start: Instant,
    status_message: Option<&'a str>,
) -> Element<'a, Message> {
    let duration = format_duration(session_start);

    // Left side: hostname and duration
    let left = row![
        text(host_name).size(12).color(theme.text_secondary),
        text(" | ").size(12).color(theme.text_muted),
        text(duration).size(12).color(theme.text_secondary),
    ]
    .align_y(Alignment::Center);

    // Center: transient status message (if any)
    let center: Element<'_, Message> = if let Some(msg) = status_message {
        text(msg).size(12).color(theme.accent).into()
    } else {
        Space::new().into()
    };

    // Right side: shortcut hint
    let right = row![
        text("Ctrl+Shift+K").size(11).color(theme.text_muted),
        text(" Install SSH Key").size(11).color(theme.text_secondary),
    ]
    .align_y(Alignment::Center);

    let content = row![
        left,
        Space::new().width(Length::Fill),
        center,
        Space::new().width(Length::Fill),
        right,
    ]
    .padding([6, 12])
    .align_y(Alignment::Center);

    container(content)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}
