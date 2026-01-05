use iced::widget::{button, column, container, row, text, text_input, Column, Row, Space};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

use crate::config::DetectedOs;
use crate::icons::{self, icon_with_color};
use crate::message::Message;
use crate::theme::{
    BORDER_RADIUS, CARD_BORDER_RADIUS, CARD_HEIGHT, GRID_PADDING, GRID_SPACING,
    MIN_CARD_WIDTH, SIDEBAR_WIDTH, SIDEBAR_WIDTH_COLLAPSED, THEME,
};

/// Group card data for the grid view
#[derive(Debug, Clone)]
pub struct GroupCard {
    pub id: Uuid,
    pub name: String,
    pub host_count: usize,
}

/// Host card data for the grid view
#[derive(Debug, Clone)]
pub struct HostCard {
    pub id: Uuid,
    pub name: String,
    pub hostname: String,
    pub detected_os: Option<DetectedOs>,
}

/// Calculate the number of columns based on available width
pub fn calculate_columns(window_width: f32, sidebar_collapsed: bool) -> usize {
    let sidebar_width = if sidebar_collapsed {
        SIDEBAR_WIDTH_COLLAPSED
    } else {
        SIDEBAR_WIDTH
    };

    // Available width for the grid
    let content_width = window_width - sidebar_width - GRID_PADDING;

    // Calculate how many cards fit
    // Formula: content_width >= n * MIN_CARD_WIDTH + (n-1) * GRID_SPACING
    // Solving: n <= (content_width + GRID_SPACING) / (MIN_CARD_WIDTH + GRID_SPACING)
    let columns = ((content_width + GRID_SPACING) / (MIN_CARD_WIDTH + GRID_SPACING)).floor() as usize;

    // Clamp between 1 and 4 columns
    columns.clamp(1, 4)
}

/// Build the host grid view (main content area)
pub fn host_grid_view(
    search_query: &str,
    groups: Vec<GroupCard>,
    hosts: Vec<HostCard>,
    column_count: usize,
) -> Element<'static, Message> {
    // Header with search bar and NEW HOST button
    let search_input = text_input("Find a host or ssh user@hostname...", search_query)
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

    let new_host_btn = button(
        row![
            icon_with_color(icons::ui::PLUS, 14, iced::Color::WHITE),
            text("NEW HOST").size(12).color(iced::Color::WHITE),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .style(|_theme, status| {
        let bg = match status {
            button::Status::Hovered => iced::Color::from_rgb8(0x00, 0x8B, 0xE8),
            _ => THEME.accent,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: iced::Color::WHITE,
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                ..Default::default()
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 120, 212, 0.3),
                offset: iced::Vector::new(0.0, 2.0),
                blur_radius: 4.0,
            },
            ..Default::default()
        }
    })
    .padding([10, 16])
    .on_press(Message::HostAdd);

    let header = row![
        search_container,
        Space::with_width(16),
        new_host_btn,
    ]
    .align_y(Alignment::Center)
    .padding(Padding::new(24.0).bottom(16.0));

    // Main scrollable content
    let mut content = Column::new().spacing(24).padding(Padding::new(24.0).top(0.0));

    // Check emptiness before moving
    let groups_empty = groups.is_empty();
    let hosts_empty = hosts.is_empty();

    // Groups section (if any groups exist)
    if !groups_empty {
        let groups_section = build_groups_section(groups, column_count);
        content = content.push(groups_section);
    }

    // Hosts section
    if hosts_empty && groups_empty {
        content = content.push(empty_state());
    } else if !hosts_empty {
        let hosts_section = build_hosts_section(hosts, column_count);
        content = content.push(hosts_section);
    }

    let scrollable_content = iced::widget::scrollable(content)
        .height(Fill)
        .width(Fill);

    let main_content = column![header, scrollable_content];

    container(main_content)
        .width(Fill)
        .height(Fill)
        .style(|_theme| container::Style {
            background: Some(THEME.background.into()),
            ..Default::default()
        })
        .into()
}

/// Build the groups section
fn build_groups_section(groups: Vec<GroupCard>, column_count: usize) -> Element<'static, Message> {
    let section_header = text("Groups")
        .size(14)
        .color(THEME.text_secondary);

    // Build grid of group cards (dynamic columns)
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut current_row: Vec<Element<'static, Message>> = Vec::new();

    for group in groups {
        current_row.push(group_card(group));

        if current_row.len() >= column_count {
            rows.push(
                Row::with_children(std::mem::take(&mut current_row))
                    .spacing(GRID_SPACING as u16)
                    .into(),
            );
        }
    }

    // Add remaining cards in the last row
    if !current_row.is_empty() {
        while current_row.len() < column_count {
            current_row.push(
                container(text(""))
                    .width(Length::FillPortion(1))
                    .into(),
            );
        }
        rows.push(Row::with_children(current_row).spacing(GRID_SPACING as u16).into());
    }

    let grid = Column::with_children(rows).spacing(GRID_SPACING as u16);

    column![section_header, grid]
        .spacing(12)
        .into()
}

/// Build the hosts section
fn build_hosts_section(hosts: Vec<HostCard>, column_count: usize) -> Element<'static, Message> {
    let section_header = text("Hosts")
        .size(14)
        .color(THEME.text_secondary);

    // Build grid of host cards (dynamic columns)
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut current_row: Vec<Element<'static, Message>> = Vec::new();

    for host in hosts {
        current_row.push(host_card(host));

        if current_row.len() >= column_count {
            rows.push(
                Row::with_children(std::mem::take(&mut current_row))
                    .spacing(GRID_SPACING as u16)
                    .into(),
            );
        }
    }

    // Add remaining cards in the last row
    if !current_row.is_empty() {
        while current_row.len() < column_count {
            current_row.push(
                container(text(""))
                    .width(Length::FillPortion(1))
                    .into(),
            );
        }
        rows.push(Row::with_children(current_row).spacing(GRID_SPACING as u16).into());
    }

    let grid = Column::with_children(rows).spacing(GRID_SPACING as u16);

    column![section_header, grid]
        .spacing(12)
        .into()
}

/// Single group card
fn group_card(group: GroupCard) -> Element<'static, Message> {
    let group_id = group.id;

    // Folder icon
    let icon_widget = container(
        icon_with_color(icons::ui::FOLDER_CLOSED, 22, THEME.accent)
    )
    .width(44)
    .height(44)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(|_theme| container::Style {
        background: Some(THEME.selected.into()),
        border: iced::Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    // Group info
    let host_text = if group.host_count == 1 {
        "1 host".to_string()
    } else {
        format!("{} hosts", group.host_count)
    };

    let info = column![
        text(group.name).size(14).color(THEME.text_primary),
        text(host_text).size(12).color(THEME.text_muted),
    ]
    .spacing(2);

    let card_content = row![icon_widget, info]
        .spacing(12)
        .align_y(Alignment::Center);

    button(
        container(card_content)
            .padding(12)
            .width(Length::Fill)
            .height(Length::Fixed(CARD_HEIGHT))
            .align_y(Alignment::Center),
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
    .height(Length::Fixed(CARD_HEIGHT))
    .on_press(Message::FolderToggle(group_id))
    .into()
}

/// Get the icon data for a detected OS
fn os_icon_data(os: &Option<DetectedOs>) -> &'static [u8] {
    match os {
        // BSD family
        Some(DetectedOs::FreeBSD) => icons::os::FREEBSD,
        Some(DetectedOs::OpenBSD) => icons::os::OPENBSD,
        Some(DetectedOs::NetBSD) => icons::os::NETBSD,
        // macOS and Windows
        Some(DetectedOs::MacOS) => icons::os::APPLE,
        Some(DetectedOs::Windows) => icons::os::WINDOWS,
        // Linux distributions
        Some(DetectedOs::Ubuntu) => icons::os::UBUNTU,
        Some(DetectedOs::Debian) => icons::os::DEBIAN,
        Some(DetectedOs::Fedora) => icons::os::FEDORA,
        Some(DetectedOs::Arch) => icons::os::ARCH,
        Some(DetectedOs::CentOS) => icons::os::CENTOS,
        Some(DetectedOs::RedHat) => icons::os::REDHAT,
        Some(DetectedOs::OpenSUSE) => icons::os::OPENSUSE,
        Some(DetectedOs::NixOS) => icons::os::NIXOS,
        Some(DetectedOs::Manjaro) => icons::os::MANJARO,
        Some(DetectedOs::Mint) => icons::os::MINT,
        Some(DetectedOs::PopOS) => icons::os::POPOS,
        Some(DetectedOs::Gentoo) => icons::os::GENTOO,
        Some(DetectedOs::Alpine) => icons::os::ALPINE,
        Some(DetectedOs::Kali) => icons::os::KALI,
        Some(DetectedOs::Rocky) => icons::os::ROCKY,
        Some(DetectedOs::Alma) => icons::os::ALMA,
        // Generic Linux fallback
        Some(DetectedOs::Linux) => icons::os::LINUX,
        // Unknown
        Some(DetectedOs::Unknown(_)) => icons::os::UNKNOWN,
        None => icons::os::UNKNOWN,
    }
}

/// Get the brand color for a detected OS
fn os_icon_color(os: &Option<DetectedOs>) -> iced::Color {
    match os {
        Some(detected) => {
            let (r, g, b) = detected.icon_color();
            iced::Color::from_rgb8(r, g, b)
        }
        None => iced::Color::from_rgb8(0x70, 0x70, 0x70), // Muted gray for unknown
    }
}

/// Single host card
fn host_card(host: HostCard) -> Element<'static, Message> {
    let host_id = host.id;

    // Get OS icon and color
    let os_icon_bytes = os_icon_data(&host.detected_os);
    let os_color = os_icon_color(&host.detected_os);

    // OS icon with brand color
    let icon_widget = container(
        icon_with_color(os_icon_bytes, 24, os_color)
    )
    .width(44)
    .height(44)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_theme| container::Style {
        background: Some(iced::Color::from_rgba(os_color.r, os_color.g, os_color.b, 0.15).into()),
        border: iced::Border {
            radius: 8.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    // Host info
    let os_text = match &host.detected_os {
        Some(os) => format!("SSH, {}", os.display_name()),
        None => "SSH".to_string(),
    };

    let info = column![
        text(host.name.clone()).size(15).color(THEME.text_primary),
        text(os_text).size(12).color(THEME.text_secondary),
    ]
    .spacing(3);

    let card_content = row![icon_widget, info]
        .spacing(14)
        .align_y(Alignment::Center);

    button(
        container(card_content)
            .padding(14)
            .width(Length::Fill)
            .height(Length::Fixed(CARD_HEIGHT))
            .align_y(Alignment::Center),
    )
    .style(|_theme, status| {
        let (bg, shadow_alpha) = match status {
            button::Status::Hovered => (THEME.hover, 0.25),
            _ => (THEME.surface, 0.15),
        };
        button::Style {
            background: Some(bg.into()),
            text_color: THEME.text_primary,
            border: iced::Border {
                radius: 12.0.into(),
                ..Default::default()
            },
            shadow: iced::Shadow {
                color: iced::Color::from_rgba8(0, 0, 0, shadow_alpha),
                offset: iced::Vector::new(0.0, 3.0),
                blur_radius: 8.0,
            },
            ..Default::default()
        }
    })
    .padding(0)
    .width(Length::FillPortion(1))
    .height(Length::Fixed(CARD_HEIGHT))
    .on_press(Message::HostConnect(host_id))
    .into()
}

/// Empty state when no hosts are configured
fn empty_state() -> Element<'static, Message> {
    let content = column![
        icon_with_color(icons::ui::SERVER, 48, THEME.text_muted),
        text("No hosts configured").size(18).color(THEME.text_primary),
        text("Click NEW HOST to add your first server")
            .size(14)
            .color(THEME.text_muted),
        Space::with_height(16),
        button(
            row![
                icon_with_color(icons::ui::PLUS, 14, iced::Color::WHITE),
                text("NEW HOST").size(14).color(iced::Color::WHITE),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => iced::Color::from_rgb8(0x00, 0x8B, 0xE8),
                _ => THEME.accent,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([10, 20])
        .on_press(Message::HostAdd),
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
