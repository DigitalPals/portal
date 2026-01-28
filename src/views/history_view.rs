use chrono::Local;
use iced::widget::{Column, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Fill, Length};

use crate::app::FocusSection;
use crate::config::{HistoryConfig, HistoryEntry, HostsConfig, SessionType};
use crate::icons::{self, icon_with_color};
use crate::message::{HistoryMessage, Message};
use crate::theme::{BORDER_RADIUS, CARD_BORDER_RADIUS, ScaledFonts, Theme};
use crate::views::host_grid::os_icon_data;

/// Format a date as a relative label ("Today", "Yesterday") or "Mon dd" format
fn format_relative_date(date: chrono::DateTime<chrono::Utc>) -> String {
    let local_date = date.with_timezone(&Local).date_naive();
    let today = Local::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);

    if local_date == today {
        "Today".to_string()
    } else if local_date == yesterday {
        "Yesterday".to_string()
    } else {
        date.with_timezone(&Local).format("%b %-d").to_string()
    }
}

/// Get the local date key for grouping (YYYY-MM-DD format)
fn date_key(date: chrono::DateTime<chrono::Utc>) -> String {
    date.with_timezone(&Local).format("%Y-%m-%d").to_string()
}

/// Build the history view showing recent connections
pub fn history_view(
    history: &HistoryConfig,
    hosts_config: &HostsConfig,
    theme: Theme,
    fonts: ScaledFonts,
    focus_section: FocusSection,
    focus_index: Option<usize>,
) -> Element<'static, Message> {
    // Header
    let header = row![
        text("Connection History")
            .size(fonts.heading)
            .color(theme.text_primary),
        Space::new().width(Length::Fill),
        button(
            text("Clear History")
                .size(fonts.label)
                .color(theme.text_secondary),
        )
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

    // History entries grouped by day as a timeline
    let content: Element<'static, Message> = if history.entries.is_empty() {
        empty_state(theme, fonts)
    } else {
        // Group entries by date
        #[allow(clippy::type_complexity)]
        let mut day_groups: Vec<(String, String, Vec<(usize, &HistoryEntry)>)> = Vec::new();
        let mut current_key: Option<String> = None;

        for (idx, entry) in history.entries.iter().enumerate() {
            let key = date_key(entry.connected_at);
            let label = format_relative_date(entry.connected_at);

            if current_key.as_deref() != Some(key.as_str()) {
                day_groups.push((key.clone(), label, vec![(idx, entry)]));
                current_key = Some(key);
            } else if let Some((_, _, entries)) = day_groups.last_mut() {
                entries.push((idx, entry));
            }
        }

        let mut main_column = Column::new()
            .spacing(0)
            .padding(iced::Padding::new(24.0).top(0.0));

        let day_count = day_groups.len();
        for (i, (_key, day_label, day_entries)) in day_groups.into_iter().enumerate() {
            let has_next_day = i < day_count - 1;
            let day_section = build_day_section(
                day_label,
                day_entries,
                hosts_config,
                theme,
                fonts,
                focus_section,
                focus_index,
                has_next_day,
            );
            main_column = main_column.push(day_section);
        }

        scrollable(main_column).height(Fill).width(Fill).into()
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

/// Build a complete day section with timeline
#[allow(clippy::too_many_arguments)]
fn build_day_section(
    day_label: String,
    entries: Vec<(usize, &HistoryEntry)>,
    hosts_config: &HostsConfig,
    theme: Theme,
    fonts: ScaledFonts,
    focus_section: FocusSection,
    focus_index: Option<usize>,
    has_next_day: bool,
) -> Element<'static, Message> {
    let line_color = theme.border;

    // Day header: dot + label
    let header_dot = container(Space::new().width(12).height(12))
        .width(12)
        .height(12)
        .style(move |_theme| container::Style {
            background: Some(theme.text_secondary.into()),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let header = row![
        container(header_dot).width(48).align_x(Alignment::Center),
        text(day_label).size(fonts.body).color(theme.text_primary),
    ]
    .align_y(Alignment::Center)
    .padding(iced::Padding::new(0.0).bottom(12.0));

    // Build entries column
    let mut entries_col = Column::new().spacing(8);
    for (idx, entry) in entries {
        let is_focused = focus_section == FocusSection::Content && focus_index == Some(idx);
        let card = build_entry_card(entry, hosts_config, theme, fonts, is_focused);
        entries_col = entries_col.push(card);
    }

    // Vertical line that runs alongside entries (and continues to next day if exists)
    let line_height = if has_next_day {
        Length::Fill
    } else {
        Length::Shrink
    };

    let timeline_line = container(Space::new().width(2).height(line_height))
        .width(2)
        .height(line_height)
        .style(move |_theme| container::Style {
            background: Some(line_color.into()),
            ..Default::default()
        });

    // Entries row: line on left, cards on right
    let entries_row = row![
        container(timeline_line)
            .width(48)
            .align_x(Alignment::Center),
        entries_col,
    ]
    .width(Fill);

    // Add extra spacing after entries before next day section
    let bottom_spacing = if has_next_day { 24 } else { 0 };

    column![header, entries_row]
        .spacing(0)
        .padding(iced::Padding::new(0.0).bottom(bottom_spacing as f32))
        .into()
}

/// Build a single entry card (no timeline elements)
fn build_entry_card(
    entry: &HistoryEntry,
    hosts_config: &HostsConfig,
    theme: Theme,
    fonts: ScaledFonts,
    is_focused: bool,
) -> Element<'static, Message> {
    let entry_id = entry.id;

    // Session type icon
    let fallback_icon_data = match entry.session_type {
        SessionType::Ssh => icons::ui::TERMINAL,
        SessionType::Sftp => icons::ui::HARD_DRIVE,
        SessionType::Local => icons::ui::TERMINAL,
    };

    let (icon_data, icon_bg) = if entry.host_id.is_nil() {
        (fallback_icon_data, theme.selected)
    } else if let Some(host) = hosts_config.find_host(entry.host_id) {
        if let Some(detected_os) = host.detected_os.as_ref() {
            let (r, g, b) = detected_os.icon_color();
            (
                os_icon_data(&host.detected_os),
                iced::Color::from_rgba8(r, g, b, 0.85),
            )
        } else {
            (fallback_icon_data, theme.selected)
        }
    } else {
        (fallback_icon_data, theme.selected)
    };

    let type_text = entry.session_type.display_name().to_string();
    let time_str = entry
        .connected_at
        .with_timezone(&Local)
        .format("%H:%M")
        .to_string();
    let duration_str = entry.duration_string();

    let host_name = entry.host_name.clone();
    let username = entry.username.clone();
    let hostname = entry.hostname.clone();

    let info = column![
        row![
            text(host_name).size(fonts.body).color(theme.text_primary),
            Space::new().width(8),
            container(
                text(type_text)
                    .size(fonts.mono_tiny)
                    .color(theme.text_secondary),
            )
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
        text(format!("{}@{} | {}", username, hostname, duration_str))
            .size(fonts.label)
            .color(theme.text_muted),
        text(time_str).size(fonts.label).color(theme.text_muted),
    ]
    .spacing(4);

    let icon_widget = container(icon_with_color(icon_data, 18, iced::Color::WHITE))
        .width(36)
        .height(36)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_theme| container::Style {
            background: Some(icon_bg.into()),
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let reconnect_btn = button(
        row![
            icon_with_color(icons::ui::REFRESH, 12, theme.text_primary),
            text("Reconnect")
                .size(fonts.label)
                .color(theme.text_primary),
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
        .max_width(600)
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
fn empty_state(theme: Theme, fonts: ScaledFonts) -> Element<'static, Message> {
    let content = column![
        icon_with_color(icons::ui::HISTORY, 48, theme.text_muted),
        text("No connection history")
            .size(fonts.heading)
            .color(theme.text_primary),
        text("Your recent connections will appear here")
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
