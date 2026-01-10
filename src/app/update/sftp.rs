//! SFTP browser message handlers

use iced::Task;
use uuid::Uuid;

use crate::app::{Portal, Tab, View};
use crate::config::SettingsConfig;
use crate::message::{Message, SftpMessage};
use crate::views::sftp::state::ColumnResizeDrag;
use crate::views::sftp::{DualPaneSftpState, PaneId, PaneSource};
use crate::views::toast::Toast;

/// Handle SFTP browser messages
pub fn handle_sftp(portal: &mut Portal, msg: SftpMessage) -> Task<Message> {
    match msg {
        SftpMessage::Open => {
            let tab_id = Uuid::new_v4();
            let dual_state = DualPaneSftpState::new_with_column_widths(
                tab_id,
                portal.sftp_column_widths.clone(),
            );
            portal.sftp.insert_tab(tab_id, dual_state);

            let tab = Tab::new_sftp(tab_id, "File Browser".to_string(), None);
            portal.tabs.push(tab);
            portal.active_tab = Some(tab_id);
            portal.active_view = View::DualSftp(tab_id);

            let left_task = portal.load_dual_pane_directory(tab_id, PaneId::Left);
            let right_task = portal.load_dual_pane_directory(tab_id, PaneId::Right);
            Task::batch([left_task, right_task])
        }
        SftpMessage::PaneSourceChanged(tab_id, pane_id, new_source) => {
            let new_path = match &new_source {
                PaneSource::Local => directories::BaseDirs::new()
                    .map(|d| d.home_dir().to_path_buf())
                    .unwrap_or_else(|| std::path::PathBuf::from("/")),
                PaneSource::Remote { session_id, .. } => {
                    if let Some(sftp) = portal.sftp.get_connection(*session_id) {
                        sftp.home_dir().to_path_buf()
                    } else {
                        tracing::warn!("SFTP connection not found");
                        return Task::none();
                    }
                }
            };

            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                let pane = tab_state.pane_mut(pane_id);
                pane.source = new_source;
                pane.current_path = new_path;
                pane.loading = true;
                pane.entries.clear();
                return portal.load_dual_pane_directory(tab_id, pane_id);
            }
            Task::none()
        }
        SftpMessage::PaneNavigate(tab_id, pane_id, path) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                tab_state.active_pane = pane_id;
                let pane = tab_state.pane_mut(pane_id);
                pane.current_path = path;
                pane.loading = true;
                return portal.load_dual_pane_directory(tab_id, pane_id);
            }
            Task::none()
        }
        SftpMessage::PaneNavigateUp(tab_id, pane_id) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                tab_state.active_pane = pane_id;
                let pane = tab_state.pane_mut(pane_id);
                if let Some(parent) = pane.current_path.parent() {
                    pane.current_path = parent.to_path_buf();
                    pane.loading = true;
                    return portal.load_dual_pane_directory(tab_id, pane_id);
                }
            }
            Task::none()
        }
        SftpMessage::PaneRefresh(tab_id, pane_id) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                tab_state.active_pane = pane_id;
                tab_state.pane_mut(pane_id).loading = true;
                return portal.load_dual_pane_directory(tab_id, pane_id);
            }
            Task::none()
        }
        SftpMessage::PaneSelect(tab_id, pane_id, index) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                tab_state.active_pane = pane_id;
                tab_state.pane_mut(pane_id).select(index);
            }
            Task::none()
        }
        SftpMessage::PaneListResult(tab_id, pane_id, result) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                let pane = tab_state.pane_mut(pane_id);
                match result {
                    Ok(entries) => pane.set_entries(entries),
                    Err(e) => pane.set_error(e),
                }
            }
            Task::none()
        }
        SftpMessage::ConnectHost(tab_id, pane_id, host_id) => {
            tracing::info!("Connecting to host for pane {:?}", pane_id);
            if let Some(host) = portal.hosts_config.find_host(host_id).cloned() {
                return portal.connect_sftp_for_pane(tab_id, pane_id, &host);
            }
            Task::none()
        }
        SftpMessage::Connected {
            tab_id,
            pane_id,
            sftp_session_id,
            host_id,
            host_name,
            sftp_session,
        } => {
            tracing::info!("SFTP connected for pane {:?}", pane_id);
            portal.sftp.clear_pending_connection();

            if let Some(host) = portal.hosts_config.find_host(host_id) {
                let entry = crate::config::HistoryEntry::new(
                    host.id,
                    host.name.clone(),
                    host.hostname.clone(),
                    host.username.clone(),
                    crate::config::SessionType::Sftp,
                );
                let entry_id = entry.id;
                portal.history_config.add_entry(entry);
                portal.sftp.insert_history_entry(sftp_session_id, entry_id);
                if let Err(e) = portal.history_config.save() {
                    tracing::error!("Failed to save history config: {}", e);
                }
            }

            let home_dir = sftp_session.home_dir().to_path_buf();
            portal.sftp.insert_connection(sftp_session_id, sftp_session);

            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                let pane = tab_state.pane_mut(pane_id);
                pane.source = PaneSource::Remote {
                    session_id: sftp_session_id,
                    host_name,
                };
                pane.current_path = home_dir;
                pane.loading = true;
                pane.entries.clear();
                return portal.load_dual_pane_directory(tab_id, pane_id);
            }
            Task::none()
        }
        SftpMessage::ShowContextMenu(tab_id, pane_id, x, y, index) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                tab_state.active_pane = pane_id;
                if let Some(idx) = index {
                    if !tab_state.pane(pane_id).is_selected(idx) {
                        tab_state.pane_mut(pane_id).select(idx);
                    }
                }
                tab_state.show_context_menu(pane_id, x, y);
            }
            Task::none()
        }
        SftpMessage::HideContextMenu(tab_id) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                tab_state.hide_context_menu();
            }
            Task::none()
        }
        SftpMessage::ContextMenuAction(tab_id, action) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                tab_state.hide_context_menu();
                return portal.handle_sftp_context_action(tab_id, action);
            }
            Task::none()
        }
        SftpMessage::DialogInputChanged(tab_id, value) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                if let Some(ref mut dialog) = tab_state.dialog {
                    dialog.input_value = value;
                    dialog.error = None;
                }
            }
            Task::none()
        }
        SftpMessage::DialogCancel(tab_id) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                tab_state.close_dialog();
            }
            Task::none()
        }
        SftpMessage::DialogSubmit(tab_id) => {
            if let Some(tab_state) = portal.sftp.get_tab(tab_id) {
                if let Some(ref dialog) = tab_state.dialog {
                    if dialog.is_valid() {
                        return portal.handle_sftp_dialog_submit(tab_id);
                    }
                }
            }
            Task::none()
        }
        SftpMessage::NewFolderResult(tab_id, pane_id, result) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                match result {
                    Ok(()) => {
                        portal.toast_manager.push(Toast::success("Folder created"));
                        tab_state.close_dialog();
                        tab_state.pane_mut(pane_id).loading = true;
                        return portal.load_dual_pane_directory(tab_id, pane_id);
                    }
                    Err(error) => {
                        if let Some(ref mut dialog) = tab_state.dialog {
                            dialog.error = Some(error);
                        }
                    }
                }
            }
            Task::none()
        }
        SftpMessage::RenameResult(tab_id, pane_id, result) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                match result {
                    Ok(()) => {
                        portal
                            .toast_manager
                            .push(Toast::success("Renamed successfully"));
                        tab_state.close_dialog();
                        tab_state.pane_mut(pane_id).loading = true;
                        return portal.load_dual_pane_directory(tab_id, pane_id);
                    }
                    Err(error) => {
                        if let Some(ref mut dialog) = tab_state.dialog {
                            dialog.error = Some(error);
                        }
                    }
                }
            }
            Task::none()
        }
        SftpMessage::DeleteResult(tab_id, pane_id, result) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                match result {
                    Ok(count) => {
                        tracing::info!("Deleted {} item(s)", count);
                        let msg = if count == 1 {
                            "Deleted 1 item".to_string()
                        } else {
                            format!("Deleted {} items", count)
                        };
                        portal.toast_manager.push(Toast::success(msg));
                        tab_state.close_dialog();
                        tab_state.pane_mut(pane_id).loading = true;
                        return portal.load_dual_pane_directory(tab_id, pane_id);
                    }
                    Err(error) => {
                        if let Some(ref mut dialog) = tab_state.dialog {
                            dialog.error = Some(error);
                        }
                    }
                }
            }
            Task::none()
        }
        SftpMessage::PermissionToggle(tab_id, bit, value) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                if let Some(ref mut dialog) = tab_state.dialog {
                    dialog.set_permission(bit, value);
                }
            }
            Task::none()
        }
        SftpMessage::PermissionsResult(tab_id, pane_id, result) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                match result {
                    Ok(()) => {
                        tracing::info!("Permissions updated successfully");
                        portal
                            .toast_manager
                            .push(Toast::success("Permissions updated"));
                        tab_state.close_dialog();
                        tab_state.pane_mut(pane_id).loading = true;
                        return portal.load_dual_pane_directory(tab_id, pane_id);
                    }
                    Err(error) => {
                        if let Some(ref mut dialog) = tab_state.dialog {
                            dialog.error = Some(error);
                        }
                    }
                }
            }
            Task::none()
        }
        SftpMessage::CopyToTarget(tab_id) => portal.handle_copy_to_target(tab_id),
        SftpMessage::CopyResult(tab_id, target_pane_id, result) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                match result {
                    Ok(count) => {
                        tracing::info!("Copied {} item(s)", count);
                        let msg = if count == 1 {
                            "Copied 1 item".to_string()
                        } else {
                            format!("Copied {} items", count)
                        };
                        portal.toast_manager.push(Toast::success(msg));
                        tab_state.pane_mut(target_pane_id).loading = true;
                        return portal.load_dual_pane_directory(tab_id, target_pane_id);
                    }
                    Err(error) => {
                        tracing::error!("Copy failed: {}", error);
                        portal
                            .toast_manager
                            .push(Toast::error(format!("Copy failed: {}", error)));
                    }
                }
            }
            Task::none()
        }
        SftpMessage::ToggleShowHidden(tab_id, pane_id) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                let pane = tab_state.pane_mut(pane_id);
                pane.show_hidden = !pane.show_hidden;
                pane.actions_menu_open = false;
                // Clear selection since indices may change
                pane.selected_indices.clear();
                pane.last_selected_index = None;
            }
            Task::none()
        }
        SftpMessage::ToggleActionsMenu(tab_id, pane_id) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                let pane = tab_state.pane_mut(pane_id);
                pane.actions_menu_open = !pane.actions_menu_open;
            }
            Task::none()
        }
        SftpMessage::FilterChanged(tab_id, pane_id, text) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                let pane = tab_state.pane_mut(pane_id);
                pane.filter_text = text;
                // Clear selection since filtered indices may change
                pane.selected_indices.clear();
                pane.last_selected_index = None;
            }
            Task::none()
        }
        SftpMessage::PaneBreadcrumbNavigate(tab_id, pane_id, path) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                tab_state.active_pane = pane_id;
                let pane = tab_state.pane_mut(pane_id);
                pane.current_path = path;
                pane.loading = true;
                return portal.load_dual_pane_directory(tab_id, pane_id);
            }
            Task::none()
        }
        SftpMessage::ColumnResizeStart(tab_id, pane_id, column, start_x) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                let pane = tab_state.pane(pane_id);
                tab_state.column_resize_drag = Some(ColumnResizeDrag {
                    pane_id,
                    column,
                    start_x,
                    original_widths: pane.column_widths.clone(),
                });
            }
            Task::none()
        }
        SftpMessage::ColumnResizing(tab_id, current_x) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                if let Some(ref drag) = tab_state.column_resize_drag.clone() {
                    // Direct pixel-based resize: new width = original + delta
                    let delta = current_x - drag.start_x;
                    let original_width = drag.original_widths.get(drag.column);
                    let new_width = original_width + delta;

                    // The set() method enforces minimum width
                    let pane = tab_state.pane_mut(drag.pane_id);
                    if (pane.column_widths.get(drag.column) - new_width).abs() > 0.5 {
                        pane.column_widths.set(drag.column, new_width);
                    }
                }
            }
            Task::none()
        }
        SftpMessage::ColumnResizeEnd(tab_id) => {
            if let Some(tab_state) = portal.sftp.get_tab_mut(tab_id) {
                if let Some(ref drag) = tab_state.column_resize_drag {
                    // Update the app's column widths from the active pane and persist to settings
                    let pane = tab_state.pane(drag.pane_id);
                    portal.sftp_column_widths = pane.column_widths.clone();

                    // Save to disk
                    let mut settings = SettingsConfig::load().unwrap_or_default();
                    settings.sftp_column_widths = portal.sftp_column_widths.clone();
                    if let Err(e) = settings.save() {
                        tracing::error!("Failed to save column widths: {}", e);
                    }
                }
                tab_state.column_resize_drag = None;
            }
            Task::none()
        }
    }
}
