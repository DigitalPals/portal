//! Tab management message handlers

use iced::Task;

use crate::app::Portal;
use crate::app::managers::SessionBackend;
use crate::message::{Message, TabContextMenuAction, TabMessage};
use crate::views::tabs::{TabType, tab_rename_input_id};
use crate::views::toast::Toast;

const MAX_TAB_TITLE_CHARS: usize = 80;

fn start_rename(portal: &mut Portal, tab_id: uuid::Uuid) -> Task<Message> {
    let Some(tab) = portal.tabs.iter_mut().find(|tab| tab.id == tab_id) else {
        return Task::none();
    };
    if tab.tab_type == TabType::NewConnection {
        return Task::none();
    }
    tab.rename_value = Some(tab.title.clone());
    portal.ui.tab_context_menu.hide();
    iced::widget::operation::focus(tab_rename_input_id(tab_id))
}

fn submit_rename(portal: &mut Portal, tab_id: uuid::Uuid) -> Task<Message> {
    let Some(tab) = portal.tabs.iter().find(|tab| tab.id == tab_id) else {
        return Task::none();
    };
    let Some(value) = tab.rename_value.as_deref() else {
        return Task::none();
    };
    let title = value.trim();
    if title.is_empty() {
        portal
            .toast_manager
            .push(Toast::warning("Tab name cannot be empty"));
        return iced::widget::operation::focus(tab_rename_input_id(tab_id));
    }
    if title.chars().count() > MAX_TAB_TITLE_CHARS {
        portal.toast_manager.push(Toast::warning(format!(
            "Tab names can be at most {MAX_TAB_TITLE_CHARS} characters"
        )));
        return iced::widget::operation::focus(tab_rename_input_id(tab_id));
    }
    if title.chars().any(char::is_control) {
        portal.toast_manager.push(Toast::warning(
            "Tab names cannot contain control characters",
        ));
        return iced::widget::operation::focus(tab_rename_input_id(tab_id));
    }

    let requested_title = title.to_string();
    let persist_to_hub = portal
        .sessions
        .get(tab_id)
        .is_some_and(|session| matches!(&session.backend, SessionBackend::Proxy(_)));
    let tab = portal
        .tabs
        .iter_mut()
        .find(|tab| tab.id == tab_id)
        .expect("tab checked above");
    let previous_title = std::mem::replace(&mut tab.title, requested_title.clone());
    tab.rename_value = None;

    if previous_title == requested_title || !persist_to_hub {
        return Task::none();
    }

    let settings = portal.prefs.portal_hub.clone();
    let persisted_title = requested_title.clone();
    Task::perform(
        async move { crate::proxy::rename_session(&settings, tab_id, Some(&persisted_title)).await },
        move |result| {
            Message::Tab(TabMessage::RenamePersisted {
                tab_id,
                requested_title,
                previous_title,
                result,
            })
        },
    )
}

/// Handle tab management messages
pub fn handle_tab(portal: &mut Portal, msg: TabMessage) -> Task<Message> {
    match msg {
        TabMessage::Select(tab_id) => {
            tracing::info!("Tab selected");
            let rename_task = portal
                .tabs
                .iter()
                .find(|tab| tab.rename_value.is_some() && tab.id != tab_id)
                .map(|tab| tab.id)
                .map(|rename_id| submit_rename(portal, rename_id))
                .unwrap_or_else(Task::none);
            portal.ui.tab_context_menu.hide();
            portal.set_active_tab(tab_id);
            rename_task
        }
        TabMessage::Close(tab_id) => {
            tracing::info!("Tab closed");
            portal.ui.tab_context_menu.hide();
            portal.close_tab(tab_id);
            Task::none()
        }
        TabMessage::Reorder { from, to } => {
            portal.move_tab(from, to);
            Task::none()
        }
        TabMessage::New => {
            tracing::info!("New tab requested");
            let rename_id = portal
                .tabs
                .iter()
                .find(|tab| tab.rename_value.is_some())
                .map(|tab| tab.id);
            let rename_task = rename_id
                .map(|rename_id| submit_rename(portal, rename_id))
                .unwrap_or_else(Task::none);
            portal.open_new_tab();
            rename_task
        }
        TabMessage::RenameStart(tab_id) => start_rename(portal, tab_id),
        TabMessage::RenameChanged(tab_id, value) => {
            if let Some(tab) = portal.tabs.iter_mut().find(|tab| tab.id == tab_id)
                && tab.rename_value.is_some()
            {
                tab.rename_value = Some(value);
            }
            Task::none()
        }
        TabMessage::RenameSubmit(tab_id) => submit_rename(portal, tab_id),
        TabMessage::RenameCancel(tab_id) => {
            if let Some(tab) = portal.tabs.iter_mut().find(|tab| tab.id == tab_id) {
                tab.rename_value = None;
            }
            Task::none()
        }
        TabMessage::RenamePersisted {
            tab_id,
            requested_title,
            previous_title,
            result,
        } => {
            if let Err(error) = result {
                if let Some(tab) = portal.tabs.iter_mut().find(|tab| tab.id == tab_id)
                    && tab.title == requested_title
                {
                    tab.title = previous_title;
                }
                portal.toast_manager.push(Toast::error(format!(
                    "Could not save tab name to Portal Hub: {error}"
                )));
            }
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
                TabContextMenuAction::Rename => return start_rename(portal, tab_id),
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
