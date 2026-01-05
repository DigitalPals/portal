use iced::widget::{button, column, container, row, text, text_input, Column, Row, Space};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::icons::{self, icon_with_color};
use crate::message::Message;
use crate::theme::{BORDER_RADIUS, CARD_BORDER_RADIUS, THEME};

/// Host card for SFTP picker (simplified version)
#[derive(Debug, Clone)]
pub struct SftpHostCard {
    pub id: Uuid,
    pub name: String,
}

/// Build the SFTP picker view
pub fn sftp_picker_view(
    search_query: &str,
    hosts: Vec<SftpHostCard>,
) -> Element<'static, Message> {
    // Header with search
    let search_input = text_input("Search hosts for SFTP connection...", search_query)
        .on_input(Message::SearchChanged)
        .padding(12)
        .width(Length::Fill);

    let search_container = container(search_input)
        .style(|_theme| container::Style {
            background: Some(THEME.surface.into()),
            border: iced::Border {
                color: THEME.border,
                width: 1.0,
                radius: BORDER_RADIUS.into(),
            },
            ..Default::default()
        });

    let header = column![
        text("SFTP Browser")
            .size(18)
            .color(THEME.text_primary),
        Space::with_height(12),
        search_container,
    ]
    .padding(Padding::new(24.0).bottom(16.0));

    // Main content
    let content: Element<'static, Message> = if hosts.is_empty() {
        empty_state()
    } else {
        let hosts_section = build_hosts_grid(hosts);
        let scrollable_content = iced::widget::scrollable(hosts_section)
            .height(Fill)
            .width(Fill);
        scrollable_content.into()
    };

    let main_content = column![header, content];

    container(main_content)
        .width(Fill)
        .height(Fill)
        .style(|_theme| container::Style {
            background: Some(THEME.background.into()),
            ..Default::default()
        })
        .into()
}

/// Build hosts grid
fn build_hosts_grid(hosts: Vec<SftpHostCard>) -> Element<'static, Message> {
    let section_header = text("Select a host for SFTP")
        .size(14)
        .color(THEME.text_secondary);

    // Build grid of host cards (3 columns)
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut current_row: Vec<Element<'static, Message>> = Vec::new();

    for host in hosts {
        current_row.push(sftp_host_card(host));

        if current_row.len() >= 3 {
            rows.push(
                Row::with_children(std::mem::take(&mut current_row))
                    .spacing(16)
                    .into(),
            );
        }
    }

    // Add remaining cards in the last row
    if !current_row.is_empty() {
        while current_row.len() < 3 {
            current_row.push(
                container(text(""))
                    .width(Length::FillPortion(1))
                    .into(),
            );
        }
        rows.push(Row::with_children(current_row).spacing(16).into());
    }

    let grid = Column::with_children(rows).spacing(16);

    column![section_header, grid]
        .spacing(12)
        .padding(Padding::new(24.0).top(0.0))
        .into()
}

/// Single SFTP host card
fn sftp_host_card(host: SftpHostCard) -> Element<'static, Message> {
    let host_id = host.id;

    // Folder icon for SFTP
    let icon_widget = container(
        icon_with_color(icons::ui::HARD_DRIVE, 20, THEME.accent)
    )
    .width(40)
    .height(40)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(|_theme| container::Style {
        background: Some(THEME.selected.into()),
        border: iced::Border {
            radius: BORDER_RADIUS.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    // Host info
    let info = column![
        text(host.name).size(14).color(THEME.text_primary),
        text("SFTP").size(12).color(THEME.text_muted),
    ]
    .spacing(2);

    let card_content = row![icon_widget, info]
        .spacing(12)
        .align_y(Alignment::Center);

    button(
        container(card_content)
            .padding(12)
            .width(Length::Fill),
    )
    .style(|_theme, status| {
        let bg = match status {
            button::Status::Hovered => THEME.hover,
            _ => THEME.surface,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: THEME.text_primary,
            border: iced::Border {
                radius: CARD_BORDER_RADIUS.into(),
                ..Default::default()
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 0, 0, 0.15),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 4.0,
            },
            ..Default::default()
        }
    })
    .padding(0)
    .width(Length::FillPortion(1))
    .on_press(Message::SftpOpen(host_id))
    .into()
}

/// Empty state when no hosts
fn empty_state() -> Element<'static, Message> {
    let content = column![
        icon_with_color(icons::ui::HARD_DRIVE, 48, THEME.text_muted),
        text("No hosts available").size(18).color(THEME.text_primary),
        text("Add hosts to use SFTP browser")
            .size(14)
            .color(THEME.text_muted),
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
