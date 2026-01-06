//! SFTP context menu rendering
//!
//! This module contains the rendering functions for the context menu
//! in the dual-pane SFTP browser.

use iced::widget::{button, container, text, Column, Space};
use iced::{Color, Element, Fill, Length, Padding};

use crate::message::{Message, SessionId, SftpMessage};
use crate::theme::Theme;
use crate::widgets::mouse_area;

use super::state::DualPaneSftpState;
use super::types::ContextMenuAction;

/// Red color for destructive actions
const DESTRUCTIVE_COLOR: Color = Color::from_rgb(0.86, 0.24, 0.24);
const CONTEXT_MENU_WIDTH: f32 = 240.0;

/// Build a context menu item button
fn context_menu_item<'a>(
    label: &'static str,
    action: ContextMenuAction,
    tab_id: SessionId,
    enabled: bool,
    is_destructive: bool,
    theme: Theme,
) -> Element<'a, Message> {
    let text_color = if !enabled {
        theme.text_muted
    } else if is_destructive {
        DESTRUCTIVE_COLOR
    } else {
        theme.text_primary
    };

    let btn = button(text(label).size(13).color(text_color))
        .padding([6, 12])
        .width(Length::Fill)
        .style(move |_theme, status| {
            let bg = if enabled {
                match status {
                    iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                    _ => None,
                }
            } else {
                None
            };
            iced::widget::button::Style {
                background: bg,
                text_color,
                border: iced::Border {
                    radius: 4.0.into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        });

    if enabled {
        btn.on_press(Message::Sftp(SftpMessage::ContextMenuAction(tab_id, action)))
            .into()
    } else {
        btn.into()
    }
}

/// Build a divider for context menu
fn context_menu_divider<'a>(theme: Theme) -> Element<'a, Message> {
    container(Space::with_height(1))
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(theme.border.into()),
            ..Default::default()
        })
        .padding([6, 12])
        .into()
}

/// Build the context menu overlay
pub fn context_menu_view(state: &DualPaneSftpState, theme: Theme) -> Element<'_, Message> {
    if !state.context_menu.visible {
        return Space::new(0, 0).into();
    }

    let pane = state.pane(state.context_menu.target_pane);
    let selection_count = pane.selected_indices.len();
    let has_selection = selection_count > 0;
    let is_single = selection_count == 1;

    // Check if any selected item is a directory or parent
    let has_dir = pane.selected_entries().iter().any(|e| e.is_dir);
    let has_parent = pane.selected_entries().iter().any(|e| e.is_parent());
    let is_file_selected = has_selection && !has_dir && !has_parent;

    let tab_id = state.tab_id;

    // Build menu items based on selection context
    // Order matches screenshot: Copy to target, Rename, Delete, divider, Refresh, New Folder, Edit Permissions
    let mut items: Vec<Element<'_, Message>> = vec![];

    // Open / Open With (only for single file selection)
    if is_single && is_file_selected {
        items.push(context_menu_item(
            "Open",
            ContextMenuAction::Open,
            tab_id,
            true,
            false,
            theme,
        ));
        items.push(context_menu_item(
            "Open With...",
            ContextMenuAction::OpenWith,
            tab_id,
            true,
            false,
            theme,
        ));
        items.push(context_menu_divider(theme));
    }

    // Copy to target directory (for any selection except parent directory)
    if has_selection && !has_parent {
        items.push(context_menu_item(
            "Copy to target directory",
            ContextMenuAction::CopyToTarget,
            tab_id,
            true,
            false,
            theme,
        ));
    }

    // Rename (only for single non-parent selection)
    if is_single && !has_parent {
        items.push(context_menu_item(
            "Rename",
            ContextMenuAction::Rename,
            tab_id,
            true,
            false,
            theme,
        ));
    }

    // Delete (for any selection except parent directory) - RED
    if has_selection && !has_parent {
        items.push(context_menu_item(
            "Delete",
            ContextMenuAction::Delete,
            tab_id,
            true,
            true, // is_destructive = true
            theme,
        ));
    }

    if has_selection && !has_parent {
        items.push(context_menu_divider(theme));
    }

    // Always available actions
    items.push(context_menu_item(
        "Refresh",
        ContextMenuAction::Refresh,
        tab_id,
        true,
        false,
        theme,
    ));
    items.push(context_menu_item(
        "New Folder",
        ContextMenuAction::NewFolder,
        tab_id,
        true,
        false,
        theme,
    ));

    // Edit Permissions (only for single file/folder selection, not parent)
    if is_single && !has_parent {
        items.push(context_menu_divider(theme));
        items.push(context_menu_item(
            "Edit Permissions",
            ContextMenuAction::EditPermissions,
            tab_id,
            true,
            false,
            theme,
        ));
    }

    // Menu container with larger radius and theme-aware background
    let menu = container(Column::with_children(items).spacing(4))
        .padding(8)
        .width(Length::Fixed(CONTEXT_MENU_WIDTH))
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 12.0.into(),
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.15),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        });
    let menu = mouse_area(menu).capture_all_events(true);

    // Position the menu at the click location
    let pos = state.context_menu.position;

    // Wrap in a clickable background to dismiss when clicking outside
    let background = mouse_area(container(Space::new(Fill, Fill)).width(Fill).height(Fill))
        .on_press(Message::Sftp(SftpMessage::HideContextMenu(tab_id)));

    // Position the menu using margins
    let positioned_menu =
        container(menu).padding(Padding::new(0.0).top(pos.y).left(pos.x));

    iced::widget::stack![background, positioned_menu].into()
}
