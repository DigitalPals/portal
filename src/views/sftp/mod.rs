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

use iced::widget::{Space, button, column, container, progress_bar, row, stack, text};
use iced::{Element, Fill, Length};
use uuid::Uuid;

use crate::app::managers::{TransferItem, TransferStatus};
use crate::icons::{self, icon_with_color};
use crate::message::Message;
use crate::theme::{STATUS_FAILURE, STATUS_PARTIAL, STATUS_SUCCESS, ScaledFonts, Theme};

use context_menu::context_menu_view;
use dialogs::sftp_dialog_view;
use pane::single_pane_view;

const MAX_VISIBLE_TRANSFER_ROWS: usize = 4;

/// Build the dual-pane SFTP browser view
pub fn dual_pane_sftp_view<'a>(
    state: &'a DualPaneSftpState,
    available_hosts: Vec<(Uuid, String)>,
    transfers: Vec<TransferItem>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
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

    let panes = row![left_pane, divider, right_pane];
    let content: Element<'_, Message> = if transfers.is_empty() {
        panes.into()
    } else {
        column![panes, transfer_panel(transfers, theme, fonts)]
            .height(Fill)
            .into()
    };

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

fn transfer_panel(
    transfers: Vec<TransferItem>,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let active_count = active_transfer_count(&transfers);
    let header = row![
        text("Transfers").size(fonts.body).color(theme.text_primary),
        text(active_transfer_label(active_count))
            .size(fonts.label)
            .color(theme.text_muted),
        Space::new().width(Fill),
        button(text("Clear").size(fonts.label).color(theme.text_secondary))
            .padding([3, 8])
            .style(move |_theme, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                    _ => None,
                };
                iced::widget::button::Style {
                    background: bg,
                    text_color: theme.text_secondary,
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .on_press(Message::Sftp(
                crate::message::SftpMessage::TransferClearFinished
            )),
    ]
    .spacing(8)
    .align_y(iced::Alignment::Center);

    let rows = transfers
        .iter()
        .take(MAX_VISIBLE_TRANSFER_ROWS)
        .map(|transfer| transfer_row(transfer, theme, fonts))
        .collect::<Vec<_>>();
    let hidden_count = hidden_transfer_count(transfers.len());
    let mut rows_column = column(rows).spacing(4);
    if hidden_count > 0 {
        rows_column = rows_column.push(
            text(hidden_transfer_label(hidden_count))
                .size(fonts.small)
                .color(theme.text_muted),
        );
    }

    container(column![header, rows_column].spacing(8))
        .width(Fill)
        .padding([10, 14])
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn transfer_row(
    transfer: &TransferItem,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let fraction = transfer.progress_fraction().unwrap_or(0.0);
    let status_color = match &transfer.status {
        TransferStatus::Completed => STATUS_SUCCESS,
        TransferStatus::Failed(_) => STATUS_FAILURE,
        TransferStatus::Cancelled => theme.text_muted,
        TransferStatus::Cancelling => STATUS_PARTIAL,
        _ => theme.accent,
    };
    let status_detail = transfer_status_detail(transfer);
    let count_text = transfer_count_text(transfer);

    let cancel_button: Element<'_, Message> = if transfer.status.is_finished() {
        Space::new().width(Length::Fixed(30.0)).into()
    } else {
        button(icon_with_color(icons::ui::X, 14, theme.text_secondary))
            .padding(6)
            .width(30)
            .height(30)
            .style(move |_theme, status| {
                let bg = match status {
                    iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                    _ => None,
                };
                iced::widget::button::Style {
                    background: bg,
                    text_color: theme.text_secondary,
                    border: iced::Border {
                        radius: 4.0.into(),
                        ..Default::default()
                    },
                    ..Default::default()
                }
            })
            .on_press(Message::Sftp(crate::message::SftpMessage::TransferCancel(
                transfer.id,
            )))
            .into()
    };

    column![
        row![
            text(transfer.direction.label())
                .size(fonts.label)
                .color(status_color),
            text(transfer.label.clone())
                .size(fonts.label)
                .color(theme.text_primary),
            Space::new().width(Fill),
            text(count_text).size(fonts.label).color(theme.text_muted),
            cancel_button,
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
        row![
            container(progress_bar(0.0..=1.0, fraction))
                .height(Length::Fixed(4.0))
                .width(Fill),
            text(status_detail)
                .size(fonts.small)
                .color(theme.text_muted)
                .wrapping(text::Wrapping::None),
        ]
        .spacing(8)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(3)
    .into()
}

fn active_transfer_count(transfers: &[TransferItem]) -> usize {
    transfers
        .iter()
        .filter(|transfer| !transfer.status.is_finished())
        .count()
}

fn active_transfer_label(active_count: usize) -> String {
    match active_count {
        1 => "1 active".to_string(),
        count => format!("{count} active"),
    }
}

fn hidden_transfer_count(total_count: usize) -> usize {
    total_count.saturating_sub(MAX_VISIBLE_TRANSFER_ROWS)
}

fn hidden_transfer_label(hidden_count: usize) -> String {
    match hidden_count {
        1 => "1 more transfer".to_string(),
        count => format!("{count} more transfers"),
    }
}

fn transfer_status_detail(transfer: &TransferItem) -> String {
    match &transfer.status {
        TransferStatus::Failed(error) => error.clone(),
        _ => transfer
            .current_item
            .clone()
            .unwrap_or_else(|| transfer.status.label().to_string()),
    }
}

fn transfer_count_text(transfer: &TransferItem) -> String {
    if transfer.total_files == 0 {
        transfer.status.label().to_string()
    } else {
        format!(
            "{}/{} files",
            transfer.completed_files, transfer.total_files
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::managers::{TransferDirection, TransferItemInit};
    use std::sync::{Arc, atomic::AtomicBool};

    fn transfer(
        status: TransferStatus,
        total_files: usize,
        completed_files: usize,
    ) -> TransferItem {
        let mut transfer = TransferItem::new(TransferItemInit {
            id: Uuid::new_v4(),
            tab_id: Uuid::new_v4(),
            target_pane: PaneId::Left,
            direction: TransferDirection::LocalToRemote,
            label: "payload.tar".to_string(),
            total_files,
            total_bytes: None,
            cancel_requested: Arc::new(AtomicBool::new(false)),
        });
        transfer.status = status;
        transfer.completed_files = completed_files;
        transfer
    }

    #[test]
    fn transfer_panel_counts_active_rows_only() {
        let transfers = vec![
            transfer(TransferStatus::Running, 1, 0),
            transfer(TransferStatus::Completed, 1, 1),
            transfer(TransferStatus::Cancelling, 1, 0),
        ];

        assert_eq!(active_transfer_count(&transfers), 2);
        assert_eq!(active_transfer_label(2), "2 active");
    }

    #[test]
    fn transfer_panel_reports_hidden_overflow_rows() {
        assert_eq!(hidden_transfer_count(MAX_VISIBLE_TRANSFER_ROWS), 0);
        assert_eq!(hidden_transfer_count(MAX_VISIBLE_TRANSFER_ROWS + 3), 3);
        assert_eq!(hidden_transfer_label(1), "1 more transfer");
        assert_eq!(hidden_transfer_label(3), "3 more transfers");
    }

    #[test]
    fn transfer_row_text_prefers_failure_and_file_counts() {
        let mut failed = transfer(TransferStatus::Failed("disk full".to_string()), 5, 2);
        failed.current_item = Some("ignored.txt".to_string());

        assert_eq!(transfer_status_detail(&failed), "disk full");
        assert_eq!(transfer_count_text(&failed), "2/5 files");
    }
}
