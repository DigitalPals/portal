//! Tab bar component for managing multiple sessions

use iced::widget::{button, container, row, text, Row};
use iced::{Alignment, Element, Length, Padding};
use uuid::Uuid;

use crate::message::Message;
use crate::theme::THEME;

/// Represents a single tab
#[derive(Debug, Clone)]
pub struct Tab {
    pub id: Uuid,
    pub title: String,
    pub tab_type: TabType,
}

/// Type of content in a tab
#[derive(Debug, Clone, PartialEq)]
pub enum TabType {
    Terminal,
    Sftp,
}

impl Tab {
    pub fn new_terminal(id: Uuid, title: String) -> Self {
        Self {
            id,
            title,
            tab_type: TabType::Terminal,
        }
    }

    pub fn new_sftp(id: Uuid, title: String) -> Self {
        Self {
            id,
            title,
            tab_type: TabType::Sftp,
        }
    }
}

/// Build the tab bar view
pub fn tab_bar_view<'a>(
    tabs: &'a [Tab],
    active_tab: Option<Uuid>,
) -> Element<'a, Message> {
    let mut tab_elements: Vec<Element<'a, Message>> = Vec::new();

    for tab in tabs {
        let is_active = active_tab == Some(tab.id);
        tab_elements.push(tab_button(tab, is_active));
    }

    // Add "+" button for new connection
    tab_elements.push(new_tab_button());

    let tabs_row = Row::with_children(tab_elements)
        .spacing(2)
        .align_y(Alignment::Center);

    container(
        row![
            // Left side: tabs
            tabs_row,
            // Right side: spacer
            container(text("")).width(Length::Fill),
        ]
        .align_y(Alignment::Center)
        .padding(Padding::new(5.0).left(8.0).right(8.0)),
    )
    .width(Length::Fill)
    .style(|_theme| container::Style {
        background: Some(THEME.surface.into()),
        border: iced::Border {
            color: THEME.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..Default::default()
    })
    .into()
}

/// Single tab button
fn tab_button(tab: &Tab, is_active: bool) -> Element<'_, Message> {
    let tab_id = tab.id;

    // Tab icon based on type
    let icon = match tab.tab_type {
        TabType::Terminal => "●",
        TabType::Sftp => "📁",
    };

    // Truncate title if too long
    let title = if tab.title.len() > 20 {
        format!("{}...", &tab.title[..17])
    } else {
        tab.title.clone()
    };

    let content = row![
        text(icon).size(11),
        text(title).size(13).color(if is_active {
            THEME.text_primary
        } else {
            THEME.text_secondary
        }),
        // Close button
        button(text("×").size(15).color(THEME.text_muted))
            .style(|_theme, status| {
                let text_color = match status {
                    iced::widget::button::Status::Hovered => THEME.text_primary,
                    _ => THEME.text_muted,
                };
                iced::widget::button::Style {
                    background: None,
                    text_color,
                    ..Default::default()
                }
            })
            .padding(0)
            .on_press(Message::TabClose(tab_id)),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let bg_color = if is_active {
        THEME.background
    } else {
        THEME.surface
    };

    button(container(content).padding(Padding::new(7.0).left(11.0).right(6.0)))
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered if !is_active => THEME.hover,
                _ => bg_color,
            };
            iced::widget::button::Style {
                background: Some(background.into()),
                text_color: THEME.text_primary,
                border: iced::Border {
                    color: if is_active { THEME.accent } else { THEME.border },
                    width: 0.0,
                    radius: iced::border::Radius {
                        top_left: 4.0,
                        top_right: 4.0,
                        bottom_left: 0.0,
                        bottom_right: 0.0,
                    },
                },
                ..Default::default()
            }
        })
        .padding(0)
        .on_press(Message::TabSelect(tab_id))
        .into()
}

/// New tab "+" button
fn new_tab_button() -> Element<'static, Message> {
    button(
        container(text("+").size(17).color(THEME.text_secondary))
            .padding(Padding::new(5.0).left(10.0).right(10.0)),
    )
    .style(|_theme, status| {
        let background = match status {
            iced::widget::button::Status::Hovered => Some(THEME.hover.into()),
            _ => None,
        };
        iced::widget::button::Style {
            background,
            text_color: THEME.text_secondary,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .padding(0)
    .on_press(Message::TabNew)
    .into()
}
