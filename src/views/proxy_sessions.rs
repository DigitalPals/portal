use iced::widget::{Column, Row, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Fill, Length, Padding};

use crate::app::managers::ProxySessionsState;
use crate::app::{SidebarState, managers::ProxySessionCard};
use crate::config::settings::TerminalMetricAdjustments;
use crate::fonts::TerminalFont;
use crate::icons::{self, icon_with_color};
use crate::keybindings::KeybindingsConfig;
use crate::message::{Message, ProxySessionsMessage};
use crate::terminal::widget::TerminalWidget;
use crate::theme::{
    BORDER_RADIUS, GRID_PADDING, GRID_SPACING, SIDEBAR_WIDTH, SIDEBAR_WIDTH_COLLAPSED, ScaledFonts,
    Theme,
};

const THUMBNAIL_HEIGHT: f32 = 150.0;
const THUMBNAIL_FONT_SIZE: f32 = 5.0;
const MIN_SESSION_CARD_WIDTH: f32 = 340.0;

pub fn calculate_columns(window_width: f32, sidebar_state: SidebarState) -> usize {
    let sidebar_width = match sidebar_state {
        SidebarState::Hidden => 0.0,
        SidebarState::IconsOnly => SIDEBAR_WIDTH_COLLAPSED,
        SidebarState::Expanded => SIDEBAR_WIDTH,
    };

    let content_width = window_width - sidebar_width - GRID_PADDING;
    let columns =
        ((content_width + GRID_SPACING) / (MIN_SESSION_CARD_WIDTH + GRID_SPACING)).floor() as usize;

    columns.clamp(1, 4)
}

pub fn proxy_sessions_view<'a>(
    state: &'a ProxySessionsState,
    column_count: usize,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let refresh_status = state
        .last_loaded_at
        .map(|time| {
            format!(
                "Auto-refreshes every 3s - Refreshed {}",
                time.with_timezone(&chrono::Local).format("%H:%M:%S")
            )
        })
        .unwrap_or_else(|| "Auto-refreshes every 3s".to_string());

    let refresh = button(row![
        icon_with_color(icons::ui::REFRESH, 14, theme.text_primary),
        text("Refresh").size(fonts.label).color(theme.text_primary)
    ])
    .padding([8, 10])
    .on_press(Message::ProxySessions(ProxySessionsMessage::Refresh));

    let header = row![
        column![
            text("Sessions")
                .size(fonts.page_title)
                .color(theme.text_primary),
            text(refresh_status)
                .size(fonts.label)
                .color(theme.text_muted),
        ]
        .spacing(4),
        Space::new().width(Fill),
        refresh,
    ]
    .align_y(Alignment::Center);

    let mut content = Column::new()
        .spacing(16)
        .padding(Padding::new(24.0).top(16.0).bottom(24.0))
        .push(header);

    if state.loading && state.sessions.is_empty() {
        content = content.push(text("Loading sessions...").color(theme.text_muted));
    } else if let Some(error) = &state.error {
        content = content.push(
            container(
                text(error.clone())
                    .size(fonts.body)
                    .color(theme.text_secondary),
            )
            .padding(14)
            .width(Fill),
        );
    } else if state.sessions.is_empty() {
        content = content.push(text("No active Portal Proxy sessions").color(theme.text_muted));
    } else {
        content = content.push(session_grid(&state.sessions, column_count, theme, fonts));
    }

    scrollable(container(content).width(Fill))
        .height(Fill)
        .into()
}

fn session_grid<'a>(
    sessions: &'a [ProxySessionCard],
    column_count: usize,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let mut current_row: Vec<Element<'a, Message>> = Vec::new();

    for session in sessions {
        current_row.push(session_card(session, theme, fonts));

        if current_row.len() >= column_count {
            rows.push(
                Row::with_children(std::mem::take(&mut current_row))
                    .spacing(GRID_SPACING)
                    .into(),
            );
        }
    }

    if !current_row.is_empty() {
        while current_row.len() < column_count {
            current_row.push(
                Space::new()
                    .width(Length::Fixed(MIN_SESSION_CARD_WIDTH))
                    .into(),
            );
        }
        rows.push(Row::with_children(current_row).spacing(GRID_SPACING).into());
    }

    Column::with_children(rows).spacing(GRID_SPACING).into()
}

fn session_card<'a>(
    session: &'a ProxySessionCard,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let title = session.display_name.clone();
    let subtitle = format!(
        "Session started at {}",
        session
            .created_at
            .with_timezone(&chrono::Local)
            .format("%H:%M:%S")
    );
    let truncated = if session.preview_truncated {
        "Preview truncated"
    } else {
        ""
    };

    let terminal = TerminalWidget::new(session.terminal.term(), |_| Message::Noop)
        .font_size(THUMBNAIL_FONT_SIZE)
        .font(TerminalFont::default())
        .metric_adjustments(TerminalMetricAdjustments::default())
        .keybindings(KeybindingsConfig::default())
        .terminal_colors(theme.terminal);

    let preview = container(terminal)
        .height(Length::Fixed(THUMBNAIL_HEIGHT))
        .width(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.terminal.background.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        });

    let meta = row![
        column![
            text(title).size(fonts.body).color(theme.text_primary),
            text(subtitle).size(fonts.label).color(theme.text_muted),
        ]
        .spacing(3),
        Space::new().width(Fill),
        text(truncated).size(fonts.label).color(theme.text_muted),
    ]
    .align_y(Alignment::Center);

    let card = column![preview, meta].spacing(10);
    button(container(card).padding(12).width(Fill))
        .padding(0)
        .width(Length::Fixed(MIN_SESSION_CARD_WIDTH))
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                _ => Some(theme.surface.into()),
            };
            iced::widget::button::Style {
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
        .on_press(Message::ProxySessions(ProxySessionsMessage::Resume(
            session.session_id,
        )))
        .into()
}
