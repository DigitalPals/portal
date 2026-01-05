use iced::widget::{button, column, container, row, text, text_input, Column, Row, Space};
use iced::{Alignment, Element, Fill, Font, Length, Padding};
use uuid::Uuid;

/// Search input ID for auto-focus
pub fn search_input_id() -> text_input::Id {
    text_input::Id::new("hosts_search")
}

use crate::config::DetectedOs;
use crate::icons::{self, icon_with_color};
use crate::message::Message;
use crate::theme::{
    BORDER_RADIUS, CARD_BORDER_RADIUS, CARD_HEIGHT, GRID_PADDING, GRID_SPACING,
    MIN_CARD_WIDTH, SIDEBAR_WIDTH, SIDEBAR_WIDTH_COLLAPSED, THEME,
};

const PORTAL_LOGO_TOP: &str = r#"                                  .             oooo
                                .o8             `888
oo.ooooo.   .ooooo.  oooo d8b .o888oo  .oooo.    888
 888' `88b d88' `88b `888""8P   888   `P  )88b   888
 888   888 888   888  888       888    .oP"888   888
 888   888 888   888  888       888 . d8(  888   888
 888bod8P' `Y8bod8P' d888b      "888" `Y888""8o o888o
 888"#;

const PORTAL_LOGO_LAST_LINE: &str = "o888o";
const LOGO_WIDTH: usize = 54;

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

/// Build the action bar with search, connect, new host, and terminal buttons
fn build_action_bar(search_query: &str) -> Element<'static, Message> {
    // Search input - pill-shaped, auto-focused
    let search_input: iced::widget::TextInput<'static, Message> =
        text_input("Find a host or ssh user@hostname...", search_query)
            .id(search_input_id())
            .on_input(Message::SearchChanged)
            .on_submit(Message::QuickConnect)
            .padding([12, 20])
            .width(Length::Fill)
            .style(|_theme, status| {
                use iced::widget::text_input::{Status, Style};
                let border_color = match status {
                    Status::Focused => THEME.accent,
                    _ => THEME.border,
                };
                Style {
                    background: THEME.background.into(),
                    border: iced::Border {
                        color: border_color,
                        width: 1.0,
                        radius: 22.0.into(),
                    },
                    icon: THEME.text_muted,
                    placeholder: THEME.text_muted,
                    value: THEME.text_primary,
                    selection: THEME.selected,
                }
            });

    // Connect button - pill-shaped, accent color
    let connect_btn = button(text("Connect").size(14).color(iced::Color::WHITE))
        .style(|_theme, status| {
            let bg = match status {
                button::Status::Hovered => iced::Color::from_rgb8(0x00, 0x8B, 0xE8),
                _ => THEME.accent,
            };
            button::Style {
                background: Some(bg.into()),
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    radius: 22.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        })
        .padding([12, 24])
        .on_press(Message::QuickConnect);

    // New Host button - pill-shaped with border
    let new_host_btn = button(
        row![
            icon_with_color(icons::ui::PLUS, 14, THEME.text_primary),
            text("New Host").size(13).color(THEME.text_primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .style(|_theme, status| {
        let bg = match status {
            button::Status::Hovered => THEME.hover,
            _ => THEME.background,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: THEME.text_primary,
            border: iced::Border {
                color: THEME.border,
                width: 1.0,
                radius: 22.0.into(),
            },
            ..Default::default()
        }
    })
    .padding([12, 20])
    .on_press(Message::HostAdd);

    // Terminal button - pill-shaped with border
    let terminal_btn = button(
        row![
            icon_with_color(icons::ui::TERMINAL, 14, THEME.text_primary),
            text("Terminal").size(13).color(THEME.text_primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .style(|_theme, status| {
        let bg = match status {
            button::Status::Hovered => THEME.hover,
            _ => THEME.background,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: THEME.text_primary,
            border: iced::Border {
                color: THEME.border,
                width: 1.0,
                radius: 22.0.into(),
            },
            ..Default::default()
        }
    })
    .padding([12, 20])
    .on_press(Message::LocalTerminal);

    // Build the bar row
    let bar_content = row![
        search_input,
        Space::with_width(12),
        connect_btn,
        Space::with_width(12),
        new_host_btn,
        Space::with_width(8),
        terminal_btn,
    ]
    .align_y(Alignment::Center);

    // Container with top border and padding
    container(bar_content)
        .width(Fill)
        .padding([16, 24])
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

/// Build the host grid view (main content area)
pub fn host_grid_view(
    search_query: &str,
    groups: Vec<GroupCard>,
    hosts: Vec<HostCard>,
    column_count: usize,
) -> Element<'static, Message> {
    // ASCII Logo with version on last line (right-aligned)
    let version = format!("v{}", env!("CARGO_PKG_VERSION"));
    let padding_len = LOGO_WIDTH
        .saturating_sub(PORTAL_LOGO_LAST_LINE.len())
        .saturating_sub(version.len())
        .saturating_sub(1); // 1 char from right edge
    let last_line = format!(
        "{}{} {}",
        PORTAL_LOGO_LAST_LINE,
        " ".repeat(padding_len),
        version
    );
    let full_logo = format!("{}\n{}", PORTAL_LOGO_TOP, last_line);

    let logo_section = container(
        text(full_logo)
            .size(10)
            .color(THEME.text_secondary)
            .font(Font::MONOSPACE),
    )
    .width(Length::Fill)
    .padding(Padding::new(16.0).top(48.0))
    .align_x(Alignment::Center);

    // Main scrollable content
    let mut content = Column::new()
        .spacing(24)
        .padding(Padding::new(24.0).top(16.0).bottom(24.0));

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

    // Add logo below hosts
    content = content.push(logo_section);

    let scrollable_content = iced::widget::scrollable(content)
        .height(Fill)
        .width(Fill);

    // Action bar (fixed at top, below tab bar)
    let action_bar = build_action_bar(search_query);

    // Main layout: action bar at top, scrollable content fills remaining space
    let main_content = column![action_bar, scrollable_content];

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
        .size(16)
        .color(THEME.text_primary);

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
        .size(16)
        .color(THEME.text_primary);

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

    // Folder icon with vibrant accent background
    let icon_widget = container(
        icon_with_color(icons::ui::FOLDER_CLOSED, 22, iced::Color::WHITE)
    )
    .width(48)
    .height(48)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(|_theme| container::Style {
        background: Some(THEME.accent.into()),
        border: iced::Border {
            radius: CARD_BORDER_RADIUS.into(),
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
        text(group.name).size(16).color(THEME.text_primary),
        text(host_text).size(12).color(THEME.text_secondary),
    ]
    .spacing(4);

    let card_content = row![icon_widget, info]
        .spacing(14)
        .align_y(Alignment::Center);

    button(
        container(card_content)
            .padding(16)
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

    // OS icon with vibrant solid background and white icon
    let icon_widget = container(
        icon_with_color(os_icon_bytes, 24, iced::Color::WHITE)
    )
    .width(48)
    .height(48)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_theme| container::Style {
        background: Some(iced::Color::from_rgba(os_color.r, os_color.g, os_color.b, 0.85).into()),
        border: iced::Border {
            radius: CARD_BORDER_RADIUS.into(),
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
        text(host.name.clone()).size(16).color(THEME.text_primary),
        text(os_text).size(12).color(THEME.text_secondary),
    ]
    .spacing(4);

    let card_content = row![icon_widget, info]
        .spacing(14)
        .align_y(Alignment::Center);

    button(
        container(card_content)
            .padding(16)
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
