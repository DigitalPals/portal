//! Dialog for choosing between existing sessions and a new host session.

use chrono::{DateTime, Utc};
use iced::widget::{Column, Row, Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Fill, Length};
use uuid::Uuid;

use crate::app::managers::TerminalPreviewHandle;
use crate::icons::{self, icon_with_color};
use crate::message::{DialogMessage, HostMessage, Message, SessionId};
use crate::proxy::ListedProxySession;
use crate::theme::{BORDER_RADIUS, GRID_SPACING, Theme};
use crate::views::proxy_sessions::{SESSION_CARD_WIDTH, terminal_thumbnail};
use crate::views::terminal_view::TerminalSession;

use super::common::{dialog_backdrop, primary_button_style, secondary_button_style};

const SESSION_CHOICE_COLUMNS: usize = 2;
const DIALOG_HORIZONTAL_PADDING: f32 = 24.0;
const SESSION_CHOICE_DIALOG_WIDTH: f32 = SESSION_CARD_WIDTH * SESSION_CHOICE_COLUMNS as f32
    + GRID_SPACING * (SESSION_CHOICE_COLUMNS as f32 - 1.0)
    + DIALOG_HORIZONTAL_PADDING * 2.0;

#[derive(Clone)]
pub struct SessionThumbnail {
    term: TerminalPreviewHandle,
}

impl SessionThumbnail {
    pub fn from_terminal(term: TerminalPreviewHandle) -> Self {
        Self { term }
    }

    pub fn from_preview(title: impl Into<String>, preview: &[u8]) -> Self {
        let (terminal, _events) = TerminalSession::new(title);
        if !preview.is_empty() {
            terminal.process_output(preview);
        }

        Self {
            term: terminal.term(),
        }
    }

    fn term(&self) -> TerminalPreviewHandle {
        self.term.clone()
    }
}

#[derive(Clone)]
pub struct LocalSessionChoice {
    pub session_id: SessionId,
    pub title: String,
    pub thumbnail: SessionThumbnail,
}

#[derive(Clone)]
pub struct DetachedProxySessionChoice {
    pub session: ListedProxySession,
    pub display_name: String,
    pub thumbnail: SessionThumbnail,
}

#[derive(Clone)]
pub struct SessionChoiceDialogState {
    pub host_id: Uuid,
    pub host_name: String,
    pub local_sessions: Vec<LocalSessionChoice>,
    pub proxy_sessions: Vec<DetachedProxySessionChoice>,
    pub proxy_loading: bool,
    pub proxy_error: Option<String>,
}

impl SessionChoiceDialogState {
    pub fn new(
        host_id: Uuid,
        host_name: String,
        local_sessions: Vec<LocalSessionChoice>,
        proxy_loading: bool,
    ) -> Self {
        Self {
            host_id,
            host_name,
            local_sessions,
            proxy_sessions: Vec::new(),
            proxy_loading,
            proxy_error: None,
        }
    }

    pub fn has_choices(&self) -> bool {
        !self.local_sessions.is_empty() || !self.proxy_sessions.is_empty()
    }

    pub fn proxy_choice(&self, session_id: SessionId) -> Option<&DetachedProxySessionChoice> {
        self.proxy_sessions
            .iter()
            .find(|choice| choice.session.session_id == session_id)
    }
}

pub fn session_choice_dialog_view(
    state: &SessionChoiceDialogState,
    theme: Theme,
) -> Element<'static, Message> {
    let title = row![
        icon_with_color(icons::ui::TERMINAL, 24, theme.accent),
        text("Open Existing Session?")
            .size(20)
            .color(theme.text_primary),
    ]
    .spacing(12)
    .align_y(Alignment::Center);

    let subtitle = text(format!("{} already has active sessions", state.host_name))
        .size(14)
        .color(theme.text_secondary);

    let new_button = button(
        row![
            icon_with_color(icons::ui::PLUS, 16, theme.text_primary),
            text("Create New Session")
                .size(14)
                .color(theme.text_primary),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .padding([10, 14])
    .width(Length::Fill)
    .style(primary_button_style(theme))
    .on_press(Message::Host(HostMessage::CreateNewSession(state.host_id)));

    let mut rows: Vec<Element<'static, Message>> = Vec::new();

    if !state.local_sessions.is_empty() {
        rows.push(section_label("Open Portal tabs", theme));
        rows.push(session_grid(
            state
                .local_sessions
                .iter()
                .map(|session| {
                    session_card(
                        icons::ui::TERMINAL,
                        session.title.clone(),
                        "Open tab".to_string(),
                        session.thumbnail.clone(),
                        Message::Host(HostMessage::OpenExistingSession(session.session_id)),
                        theme,
                    )
                })
                .collect(),
        ));
    }

    if !state.proxy_sessions.is_empty() {
        rows.push(section_label("Detached Portal Hub sessions", theme));
        rows.push(session_grid(
            state
                .proxy_sessions
                .iter()
                .map(|choice| {
                    session_card(
                        icons::ui::SERVER,
                        choice.display_name.clone(),
                        proxy_detail(&choice.session),
                        choice.thumbnail.clone(),
                        Message::Host(HostMessage::OpenDetachedProxySession(
                            choice.session.session_id,
                        )),
                        theme,
                    )
                })
                .collect(),
        ));
    }

    if state.proxy_loading {
        rows.push(status_row("Loading Portal Hub sessions...", theme));
    }

    if let Some(error) = &state.proxy_error {
        rows.push(error_row(error.clone(), theme));
    }

    if rows.is_empty() {
        rows.push(status_row("No existing sessions found", theme));
    }

    let session_list = scrollable(column(rows).spacing(8))
        .height(Length::Fixed(260.0))
        .width(Length::Fill);

    let cancel_button = button(text("Cancel").size(14).color(theme.text_primary))
        .padding([8, 16])
        .style(secondary_button_style(theme))
        .on_press(Message::Dialog(DialogMessage::Close));

    let content = column![
        title,
        subtitle,
        Space::new().height(12),
        new_button,
        Space::new().height(8),
        session_list,
        Space::new().height(12),
        row![Space::new().width(Length::Fill), cancel_button].spacing(8),
    ]
    .spacing(4)
    .padding(DIALOG_HORIZONTAL_PADDING)
    .width(Length::Fixed(SESSION_CHOICE_DIALOG_WIDTH));

    dialog_backdrop(content, theme)
}

fn section_label(label: &'static str, theme: Theme) -> Element<'static, Message> {
    text(label).size(12).color(theme.text_muted).into()
}

fn status_row(label: &'static str, theme: Theme) -> Element<'static, Message> {
    container(text(label).size(13).color(theme.text_secondary))
        .padding([10, 12])
        .width(Length::Fill)
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

fn error_row(error: String, _theme: Theme) -> Element<'static, Message> {
    let error_color = iced::Color::from_rgb8(220, 80, 80);
    container(text(error).size(12).color(error_color))
        .padding([10, 12])
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            background: Some(iced::Color::from_rgba8(220, 80, 80, 0.1).into()),
            border: iced::Border {
                color: error_color,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        })
        .into()
}

fn session_card(
    icon: &'static [u8],
    title: String,
    detail: String,
    thumbnail: SessionThumbnail,
    on_press: Message,
    theme: Theme,
) -> Element<'static, Message> {
    let preview = terminal_thumbnail(thumbnail.term(), theme);
    let meta = row![
        row![
            icon_with_color(icon, 18, theme.accent),
            column![
                text(title).size(14).color(theme.text_primary),
                text(detail).size(12).color(theme.text_secondary),
            ]
            .spacing(2)
            .width(Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .width(Fill),
    ]
    .align_y(Alignment::Center);

    let card = column![preview, meta].spacing(10);
    button(container(card).padding(12).width(Fill))
        .padding(0)
        .width(Length::Fixed(SESSION_CARD_WIDTH))
        .style(secondary_button_style(theme))
        .on_press(on_press)
        .into()
}

fn session_grid(cards: Vec<Element<'static, Message>>) -> Element<'static, Message> {
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut current_row: Vec<Element<'static, Message>> = Vec::new();

    for card in cards {
        current_row.push(card);

        if current_row.len() >= SESSION_CHOICE_COLUMNS {
            rows.push(
                Row::with_children(std::mem::take(&mut current_row))
                    .spacing(GRID_SPACING)
                    .into(),
            );
        }
    }

    if !current_row.is_empty() {
        while current_row.len() < SESSION_CHOICE_COLUMNS {
            current_row.push(Space::new().width(Length::Fixed(SESSION_CARD_WIDTH)).into());
        }
        rows.push(Row::with_children(current_row).spacing(GRID_SPACING).into());
    }

    Column::with_children(rows).spacing(GRID_SPACING).into()
}

fn proxy_detail(session: &ListedProxySession) -> String {
    format!(
        "Detached proxy session - updated {}",
        format_time(session.updated_at)
    )
}

fn format_time(value: DateTime<Utc>) -> String {
    value.format("%Y-%m-%d %H:%M").to_string()
}
