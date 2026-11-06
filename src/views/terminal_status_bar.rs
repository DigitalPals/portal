//! Terminal status bar component
//!
//! Displays connection duration, hostname, and keyboard shortcut hints
//! at the bottom of the terminal view.

use std::time::Instant;

use iced::widget::{Space, container, row, text};
use iced::{Alignment, Element, Length};

use crate::message::Message;
use crate::theme::{ScaledFonts, Theme};
use crate::views::components::kbd;

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
    fonts: ScaledFonts,
    host_name: &'a str,
    can_install_key: bool,
    session_start: Instant,
    status_message: Option<String>,
) -> Element<'a, Message> {
    let duration = format_duration(session_start);

    // Left side: hostname and duration
    let left = row![
        text(host_name)
            .size(fonts.caption)
            .color(theme.text_secondary),
        text(" | ").size(fonts.caption).color(theme.text_muted),
        text(duration)
            .size(fonts.caption)
            .color(theme.text_secondary),
    ]
    .align_y(Alignment::Center);

    // Center: transient status message (if any)
    let center: Element<'_, Message> = if let Some(msg) = status_message {
        text(msg).size(fonts.caption).color(theme.accent).into()
    } else {
        Space::new().into()
    };

    // Right side: shortcut hint
    let right: Element<'_, Message> = if can_install_key {
        row![
            kbd("Ctrl+Shift+K", theme, fonts),
            text("Install SSH Key")
                .size(fonts.small)
                .color(theme.text_secondary),
        ]
        .spacing(6)
        .align_y(Alignment::Center)
        .into()
    } else {
        Space::new().into()
    };

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{ThemeId, get_theme};

    #[test]
    fn key_install_hint_only_appears_when_the_session_supports_it() {
        for theme in ThemeId::all() {
            for can_install_key in [false, true] {
                let mut ui = iced_test::simulator(terminal_status_bar(
                    get_theme(*theme),
                    ScaledFonts::new(1.0),
                    "Terminal",
                    can_install_key,
                    Instant::now(),
                    Some("Copied".into()),
                ));
                assert_eq!(ui.find("Ctrl+Shift+K").is_ok(), can_install_key);
                assert_eq!(ui.find("Install SSH Key").is_ok(), can_install_key);
                assert!(ui.find("Terminal").is_ok());
                assert!(ui.find("Copied").is_ok());
            }
        }
    }
}
