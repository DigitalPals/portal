//! Tab bar component for managing multiple sessions

use iced::widget::{button, container, row, text, Row};
use iced::{Alignment, Element, Length, Padding};
use uuid::Uuid;

use crate::app::FocusSection;
use crate::icons::{self, icon_with_color};
use crate::message::{Message, TabMessage};
use crate::theme::Theme;

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
    FileViewer,
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

    pub fn new_file_viewer(id: Uuid, title: String) -> Self {
        Self {
            id,
            title,
            tab_type: TabType::FileViewer,
        }
    }
}

/// Build the tab bar view
pub fn tab_bar_view<'a>(
    tabs: &'a [Tab],
    active_tab: Option<Uuid>,
    theme: Theme,
    focus_section: FocusSection,
    focus_index: usize,
) -> Element<'a, Message> {
    let mut tab_elements: Vec<Element<'a, Message>> = Vec::new();

    for (idx, tab) in tabs.iter().enumerate() {
        let is_active = active_tab == Some(tab.id);
        let is_focused = focus_section == FocusSection::TabBar && idx == focus_index;
        tab_elements.push(tab_button(tab, is_active, is_focused, theme));
    }

    // Add "+" button for new connection
    tab_elements.push(new_tab_button(theme));

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

/// Single tab button
fn tab_button(tab: &Tab, is_active: bool, is_focused: bool, theme: Theme) -> Element<'_, Message> {
    let tab_id = tab.id;

    // Tab icon based on type (SVG icons)
    let icon_data = match tab.tab_type {
        TabType::Terminal => icons::ui::TERMINAL,
        TabType::Sftp => icons::ui::FOLDER_CLOSED,
        TabType::FileViewer => icons::files::FILE_TEXT,
    };
    let icon_color = if is_active || is_focused {
        theme.text_primary
    } else {
        theme.text_secondary
    };
    let icon = icon_with_color(icon_data, 12, icon_color);

    // Truncate title if too long
    let title = if tab.title.len() > 20 {
        format!("{}...", &tab.title[..17])
    } else {
        tab.title.clone()
    };

    let content = row![
        icon,
        text(title).size(13).color(if is_active || is_focused {
            theme.text_primary
        } else {
            theme.text_secondary
        }),
        // Close button
        button(text("×").size(15).color(theme.text_muted))
            .style(move |_theme, status| {
                let text_color = match status {
                    iced::widget::button::Status::Hovered => theme.text_primary,
                    _ => theme.text_muted,
                };
                iced::widget::button::Style {
                    background: None,
                    text_color,
                    ..Default::default()
                }
            })
            .padding(0)
            .on_press(Message::Tab(TabMessage::Close(tab_id))),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    let bg_color = if is_active {
        theme.background
    } else if is_focused {
        theme.hover
    } else {
        theme.surface
    };

    button(container(content).padding(Padding::new(7.0).left(11.0).right(6.0)))
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered if !is_active => theme.hover,
                _ => bg_color,
            };
            // Focus ring border
            let border_color = if is_focused {
                theme.focus_ring
            } else if is_active {
                theme.accent
            } else {
                theme.border
            };
            let border_width = if is_focused { 2.0 } else { 0.0 };
            iced::widget::button::Style {
                background: Some(background.into()),
                text_color: theme.text_primary,
                border: iced::Border {
                    color: border_color,
                    width: border_width,
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
        .on_press(Message::Tab(TabMessage::Select(tab_id)))
        .into()
}

/// New tab "+" button
fn new_tab_button(theme: Theme) -> Element<'static, Message> {
    button(
        container(text("+").size(17).color(theme.text_secondary))
            .padding(Padding::new(5.0).left(10.0).right(10.0)),
    )
    .style(move |_theme, status| {
        let background = match status {
            iced::widget::button::Status::Hovered => Some(theme.hover.into()),
            _ => None,
        };
        iced::widget::button::Style {
            background,
            text_color: theme.text_secondary,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .padding(0)
    .on_press(Message::Tab(TabMessage::New))
    .into()
}
