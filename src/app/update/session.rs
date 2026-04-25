//! Terminal session message handlers

use chrono::{DateTime, Utc};
use futures::stream;
use iced::Task;
use iced::clipboard;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::app::managers::{ActiveSession, SessionBackend};
use crate::app::services::{connection, history};
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
const BASE_OUTPUT_BYTES_PER_TICK: usize = 64 * 1024;
const MEDIUM_OUTPUT_BYTES_PER_TICK: usize = 256 * 1024;
const LARGE_OUTPUT_BYTES_PER_TICK: usize = 1024 * 1024;
const MEDIUM_BACKLOG_THRESHOLD: usize = 512 * 1024;
const LARGE_BACKLOG_THRESHOLD: usize = 4 * 1024 * 1024;
const OUTPUT_PROCESS_TIME_BUDGET: Duration = Duration::from_millis(8);
const OUTPUT_COALESCE_DELAY: Duration = Duration::from_millis(8);
const OUTPUT_COALESCE_BYPASS_BYTES: usize = 8 * 1024;
const BACKLOG_WARNING_THRESHOLD: usize = 1024 * 1024;
const BACKLOG_AGE_WARNING: Duration = Duration::from_millis(500);
const BACKLOG_WARNING_INTERVAL: Duration = Duration::from_secs(5);

fn output_budget_for_pending(pending_bytes: usize) -> usize {
    if pending_bytes >= LARGE_BACKLOG_THRESHOLD {
        LARGE_OUTPUT_BYTES_PER_TICK
    } else if pending_bytes >= MEDIUM_BACKLOG_THRESHOLD {
        MEDIUM_OUTPUT_BYTES_PER_TICK
    } else {
        BASE_OUTPUT_BYTES_PER_TICK
    }
}

fn should_defer_for_output_coalesce(session: &ActiveSession, now: Instant) -> bool {
    session.pending_output_bytes < OUTPUT_COALESCE_BYPASS_BYTES
        && session
            .last_data_received_at
            .is_some_and(|t| now.duration_since(t) < OUTPUT_COALESCE_DELAY)
}

fn queue_terminal_output(session: &mut ActiveSession, data: Vec<u8>, now: Instant) {
    if session.pending_output_bytes == 0 {
        session.pending_output_started_at = Some(now);
    }

    session.pending_output_bytes = session.pending_output_bytes.saturating_add(data.len());
    session.max_pending_output_bytes = session
        .max_pending_output_bytes
        .max(session.pending_output_bytes);
    session.pending_output.push_back(data);
    session.last_data_received_at = Some(now);

    enforce_pending_output_limit(session, now);
}

fn enforce_pending_output_limit(session: &mut ActiveSession, now: Instant) {
    let mut dropped_this_pass = 0usize;

    while session.pending_output_bytes > MAX_PENDING_OUTPUT_BYTES {
        if let Some(dropped) = session.pending_output.pop_front() {
            let dropped_len = dropped.len();
            session.pending_output_bytes = session.pending_output_bytes.saturating_sub(dropped_len);
            session.dropped_output_bytes = session.dropped_output_bytes.saturating_add(dropped_len);
            dropped_this_pass = dropped_this_pass.saturating_add(dropped_len);
        } else {
            session.pending_output_bytes = 0;
            session.pending_output_started_at = None;
            break;
        }
    }

    if session.pending_output_bytes == 0 {
        session.pending_output_started_at = None;
    }

    if dropped_this_pass > 0 {
        tracing::warn!(
            host = %session.host_name,
            dropped_bytes = dropped_this_pass,
            total_dropped_bytes = session.dropped_output_bytes,
            pending_bytes = session.pending_output_bytes,
            "Terminal output backlog exceeded limit; dropped oldest queued output"
        );
        session.status_message = Some(("Terminal output skipped; catching up".to_string(), now));
    }
}

fn maybe_warn_terminal_backlog(session: &mut ActiveSession, now: Instant) {
    let backlog_age = session
        .pending_output_started_at
        .map(|started| now.duration_since(started));
    let should_warn = session.pending_output_bytes >= BACKLOG_WARNING_THRESHOLD
        || backlog_age.is_some_and(|age| age >= BACKLOG_AGE_WARNING);

    if !should_warn {
        return;
    }

    if session
        .last_backlog_warning_at
        .is_some_and(|last| now.duration_since(last) < BACKLOG_WARNING_INTERVAL)
    {
        return;
    }

    session.last_backlog_warning_at = Some(now);
    let (alt_screen, app_cursor) = {
        let term = session.terminal.term();
        let term = term.lock();
        let mode = term.mode();
        (
            mode.contains(alacritty_terminal::term::TermMode::ALT_SCREEN),
            mode.contains(alacritty_terminal::term::TermMode::APP_CURSOR),
        )
    };
    tracing::warn!(
        host = %session.host_name,
        pending_bytes = session.pending_output_bytes,
        pending_chunks = session.pending_output.len(),
        max_pending_bytes = session.max_pending_output_bytes,
        dropped_bytes = session.dropped_output_bytes,
        alt_screen,
        app_cursor,
        backlog_age_ms = backlog_age.map(|age| age.as_millis()),
        last_process_ms = session
            .last_output_process_duration
            .map(|duration| duration.as_millis()),
        "Terminal output backlog is delaying rendering"
    );
}

fn process_terminal_output_tick(session: &mut ActiveSession, now: Instant) {
    if should_defer_for_output_coalesce(session, now) {
        return;
    }

    let mut byte_budget = output_budget_for_pending(session.pending_output_bytes);
    let started = Instant::now();
    let mut processed_any = false;

    while byte_budget > 0 {
        if processed_any && started.elapsed() >= OUTPUT_PROCESS_TIME_BUDGET {
            break;
        }

        let Some(mut chunk) = session.pending_output.pop_front() else {
            break;
        };

        if chunk.len() > byte_budget {
            let remainder = chunk.split_off(byte_budget);
            session.terminal.process_output(&chunk);
            session.pending_output_bytes = session.pending_output_bytes.saturating_sub(chunk.len());
            session.pending_output.push_front(remainder);
            processed_any = true;
            break;
        }

        let chunk_len = chunk.len();
        session.terminal.process_output(&chunk);
        session.pending_output_bytes = session.pending_output_bytes.saturating_sub(chunk_len);
        byte_budget = byte_budget.saturating_sub(chunk_len);
        processed_any = true;
    }

    if processed_any {
        session.last_output_process_duration = Some(started.elapsed());
    }

    if session.pending_output_bytes == 0 {
        session.pending_output_started_at = None;
    } else {
        maybe_warn_terminal_backlog(session, now);
    }
}

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

fn session_start_from_proxy_created_at(created_at: Option<DateTime<Utc>>) -> Instant {
    let now = Instant::now();
    let Some(created_at) = created_at else {
        return now;
    };

    let Ok(age) = Utc::now().signed_duration_since(created_at).to_std() else {
        return now;
    };

    now.checked_sub(age).unwrap_or(now)
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
    session_start: Instant,
) -> Task<Message> {
    // Create terminal session
    let (terminal, terminal_events) = TerminalSession::new(&host_name);

    // Store the active session
    portal.sessions.insert(
        session_id,
        ActiveSession {
            backend,
            terminal,
            session_start,
            host_name: host_name.clone(),
            host_id,
            history_entry_id,
            status_message: None,
            reconnect_attempts: 0,
            reconnect_next_attempt: None,
            pending_output: VecDeque::new(),
            pending_output_bytes: 0,
            last_data_received_at: None,
            pending_output_started_at: None,
            max_pending_output_bytes: 0,
            dropped_output_bytes: 0,
            last_output_process_duration: None,
            last_backlog_warning_at: None,
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
        if history::mark_entry_disconnected(&mut portal.config.history, session.history_entry_id) {
            if let Err(e) = portal.config.history.save() {
                tracing::error!("Failed to save history config: {}", e);
            }
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

    if !matches!(
        session.backend,
        SessionBackend::Ssh(_) | SessionBackend::Proxy(_)
    ) {
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
            portal.finish_pending_connect();

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
                Instant::now(),
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
                Instant::now(),
            )
        }
        SessionMessage::ProxyConnected {
            session_id,
            proxy_session,
            host_name,
            host_id,
            session_started_at,
        } => {
            tracing::info!("Portal Proxy connected");
            portal.finish_pending_connect();
            let has_proxy_started_at = session_started_at.is_some();
            let proxy_session_start = session_start_from_proxy_created_at(session_started_at);

            if let Some(session) = portal.sessions.get_mut(session_id) {
                session.backend = SessionBackend::Proxy(proxy_session);
                if has_proxy_started_at {
                    session.session_start = proxy_session_start;
                }
                session.reconnect_attempts = 0;
                session.reconnect_next_attempt = None;
                session.status_message =
                    Some(("Reattached via Portal Proxy".to_string(), Instant::now()));
                start_session_logger(portal, session_id);
                return Task::none();
            }

            if let Some(host_id) = host_id {
                if let Some(host) = portal.config.hosts.find_host_mut(host_id) {
                    host.last_connected = Some(chrono::Utc::now());
                    host.updated_at = chrono::Utc::now();
                    if let Err(e) = portal.config.hosts.save() {
                        tracing::error!("Failed to save host connection time: {}", e);
                    }
                }
            }

            let history_entry_id = if let Some(host_id) = host_id {
                if let Some(host) = portal.config.hosts.find_host(host_id) {
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
                }
            } else {
                Uuid::new_v4()
            };

            start_terminal_session(
                portal,
                session_id,
                SessionBackend::Proxy(proxy_session),
                host_name,
                host_id,
                history_entry_id,
                proxy_session_start,
            )
        }
        SessionMessage::Data(session_id, data) => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                if !data.is_empty() {
                    if let Some(logger) = session.logger.as_ref() {
                        logger.write(&data);
                    }
                    queue_terminal_output(session, data, Instant::now());
                }
            }
            Task::none()
        }
        SessionMessage::ProcessOutputTick => {
            let now = Instant::now();
            for session in portal.sessions.values_mut() {
                process_terminal_output_tick(session, now);
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
                    && matches!(
                        session.backend,
                        SessionBackend::Ssh(_) | SessionBackend::Proxy(_)
                    )
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

            let is_proxy = matches!(session.backend, SessionBackend::Proxy(_));
            if !matches!(
                session.backend,
                SessionBackend::Ssh(_) | SessionBackend::Proxy(_)
            ) {
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

            let use_proxy =
                is_proxy && connection::should_use_portal_proxy(&portal.prefs.portal_proxy, &host);
            let host = Arc::new(host);
            if use_proxy {
                return connection::proxy_connect_tasks(
                    portal.prefs.portal_proxy.clone(),
                    host,
                    session_id,
                    host_id,
                );
            }

            let should_detect_os = connection::should_detect_os(host.detected_os.as_ref());
            connection::ssh_connect_tasks(
                host,
                session_id,
                host_id,
                should_detect_os,
                portal.prefs.allow_agent_forwarding,
            )
        }
        SessionMessage::Error(error) => {
            tracing::error!("Session error: {}", error);
            portal.finish_pending_connect();
            portal.toast_manager.push(Toast::error(error));
            Task::none()
        }
        SessionMessage::ConnectFailed { session_id, error } => {
            tracing::error!("Session connection failed: {}", error);
            portal.finish_pending_connect();
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
                    SessionBackend::Proxy(proxy_session) => {
                        let proxy_session = proxy_session.clone();
                        return Task::perform(
                            async move {
                                if let Err(e) = proxy_session.send(&bytes).await {
                                    tracing::error!("Failed to send to Portal Proxy: {}", e);
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
                    SessionBackend::Proxy(proxy_session) => {
                        let proxy_session = proxy_session.clone();
                        return Task::perform(
                            async move {
                                if let Err(e) = proxy_session.resize(cols, rows).await {
                                    tracing::error!("Failed to resize Portal Proxy PTY: {}", e);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::LocalSession;

    fn create_test_session() -> ActiveSession {
        let (terminal, _rx) = TerminalSession::new("test-host");
        ActiveSession {
            backend: SessionBackend::Local(Arc::new(LocalSession::new_test_stub())),
            terminal,
            session_start: Instant::now(),
            host_name: "test-host".to_string(),
            host_id: None,
            history_entry_id: Uuid::new_v4(),
            status_message: None,
            reconnect_attempts: 0,
            reconnect_next_attempt: None,
            pending_output: VecDeque::new(),
            pending_output_bytes: 0,
            last_data_received_at: None,
            pending_output_started_at: None,
            max_pending_output_bytes: 0,
            dropped_output_bytes: 0,
            last_output_process_duration: None,
            last_backlog_warning_at: None,
            logger: None,
        }
    }

    #[test]
    fn output_budget_scales_with_backlog() {
        assert_eq!(
            output_budget_for_pending(MEDIUM_BACKLOG_THRESHOLD - 1),
            BASE_OUTPUT_BYTES_PER_TICK
        );
        assert_eq!(
            output_budget_for_pending(MEDIUM_BACKLOG_THRESHOLD),
            MEDIUM_OUTPUT_BYTES_PER_TICK
        );
        assert_eq!(
            output_budget_for_pending(LARGE_BACKLOG_THRESHOLD),
            LARGE_OUTPUT_BYTES_PER_TICK
        );
    }

    #[test]
    fn proxy_session_start_uses_created_at_age() {
        let created_at = Utc::now() - chrono::Duration::seconds(120);

        let session_start = session_start_from_proxy_created_at(Some(created_at));

        let elapsed = session_start.elapsed();
        assert!(elapsed >= Duration::from_secs(119));
        assert!(elapsed <= Duration::from_secs(121));
    }

    #[test]
    fn proxy_session_start_ignores_future_created_at() {
        let created_at = Utc::now() + chrono::Duration::seconds(60);

        let session_start = session_start_from_proxy_created_at(Some(created_at));

        assert!(session_start.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn small_recent_output_defers_for_coalescing() {
        let mut session = create_test_session();
        let now = Instant::now();
        queue_terminal_output(&mut session, b"prompt".to_vec(), now);

        process_terminal_output_tick(&mut session, now);

        assert_eq!(session.pending_output_bytes, b"prompt".len());
        assert_eq!(session.pending_output.len(), 1);
    }

    #[test]
    fn large_backlog_uses_catch_up_budget() {
        let mut session = create_test_session();
        let now = Instant::now();
        queue_terminal_output(&mut session, vec![b'x'; 600 * 1024], now);

        process_terminal_output_tick(&mut session, now + OUTPUT_COALESCE_DELAY);

        assert_eq!(
            session.pending_output_bytes,
            600 * 1024 - MEDIUM_OUTPUT_BYTES_PER_TICK
        );
        assert_eq!(session.pending_output.len(), 1);
        assert!(session.last_output_process_duration.is_some());
    }

    #[test]
    fn overflow_drops_whole_chunks_and_records_status() {
        let mut session = create_test_session();
        let now = Instant::now();
        let chunk_len = 8 * 1024 * 1024;

        queue_terminal_output(&mut session, vec![b'a'; chunk_len], now);
        queue_terminal_output(&mut session, vec![b'b'; chunk_len], now);
        queue_terminal_output(&mut session, vec![b'c'; 1], now);

        assert_eq!(session.pending_output_bytes, chunk_len + 1);
        assert_eq!(session.dropped_output_bytes, chunk_len);
        assert_eq!(session.pending_output.len(), 2);
        assert_eq!(
            session
                .status_message
                .as_ref()
                .map(|(message, _)| message.as_str()),
            Some("Terminal output skipped; catching up")
        );
    }
}
