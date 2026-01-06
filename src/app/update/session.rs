//! Terminal session message handlers

use std::time::Instant;
use iced::Task;
use uuid::Uuid;

use crate::app::managers::{ActiveSession, SessionBackend};
use crate::app::{Portal, Tab, View};
use crate::message::{Message, SessionMessage};
use crate::views::toast::Toast;
use crate::views::terminal_view::TerminalSession;

/// Handle terminal session messages
pub fn handle_session(portal: &mut Portal, msg: SessionMessage) -> Task<Message> {
    match msg {
        SessionMessage::Connected {
            session_id,
            host_name,
            ssh_session,
            host_id,
            detected_os,
        } => {
            tracing::info!("SSH connected to {}", host_name);

            // Update host with detected OS if available
            if let Some(os) = detected_os {
                if let Some(host) = portal.hosts_config.find_host_mut(host_id) {
                    host.detected_os = Some(os);
                    host.last_connected = Some(chrono::Utc::now());
                    host.updated_at = chrono::Utc::now();
                    if let Err(e) = portal.hosts_config.save() {
                        tracing::error!("Failed to save hosts config with detected OS: {}", e);
                    }
                }
            } else {
                // Just update last_connected
                if let Some(host) = portal.hosts_config.find_host_mut(host_id) {
                    host.last_connected = Some(chrono::Utc::now());
                    host.updated_at = chrono::Utc::now();
                    let _ = portal.hosts_config.save();
                }
            }

            // Create history entry for this connection
            let history_entry_id = if let Some(host) = portal.hosts_config.find_host(host_id) {
                let entry = crate::config::HistoryEntry::new(
                    host.id,
                    host.name.clone(),
                    host.hostname.clone(),
                    host.username.clone(),
                    crate::config::SessionType::Ssh,
                );
                let entry_id = entry.id;
                portal.history_config.add_entry(entry);
                if let Err(e) = portal.history_config.save() {
                    tracing::error!("Failed to save history config: {}", e);
                }
                entry_id
            } else {
                Uuid::new_v4()
            };

            // Create terminal session for this connection
            let terminal = TerminalSession::new(&host_name);

            // Store the active session
            portal.sessions.insert(
                session_id,
                ActiveSession {
                    backend: SessionBackend::Ssh(ssh_session),
                    terminal,
                    session_start: Instant::now(),
                    host_name: host_name.clone(),
                    history_entry_id,
                    status_message: None,
                },
            );

            // Create a new tab for this session
            let tab = Tab::new_terminal(session_id, host_name);
            portal.tabs.push(tab);
            portal.active_tab = Some(session_id);

            // Switch to terminal view
            portal.active_view = View::Terminal(session_id);

            Task::none()
        }
        SessionMessage::LocalConnected {
            session_id,
            local_session,
        } => {
            tracing::info!("Local terminal session started");

            // Create history entry for this local session
            let entry = crate::config::HistoryEntry::new_local();
            let history_entry_id = entry.id;
            portal.history_config.add_entry(entry);
            if let Err(e) = portal.history_config.save() {
                tracing::error!("Failed to save history config: {}", e);
            }

            // Create terminal session
            let terminal = TerminalSession::new("Local Terminal");

            // Store the active session
            portal.sessions.insert(
                session_id,
                ActiveSession {
                    backend: SessionBackend::Local(local_session),
                    terminal,
                    session_start: Instant::now(),
                    host_name: "Local Terminal".to_string(),
                    history_entry_id,
                    status_message: None,
                },
            );

            // Create a new tab for this session
            let tab = Tab::new_terminal(session_id, "Local Terminal".to_string());
            portal.tabs.push(tab);
            portal.active_tab = Some(session_id);

            // Switch to terminal view
            portal.active_view = View::Terminal(session_id);

            Task::none()
        }
        SessionMessage::Data(session_id, data) => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                session.terminal.process_output(&data);
            }
            Task::none()
        }
        SessionMessage::Disconnected(session_id) => {
            tracing::info!("Terminal session disconnected: {}", session_id);
            if let Some(session) = portal.sessions.get(session_id) {
                portal.history_config.mark_disconnected(session.history_entry_id);
                if let Err(e) = portal.history_config.save() {
                    tracing::error!("Failed to save history config: {}", e);
                }
            }
            portal.close_tab(session_id);
            Task::none()
        }
        SessionMessage::Error(error) => {
            tracing::error!("Session error: {}", error);
            portal.toast_manager.push(Toast::error(error));
            Task::none()
        }
        SessionMessage::Input(session_id, bytes) => {
            tracing::debug!("Terminal input for session {}: {:?}", session_id, bytes);
            if let Some(session) = portal.sessions.get(session_id) {
                match &session.backend {
                    SessionBackend::Ssh(ssh_session) => {
                        let ssh_session = ssh_session.clone();
                        return Task::perform(
                            async move {
                                if let Err(e) = ssh_session.send(&bytes).await {
                                    tracing::error!("Failed to send to SSH: {}", e);
                                }
                            },
                            |_| Message::Noop,
                        );
                    }
                    SessionBackend::Local(local_session) => {
                        let local_session = local_session.clone();
                        // Local session send is sync, but we run it in a task for consistency
                        return Task::perform(
                            async move {
                                if let Err(e) = local_session.send(&bytes) {
                                    tracing::error!("Failed to send to local PTY: {}", e);
                                }
                            },
                            |_| Message::Noop,
                        );
                    }
                }
            }
            Task::none()
        }
        SessionMessage::Resize(session_id, cols, rows) => {
            tracing::debug!("Terminal resize for session {}: {}x{}", session_id, cols, rows);
            if let Some(session) = portal.sessions.get_mut(session_id) {
                session.terminal.resize(cols, rows);
                match &session.backend {
                    SessionBackend::Ssh(ssh_session) => {
                        if let Err(e) = ssh_session.window_change(cols, rows) {
                            tracing::error!("Failed to send window change: {}", e);
                        }
                    }
                    SessionBackend::Local(local_session) => {
                        if let Err(e) = local_session.resize(cols, rows) {
                            tracing::error!("Failed to resize local PTY: {}", e);
                        }
                    }
                }
            }
            Task::none()
        }
        SessionMessage::DurationTick => {
            // No-op: triggers a re-render to update duration display
            Task::none()
        }
        SessionMessage::InstallKey(session_id) => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                if let SessionBackend::Ssh(ssh_session) = &session.backend {
                    session.status_message = Some(("Installing key...".to_string(), Instant::now()));
                    let ssh_session = ssh_session.clone();
                    return Task::perform(
                        async move { crate::ssh::install_ssh_key(&ssh_session).await },
                        move |result| {
                            Message::Session(SessionMessage::InstallKeyResult(
                                session_id,
                                result.map_err(|e| e.to_string()),
                            ))
                        },
                    );
                } else {
                    // Key installation only applies to SSH sessions
                    portal.toast_manager.push(Toast::error("Key installation is only available for SSH sessions"));
                }
            }
            Task::none()
        }
        SessionMessage::InstallKeyResult(session_id, result) => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                session.status_message = None;
            }
            match result {
                Ok(true) => {
                    portal.toast_manager.push(Toast::success("SSH key installed on remote server"));
                }
                Ok(false) => {
                    portal.toast_manager.push(Toast::success("SSH key already installed"));
                }
                Err(e) => {
                    portal.toast_manager.push(Toast::error(format!("Failed to install key: {}", e)));
                }
            }
            Task::none()
        }
    }
}
