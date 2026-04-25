//! Dialog for choosing between existing sessions and a new host session.

use chrono::{DateTime, Utc};
use iced::widget::{Space, button, column, container, row, scrollable, text};
use iced::{Alignment, Element, Length};
use uuid::Uuid;

use crate::icons::{self, icon_with_color};
use crate::message::{DialogMessage, HostMessage, Message, SessionId};
use crate::proxy::ListedProxySession;
use crate::theme::{BORDER_RADIUS, Theme};

use super::common::{dialog_backdrop, primary_button_style, secondary_button_style};

#[derive(Debug, Clone)]
pub struct LocalSessionChoice {
    pub session_id: SessionId,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct DetachedProxySessionChoice {
    pub session: ListedProxySession,
    pub display_name: String,
}

#[derive(Debug, Clone)]
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
        for session in &state.local_sessions {
            rows.push(session_row(
                icons::ui::TERMINAL,
                session.title.clone(),
                "Open tab".to_string(),
                Message::Host(HostMessage::OpenExistingSession(session.session_id)),
                theme,
            ));
        }
    }

    if !state.proxy_sessions.is_empty() {
        rows.push(section_label("Detached Portal Proxy sessions", theme));
        for choice in &state.proxy_sessions {
            rows.push(session_row(
                icons::ui::SERVER,
                choice.display_name.clone(),
                proxy_detail(&choice.session),
                Message::Host(HostMessage::OpenDetachedProxySession(
                    choice.session.session_id,
                )),
                theme,
            ));
        }
    }

    if state.proxy_loading {
        rows.push(status_row("Loading Portal Proxy sessions...", theme));
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
    .padding(24)
    .width(Length::Fixed(460.0));

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

fn session_row(
    icon: &'static [u8],
    title: String,
    detail: String,
    on_press: Message,
    theme: Theme,
) -> Element<'static, Message> {
    button(
        row![
            icon_with_color(icon, 18, theme.accent),
            column![
                text(title).size(14).color(theme.text_primary),
                text(detail).size(12).color(theme.text_secondary),
            ]
            .spacing(2)
            .width(Length::Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([10, 12])
    .width(Length::Fill)
    .style(secondary_button_style(theme))
    .on_press(on_press)
    .into()
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
