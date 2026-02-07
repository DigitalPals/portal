//! Terminal session message handlers

use futures::stream;
use iced::Task;
use iced::clipboard;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::app::managers::{ActiveSession, SessionBackend};
use crate::app::services::connection;
use crate::app::{Portal, Tab};
use crate::config::AuthMethod;
use crate::message::{Message, SessionId, SessionMessage};
use crate::ssh::reconnect::ReconnectPolicy;
use crate::terminal::backend::TerminalEvent;
use crate::terminal::logger::SessionLogger;
use crate::views::terminal_view::TerminalSession;
use crate::views::toast::Toast;

/// Maximum bytes to buffer before dropping oldest data.
/// 16MB is generous - if we hit this, data is arriving faster than humanly readable.
const MAX_PENDING_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

fn start_session_logger(portal: &mut Portal, session_id: SessionId) {
    if !portal.prefs.session_logging_enabled {
        return;
    }

    let Some(session) = portal.sessions.get_mut(session_id) else {
        return;
    };

    if let Some(logger) = session.logger.take() {
        tokio::spawn(async move {
            logger.shutdown().await;
        });
    }

    let Some(log_dir) = portal.prefs.session_log_dir.clone() else {
        return;
    };

    match SessionLogger::start(&session.host_name, log_dir, portal.prefs.session_log_format) {
        Ok(logger) => {
            session.logger = Some(logger);
        }
        Err(error) => {
            tracing::error!("Failed to start session logger: {}", error);
        }
    }
}

fn close_session_logger(portal: &mut Portal, session_id: SessionId) -> Task<Message> {
    let Some(session) = portal.sessions.get_mut(session_id) else {
        return Task::none();
    };

    if let Some(logger) = session.logger.take() {
        return Task::perform(async move { logger.shutdown().await }, |_| Message::Noop);
    }

    Task::none()
}

fn start_terminal_session(
    portal: &mut Portal,
    session_id: SessionId,
    backend: SessionBackend,
    host_name: String,
    host_id: Option<Uuid>,
    history_entry_id: Uuid,
) -> Task<Message> {
    // Create terminal session
    let (terminal, terminal_events) = TerminalSession::new(&host_name);

    // Store the active session
    portal.sessions.insert(
        session_id,
        ActiveSession {
            backend,
            terminal,
            session_start: Instant::now(),
            host_name: host_name.clone(),
            host_id,
            history_entry_id,
            status_message: None,
            reconnect_attempts: 0,
            reconnect_next_attempt: None,
            pending_output: VecDeque::new(),
            logger: None,
        },
    );

    // Create a new tab for this session
    let tab = Tab::new_terminal(session_id, host_name, host_id);
    portal.tabs.push(tab);

    // Switch to terminal view and hide sidebar
    portal.enter_terminal_view(session_id, true);

    start_session_logger(portal, session_id);

    Task::run(
        stream::unfold(terminal_events, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        }),
        move |event| Message::Session(SessionMessage::TerminalEvent(session_id, event)),
    )
}

fn finalize_disconnection(portal: &mut Portal, session_id: SessionId) {
    if let Some(session) = portal.sessions.get(session_id) {
        portal
            .config
            .history
            .mark_disconnected(session.history_entry_id);
        if let Err(e) = portal.config.history.save() {
            tracing::error!("Failed to save history config: {}", e);
        }
    }
    portal.close_tab(session_id);
}

fn schedule_reconnect(portal: &mut Portal, session_id: SessionId) -> Task<Message> {
    let Some(session) = portal.sessions.get_mut(session_id) else {
        return Task::none();
    };

    if !portal.prefs.auto_reconnect {
        return Task::none();
    }

    if !matches!(session.backend, SessionBackend::Ssh(_)) {
        return Task::none();
    }

    if let Some(next_attempt) = session.reconnect_next_attempt {
        if next_attempt > Instant::now() {
            return Task::none();
        }
    }

    let policy = ReconnectPolicy::new(
        portal.prefs.reconnect_base_delay_ms,
        portal.prefs.reconnect_max_delay_ms,
        portal.prefs.reconnect_max_attempts,
    );

    if session.reconnect_attempts >= policy.max_attempts {
        session.reconnect_next_attempt = None;
        portal.toast_manager.push(Toast::error("Reconnect failed"));
        finalize_disconnection(portal, session_id);
        return Task::none();
    }

    let delay = policy.delay_with_jitter(session.reconnect_attempts);
    session.reconnect_attempts += 1;
    session.reconnect_next_attempt = Some(Instant::now() + delay);

    let delay_secs = delay.as_millis().div_ceil(1000).max(1);
    portal.toast_manager.push(Toast::warning(format!(
        "Reconnecting in {}s...",
        delay_secs
    )));

    Task::perform(
        async move {
            tokio::time::sleep(delay).await;
            session_id
        },
        |session_id| Message::Session(SessionMessage::Reconnect(session_id)),
    )
}

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
            tracing::info!("SSH connected");
            portal.dialogs.close_connecting();

            if let Some(session) = portal.sessions.get_mut(session_id) {
                if let Some(os) = detected_os {
                    if let Some(host) = portal.config.hosts.find_host_mut(host_id) {
                        host.detected_os = Some(os);
                        host.last_connected = Some(chrono::Utc::now());
                        host.updated_at = chrono::Utc::now();
                        if let Err(e) = portal.config.hosts.save() {
                            tracing::error!("Failed to save hosts config with detected OS: {}", e);
                        }
                    }
                } else if let Some(host) = portal.config.hosts.find_host_mut(host_id) {
                    host.last_connected = Some(chrono::Utc::now());
                    host.updated_at = chrono::Utc::now();
                    if let Err(e) = portal.config.hosts.save() {
                        tracing::error!("Failed to save host connection time: {}", e);
                    }
                }

                session.backend = SessionBackend::Ssh(ssh_session);
                session.session_start = Instant::now();
                session.reconnect_attempts = 0;
                session.reconnect_next_attempt = None;
                session.status_message = Some(("Reconnected".to_string(), Instant::now()));
                start_session_logger(portal, session_id);
                return Task::none();
            }

            // Update host with detected OS if available
            if let Some(os) = detected_os {
                if let Some(host) = portal.config.hosts.find_host_mut(host_id) {
                    host.detected_os = Some(os);
                    host.last_connected = Some(chrono::Utc::now());
                    host.updated_at = chrono::Utc::now();
                    if let Err(e) = portal.config.hosts.save() {
                        tracing::error!("Failed to save hosts config with detected OS: {}", e);
                    }
                }
            } else {
                // Just update last_connected
                if let Some(host) = portal.config.hosts.find_host_mut(host_id) {
                    host.last_connected = Some(chrono::Utc::now());
                    host.updated_at = chrono::Utc::now();
                    if let Err(e) = portal.config.hosts.save() {
                        tracing::error!("Failed to save host connection time: {}", e);
                    }
                }
            }

            // Create history entry for this connection
            let history_entry_id = if let Some(host) = portal.config.hosts.find_host(host_id) {
                let entry = crate::config::HistoryEntry::new(
                    host.id,
                    host.name.clone(),
                    host.hostname.clone(),
                    host.effective_username(),
                    crate::config::SessionType::Ssh,
                );
                let entry_id = entry.id;
                portal.config.history.add_entry(entry);
                if let Err(e) = portal.config.history.save() {
                    tracing::error!("Failed to save history config: {}", e);
                }
                entry_id
            } else {
                Uuid::new_v4()
            };

            start_terminal_session(
                portal,
                session_id,
                SessionBackend::Ssh(ssh_session),
                host_name,
                Some(host_id),
                history_entry_id,
            )
        }
        SessionMessage::LocalConnected {
            session_id,
            local_session,
        } => {
            tracing::info!("Local terminal session started");

            // Create history entry for this local session
            let entry = crate::config::HistoryEntry::new_local();
            let history_entry_id = entry.id;
            portal.config.history.add_entry(entry);
            if let Err(e) = portal.config.history.save() {
                tracing::error!("Failed to save history config: {}", e);
            }

            start_terminal_session(
                portal,
                session_id,
                SessionBackend::Local(local_session),
                "Local Terminal".to_string(),
                None,
                history_entry_id,
            )
        }
        SessionMessage::Data(session_id, data) => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                if !data.is_empty() {
                    if let Some(logger) = session.logger.as_ref() {
                        logger.write(&data);
                    }
                    session.pending_output.push_back(data);

                    // Enforce buffer size limit by dropping oldest data
                    let mut total_size: usize =
                        session.pending_output.iter().map(|chunk| chunk.len()).sum();

                    while total_size > MAX_PENDING_OUTPUT_BYTES {
                        if let Some(dropped) = session.pending_output.pop_front() {
                            total_size -= dropped.len();
                        } else {
                            break;
                        }
                    }
                }
            }
            Task::none()
        }
        SessionMessage::ProcessOutputTick => {
            // Increased from 16KB to 64KB for better throughput in fast SSH sessions.
            // At 60fps (16ms ticks), this allows ~4MB/s vs previous ~1MB/s.
            const MAX_OUTPUT_BYTES_PER_TICK: usize = 64 * 1024;

            for session in portal.sessions.values_mut() {
                let mut budget = MAX_OUTPUT_BYTES_PER_TICK;
                while budget > 0 {
                    let Some(mut chunk) = session.pending_output.pop_front() else {
                        break;
                    };

                    if chunk.len() > budget {
                        let remainder = chunk.split_off(budget);
                        session.terminal.process_output(&chunk);
                        session.pending_output.push_front(remainder);
                        budget = 0;
                    } else {
                        session.terminal.process_output(&chunk);
                        budget -= chunk.len();
                    }
                }
            }

            Task::none()
        }
        SessionMessage::Disconnected { session_id, clean } => {
            tracing::info!("Terminal session disconnected (clean: {})", clean);
            let close_task = close_session_logger(portal, session_id);
            if let Some(session) = portal.sessions.get(session_id) {
                // Only auto-reconnect for unexpected disconnections, not clean exits
                if !clean
                    && portal.prefs.auto_reconnect
                    && matches!(session.backend, SessionBackend::Ssh(_))
                {
                    let reconnect_task = schedule_reconnect(portal, session_id);
                    return Task::batch([close_task, reconnect_task]);
                }
            }
            finalize_disconnection(portal, session_id);
            close_task
        }
        SessionMessage::Reconnect(session_id) => {
            let Some(session) = portal.sessions.get_mut(session_id) else {
                return Task::none();
            };

            if !portal.prefs.auto_reconnect {
                session.reconnect_next_attempt = None;
                return Task::none();
            }

            if !matches!(session.backend, SessionBackend::Ssh(_)) {
                session.reconnect_next_attempt = None;
                return Task::none();
            }

            let Some(host_id) = session.host_id else {
                session.reconnect_next_attempt = None;
                portal.toast_manager.push(Toast::error("Reconnect failed"));
                finalize_disconnection(portal, session_id);
                return Task::none();
            };

            let Some(host) = portal.config.hosts.find_host(host_id).cloned() else {
                session.reconnect_next_attempt = None;
                portal.toast_manager.push(Toast::error("Reconnect failed"));
                finalize_disconnection(portal, session_id);
                return Task::none();
            };

            if matches!(host.auth, AuthMethod::Password) {
                session.reconnect_next_attempt = None;
                portal
                    .toast_manager
                    .push(Toast::error("Reconnect failed (password required)"));
                finalize_disconnection(portal, session_id);
                return Task::none();
            }

            let should_detect_os = connection::should_detect_os(host.detected_os.as_ref());
            connection::ssh_connect_tasks(
                Arc::new(host),
                session_id,
                host_id,
                should_detect_os,
                portal.prefs.allow_agent_forwarding,
            )
        }
        SessionMessage::Error(error) => {
            tracing::error!("Session error: {}", error);
            portal.dialogs.close_connecting();
            portal.toast_manager.push(Toast::error(error));
            Task::none()
        }
        SessionMessage::ConnectFailed { session_id, error } => {
            tracing::error!("Session connection failed: {}", error);
            portal.dialogs.close_connecting();
            if portal.sessions.contains(session_id) {
                return schedule_reconnect(portal, session_id);
            }
            portal.toast_manager.push(Toast::error(error));
            Task::none()
        }
        SessionMessage::TerminalEvent(session_id, event) => match event {
            TerminalEvent::Title(title) => {
                if let Some(tab) = portal.tabs.iter_mut().find(|tab| tab.id == session_id) {
                    tab.title = title;
                }
                Task::none()
            }
            TerminalEvent::Bell => {
                portal
                    .toast_manager
                    .push_or_refresh(Toast::warning("Terminal bell"));
                Task::none()
            }
            TerminalEvent::ClipboardStore(contents) => clipboard::write::<Message>(contents),
            TerminalEvent::ClipboardLoad => clipboard::read().map(move |contents| {
                Message::Session(SessionMessage::ClipboardLoaded(session_id, contents))
            }),
            TerminalEvent::PtyWrite(bytes) => {
                handle_session(portal, SessionMessage::Input(session_id, bytes))
            }
            TerminalEvent::Exit => handle_session(
                portal,
                SessionMessage::Disconnected {
                    session_id,
                    clean: true,
                },
            ),
            TerminalEvent::Wakeup => Task::none(),
        },
        SessionMessage::ClipboardLoaded(session_id, contents) => {
            if let Some(text) = contents {
                return handle_session(
                    portal,
                    SessionMessage::Input(session_id, text.into_bytes()),
                );
            }
            Task::none()
        }
        SessionMessage::Input(session_id, bytes) => {
            tracing::debug!("Terminal input ({} bytes)", bytes.len());
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
                        return Task::perform(
                            async move {
                                if let Err(e) = local_session.send(&bytes).await {
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
            tracing::debug!("Terminal resize: {}x{}", cols, rows);
            if let Some(session) = portal.sessions.get_mut(session_id) {
                session.terminal.resize(cols, rows);
                match &session.backend {
                    SessionBackend::Ssh(ssh_session) => {
                        let ssh_session = ssh_session.clone();
                        return Task::perform(
                            async move {
                                if let Err(e) = ssh_session.window_change(cols, rows).await {
                                    tracing::error!("Failed to send window change: {}", e);
                                }
                            },
                            |_| Message::Noop,
                        );
                    }
                    SessionBackend::Local(local_session) => {
                        let local_session = local_session.clone();
                        return Task::perform(
                            async move {
                                if let Err(e) = local_session.resize(cols, rows).await {
                                    tracing::error!("Failed to resize local PTY: {}", e);
                                }
                            },
                            |_| Message::Noop,
                        );
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
                    session.status_message =
                        Some(("Installing key...".to_string(), Instant::now()));
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
                    portal.toast_manager.push(Toast::error(
                        "Key installation is only available for SSH sessions",
                    ));
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
                    portal
                        .toast_manager
                        .push(Toast::success("SSH key installed on remote server"));
                }
                Ok(false) => {
                    portal
                        .toast_manager
                        .push(Toast::success("SSH key already installed"));
                }
                Err(e) => {
                    portal
                        .toast_manager
                        .push(Toast::error(format!("Failed to install key: {}", e)));
                }
            }
            Task::none()
        }
    }
}
