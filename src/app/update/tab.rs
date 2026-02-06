//! Tab management message handlers

use iced::Task;

use crate::app::Portal;
use crate::message::{Message, TabContextMenuAction, TabMessage};
use crate::views::toast::Toast;

/// Handle tab management messages
pub fn handle_tab(portal: &mut Portal, msg: TabMessage) -> Task<Message> {
    match msg {
        TabMessage::Select(tab_id) => {
            tracing::info!("Tab selected");
            portal.ui.tab_context_menu.hide();
            portal.set_active_tab(tab_id);
            Task::none()
        }
        TabMessage::Close(tab_id) => {
            tracing::info!("Tab closed");
            portal.ui.tab_context_menu.hide();
            portal.close_tab(tab_id);
            Task::none()
        }
        TabMessage::New => {
            tracing::info!("New tab requested");
            portal.restore_sidebar_after_session();
            portal.enter_host_grid();
            Task::none()
        }
        TabMessage::Hover(id) => {
            portal.ui.hovered_tab = id;
            Task::none()
        }
        TabMessage::ShowContextMenu(tab_id, x, y) => {
            if portal.sessions.contains(tab_id) {
                portal.ui.tab_context_menu.show(tab_id, x, y);
            }
            Task::none()
        }
        TabMessage::HideContextMenu => {
            portal.ui.tab_context_menu.hide();
            Task::none()
        }
        TabMessage::ContextMenuAction(tab_id, action) => {
            portal.ui.tab_context_menu.hide();

            match action {
                TabContextMenuAction::OpenLogFile => {
                    if let Some(path) = portal.sessions.log_path(tab_id) {
                        if let Err(error) = open::that(&path) {
                            portal
                                .toast_manager
                                .push(Toast::error(format!("Failed to open log file: {}", error)));
                        }
                    } else {
                        portal
                            .toast_manager
                            .push(Toast::warning("No log file available for this session"));
                    }
                }
                TabContextMenuAction::OpenLogDirectory => {
                    let log_dir = portal
                        .sessions
                        .log_path(tab_id)
                        .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
                        .or_else(|| portal.prefs.session_log_dir.clone());

                    if let Some(dir) = log_dir {
                        if let Err(error) = open::that(&dir) {
                            portal.toast_manager.push(Toast::error(format!(
                                "Failed to open log directory: {}",
                                error
                            )));
                        }
                    } else {
                        portal
                            .toast_manager
                            .push(Toast::warning("Log directory is not configured"));
                    }
                }
            }

            Task::none()
        }
    }
}
