use iced::widget::{Column, Row, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::app::managers::{ProxySessionsState, TerminalPreviewHandle};
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
pub const SESSION_CARD_WIDTH: f32 = 340.0;

pub fn calculate_columns(window_width: f32, sidebar_state: SidebarState) -> usize {
    let sidebar_width = match sidebar_state {
        SidebarState::Hidden => 0.0,
        SidebarState::IconsOnly => SIDEBAR_WIDTH_COLLAPSED,
        SidebarState::Expanded => SIDEBAR_WIDTH,
    };

    let content_width = window_width - sidebar_width - GRID_PADDING;
    let columns = grid_columns_for_width(content_width, SESSION_CARD_WIDTH);

    columns.clamp(1, 4)
}

fn grid_columns_for_width(content_width: f32, min_card_width: f32) -> usize {
    if !content_width.is_finite() || !min_card_width.is_finite() || min_card_width <= 0.0 {
        return 1;
    }

    ((content_width.max(0.0) + GRID_SPACING) / (min_card_width + GRID_SPACING)).floor() as usize
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
        content = content.push(text("No active Portal Hub sessions").color(theme.text_muted));
    } else {
        content = content.push(session_grid(
            &state.sessions,
            state.kill_requested,
            column_count,
            theme,
            fonts,
        ));
    }

    scrollable(container(content).width(Fill))
        .height(Fill)
        .into()
}

fn session_grid<'a>(
    sessions: &'a [ProxySessionCard],
    kill_requested: Option<Uuid>,
    column_count: usize,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    let mut current_row: Vec<Element<'a, Message>> = Vec::new();

    for session in sessions {
        current_row.push(session_card(
            session,
            kill_requested == Some(session.session_id),
            theme,
            fonts,
        ));

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
            current_row.push(Space::new().width(Length::Fixed(SESSION_CARD_WIDTH)).into());
        }
        rows.push(Row::with_children(current_row).spacing(GRID_SPACING).into());
    }

    Column::with_children(rows).spacing(GRID_SPACING).into()
}

fn session_card<'a>(
    session: &'a ProxySessionCard,
    kill_requested: bool,
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

    let preview = terminal_thumbnail(session.terminal.term(), theme);

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

    let resume_card = column![preview, meta].spacing(10);
    let resume_area = button(container(resume_card).padding(12).width(Fill))
        .padding(0)
        .width(Fill)
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                _ => Some(iced::Color::TRANSPARENT.into()),
            };
            iced::widget::button::Style {
                background,
                text_color: theme.text_primary,
                border: iced::Border {
                    color: iced::Color::TRANSPARENT,
                    width: 0.0,
                    radius: BORDER_RADIUS.into(),
                },
                ..Default::default()
            }
        })
        .on_press(Message::ProxySessions(ProxySessionsMessage::Resume(
            session.session_id,
        )));

    let actions = if kill_requested {
        row![
            Space::new().width(Fill),
            small_session_button("Cancel", theme, fonts)
                .on_press(Message::ProxySessions(ProxySessionsMessage::KillCanceled,)),
            destructive_session_button("Kill session", theme, fonts).on_press(
                Message::ProxySessions(ProxySessionsMessage::KillConfirmed(session.session_id)),
            ),
        ]
    } else {
        row![
            Space::new().width(Fill),
            small_session_button("Kill", theme, fonts).on_press(Message::ProxySessions(
                ProxySessionsMessage::KillRequested(session.session_id),
            )),
        ]
    }
    .spacing(8)
    .align_y(Alignment::Center);
    let actions = container(actions)
        .padding(Padding::new(12.0).top(0.0))
        .width(Fill);

    let card = column![resume_area, actions].spacing(0);
    container(card)
        .width(Length::Fixed(SESSION_CARD_WIDTH))
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        })
        .into()
}

fn small_session_button<'a>(
    label: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
) -> iced::widget::Button<'a, Message> {
    button(text(label).size(fonts.label).color(theme.text_primary))
        .padding([6, 10])
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                _ => Some(theme.background.into()),
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
}

fn destructive_session_button<'a>(
    label: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
) -> iced::widget::Button<'a, Message> {
    let danger = iced::Color::from_rgb(0.86, 0.31, 0.31);
    button(text(label).size(fonts.label))
        .padding([6, 10])
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered => iced::Color::from_rgb8(0xf3, 0x8b, 0xa8),
                _ => danger,
            };
            iced::widget::button::Style {
                background: Some(background.into()),
                text_color: theme.text_on(background),
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
}

pub fn terminal_thumbnail(term: TerminalPreviewHandle, theme: Theme) -> Element<'static, Message> {
    let terminal = TerminalWidget::new(term, |_| Message::Noop)
        .font_size(THUMBNAIL_FONT_SIZE)
        .font(TerminalFont::default())
        .metric_adjustments(TerminalMetricAdjustments::default())
        .keybindings(KeybindingsConfig::default())
        .terminal_colors(theme.terminal);

    container(terminal)
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
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::calculate_columns;
    use crate::app::SidebarState;

    #[test]
    fn calculate_columns_handles_non_finite_width() {
        assert_eq!(calculate_columns(f32::NAN, SidebarState::Hidden), 1);
        assert_eq!(calculate_columns(f32::INFINITY, SidebarState::Hidden), 1);
    }
}
