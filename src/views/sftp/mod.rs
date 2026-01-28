//! SFTP file browser view - Dual-pane implementation
//!
//! This module provides a dual-pane file browser for SFTP operations.
//!
//! # Module Structure
//! - `types` - Type definitions (PaneId, PaneSource, ContextMenuAction, etc.)
//! - `state` - State management (FilePaneState, DualPaneSftpState, SftpDialogState)
//! - `pane` - Single pane rendering (header, file list, footer)
//! - `context_menu` - Context menu rendering
//! - `dialogs` - Dialog rendering (New Folder, Rename, Delete, Permissions)

mod context_menu;
mod dialogs;
mod pane;
pub mod state;
pub mod types;

// Re-export types for external use
pub use state::DualPaneSftpState;
pub use types::{
    ColumnWidths, ContextMenuAction, PaneId, PaneSource, PermissionBit, PermissionBits, SftpColumn,
    SftpDialogType,
};

use iced::widget::{Space, container, row, stack};
use iced::{Element, Fill, Length};
use uuid::Uuid;

use crate::message::Message;
use crate::theme::{ScaledFonts, Theme};

use context_menu::context_menu_view;
use dialogs::sftp_dialog_view;
use pane::single_pane_view;

/// Build the dual-pane SFTP browser view
pub fn dual_pane_sftp_view(
    state: &DualPaneSftpState,
    available_hosts: Vec<(Uuid, String)>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'_, Message> {
    let left_pane = single_pane_view(
        &state.left_pane,
        PaneId::Left,
        state.tab_id,
        available_hosts.clone(),
        state.active_pane == PaneId::Left,
        state.context_menu.visible,
        &state.left_pane.column_widths,
        theme,
        fonts,
    );

    let right_pane = single_pane_view(
        &state.right_pane,
        PaneId::Right,
        state.tab_id,
        available_hosts,
        state.active_pane == PaneId::Right,
        state.context_menu.visible,
        &state.right_pane.column_widths,
        theme,
        fonts,
    );

    // Vertical divider between panes
    let divider = container(Space::new().width(0))
        .width(Length::Fixed(1.0))
        .height(Fill)
        .style(move |_| container::Style {
            background: Some(theme.border.into()),
            ..Default::default()
        });

    let content = row![left_pane, divider, right_pane];

    let main = container(content)
        .width(Fill)
        .height(Fill)
        .style(move |_theme| container::Style {
            background: Some(theme.background.into()),
            ..Default::default()
        });

    // Overlay dialog if open (context menu is rendered at app level for correct positioning)
    if state.dialog.is_some() {
        stack![main, sftp_dialog_view(state, theme, fonts)].into()
    } else {
        main.into()
    }
}

/// Build the context menu overlay - should be rendered at app level for correct window positioning
pub fn sftp_context_menu_overlay(
    state: &DualPaneSftpState,
    theme: Theme,
    fonts: ScaledFonts,
    window_size: iced::Size,
) -> Element<'_, Message> {
    context_menu_view(state, theme, fonts, window_size)
}

/// Check if any actions menu is open in the SFTP state
pub fn has_actions_menu_open(state: &DualPaneSftpState) -> bool {
    state.left_pane.actions_menu_open || state.right_pane.actions_menu_open
}

/// Build a window-wide dismiss background for actions menus
/// This should be rendered at the app level to allow clicking anywhere to dismiss
pub fn sftp_actions_menu_dismiss_overlay(state: &DualPaneSftpState) -> Element<'_, Message> {
    use crate::widgets::mouse_area;

    // Determine which pane's menu is open (if any) to send the correct toggle message
    let (tab_id, pane_id) = if state.left_pane.actions_menu_open {
        (state.tab_id, PaneId::Left)
    } else if state.right_pane.actions_menu_open {
        (state.tab_id, PaneId::Right)
    } else {
        return Space::new().into();
    };

    mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill),
    )
    .on_press(Message::Sftp(
        crate::message::SftpMessage::ToggleActionsMenu(tab_id, pane_id),
    ))
    .into()
}
