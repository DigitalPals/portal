//! Snippet management message handlers

use iced::Task;

use crate::app::Portal;
use crate::app::managers::SessionBackend;
use crate::config::Snippet;
use crate::message::{Message, SnippetField, SnippetMessage};

/// Handle snippet messages
pub fn handle_snippet(portal: &mut Portal, msg: SnippetMessage) -> Task<Message> {
    match msg {
        SnippetMessage::Select(id) => {
            if let Some(dialog) = portal.dialogs.snippets_mut() {
                dialog.selected_id = Some(id);
            }
            Task::none()
        }
        SnippetMessage::New => {
            if let Some(dialog) = portal.dialogs.snippets_mut() {
                dialog.start_new();
            }
            Task::none()
        }
        SnippetMessage::Edit(id) => {
            if let Some(dialog) = portal.dialogs.snippets_mut() {
                if let Some(snippet) = dialog.snippets.iter().find(|s| s.id == id).cloned() {
                    dialog.start_edit(&snippet);
                }
            }
            Task::none()
        }
        SnippetMessage::Delete(id) => {
            if let Some(dialog) = portal.dialogs.snippets_mut() {
                dialog.snippets.retain(|s| s.id != id);
                dialog.selected_id = None;
            }
            let _ = portal.snippets_config.delete_snippet(id);
            let _ = portal.snippets_config.save();
            Task::none()
        }
        SnippetMessage::Insert(id) => {
            tracing::debug!("Snippet insert requested for id: {}", id);
            if let Some(snippet) = portal.snippets_config.find_snippet(id) {
                let command = snippet.command.clone();
                tracing::debug!("Found snippet '{}', command: {}", snippet.name, command);
                if let Some(session_id) = portal.active_tab {
                    tracing::debug!("Active tab: {}", session_id);
                    if let Some(session) = portal.sessions.get(session_id) {
                        tracing::info!("Inserting snippet '{}' into terminal", snippet.name);
                        let data = command.into_bytes();
                        portal.dialogs.close();
                        match &session.backend {
                            SessionBackend::Ssh(ssh_session) => {
                                let ssh = ssh_session.clone();
                                return Task::perform(
                                    async move {
                                        let _ = ssh.send(&data).await;
                                    },
                                    move |_| Message::Noop,
                                );
                            }
                            SessionBackend::Local(local_session) => {
                                let local = local_session.clone();
                                return Task::perform(
                                    async move {
                                        let _ = local.send(&data);
                                    },
                                    move |_| Message::Noop,
                                );
                            }
                        }
                    } else {
                        tracing::warn!("No session found for active tab {}", session_id);
                    }
                } else {
                    tracing::warn!("No active tab when inserting snippet");
                }
            } else {
                tracing::warn!("Snippet not found: {}", id);
            }
            portal.dialogs.close();
            Task::none()
        }
        SnippetMessage::FieldChanged(field, value) => {
            if let Some(dialog) = portal.dialogs.snippets_mut() {
                match field {
                    SnippetField::Name => dialog.edit_name = value,
                    SnippetField::Command => dialog.edit_command = value,
                    SnippetField::Description => dialog.edit_description = value,
                }
            }
            Task::none()
        }
        SnippetMessage::EditCancel => {
            if let Some(dialog) = portal.dialogs.snippets_mut() {
                dialog.cancel_edit();
            }
            Task::none()
        }
        SnippetMessage::Save => {
            if let Some(dialog) = portal.dialogs.snippets_mut() {
                if dialog.is_form_valid() {
                    let now = chrono::Utc::now();
                    if let Some(id) = dialog.selected_id {
                        // Editing existing snippet
                        if let Some(snippet) = dialog.snippets.iter_mut().find(|s| s.id == id) {
                            snippet.name = dialog.edit_name.trim().to_string();
                            snippet.command = dialog.edit_command.trim().to_string();
                            snippet.description = if dialog.edit_description.trim().is_empty() {
                                None
                            } else {
                                Some(dialog.edit_description.trim().to_string())
                            };
                            snippet.updated_at = now;
                        }
                        if let Some(snippet) = portal.snippets_config.find_snippet_mut(id) {
                            snippet.name = dialog.edit_name.trim().to_string();
                            snippet.command = dialog.edit_command.trim().to_string();
                            snippet.description = if dialog.edit_description.trim().is_empty() {
                                None
                            } else {
                                Some(dialog.edit_description.trim().to_string())
                            };
                            snippet.updated_at = now;
                        }
                    } else {
                        // Creating new snippet
                        let mut snippet = Snippet::new(
                            dialog.edit_name.trim().to_string(),
                            dialog.edit_command.trim().to_string(),
                        );
                        if !dialog.edit_description.trim().is_empty() {
                            snippet.description = Some(dialog.edit_description.trim().to_string());
                        }
                        dialog.snippets.push(snippet.clone());
                        portal.snippets_config.add_snippet(snippet);
                    }
                    let _ = portal.snippets_config.save();
                    dialog.cancel_edit();
                }
            }
            Task::none()
        }
    }
}
