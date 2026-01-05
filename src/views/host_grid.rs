use iced::widget::{button, column, container, row, text, Column, Row};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::message::Message;
use crate::theme::{BORDER_RADIUS, THEME};

/// Host card data for the grid view
#[derive(Debug, Clone)]
pub struct HostCard {
    pub id: Uuid,
    pub name: String,
    pub hostname: String,
    pub username: String,
    pub tags: Vec<String>,
    pub last_connected: Option<String>,
}

/// Build the host grid view (main content area when no session is active)
pub fn host_grid_view(hosts: Vec<HostCard>) -> Element<'static, Message> {
    if hosts.is_empty() {
        return empty_state();
    }

    // Build grid of host cards (3 columns)
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut current_row: Vec<Element<'static, Message>> = Vec::new();

    for host in hosts {
        current_row.push(host_card(host));

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
        // Pad with empty containers to maintain grid alignment
        while current_row.len() < 3 {
            current_row.push(
                container(text(""))
                    .width(Length::FillPortion(1))
                    .into(),
            );
        }
        rows.push(
            Row::with_children(current_row)
                .spacing(16)
                .into(),
        );
    }

    let grid = Column::with_children(rows)
        .spacing(16)
        .padding(24);

    let scrollable_grid = iced::widget::scrollable(grid)
        .height(Fill)
        .width(Fill);

    container(scrollable_grid)
        .width(Fill)
        .height(Fill)
        .style(|_theme| container::Style {
            background: Some(THEME.background.into()),
            ..Default::default()
        })
        .into()
}

/// Single host card
fn host_card(host: HostCard) -> Element<'static, Message> {
    // Avatar with first letter
    let first_char = host.name.chars().next().unwrap_or('?').to_uppercase().to_string();
    let avatar = container(
        text(first_char)
            .size(24)
            .color(THEME.text_primary),
    )
    .width(48)
    .height(48)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(|_theme| container::Style {
        background: Some(THEME.accent.into()),
        border: iced::Border {
            radius: BORDER_RADIUS.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    // Host info
    let info = column![
        text(host.name.clone()).size(16).color(THEME.text_primary),
        text(format!("{}@{}", host.username, host.hostname))
            .size(12)
            .color(THEME.text_muted),
    ]
    .spacing(4);

    // Tags
    let tags_row: Element<'static, Message> = if !host.tags.is_empty() {
        let tag_elements: Vec<Element<'static, Message>> = host
            .tags
            .iter()
            .take(3)
            .map(|tag| {
                container(text(tag.clone()).size(10).color(THEME.text_secondary))
                    .padding(Padding::new(2.0).left(6.0).right(6.0))
                    .style(|_theme| container::Style {
                        background: Some(THEME.surface.into()),
                        border: iced::Border {
                            radius: 2.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    })
                    .into()
            })
            .collect();
        Row::with_children(tag_elements).spacing(4).into()
    } else {
        text("").into()
    };

    // Last connected
    let last_connected = host.last_connected.as_ref().map_or_else(
        || text("Never connected").size(11).color(THEME.text_muted),
        |time| text(format!("Last: {}", time)).size(11).color(THEME.text_muted),
    );

    let host_id = host.id;

    // Action buttons
    let ssh_btn = button(
        text("SSH").size(12).color(THEME.text_primary),
    )
    .style(|_theme, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => THEME.accent,
            _ => THEME.hover,
        };
        iced::widget::button::Style {
            background: Some(bg.into()),
            text_color: THEME.text_primary,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .padding([4, 12])
    .on_press(Message::HostConnect(host_id));

    let sftp_btn = button(
        text("SFTP").size(12).color(THEME.text_primary),
    )
    .style(|_theme, status| {
        let bg = match status {
            iced::widget::button::Status::Hovered => THEME.accent,
            _ => THEME.hover,
        };
        iced::widget::button::Style {
            background: Some(bg.into()),
            text_color: THEME.text_primary,
            border: iced::Border {
                radius: 4.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .padding([4, 12])
    .on_press(Message::SftpOpen(host_id));

    let action_row = row![ssh_btn, sftp_btn]
        .spacing(8);

    let card_content = column![
        row![avatar, info].spacing(12).align_y(Alignment::Center),
        tags_row,
        last_connected,
        action_row,
    ]
    .spacing(8);

    container(card_content)
        .padding(16)
        .width(Length::FillPortion(1))
        .style(|_theme| container::Style {
            background: Some(THEME.surface.into()),
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                ..Default::default()
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 0, 0, 0.2),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 4.0,
            },
            ..Default::default()
        })
        .into()
}

/// Empty state when no hosts are configured
fn empty_state() -> Element<'static, Message> {
    let content = column![
        text("No hosts configured").size(20).color(THEME.text_primary),
        text("Add a host to get started")
            .size(14)
            .color(THEME.text_muted),
    ]
    .spacing(8)
    .align_x(Alignment::Center);

    container(content)
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(|_theme| container::Style {
            background: Some(THEME.background.into()),
            ..Default::default()
        })
        .into()
}
