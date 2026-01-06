use iced::widget::{Column, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Fill, Length};

use crate::app::FocusSection;
use crate::config::{HistoryConfig, SessionType};
use crate::icons::{self, icon_with_color};
use crate::message::{HistoryMessage, Message};
use crate::theme::{BORDER_RADIUS, CARD_BORDER_RADIUS, Theme};

/// Build the history view showing recent connections
pub fn history_view(
    history: &HistoryConfig,
    theme: Theme,
    focus_section: FocusSection,
    focus_index: Option<usize>,
) -> Element<'static, Message> {
    // Header
    let header = row![
        text("Connection History")
            .size(18)
            .color(theme.text_primary),
        Space::new().width(Length::Fill),
        button(text("Clear History").size(12).color(theme.text_secondary),)
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered => Some(theme.hover.into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: theme.text_secondary,
                    border: iced::Border {
                        color: theme.border,
                        width: 1.0,
                        radius: BORDER_RADIUS.into(),
                    },
                    ..Default::default()
                }
            })
            .padding([6, 12])
            .on_press(Message::History(HistoryMessage::Clear)),
    ]
    .align_y(Alignment::Center)
    .padding(iced::Padding::new(24.0).bottom(16.0));

    // History entries
    let content: Element<'static, Message> = if history.entries.is_empty() {
        empty_state(theme)
    } else {
        let mut entries_column = Column::new()
            .spacing(8)
            .padding(iced::Padding::new(24.0).top(0.0));

        for (idx, entry) in history.entries.iter().enumerate() {
            let is_focused = focus_section == FocusSection::Content && focus_index == Some(idx);
            entries_column = entries_column.push(history_entry_row(entry, theme, is_focused));
        }

        scrollable(entries_column).height(Fill).width(Fill).into()
    };

    let main_content = column![header, content];

    container(main_content)
        .width(Fill)
        .height(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.background.into()),
            ..Default::default()
        })
        .into()
}

/// Single history entry row
fn history_entry_row(
    entry: &crate::config::HistoryEntry,
    theme: Theme,
    is_focused: bool,
) -> Element<'static, Message> {
    let entry_id = entry.id;

    // Session type icon
    let icon_data = match entry.session_type {
        SessionType::Ssh => icons::ui::TERMINAL,
        SessionType::Sftp => icons::ui::HARD_DRIVE,
        SessionType::Local => icons::ui::TERMINAL,
    };

    let type_text = entry.session_type.display_name().to_string();

    // Format connection time
    let time_str = entry.connected_at.format("%Y-%m-%d %H:%M").to_string();

    // Duration (if disconnected)
    let duration_str = entry.duration_string();

    // Clone all needed strings
    let host_name = entry.host_name.clone();
    let username = entry.username.clone();
    let hostname = entry.hostname.clone();

    let info = column![
        row![
            text(host_name).size(14).color(theme.text_primary),
            Space::new().width(8),
            container(text(type_text).size(10).color(theme.text_secondary),)
                .padding([2, 6])
                .style(move |_theme| container::Style {
                    background: Some(theme.surface.into()),
                    border: iced::Border {
                        radius: 2.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }),
        ]
        .align_y(Alignment::Center),
        text(format!(
            "{}@{} | {} | {}",
            username, hostname, time_str, duration_str
        ))
        .size(12)
        .color(theme.text_muted),
    ]
    .spacing(4);

    let icon_widget = container(icon_with_color(icon_data, 18, theme.accent))
        .width(36)
        .height(36)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_theme| container::Style {
            background: Some(theme.selected.into()),
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let reconnect_btn = button(
        row![
            icon_with_color(icons::ui::REFRESH, 12, theme.text_primary),
            text("Reconnect").size(12).color(theme.text_primary),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .style(move |_theme, status| {
        let bg = match status {
            button::Status::Hovered => theme.accent,
            _ => theme.surface,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .padding([6, 12])
    .on_press(Message::History(HistoryMessage::Reconnect(entry_id)));

    let card_content = row![
        icon_widget,
        info,
        Space::new().width(Length::Fill),
        reconnect_btn,
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let bg = if is_focused {
        theme.hover
    } else {
        theme.surface
    };
    let border = if is_focused {
        iced::Border {
            color: theme.focus_ring,
            width: 2.0,
            radius: CARD_BORDER_RADIUS.into(),
        }
    } else {
        iced::Border {
            radius: CARD_BORDER_RADIUS.into(),
            ..Default::default()
        }
    };

    container(card_content)
        .padding(12)
        .width(Fill)
        .style(move |_theme| container::Style {
            background: Some(bg.into()),
            border,
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 0, 0, 0.1),
                offset: iced::Vector::new(0.0, 1.0),
                blur_radius: 2.0,
            },
            ..Default::default()
        })
        .into()
}

/// Empty state when no history
fn empty_state(theme: Theme) -> Element<'static, Message> {
    let content = column![
        icon_with_color(icons::ui::HISTORY, 48, theme.text_muted),
        text("No connection history")
            .size(18)
            .color(theme.text_primary),
        text("Your recent connections will appear here")
            .size(14)
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
