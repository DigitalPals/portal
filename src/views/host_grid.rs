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

/// Format a timestamp as a relative time string (e.g. "2h ago", "3d ago")
fn format_relative_time(dt: &chrono::DateTime<chrono::Utc>) -> String {
    let now = chrono::Utc::now();
    let duration = now.signed_duration_since(*dt);

    if duration.num_minutes() < 1 {
        "just now".to_string()
    } else if duration.num_minutes() < 60 {
        format!("{}m ago", duration.num_minutes())
    } else if duration.num_hours() < 24 {
        format!("{}h ago", duration.num_hours())
    } else if duration.num_days() < 30 {
        format!("{}d ago", duration.num_days())
    } else if duration.num_days() < 365 {
        let months = duration.num_days() / 30;
        format!("{}mo ago", months)
    } else {
        let years = duration.num_days() / 365;
        format!("{}y ago", years)
    }
}

/// Group card data for the grid view
#[derive(Debug, Clone)]
pub struct GroupCard {
    pub id: Uuid,
    pub name: String,
    pub collapsed: bool,
}

/// Host card data for the grid view
#[derive(Debug, Clone)]
pub struct HostCard {
    pub id: Uuid,
    pub name: String,
    pub hostname: String,
    pub detected_os: Option<DetectedOs>,
    pub protocol: Protocol,
    pub last_connected: Option<chrono::DateTime<chrono::Utc>>,
    pub group_id: Option<Uuid>,
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

    let groups_empty = groups.is_empty();
    let hosts_empty = hosts.is_empty();

    if hosts_empty && groups_empty {
        content = content.push(empty_state(theme, fonts));
    } else {
        // Track global focus index across all hosts
        let mut global_idx: usize = 0;

        // Render each group with its hosts inline
        for group in &groups {
            let group_hosts: Vec<&HostCard> = hosts
                .iter()
                .filter(|h| h.group_id == Some(group.id))
                .collect();

            // Group section header
            let header = build_group_header(group, group_hosts.len(), theme, fonts);
            content = content.push(header);

            // Render hosts if not collapsed
            if !group.collapsed {
                let section = build_host_cards_grid(
                    &group_hosts,
                    column_count,
                    theme,
                    fonts,
                    focus_section,
                    focus_index,
                    hovered_host,
                    global_idx,
                );
                content = content.push(section);
            }
            global_idx += group_hosts.len();
        }

        // Ungrouped hosts
        let ungrouped: Vec<&HostCard> = hosts.iter().filter(|h| h.group_id.is_none()).collect();
        if !ungrouped.is_empty() {
            if !groups_empty {
                let header_text = format!("Ungrouped  ({} hosts)", ungrouped.len());
                let header = text(header_text)
                    .size(fonts.section)
                    .color(theme.text_muted);
                content = content.push(header);
            }
            let section = build_host_cards_grid(
                &ungrouped,
                column_count,
                theme,
                fonts,
                focus_section,
                focus_index,
                hovered_host,
                global_idx,
            );
            content = content.push(section);
        }
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

/// Build a group section header (clickable to collapse/expand)
fn build_group_header(
    group: &GroupCard,
    host_count: usize,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let group_id = group.id;
    let collapsed = group.collapsed;

    let chevron_icon = if collapsed {
        icons::ui::CHEVRON_RIGHT
    } else {
        icons::ui::CHEVRON_DOWN
    };

    let count_text = if host_count == 1 {
        "1 host".to_string()
    } else {
        format!("{} hosts", host_count)
    };

    let header_content = row![
        icon_with_color(icons::ui::FOLDER_CLOSED, 16, theme.accent),
        text(group.name.clone())
            .size(fonts.section)
            .color(theme.text_primary),
        text(format!("({})", count_text))
            .size(fonts.label)
            .color(theme.text_muted),
        Space::new().width(Length::Fill),
        icon_with_color(chevron_icon, 14, theme.text_muted),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    let header_btn = button(
        container(header_content)
            .padding(Padding::from([8, 4]))
            .width(Length::Fill),
    )
    .style(move |_theme, status| {
        let bg = match status {
            button::Status::Hovered => theme.hover,
            _ => iced::Color::TRANSPARENT,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: theme.text_primary,
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .padding(0)
    .width(Length::Fill)
    .on_press(Message::Ui(UiMessage::FolderToggle(group_id)));

    // Subtle bottom border
    container(header_btn)
        .width(Length::Fill)
        .style(move |_theme| container::Style {
            border: iced::Border {
                color: theme.border,
                width: 0.0,
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// Build a grid of host cards from a slice of host references
#[allow(clippy::too_many_arguments)]
fn build_host_cards_grid(
    hosts: &[&HostCard],
    column_count: usize,
    theme: Theme,
    fonts: ScaledFonts,
    focus_section: FocusSection,
    focus_index: Option<usize>,
    hovered_host: Option<Uuid>,
    global_offset: usize,
) -> Element<'static, Message> {
    let mut rows: Vec<Element<'static, Message>> = Vec::new();
    let mut current_row: Vec<Element<'static, Message>> = Vec::new();

    for (idx, host) in hosts.iter().enumerate() {
        let global_idx = global_offset + idx;
        let is_focused =
            focus_section == FocusSection::Content && focus_index == Some(global_idx);
        let is_hovered = hovered_host == Some(host.id);
        current_row.push(host_card((*host).clone(), theme, fonts, is_focused, is_hovered));

        if current_row.len() >= column_count {
            rows.push(
                Row::with_children(std::mem::take(&mut current_row))
                    .spacing(GRID_SPACING)
                    .into(),
            );
        }
    }

    if !current_row.is_empty() {
        while current_row.len() < column_count {
            current_row.push(container(text("")).width(Length::FillPortion(1)).into());
        }
        rows.push(Row::with_children(current_row).spacing(GRID_SPACING).into());
    }

    Column::with_children(rows).spacing(GRID_SPACING).into()
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
    let badge_color = match host.protocol {
        Protocol::Ssh => iced::Color::from_rgb8(59, 130, 246),
        Protocol::Vnc => iced::Color::from_rgb8(139, 92, 246),
    };
    let badge = container(
        text(protocol_label)
            .size(fonts.label)
            .color(iced::Color::WHITE),
    )
    .padding(Padding::from([2, 8]))
    .style(move |_| container::Style {
        background: Some(badge_color.into()),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
    });

    // Detail row with OS and last connected
    let last_connected_text = match &host.last_connected {
        Some(dt) => format_relative_time(dt),
        None => "never".to_string(),
    };

    let mut detail_row = Row::new().spacing(6).align_y(Alignment::Center);
    if let Some(os) = &host.detected_os {
        detail_row = detail_row.push(
            text(os.display_name().to_string())
                .size(fonts.label)
                .color(theme.text_secondary),
        );
        detail_row = detail_row.push(
            text("·")
                .size(fonts.label)
                .color(theme.text_muted),
        );
    }
    detail_row = detail_row.push(
        text(last_connected_text)
            .size(fonts.label)
            .color(theme.text_secondary),
    );

    let info = column![
        text(host.name.clone())
            .size(fonts.section)
            .color(iced::Color::WHITE),
        detail_row,
    ]
    .spacing(4);

    // Protocol badge (top-right)
    let top_right = column![badge]
        .align_x(Alignment::End);

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

    // Right side: badge at top, edit button below
    let right_side: Element<'static, Message> = column![top_right, edit_button]
        .spacing(4)
        .align_x(Alignment::End)
        .into();

    let card_content = row![
        icon_widget,
        container(info).width(Length::Fill),
        right_side,
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
