use chrono::Local;
use iced::widget::{Column, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Fill, Length, Padding};

use crate::config::{AgentNotificationEntry, AgentNotificationsConfig};
use crate::icons::{self, icon_with_color};
use crate::message::{AgentNotificationMessage, Message};
use crate::theme::{BORDER_RADIUS, CARD_BORDER_RADIUS, ScaledFonts, Theme};

pub fn notification_center_view(
    notifications: &AgentNotificationsConfig,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let unread_count = notifications.unread_count();
    let active_count = notifications.active_count();

    let header = row![
        column![
            text("Agent Notifications")
                .size(fonts.page_title)
                .color(theme.text_primary),
            text(format!(
                "{} unread - {} active - {} stored",
                unread_count,
                active_count,
                notifications.entries.len()
            ))
            .size(fonts.label)
            .color(theme.text_muted),
        ]
        .spacing(4),
        Space::new().width(Fill),
        action_button("Jump newest", theme, fonts).on_press(Message::AgentNotification(
            AgentNotificationMessage::JumpLatestUnread,
        )),
        action_button("Mark all read", theme, fonts).on_press(Message::AgentNotification(
            AgentNotificationMessage::MarkAllRead,
        )),
        action_button("Clear read", theme, fonts).on_press(Message::AgentNotification(
            AgentNotificationMessage::ClearRead,
        )),
        destructive_button("Clear all", theme, fonts).on_press(Message::AgentNotification(
            AgentNotificationMessage::ClearAll,
        )),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let mut content = Column::new()
        .spacing(18)
        .padding(Padding::new(24.0).top(16.0).bottom(24.0))
        .push(header);

    if notifications.entries.is_empty() {
        content = content.push(empty_state(theme, fonts));
    } else {
        let latest = notifications.latest_active_by_session();
        if !latest.is_empty() {
            content = content.push(section_title("Latest by Session", theme, fonts));
            content = content.push(session_summary_list(latest, theme, fonts));
        }

        let active: Vec<_> = notifications.active_entries().collect();
        if !active.is_empty() {
            content = content.push(section_title("Active Queue", theme, fonts));
            content = content.push(notification_list(active, true, theme, fonts));
        }

        content = content.push(section_title("History", theme, fonts));
        content = content.push(notification_list(
            notifications.entries.iter().take(100).collect(),
            false,
            theme,
            fonts,
        ));
    }

    scrollable(container(content).width(Fill))
        .height(Fill)
        .into()
}

fn section_title(
    label: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    text(label)
        .size(fonts.heading)
        .color(theme.text_primary)
        .into()
}

fn session_summary_list(
    entries: Vec<&AgentNotificationEntry>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let rows: Vec<Element<'static, Message>> = entries
        .into_iter()
        .map(|entry| session_summary_row(entry, theme, fonts))
        .collect();
    Column::with_children(rows).spacing(8).into()
}

fn session_summary_row(
    entry: &AgentNotificationEntry,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let unread = entry.is_unread();
    let title = entry.display_title().to_string();
    let body = entry.display_body().to_string();
    let host_name = entry.host_name.clone();
    let time = format_timestamp(entry.received_at);

    let status = if unread { "Unread" } else { "Read" };
    let status_color = if unread {
        theme.accent
    } else {
        theme.text_muted
    };

    let content = row![
        attention_icon(unread, theme),
        column![
            row![
                text(host_name).size(fonts.body).color(theme.text_primary),
                text(status).size(fonts.label).color(status_color),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(title).size(fonts.label).color(theme.text_secondary),
            text(body).size(fonts.label).color(theme.text_muted),
        ]
        .spacing(3),
        Space::new().width(Fill),
        text(time).size(fonts.label).color(theme.text_muted),
        action_button("Jump", theme, fonts).on_press(Message::AgentNotification(
            AgentNotificationMessage::Jump(entry.id),
        )),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    card(content, unread, theme)
}

fn notification_list(
    entries: Vec<&AgentNotificationEntry>,
    active_actions: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    if entries.is_empty() {
        return container(
            text("No notifications")
                .size(fonts.body)
                .color(theme.text_muted),
        )
        .padding(16)
        .width(Fill)
        .into();
    }

    let rows: Vec<Element<'static, Message>> = entries
        .into_iter()
        .map(|entry| notification_row(entry, active_actions, theme, fonts))
        .collect();
    Column::with_children(rows).spacing(8).into()
}

fn notification_row(
    entry: &AgentNotificationEntry,
    active_actions: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let unread = entry.is_unread();
    let cleared = entry.cleared_at.is_some();
    let title = entry.display_title().to_string();
    let body = entry.display_body().to_string();
    let host_name = entry.host_name.clone();
    let time = format_timestamp(entry.received_at);

    let status = if cleared {
        "Cleared"
    } else if unread {
        "Unread"
    } else {
        "Read"
    };
    let status_color = if unread {
        theme.accent
    } else {
        theme.text_muted
    };

    let read_toggle = if unread {
        action_button("Read", theme, fonts).on_press(Message::AgentNotification(
            AgentNotificationMessage::MarkRead(entry.id),
        ))
    } else {
        action_button("Unread", theme, fonts).on_press(Message::AgentNotification(
            AgentNotificationMessage::MarkUnread(entry.id),
        ))
    };

    let mut actions = row![
        action_button("Jump", theme, fonts).on_press(Message::AgentNotification(
            AgentNotificationMessage::Jump(entry.id),
        )),
        read_toggle,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    if active_actions && !cleared {
        actions = actions.push(action_button("Clear", theme, fonts).on_press(
            Message::AgentNotification(AgentNotificationMessage::Clear(entry.id)),
        ));
    }

    let content = row![
        attention_icon(unread, theme),
        column![
            row![
                text(host_name).size(fonts.body).color(theme.text_primary),
                text(status).size(fonts.label).color(status_color),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            text(title).size(fonts.label).color(theme.text_secondary),
            text(body).size(fonts.label).color(theme.text_muted),
        ]
        .spacing(3),
        Space::new().width(Fill),
        text(time).size(fonts.label).color(theme.text_muted),
        actions,
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    card(content, unread, theme)
}

fn attention_icon(unread: bool, theme: Theme) -> Element<'static, Message> {
    let color = if unread {
        theme.accent
    } else {
        theme.text_muted
    };
    container(icon_with_color(icons::ui::ALERT_TRIANGLE, 18, color))
        .width(36)
        .height(36)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color,
                width: if unread { 1.0 } else { 0.0 },
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        })
        .into()
}

fn card(
    content: iced::widget::Row<'static, Message>,
    unread: bool,
    theme: Theme,
) -> Element<'static, Message> {
    container(content)
        .padding(12)
        .width(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: if unread { theme.accent } else { theme.border },
                width: if unread { 1.0 } else { 0.0 },
                radius: CARD_BORDER_RADIUS.into(),
            },
            ..Default::default()
        })
        .into()
}

fn action_button<'a>(
    label: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
) -> iced::widget::Button<'a, Message> {
    button(text(label).size(fonts.label).color(theme.text_primary))
        .padding([6, 10])
        .style(move |_theme, status| {
            let background = match status {
                button::Status::Hovered => Some(theme.hover.into()),
                _ => Some(theme.background.into()),
            };
            button::Style {
                background,
                text_color: theme.text_primary,
                border: iced::Border {
                    color: theme.border,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            }
        })
}

fn destructive_button<'a>(
    label: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
) -> iced::widget::Button<'a, Message> {
    let danger = iced::Color::from_rgb8(0xd2, 0x0f, 0x39);
    button(text(label).size(fonts.label).color(theme.text_primary))
        .padding([6, 10])
        .style(move |_theme, status| {
            let background = match status {
                button::Status::Hovered => Some(danger.into()),
                _ => Some(theme.background.into()),
            };
            button::Style {
                background,
                text_color: theme.text_primary,
                border: iced::Border {
                    color: danger,
                    width: 1.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            }
        })
}

fn empty_state(theme: Theme, fonts: ScaledFonts) -> Element<'static, Message> {
    let content = column![
        icon_with_color(icons::ui::ALERT_TRIANGLE, 48, theme.text_muted),
        text("No agent notifications")
            .size(fonts.heading)
            .color(theme.text_primary),
        text("Terminal attention events from Codex, Claude Code, OpenCode, and other agents will appear here")
            .size(fonts.body)
            .color(theme.text_muted),
    ]
    .spacing(8)
    .align_x(Alignment::Center);

    container(content)
        .width(Fill)
        .height(Length::Fixed(300.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .into()
}

fn format_timestamp(date: chrono::DateTime<chrono::Utc>) -> String {
    let local = date.with_timezone(&Local);
    let today = Local::now().date_naive();
    if local.date_naive() == today {
        local.format("%H:%M:%S").to_string()
    } else {
        local.format("%b %-d %H:%M").to_string()
    }
}
