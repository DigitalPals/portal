//! Tab bar component for managing multiple sessions

use iced::widget::{button, container, mouse_area, row, text, Row};
use iced::{Alignment, Color, Element, Length, Padding};
use uuid::Uuid;

use crate::app::{FocusSection, SidebarState, View};
use crate::config::HostsConfig;
use crate::icons::{self, icon_with_color};
use crate::message::{Message, TabMessage, UiMessage};
use crate::theme::Theme;
use crate::views::host_grid::os_icon_data;

/// Represents a single tab
#[derive(Debug, Clone)]
pub struct Tab {
    pub id: Uuid,
    pub title: String,
    pub tab_type: TabType,
    /// Host ID for looking up detected_os (None for local terminal)
    pub host_id: Option<Uuid>,
}

/// Type of content in a tab
#[derive(Debug, Clone, PartialEq)]
pub enum TabType {
    Terminal,
    Sftp,
    FileViewer,
}

impl Tab {
    pub fn new_terminal(id: Uuid, title: String, host_id: Option<Uuid>) -> Self {
        Self {
            id,
            title,
            tab_type: TabType::Terminal,
            host_id,
        }
    }

    pub fn new_sftp(id: Uuid, title: String, host_id: Option<Uuid>) -> Self {
        Self {
            id,
            title,
            tab_type: TabType::Sftp,
            host_id,
        }
    }

    pub fn new_file_viewer(id: Uuid, title: String) -> Self {
        Self {
            id,
            title,
            tab_type: TabType::FileViewer,
            host_id: None,
        }
    }
}

/// Build the tab bar view
#[allow(clippy::too_many_arguments)]
pub fn tab_bar_view<'a>(
    tabs: &'a [Tab],
    active_tab: Option<Uuid>,
    _sidebar_state: SidebarState,
    theme: Theme,
    focus_section: FocusSection,
    focus_index: usize,
    active_view: &View,
    hosts_config: &'a HostsConfig,
    hovered_tab: Option<Uuid>,
) -> Element<'a, Message> {
    // Determine if we should use terminal background (seamless look)
    let use_terminal_bg = matches!(active_view, View::Terminal(_) | View::DualSftp(_) | View::FileViewer(_));
    // Hamburger menu button for sidebar toggle
    let menu_icon = icons::ui::MENU;

    let hamburger_btn = button(
        container(icon_with_color(menu_icon, 20, theme.text_secondary))
            .padding(Padding::new(10.0)),
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
    .on_press(Message::Ui(UiMessage::SidebarToggleCollapse));

    let mut tab_elements: Vec<Element<'a, Message>> = Vec::new();

    for (idx, tab) in tabs.iter().enumerate() {
        let is_active = active_tab == Some(tab.id);
        let is_focused = focus_section == FocusSection::TabBar && idx == focus_index;
        let is_hovered = hovered_tab == Some(tab.id);
        tab_elements.push(tab_button(tab, is_active, is_focused, is_hovered, theme, hosts_config));
    }

    // Add "+" button for new connection
    tab_elements.push(new_tab_button(theme));

    let tabs_row = Row::with_children(tab_elements)
        .spacing(4)
        .align_y(Alignment::Center);

    container(
        row![
            // Left side: hamburger menu
            hamburger_btn,
            // Center: tabs
            container(tabs_row).padding(Padding::new(0.0).left(8.0)),
            // Right side: spacer
            container(text("")).width(Length::Fill),
        ]
        .spacing(4)
        .align_y(Alignment::Center)
        .padding(Padding::new(8.0).left(10.0).right(10.0)),
    )
    .width(Length::Fill)
    .style(move |_theme| {
        let bg_color = if use_terminal_bg {
            theme.terminal.background
        } else {
            theme.tab_bar
        };
        container::Style {
            background: Some(bg_color.into()),
            border: iced::Border::default(),
            ..Default::default()
        }
    })
    .into()
}

/// Single tab button
fn tab_button<'a>(
    tab: &'a Tab,
    is_active: bool,
    is_focused: bool,
    is_hovered: bool,
    theme: Theme,
    hosts_config: &'a HostsConfig,
) -> Element<'a, Message> {
    let tab_id = tab.id;

    // Colors based on active state
    let text_icon_color = if is_active {
        Color::from_rgb8(0xCD, 0xD6, 0xF4) // #CDD6F4 - active
    } else {
        Color::from_rgb8(0x77, 0x77, 0x90) // #777790 - inactive
    };

    // Get icon - use distro icon if host_id is set and OS is detected
    let icon_data = if let Some(host_id) = tab.host_id {
        if let Some(host) = hosts_config.find_host(host_id) {
            if host.detected_os.is_some() {
                os_icon_data(&host.detected_os)
            } else {
                // Fallback to terminal icon if no detected OS
                icons::ui::TERMINAL
            }
        } else {
            icons::ui::TERMINAL
        }
    } else {
        // No host_id - use type-based icon
        match tab.tab_type {
            TabType::Terminal => icons::ui::TERMINAL,
            TabType::Sftp => icons::ui::FOLDER_CLOSED,
            TabType::FileViewer => icons::files::FILE_TEXT,
        }
    };
    let icon = icon_with_color(icon_data, 14, text_icon_color);

    // Truncate title if too long
    let title = if tab.title.len() > 20 {
        format!("{}...", &tab.title[..17])
    } else {
        tab.title.clone()
    };

    // Close button - only show when hovered (always reserve space to prevent resize)
    let close_button_width = 16.0;
    let close_button: Element<'_, Message> = if is_hovered {
        container(
            button(text("×").size(16).color(text_icon_color))
                .style(move |_theme, status| {
                    let text_color = match status {
                        iced::widget::button::Status::Hovered => Color::from_rgb8(0xCD, 0xD6, 0xF4),
                        _ => text_icon_color,
                    };
                    iced::widget::button::Style {
                        background: None,
                        text_color,
                        ..Default::default()
                    }
                })
                .padding(0)
                .on_press(Message::Tab(TabMessage::Close(tab_id))),
        )
        .width(close_button_width)
        .align_x(Alignment::Center)
        .into()
    } else {
        // Empty spacer with same width to prevent resize on hover
        container(text("")).width(close_button_width).into()
    };

    let content = row![
        icon,
        text(title).size(14).color(text_icon_color),
        close_button,
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    // Background colors
    let bg_color = if is_active {
        Color::from_rgb8(0x41, 0x43, 0x55) // #414355 - active
    } else {
        Color::from_rgb8(0x27, 0x27, 0x38) // #272738 - inactive
    };

    let tab_button = button(container(content).padding(Padding::new(6.0).left(14.0).right(8.0)))
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered if !is_active => {
                    Color::from_rgb8(0x35, 0x35, 0x48) // Slightly lighter on hover
                }
                _ => bg_color,
            };
            // Focus ring border
            let border_color = if is_focused {
                theme.focus_ring
            } else {
                Color::TRANSPARENT
            };
            let border_width = if is_focused { 2.0 } else { 0.0 };
            iced::widget::button::Style {
                background: Some(background.into()),
                text_color: text_icon_color,
                border: iced::Border {
                    color: border_color,
                    width: border_width,
                    radius: 12.0.into(),
                },
                ..Default::default()
            }
        })
        .padding(0)
        .on_press(Message::Tab(TabMessage::Select(tab_id)));

    // Wrap in mouse_area for hover detection
    mouse_area(tab_button)
        .on_enter(Message::Tab(TabMessage::Hover(Some(tab_id))))
        .on_exit(Message::Tab(TabMessage::Hover(None)))
        .into()
}

/// New tab "+" button
fn new_tab_button(theme: Theme) -> Element<'static, Message> {
    button(
        container(text("+").size(18).color(theme.text_secondary))
            .padding(Padding::new(7.0).left(12.0).right(12.0)),
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
