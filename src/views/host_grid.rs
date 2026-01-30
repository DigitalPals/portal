use iced::widget::{
    Column, Row, Space, button, column, container, mouse_area, row, text, text_input,
};
use iced::{Alignment, Element, Fill, Length, Padding};
use uuid::Uuid;

/// Search input ID for auto-focus
pub fn search_input_id() -> iced::widget::Id {
    iced::widget::Id::new("hosts_search")
}

use crate::app::{FocusSection, SidebarState};
use crate::config::{DetectedOs, Protocol};
use crate::icons::{self, icon_with_color};
use crate::message::{HostMessage, Message, UiMessage};
use crate::theme::{
    BORDER_RADIUS, CARD_BORDER_RADIUS, CARD_HEIGHT, GRID_PADDING, GRID_SPACING, MIN_CARD_WIDTH,
    SIDEBAR_WIDTH, SIDEBAR_WIDTH_COLLAPSED, ScaledFonts, Theme,
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
    pub protocol: Protocol,
}

/// Calculate the number of columns based on available width
pub fn calculate_columns(window_width: f32, sidebar_state: SidebarState) -> usize {
    let sidebar_width = match sidebar_state {
        SidebarState::Hidden => 0.0,
        SidebarState::IconsOnly => SIDEBAR_WIDTH_COLLAPSED,
        SidebarState::Expanded => SIDEBAR_WIDTH,
    };

    // Available width for the grid
    let content_width = window_width - sidebar_width - GRID_PADDING;

    // Calculate how many cards fit
    // Formula: content_width >= n * MIN_CARD_WIDTH + (n-1) * GRID_SPACING
    // Solving: n <= (content_width + GRID_SPACING) / (MIN_CARD_WIDTH + GRID_SPACING)
    let columns =
        ((content_width + GRID_SPACING) / (MIN_CARD_WIDTH + GRID_SPACING)).floor() as usize;

    // Clamp between 1 and 4 columns
    columns.clamp(1, 4)
}

/// Build the action bar with search, connect, new host, and terminal buttons
fn build_action_bar(
    search_query: &str,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    // Search input - pill-shaped, auto-focused
    let search_input: iced::widget::TextInput<'static, Message> =
        text_input("Search hosts...", search_query)
            .id(search_input_id())
            .on_input(|s| Message::Ui(UiMessage::SearchChanged(s)))
            .padding([12, 20])
            .width(Length::Fill)
            .style(move |_theme, status| {
                use iced::widget::text_input::{Status, Style};
                let border_color = match status {
                    Status::Focused { .. } => theme.accent,
                    _ => theme.border,
                };
                Style {
                    background: theme.surface.into(),
                    border: iced::Border {
                        color: border_color,
                        width: 1.0,
                        radius: 22.0.into(),
                    },
                    icon: theme.text_muted,
                    placeholder: theme.text_muted,
                    value: theme.text_primary,
                    selection: theme.selected,
                }
            });

    // Connect button - pill-shaped with border (matches New Host styling)
    let connect_btn = button(
        row![
            icon_with_color(icons::ui::ZAP, 14, theme.text_primary),
            text("Connect")
                .size(fonts.button_small)
                .color(theme.text_primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .style(move |_theme, status| {
        let bg = match status {
            button::Status::Hovered => theme.hover,
            _ => theme.background,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 22.0.into(),
            },
            ..Default::default()
        }
    })
    .padding([12, 20])
    .on_press(Message::Host(HostMessage::QuickConnect));

    // New Host button - pill-shaped with border
    let new_host_btn = button(
        row![
            icon_with_color(icons::ui::PLUS, 14, theme.text_primary),
            text("New Host")
                .size(fonts.button_small)
                .color(theme.text_primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .style(move |_theme, status| {
        let bg = match status {
            button::Status::Hovered => theme.hover,
            _ => theme.background,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 22.0.into(),
            },
            ..Default::default()
        }
    })
    .padding([12, 20])
    .on_press(Message::Host(HostMessage::Add));

    // Terminal button - pill-shaped with border
    let terminal_btn = button(
        row![
            icon_with_color(icons::ui::TERMINAL, 14, theme.text_primary),
            text("Terminal")
                .size(fonts.button_small)
                .color(theme.text_primary),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .style(move |_theme, status| {
        let bg = match status {
            button::Status::Hovered => theme.hover,
            _ => theme.background,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 22.0.into(),
            },
            ..Default::default()
        }
    })
    .padding([12, 20])
    .on_press(Message::Host(HostMessage::LocalTerminal));

    // Build the bar row
    let bar_content = row![
        search_input,
        Space::new().width(12),
        connect_btn,
        Space::new().width(12),
        new_host_btn,
        Space::new().width(8),
        terminal_btn,
    ]
    .align_y(Alignment::Center);

    // Container with padding (no border)
    container(bar_content)
        .width(Fill)
        .padding([16, 24])
        .style(move |_theme| container::Style {
            background: Some(theme.background.into()),
            border: iced::Border::default(),
            ..Default::default()
        })
        .into()
}

/// Build the host grid view (main content area)
#[allow(clippy::too_many_arguments)]
pub fn host_grid_view(
    search_query: &str,
    groups: Vec<GroupCard>,
    hosts: Vec<HostCard>,
    column_count: usize,
    theme: Theme,
    fonts: ScaledFonts,
    focus_section: FocusSection,
    focus_index: Option<usize>,
    hovered_host: Option<Uuid>,
) -> Element<'static, Message> {
    // Main scrollable content
    let mut content = Column::new()
        .spacing(24)
        .padding(Padding::new(24.0).top(16.0).bottom(24.0));

    // Check emptiness before moving
    let groups_len = groups.len();
    let groups_empty = groups.is_empty();
    let hosts_empty = hosts.is_empty();

    // Groups section (if any groups exist)
    if !groups_empty {
        let groups_section = build_groups_section(
            groups,
            column_count,
            theme,
            fonts,
            focus_section,
            focus_index,
        );
        content = content.push(groups_section);
    }

    // Hosts section
    if hosts_empty && groups_empty {
        content = content.push(empty_state(theme, fonts));
    } else if !hosts_empty {
        // Adjust focus index for hosts (groups come first)
        let host_focus_index = if focus_section == FocusSection::Content {
            focus_index.and_then(|idx| idx.checked_sub(groups_len))
        } else {
            None
        };
        let hosts_section = build_hosts_section(
            hosts,
            column_count,
            theme,
            fonts,
            focus_section,
            host_focus_index,
            hovered_host,
        );
        content = content.push(hosts_section);
    }

    let scrollable_content = iced::widget::scrollable(content)
        .height(Fill)
        .width(Fill)
        .style(move |_iced_theme, _status| {
            use iced::widget::scrollable::{AutoScroll, Rail, Scroller, Style};
            let scroller_color = iced::Color::from_rgba8(0x60, 0x60, 0x70, 0.5);
            Style {
                container: container::Style {
                    background: Some(theme.background.into()),
                    ..Default::default()
                },
                vertical_rail: Rail {
                    background: None,
                    border: iced::Border::default(),
                    scroller: Scroller {
                        background: scroller_color.into(),
                        border: iced::Border {
                            radius: 3.0.into(),
                            ..Default::default()
                        },
                    },
                },
                horizontal_rail: Rail {
                    background: None,
                    border: iced::Border::default(),
                    scroller: Scroller {
                        background: iced::Color::TRANSPARENT.into(),
                        border: iced::Border::default(),
                    },
                },
                gap: None,
                auto_scroll: AutoScroll {
                    background: iced::Color::TRANSPARENT.into(),
                    border: iced::Border::default(),
                    shadow: iced::Shadow::default(),
                    icon: iced::Color::TRANSPARENT,
                },
            }
        });

    // Action bar (fixed at top, below tab bar)
    let action_bar = build_action_bar(search_query, theme, fonts);

    // Main layout: action bar at top, scrollable content fills remaining space
    let main_content = column![action_bar, scrollable_content];

    container(main_content)
        .width(Fill)
        .height(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.background.into()),
            ..Default::default()
        })
        .into()
}

/// Build the groups section
fn build_groups_section(
    groups: Vec<GroupCard>,
    column_count: usize,
    theme: Theme,
    fonts: ScaledFonts,
    focus_section: FocusSection,
    focus_index: Option<usize>,
) -> Element<'static, Message> {
    let section_header = text("Groups").size(fonts.section).color(theme.text_primary);

    // Build grid of group cards (dynamic columns)
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut current_row: Vec<Element<'static, Message>> = Vec::new();

    for (idx, group) in groups.into_iter().enumerate() {
        let is_focused = focus_section == FocusSection::Content && focus_index == Some(idx);
        current_row.push(group_card(group, theme, fonts, is_focused));

        if current_row.len() >= column_count {
            rows.push(
                Row::with_children(std::mem::take(&mut current_row))
                    .spacing(GRID_SPACING)
                    .into(),
            );
        }
    }

    // Add remaining cards in the last row
    if !current_row.is_empty() {
        while current_row.len() < column_count {
            current_row.push(container(text("")).width(Length::FillPortion(1)).into());
        }
        rows.push(Row::with_children(current_row).spacing(GRID_SPACING).into());
    }

    let grid = Column::with_children(rows).spacing(GRID_SPACING);

    column![section_header, grid].spacing(12).into()
}

/// Build the hosts section
fn build_hosts_section(
    hosts: Vec<HostCard>,
    column_count: usize,
    theme: Theme,
    fonts: ScaledFonts,
    focus_section: FocusSection,
    focus_index: Option<usize>,
    hovered_host: Option<Uuid>,
) -> Element<'static, Message> {
    let section_header = text("Hosts").size(fonts.section).color(theme.text_primary);

    // Build grid of host cards (dynamic columns)
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut current_row: Vec<Element<'static, Message>> = Vec::new();

    for (idx, host) in hosts.into_iter().enumerate() {
        let is_focused = focus_section == FocusSection::Content && focus_index == Some(idx);
        let is_hovered = hovered_host == Some(host.id);
        current_row.push(host_card(host, theme, fonts, is_focused, is_hovered));

        if current_row.len() >= column_count {
            rows.push(
                Row::with_children(std::mem::take(&mut current_row))
                    .spacing(GRID_SPACING)
                    .into(),
            );
        }
    }

    // Add remaining cards in the last row
    if !current_row.is_empty() {
        while current_row.len() < column_count {
            current_row.push(container(text("")).width(Length::FillPortion(1)).into());
        }
        rows.push(Row::with_children(current_row).spacing(GRID_SPACING).into());
    }

    let grid = Column::with_children(rows).spacing(GRID_SPACING);

    column![section_header, grid].spacing(12).into()
}

/// Single group card
fn group_card(
    group: GroupCard,
    theme: Theme,
    fonts: ScaledFonts,
    is_focused: bool,
) -> Element<'static, Message> {
    let group_id = group.id;

    // Folder icon with vibrant accent background
    let icon_widget = container(icon_with_color(
        icons::ui::FOLDER_CLOSED,
        18,
        iced::Color::WHITE,
    ))
    .width(40)
    .height(40)
    .align_x(Alignment::Center)
    .align_y(Alignment::Center)
    .style(move |_theme| container::Style {
        background: Some(theme.accent.into()),
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
        text(group.name)
            .size(fonts.section)
            .color(theme.text_primary),
        text(host_text)
            .size(fonts.label)
            .color(theme.text_secondary),
    ]
    .spacing(4);

    let card_content = row![icon_widget, info]
        .spacing(14)
        .align_y(Alignment::Center);

    button(
        container(card_content)
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fixed(CARD_HEIGHT))
            .align_y(Alignment::Center),
    )
    .style(move |_theme, status| {
        let card_bg = theme.surface;
        let bg = match (status, is_focused) {
            (_, true) => theme.hover,
            (button::Status::Hovered, _) => theme.hover,
            _ => card_bg,
        };
        let border = if is_focused {
            iced::Border {
                color: theme.focus_ring,
                width: 2.0,
                radius: CARD_BORDER_RADIUS.into(),
            }
        } else {
            iced::Border {
                radius: CARD_BORDER_RADIUS.into(),
                ..Default::default()
            }
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border,
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
    .on_press(Message::Ui(UiMessage::FolderToggle(group_id)))
    .into()
}

/// Get the icon data for a detected OS
pub fn os_icon_data(os: &Option<DetectedOs>) -> &'static [u8] {
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
fn host_card(
    host: HostCard,
    theme: Theme,
    fonts: ScaledFonts,
    is_focused: bool,
    is_hovered: bool,
) -> Element<'static, Message> {
    let host_id = host.id;

    // Get OS icon and color
    let os_icon_bytes = os_icon_data(&host.detected_os);
    let os_color = os_icon_color(&host.detected_os);

    // OS icon with vibrant solid background and white icon
    let icon_widget = container(icon_with_color(os_icon_bytes, 20, iced::Color::WHITE))
        .width(40)
        .height(40)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |_theme| container::Style {
            background: Some(
                iced::Color::from_rgba(os_color.r, os_color.g, os_color.b, 0.85).into(),
            ),
            border: iced::Border {
                radius: CARD_BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    // Host info
    let protocol_label = match host.protocol {
        Protocol::Ssh => "SSH",
        Protocol::Vnc => "VNC",
    };
    let os_text = match &host.detected_os {
        Some(os) => format!("{}, {}", protocol_label, os.display_name()),
        None => protocol_label.to_string(),
    };

    let info = column![
        text(host.name.clone())
            .size(fonts.section)
            .color(iced::Color::WHITE),
        text(os_text).size(fonts.label).color(theme.text_secondary),
    ]
    .spacing(4);

    // Edit button - only visible on hover
    // Use fixed dimensions (16px icon + 8px padding each side = 32px)
    let edit_button: Element<'static, Message> = if is_hovered {
        button(icon_with_color(icons::ui::PENCIL, 16, theme.text_secondary))
            .padding(8)
            .width(32)
            .height(32)
            .style(move |_theme, status| {
                let bg = match status {
                    button::Status::Hovered => theme.hover,
                    _ => iced::Color::TRANSPARENT,
                };
                button::Style {
                    background: Some(bg.into()),
                    text_color: theme.text_secondary,
                    border: iced::Border {
                        radius: 6.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .on_press(Message::Host(HostMessage::Edit(host_id)))
            .into()
    } else {
        Space::new().width(32).height(32).into()
    };

    let card_content = row![
        icon_widget,
        container(info).width(Length::Fill),
        edit_button,
    ]
    .spacing(14)
    .align_y(Alignment::Center);

    let card_button = button(
        container(card_content)
            .padding(10)
            .width(Length::Fill)
            .height(Length::Fixed(CARD_HEIGHT))
            .align_y(Alignment::Center),
    )
    .style(move |_theme, status| {
        let card_bg = theme.surface;
        let (bg, shadow_alpha) = match (status, is_focused, is_hovered) {
            (_, true, _) => (theme.hover, 0.25),
            (_, _, true) => (theme.hover, 0.25),
            (button::Status::Hovered, _, _) => (theme.hover, 0.25),
            _ => (card_bg, 0.15),
        };
        let border = if is_focused {
            iced::Border {
                color: theme.focus_ring,
                width: 2.0,
                radius: 12.0.into(),
            }
        } else {
            iced::Border {
                radius: 12.0.into(),
                ..Default::default()
            }
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border,
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
    .on_press(Message::Host(HostMessage::Connect(host_id)));

    // Wrap in mouse_area for hover detection
    mouse_area(card_button)
        .on_enter(Message::Host(HostMessage::Hover(Some(host_id))))
        .on_exit(Message::Host(HostMessage::Hover(None)))
        .into()
}

/// Empty state when no hosts are configured
fn empty_state(theme: Theme, fonts: ScaledFonts) -> Element<'static, Message> {
    let content = column![
        icon_with_color(icons::ui::SERVER, 48, theme.text_muted),
        text("No hosts configured")
            .size(fonts.heading)
            .color(theme.text_primary),
        text("Click NEW HOST to add your first server")
            .size(fonts.body)
            .color(theme.text_muted),
        Space::new().height(16),
        button(
            row![
                icon_with_color(icons::ui::PLUS, 14, iced::Color::WHITE),
                text("NEW HOST").size(fonts.body).color(iced::Color::WHITE),
            ]
            .spacing(6)
            .align_y(Alignment::Center),
        )
        .style(move |_theme, status| {
            let bg = match status {
                button::Status::Hovered => iced::Color::from_rgb8(0x00, 0x8B, 0xE8),
                _ => theme.accent,
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
        .on_press(Message::Host(HostMessage::Add)),
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
