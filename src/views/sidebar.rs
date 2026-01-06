use iced::widget::{Column, button, column, container, row, text, tooltip};
use iced::{Alignment, Element, Fill, Length};

use crate::app::{FocusSection, SidebarState};
use crate::icons::{self, icon_with_color};
use crate::message::{Message, SidebarMenuItem, UiMessage};
use crate::theme::{BORDER_RADIUS, SIDEBAR_WIDTH, SIDEBAR_WIDTH_COLLAPSED, Theme};

/// Menu item definition
struct MenuItem {
    item: SidebarMenuItem,
    icon: &'static [u8],
    label: &'static str,
}

const MENU_ITEMS: &[MenuItem] = &[
    MenuItem {
        item: SidebarMenuItem::Hosts,
        icon: icons::ui::SERVER,
        label: "Hosts",
    },
    MenuItem {
        item: SidebarMenuItem::Sftp,
        icon: icons::ui::HARD_DRIVE,
        label: "SFTP",
    },
    MenuItem {
        item: SidebarMenuItem::Snippets,
        icon: icons::ui::CODE,
        label: "Snippets",
    },
    MenuItem {
        item: SidebarMenuItem::History,
        icon: icons::ui::HISTORY,
        label: "History",
    },
    MenuItem {
        item: SidebarMenuItem::Settings,
        icon: icons::ui::SETTINGS,
        label: "Settings",
    },
    MenuItem {
        item: SidebarMenuItem::About,
        icon: icons::ui::INFO,
        label: "About",
    },
];

/// Build the sidebar view
pub fn sidebar_view(
    theme: Theme,
    state: SidebarState,
    selected: SidebarMenuItem,
    focus_section: FocusSection,
    focus_index: usize,
) -> Element<'static, Message> {
    // Completely hide sidebar when hidden
    if state == SidebarState::Hidden {
        return container(column![])
            .width(Length::Fixed(0.0))
            .height(Fill)
            .into();
    }

    let icons_only = state == SidebarState::IconsOnly;
    let sidebar_width = if icons_only {
        SIDEBAR_WIDTH_COLLAPSED
    } else {
        SIDEBAR_WIDTH
    };

    // Build menu items
    let mut menu_items = Column::new().spacing(4).padding([32, 8]);

    for (idx, menu_item) in MENU_ITEMS.iter().enumerate() {
        let is_selected = selected == menu_item.item;
        let is_focused = focus_section == FocusSection::Sidebar && idx == focus_index;
        let item_element = menu_item_button(menu_item, is_selected, is_focused, icons_only, theme);
        menu_items = menu_items.push(item_element);
    }

    let sidebar_content = column![menu_items,].height(Fill);

    // Right border (1px vertical line)
    let right_border = container(column![])
        .width(Length::Fixed(1.0))
        .height(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.border.into()),
            ..Default::default()
        });

    row![
        container(sidebar_content)
            .width(Length::Fixed(sidebar_width - 1.0))
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(theme.surface.into()),
                ..Default::default()
            }),
        right_border,
    ]
    .width(Length::Fixed(sidebar_width))
    .height(Fill)
    .into()
}

/// Single menu item button
fn menu_item_button(
    menu_item: &MenuItem,
    is_selected: bool,
    is_focused: bool,
    collapsed: bool,
    theme: Theme,
) -> Element<'static, Message> {
    let icon_color = if is_selected || is_focused {
        theme.accent
    } else {
        theme.text_secondary
    };

    let icon_widget = icon_with_color(menu_item.icon, 18, icon_color);

    let content: Element<'static, Message> = if collapsed {
        // Collapsed: just icon, centered
        container(icon_widget)
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .into()
    } else {
        // Expanded: icon + label
        row![
            container(icon_widget).width(32).align_x(Alignment::Center),
            text(menu_item.label)
                .size(16)
                .color(if is_selected || is_focused {
                    theme.text_primary
                } else {
                    theme.text_secondary
                }),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
    };

    let bg_color = if is_selected {
        Some(theme.selected.into())
    } else if is_focused {
        Some(theme.hover.into())
    } else {
        None
    };

    let btn = button(content)
        .style(move |_theme, status| {
            let background = match status {
                button::Status::Hovered if !is_selected => Some(theme.hover.into()),
                _ => bg_color,
            };
            // Focus ring border
            let border = if is_focused {
                iced::Border {
                    color: theme.focus_ring,
                    width: 2.0,
                    radius: BORDER_RADIUS.into(),
                }
            } else {
                iced::Border {
                    radius: BORDER_RADIUS.into(),
                    ..Default::default()
                }
            };
            button::Style {
                background,
                text_color: theme.text_primary,
                border,
                ..Default::default()
            }
        })
        .padding([10, 12])
        .width(Length::Fill)
        .on_press(Message::Ui(UiMessage::SidebarItemSelect(menu_item.item)));

    if collapsed {
        // Add tooltip when collapsed
        tooltip(
            btn,
            text(menu_item.label).size(12),
            tooltip::Position::Right,
        )
        .style(move |_theme| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 4.0.into(),
            },
            ..Default::default()
        })
        .padding(8)
        .into()
    } else {
        btn.into()
    }
}
