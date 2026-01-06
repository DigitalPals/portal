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
pub use types::{
    ContextMenuAction, PaneId, PaneSource, PermissionBit, PermissionBits, SftpDialogType,
};
pub use state::DualPaneSftpState;

use iced::widget::{container, row, stack, Space};
use iced::{Element, Fill, Length};
use uuid::Uuid;

use crate::message::Message;
use crate::theme::Theme;

use context_menu::context_menu_view;
use dialogs::sftp_dialog_view;
use pane::single_pane_view;

/// Build the dual-pane SFTP browser view
pub fn dual_pane_sftp_view(
    state: &DualPaneSftpState,
    available_hosts: Vec<(Uuid, String)>,
    theme: Theme,
) -> Element<'_, Message> {
    let left_pane = single_pane_view(
        &state.left_pane,
        PaneId::Left,
        state.tab_id,
        available_hosts.clone(),
        state.active_pane == PaneId::Left,
        state.context_menu.visible,
        theme,
    );

    let right_pane = single_pane_view(
        &state.right_pane,
        PaneId::Right,
        state.tab_id,
        available_hosts,
        state.active_pane == PaneId::Right,
        state.context_menu.visible,
        theme,
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
        stack![main, sftp_dialog_view(state, theme)].into()
    } else {
        main.into()
    }
}

/// Build the context menu overlay - should be rendered at app level for correct window positioning
pub fn sftp_context_menu_overlay(state: &DualPaneSftpState, theme: Theme) -> Element<'_, Message> {
    context_menu_view(state, theme)
}
