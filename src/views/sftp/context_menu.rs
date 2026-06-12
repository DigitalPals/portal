//! SFTP context menu rendering
//!
//! This module contains the rendering functions for the context menu
//! in the dual-pane SFTP browser.

use iced::widget::{Column, Space, button, container, text};
use iced::{Color, Element, Fill, Length, Padding};

use crate::message::{Message, SessionId, SftpMessage};
use crate::sftp::FileEntry;
use crate::theme::{ScaledFonts, Theme};
use crate::widgets::mouse_area;

use super::state::DualPaneSftpState;
use super::types::ContextMenuAction;

/// Red color for destructive actions
const DESTRUCTIVE_COLOR: Color = Color::from_rgb(0.86, 0.24, 0.24);
const CONTEXT_MENU_WIDTH: f32 = 240.0;
/// Estimated max menu height for bounds checking (7 items max * ~28px + padding)
const ESTIMATED_MENU_HEIGHT: f32 = 220.0;

/// Build a context menu item button
fn context_menu_item<'a>(
    label: &'static str,
    action: ContextMenuAction,
    tab_id: SessionId,
    enabled: bool,
    is_destructive: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let text_color = if !enabled {
        theme.text_muted
    } else if is_destructive {
        DESTRUCTIVE_COLOR
    } else {
        theme.text_primary
    };

    let btn = button(text(label).size(fonts.button_small).color(text_color))
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
        btn.on_press(Message::Sftp(SftpMessage::ContextMenuAction(
            tab_id, action,
        )))
        .into()
    } else {
        btn.into()
    }
}

fn selection_has_parent(entries: &[&FileEntry]) -> bool {
    entries.iter().any(|entry| entry.is_parent())
}

fn selection_has_dir(entries: &[&FileEntry]) -> bool {
    entries.iter().any(|entry| entry.is_dir)
}

fn selection_has_symlink(entries: &[&FileEntry]) -> bool {
    entries.iter().any(|entry| entry.is_symlink)
}

fn can_open_selection(entries: &[&FileEntry]) -> bool {
    entries.len() == 1
        && !selection_has_parent(entries)
        && !selection_has_dir(entries)
        && !selection_has_symlink(entries)
}

fn can_copy_selection(entries: &[&FileEntry]) -> bool {
    !entries.is_empty() && !selection_has_parent(entries) && !selection_has_symlink(entries)
}

fn can_edit_permissions_selection(entries: &[&FileEntry]) -> bool {
    entries.len() == 1 && !selection_has_parent(entries) && !selection_has_symlink(entries)
}

/// Build the context menu overlay
pub fn context_menu_view(
    state: &DualPaneSftpState,
    theme: Theme,
    fonts: ScaledFonts,
    window_size: iced::Size,
) -> Element<'_, Message> {
    if !state.context_menu.visible {
        return Space::new().into();
    }

    let pane = state.pane(state.context_menu.target_pane);
    let selected_entries = pane.selected_entries();
    let has_selection = !selected_entries.is_empty();
    let is_single = selected_entries.len() == 1;
    let has_parent = selection_has_parent(&selected_entries);

    let tab_id = state.tab_id;

    // Build menu items based on selection context
    // Order matches screenshot: Copy to target, Rename, Delete, divider, Refresh, New Folder, Edit Permissions
    let mut items: Vec<Element<'_, Message>> = vec![];

    // Open (only for single file selection)
    if can_open_selection(&selected_entries) {
        items.push(context_menu_item(
            "Open",
            ContextMenuAction::Open,
            tab_id,
            true,
            false,
            theme,
            fonts,
        ));
    }

    // Copy to target directory (for any selection except parent directory)
    if can_copy_selection(&selected_entries) {
        items.push(context_menu_item(
            "Copy to target directory",
            ContextMenuAction::CopyToTarget,
            tab_id,
            true,
            false,
            theme,
            fonts,
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
            fonts,
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
            fonts,
        ));
    }

    // Always available actions
    items.push(context_menu_item(
        "Refresh",
        ContextMenuAction::Refresh,
        tab_id,
        true,
        false,
        theme,
        fonts,
    ));
    items.push(context_menu_item(
        "New Folder",
        ContextMenuAction::NewFolder,
        tab_id,
        true,
        false,
        theme,
        fonts,
    ));

    // Edit Permissions (only for single file/folder selection, not parent)
    if can_edit_permissions_selection(&selected_entries) {
        items.push(context_menu_item(
            "Edit Permissions",
            ContextMenuAction::EditPermissions,
            tab_id,
            true,
            false,
            theme,
            fonts,
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

    // Position the menu at the click location, adjusting if it would overflow window bounds
    let pos = state.context_menu.position;

    let mut x = pos.x;
    let mut y = pos.y;

    // Adjust if menu would overflow right edge
    if x + CONTEXT_MENU_WIDTH > window_size.width {
        x = (window_size.width - CONTEXT_MENU_WIDTH).max(0.0);
    }

    // Adjust if menu would overflow bottom edge
    if y + ESTIMATED_MENU_HEIGHT > window_size.height {
        y = (window_size.height - ESTIMATED_MENU_HEIGHT).max(0.0);
    }

    // Wrap in a clickable background to dismiss when clicking outside
    let background = mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill),
    )
    .on_press(Message::Sftp(SftpMessage::HideContextMenu(tab_id)));

    // Position the menu using margins
    let positioned_menu = container(menu).padding(Padding::new(0.0).top(y).left(x));

    iced::widget::stack![background, positioned_menu].into()
}

#[cfg(test)]
mod tests {
    use super::{can_copy_selection, can_edit_permissions_selection, can_open_selection};
    use crate::sftp::FileEntry;
    use std::path::PathBuf;

    fn entry(name: &str, is_dir: bool, is_symlink: bool) -> FileEntry {
        FileEntry {
            name: name.to_string(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink,
            size: 0,
            modified: None,
        }
    }

    #[test]
    fn file_selection_can_open_copy_and_edit_permissions() {
        let file = entry("file.txt", false, false);
        let selection = vec![&file];

        assert!(can_open_selection(&selection));
        assert!(can_copy_selection(&selection));
        assert!(can_edit_permissions_selection(&selection));
    }

    #[test]
    fn symlink_selection_cannot_open_copy_or_edit_permissions() {
        let symlink = entry("link.txt", false, true);
        let selection = vec![&symlink];

        assert!(!can_open_selection(&selection));
        assert!(!can_copy_selection(&selection));
        assert!(!can_edit_permissions_selection(&selection));
    }

    #[test]
    fn directory_selection_can_copy_and_edit_but_not_open() {
        let dir = entry("dir", true, false);
        let selection = vec![&dir];

        assert!(!can_open_selection(&selection));
        assert!(can_copy_selection(&selection));
        assert!(can_edit_permissions_selection(&selection));
    }

    #[test]
    fn parent_selection_cannot_use_file_actions() {
        let parent = entry("..", true, false);
        let selection = vec![&parent];

        assert!(!can_open_selection(&selection));
        assert!(!can_copy_selection(&selection));
        assert!(!can_edit_permissions_selection(&selection));
    }

    #[test]
    fn mixed_selection_with_symlink_cannot_copy() {
        let file = entry("file.txt", false, false);
        let symlink = entry("link.txt", false, true);
        let selection = vec![&file, &symlink];

        assert!(!can_copy_selection(&selection));
    }
}
