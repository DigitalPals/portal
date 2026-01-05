use iced::widget::{button, column, container, row, scrollable, text, text_input, Column, Space};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::message::Message;
use crate::theme::{BORDER_RADIUS, SIDEBAR_WIDTH, THEME};

/// Placeholder host for the sidebar display
#[derive(Debug, Clone)]
pub struct SidebarHost {
    pub id: Uuid,
    pub name: String,
    pub hostname: String,
    pub folder: Option<String>,
}

/// Placeholder folder for the sidebar display
#[derive(Debug, Clone)]
pub struct SidebarFolder {
    pub id: Uuid,
    pub name: String,
    pub expanded: bool,
}

/// Build the sidebar view
pub fn sidebar_view(
    search_query: &str,
    folders: Vec<SidebarFolder>,
    hosts: Vec<SidebarHost>,
    selected_host: Option<Uuid>,
) -> Element<'static, Message> {
    // Header with title and add button
    let header = row![
        text("HOSTS").size(12).color(THEME.text_muted),
        Space::with_width(Length::Fill),
        button(text("+").size(16).color(THEME.text_primary))
            .style(|_theme, status| {
                let bg = match status {
                    button::Status::Hovered => Some(THEME.hover.into()),
                    _ => None,
                };
                button::Style {
                    background: bg,
                    text_color: THEME.text_primary,
                    border: iced::Border {
                        radius: BORDER_RADIUS.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .padding([2, 8])
            .on_press(Message::HostAdd),
    ]
    .align_y(Alignment::Center)
    .padding(Padding::new(12.0).top(16.0).bottom(8.0));

    let search_input = text_input("Search hosts...", search_query)
        .on_input(Message::SearchChanged)
        .padding(8)
        .width(Length::Fill);

    let search_container = container(search_input)
        .padding(Padding::new(12.0).bottom(8.0))
        .width(Length::Fill);

    // Build host list
    let mut host_list = Column::new().spacing(2);

    // Group hosts by folder
    for folder in folders {
        // Folder header
        let folder_icon = if folder.expanded { "▼" } else { "▶" };
        let folder_row = row![
            text(folder_icon).size(12),
            text(folder.name.clone()).size(14),
        ]
        .spacing(8)
        .align_y(Alignment::Center);

        let folder_button = button(folder_row)
            .style(|_theme, _status| button::Style {
                background: None,
                text_color: THEME.text_primary,
                ..Default::default()
            })
            .padding([6, 12])
            .width(Length::Fill)
            .on_press(Message::FolderToggle(folder.id));

        host_list = host_list.push(folder_button);

        // Hosts in this folder (if expanded)
        if folder.expanded {
            for host in hosts.iter().filter(|h| h.folder.as_ref() == Some(&folder.name)) {
                host_list = host_list.push(host_item(host.clone(), selected_host));
            }
        }
    }

    // Ungrouped hosts (no folder)
    for host in hosts.iter().filter(|h| h.folder.is_none()) {
        host_list = host_list.push(host_item(host.clone(), selected_host));
    }

    let scrollable_hosts = scrollable(host_list.padding([0, 8]))
        .height(Length::Fill)
        .width(Length::Fill);

    // Bottom section with Snippets and Settings
    let bottom_section = column![
        container(text("─".repeat(20)).size(12).color(THEME.border))
            .padding([8, 12]),
        button(text("Snippets").size(14))
            .style(|_theme, _status| button::Style {
                background: None,
                text_color: THEME.text_secondary,
                ..Default::default()
            })
            .padding([6, 12])
            .width(Length::Fill)
            .on_press(Message::SnippetsOpen),
        button(text("Settings").size(14))
            .style(|_theme, _status| button::Style {
                background: None,
                text_color: THEME.text_secondary,
                ..Default::default()
            })
            .padding([6, 12])
            .width(Length::Fill)
            .on_press(Message::SettingsOpen),
    ];

    let sidebar_content = column![
        header,
        search_container,
        scrollable_hosts,
        bottom_section,
    ]
    .height(Fill);

    container(sidebar_content)
        .width(Length::Fixed(SIDEBAR_WIDTH))
        .height(Fill)
        .style(|_theme| container::Style {
            background: Some(THEME.sidebar.into()),
            border: iced::Border {
                color: THEME.border,
                width: 0.0,
                radius: 0.0.into(),
            },
            ..Default::default()
        })
        .into()
}

/// Single host item in the sidebar
fn host_item(host: SidebarHost, selected: Option<Uuid>) -> Element<'static, Message> {
    let is_selected = selected == Some(host.id);
    let host_id = host.id;

    let host_content = row![
        column![
            text(host.name.clone()).size(14).color(THEME.text_primary),
            text(host.hostname.clone()).size(11).color(THEME.text_muted),
        ]
        .spacing(2)
        .width(Length::Fill),
        // Edit button (pencil icon)
        button(text("✎").size(12).color(THEME.text_muted))
            .style(|_theme, status| {
                let color = match status {
                    button::Status::Hovered => THEME.text_primary,
                    _ => THEME.text_muted,
                };
                button::Style {
                    background: None,
                    text_color: color,
                    ..Default::default()
                }
            })
            .padding([4, 6])
            .on_press(Message::HostEdit(host_id)),
    ]
    .align_y(Alignment::Center);

    let bg_color = if is_selected {
        Some(THEME.selected.into())
    } else {
        None
    };

    button(
        container(host_content)
            .padding([6, 8])
            .width(Length::Fill),
    )
    .style(move |_theme, status| {
        let background = match status {
            button::Status::Hovered if !is_selected => Some(THEME.hover.into()),
            _ => bg_color,
        };
        button::Style {
            background,
            text_color: THEME.text_primary,
            border: iced::Border::default(),
            ..Default::default()
        }
    })
    .padding(0)
    .width(Length::Fill)
    .on_press(Message::HostSelected(host_id))
    .into()
}
