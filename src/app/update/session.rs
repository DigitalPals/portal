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
use crate::app::{Portal, Tab, View};
use crate::config::AuthMethod;
use crate::message::{Message, SearchMessage, SessionId, SessionMessage};
use crate::platform;
use crate::sftp::session::SftpSession;
use crate::ssh::reconnect::ReconnectPolicy;
use crate::terminal::backend::{TerminalEvent, paste_bytes_for_mode};
use crate::terminal::logger::SessionLogger;
use crate::terminal::search::{self as terminal_search, TerminalSearchState};
use crate::terminal_paste::{self, TerminalPastePayload};
use crate::views::tabs::{TabAgentActivity, TabAgentKind, TabAgentStatus, TabType};
use crate::views::terminal_view::TerminalSession;
use crate::views::toast::Toast;

enum ClipboardImageUploadTarget {
    Ssh(Arc<crate::ssh::SshSession>),
    Proxy(Arc<crate::proxy::ProxySession>),
    Local,
}

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
const TERMINAL_AGENT_TITLE_NOTIFICATION_THRESHOLD: Duration = Duration::from_secs(5);
const TERMINAL_NOTIFICATION_COOLDOWN: Duration = Duration::from_secs(10);
const MAX_PRE_SESSION_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const PROXY_RESUME_SNAPSHOT_PROTECTION: Duration = Duration::from_millis(750);

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

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn looks_like_attach_redraw_clear(data: &[u8]) -> bool {
    let prefix = &data[..data.len().min(256)];
    [
        b"\x1b[H\x1b[J".as_slice(),
        b"\x1b[H\x1b[2J".as_slice(),
        b"\x1b[2J".as_slice(),
        b"\x1b[3J".as_slice(),
        b"\x1b[J".as_slice(),
        b"\x1bc".as_slice(),
    ]
    .into_iter()
    .any(|sequence| contains_bytes(prefix, sequence))
}

fn should_drop_resume_attach_redraw(
    session: &mut ActiveSession,
    data: &[u8],
    now: Instant,
) -> bool {
    let Some(protected_until) = session.resume_snapshot_protected_until else {
        return false;
    };

    if now >= protected_until {
        session.resume_snapshot_protected_until = None;
        return false;
    }

    if looks_like_attach_redraw_clear(data) {
        session.resume_snapshot_protected_until = None;
        tracing::debug!(
            host = %session.host_name,
            bytes = data.len(),
            "Dropped initial Portal Hub attach redraw after seeded resume snapshot"
        );
        return true;
    }

    false
}

fn buffer_pre_session_terminal_output(portal: &mut Portal, session_id: SessionId, data: Vec<u8>) {
    if data.is_empty() {
        return;
    }

    if !portal
        .pending_connect
        .as_ref()
        .is_some_and(|pending| pending.is_for(session_id))
    {
        return;
    }

    portal
        .pre_session_terminal_output
        .entry(session_id)
        .or_default()
        .push_bounded(data, MAX_PRE_SESSION_OUTPUT_BYTES);
}

fn flush_pre_session_terminal_output(portal: &mut Portal, session_id: SessionId) {
    let Some(buffered) = portal.pre_session_terminal_output.remove(&session_id) else {
        return;
    };
    let Some(session) = portal.sessions.get_mut(session_id) else {
        return;
    };

    let now = Instant::now();
    for data in buffered.drain() {
        if should_drop_resume_attach_redraw(session, &data, now) {
            continue;
        }
        if let Some(logger) = session.logger.as_ref() {
            logger.write(&data);
        }
        queue_terminal_output(session, data, now);
    }
    process_terminal_output_tick(session, now + OUTPUT_COALESCE_DELAY);
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

struct TerminalSessionStart {
    history_entry_id: Uuid,
    session_start: Instant,
    resume_preview: Vec<u8>,
}

impl TerminalSessionStart {
    fn new(history_entry_id: Uuid, session_start: Instant) -> Self {
        Self {
            history_entry_id,
            session_start,
            resume_preview: Vec::new(),
        }
    }

    fn with_resume_preview(
        history_entry_id: Uuid,
        session_start: Instant,
        resume_preview: Vec<u8>,
    ) -> Self {
        Self {
            history_entry_id,
            session_start,
            resume_preview,
        }
    }
}

fn start_terminal_session(
    portal: &mut Portal,
    session_id: SessionId,
    backend: SessionBackend,
    host_name: String,
    host_id: Option<Uuid>,
    start: TerminalSessionStart,
) -> Task<Message> {
    // Create terminal session
    let (cols, rows) = portal.terminal_initial_size();
    let (terminal, terminal_events) = TerminalSession::new_with_size(&host_name, cols, rows);
    let resume_snapshot_protected_until = if !start.resume_preview.is_empty() {
        Some(Instant::now() + PROXY_RESUME_SNAPSHOT_PROTECTION)
    } else {
        None
    };
    if !start.resume_preview.is_empty() {
        terminal.replace_with_rendered_snapshot(&start.resume_preview);
    }
    let terminal_size = terminal.size();

    // Store the active session
    portal.sessions.insert(
        session_id,
        ActiveSession {
            backend,
            terminal,
            session_start: start.session_start,
            host_name: host_name.clone(),
            host_id,
            history_entry_id: start.history_entry_id,
            status_message: None,
            reconnect_attempts: 0,
            reconnect_next_attempt: None,
            last_terminal_size: terminal_size,
            pending_output: VecDeque::new(),
            pending_output_bytes: 0,
            last_data_received_at: None,
            pending_output_started_at: None,
            max_pending_output_bytes: 0,
            dropped_output_bytes: 0,
            last_output_process_duration: None,
            last_backlog_warning_at: None,
            terminal_agent_turn_started_at: None,
            last_terminal_notification_at: None,
            resume_snapshot_protected_until,
            logger: None,
            search: TerminalSearchState::default(),
        },
    );

    // Create a new tab for this session
    let session_number = next_terminal_session_number(&portal.tabs, &host_name);
    let tab = Tab::new_terminal(session_id, host_name, host_id, session_number);
    portal.tabs.push(tab);

    // Switch to terminal view and hide sidebar
    portal.enter_terminal_view(session_id, true);

    start_session_logger(portal, session_id);
    flush_pre_session_terminal_output(portal, session_id);

    Task::run(
        stream::unfold(terminal_events, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        }),
        move |event| Message::Session(SessionMessage::TerminalEvent(session_id, event)),
    )
}

fn finalize_disconnection(portal: &mut Portal, session_id: SessionId) {
    if let Some(session) = portal.sessions.get(session_id)
        && history::mark_entry_disconnected(&mut portal.config.history, session.history_entry_id)
        && let Err(e) = portal.config.history.save()
    {
        tracing::error!("Failed to save history config: {}", e);
    }
    portal.close_tab(session_id);
}

fn terminal_notification_name(portal: &Portal, session_id: SessionId) -> String {
    portal
        .tabs
        .iter()
        .find(|tab| tab.id == session_id)
        .map(|tab| tab.title.as_str())
        .or_else(|| {
            portal
                .sessions
                .get(session_id)
                .map(|session| session.host_name.as_str())
        })
        .unwrap_or("Terminal")
        .to_string()
}

fn next_terminal_session_number(tabs: &[Tab], title: &str) -> usize {
    let mut used: Vec<usize> = tabs
        .iter()
        .filter(|tab| tab.tab_type == TabType::Terminal && tab.title == title)
        .filter_map(|tab| tab.session_number)
        .collect();
    used.sort_unstable();

    let mut next = 1;
    for number in used {
        if number == next {
            next += 1;
        } else if number > next {
            break;
        }
    }
    next
}

fn mark_terminal_attention(portal: &mut Portal, session_id: SessionId) {
    if terminal_is_visible_and_focused(portal, session_id) {
        return;
    }

    if let Some(tab) = portal.tabs.iter_mut().find(|tab| tab.id == session_id) {
        tab.needs_attention = true;
    }
}

fn terminal_is_visible_and_focused(portal: &Portal, session_id: SessionId) -> bool {
    portal.ui.window_focused
        && matches!(portal.ui.active_view, View::Terminal(active_id) if active_id == session_id)
}

fn send_terminal_desktop_notification(
    portal: &mut Portal,
    session_id: SessionId,
    summary: impl Into<String>,
    body: impl Into<String>,
) {
    if terminal_is_visible_and_focused(portal, session_id) {
        return;
    }

    let now = Instant::now();
    let Some(session) = portal.sessions.get_mut(session_id) else {
        return;
    };

    if session
        .last_terminal_notification_at
        .is_some_and(|last| now.duration_since(last) < TERMINAL_NOTIFICATION_COOLDOWN)
    {
        return;
    }

    session.last_terminal_notification_at = Some(now);
    platform::send_desktop_notification(summary.into(), body.into());
}

fn notify_terminal_bell(portal: &mut Portal, session_id: SessionId) {
    let terminal_name = terminal_notification_name(portal, session_id);
    send_terminal_desktop_notification(
        portal,
        session_id,
        format!("Terminal bell: {}", terminal_name),
        "Background session needs attention.",
    );
}

fn notify_terminal_finished(portal: &mut Portal, session_id: SessionId) {
    let terminal_name = terminal_notification_name(portal, session_id);
    send_terminal_desktop_notification(
        portal,
        session_id,
        format!("Session closed: {}", terminal_name),
        "The terminal ended cleanly.",
    );
}

fn notify_terminal_osc(portal: &mut Portal, session_id: SessionId, title: String, body: String) {
    let terminal_name = terminal_notification_name(portal, session_id);
    let body = if body.trim().is_empty() {
        format!("Host: {}", terminal_name)
    } else {
        format!("Host: {}\n{}", terminal_name, body)
    };
    send_terminal_desktop_notification(portal, session_id, title, body);
}

fn notify_command_finished(
    portal: &mut Portal,
    session_id: SessionId,
    exit_status: Option<i32>,
    duration: Duration,
) {
    let terminal_name = terminal_notification_name(portal, session_id);
    let seconds = duration.as_secs().max(1);
    let status = exit_status
        .map(|status| status.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    send_terminal_desktop_notification(
        portal,
        session_id,
        format!("Command finished: {}", terminal_name),
        format!("Duration: {}s\nExit status: {}", seconds, status),
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalAgentTitleState {
    Active(TabAgentKind),
    Ready(Option<TabAgentKind>),
    NeedsInput(TabAgentKind),
    Clear,
}

fn terminal_agent_title_state(title: &str, turn_active: bool) -> Option<TerminalAgentTitleState> {
    let lower = title.to_ascii_lowercase();

    if codex_spinner_title(title) {
        return Some(TerminalAgentTitleState::Active(TabAgentKind::Codex));
    }

    if claude_spinner_title(title) {
        return Some(TerminalAgentTitleState::Active(TabAgentKind::Claude));
    }

    let agent_kind = if lower.contains("codex") {
        Some(TabAgentKind::Codex)
    } else if lower.contains("claude") {
        Some(TabAgentKind::Claude)
    } else {
        None
    };

    if let Some(kind) = agent_kind {
        if lower.contains("need input")
            || lower.contains("needs input")
            || lower.contains("waiting for input")
            || lower.contains("approval")
            || lower.contains("permission")
            || lower.contains("confirm")
        {
            return Some(TerminalAgentTitleState::NeedsInput(kind));
        }

        if lower.contains("working") || lower.contains("thinking") {
            return Some(TerminalAgentTitleState::Active(kind));
        }

        if lower.contains("ready") || lower.contains("idle") || lower.contains("done") {
            return Some(TerminalAgentTitleState::Ready(Some(kind)));
        }
    }

    if turn_active {
        Some(TerminalAgentTitleState::Ready(None))
    } else if agent_kind.is_none() {
        Some(TerminalAgentTitleState::Clear)
    } else {
        None
    }
}

fn codex_spinner_title(title: &str) -> bool {
    matches!(
        title.trim_start().chars().next(),
        Some(
            '\u{280b}'
                | '\u{2819}'
                | '\u{2839}'
                | '\u{2838}'
                | '\u{283c}'
                | '\u{2834}'
                | '\u{2826}'
                | '\u{2827}'
                | '\u{2807}'
                | '\u{280f}'
        )
    )
}

fn claude_spinner_title(title: &str) -> bool {
    matches!(
        title.trim_start().chars().next(),
        Some('\u{2802}' | '\u{2810}')
    )
}

fn handle_terminal_agent_title_state(portal: &mut Portal, session_id: SessionId, title: &str) {
    let turn_active = portal
        .sessions
        .get(session_id)
        .is_some_and(|session| session.terminal_agent_turn_started_at.is_some());
    let Some(state) = terminal_agent_title_state(title, turn_active) else {
        return;
    };

    match state {
        TerminalAgentTitleState::Active(kind) => {
            if let Some(session) = portal.sessions.get_mut(session_id)
                && session.terminal_agent_turn_started_at.is_none()
            {
                session.terminal_agent_turn_started_at =
                    Some((agent_display_name(kind).to_string(), Instant::now()));
            }
            set_tab_agent_status(portal, session_id, kind, TabAgentActivity::Working);
        }
        TerminalAgentTitleState::NeedsInput(kind) => {
            if let Some(session) = portal.sessions.get_mut(session_id)
                && session.terminal_agent_turn_started_at.is_none()
            {
                session.terminal_agent_turn_started_at =
                    Some((agent_display_name(kind).to_string(), Instant::now()));
            }
            set_tab_agent_status(portal, session_id, kind, TabAgentActivity::NeedsInput);
            mark_terminal_attention(portal, session_id);
            let terminal_name = terminal_notification_name(portal, session_id);
            send_terminal_desktop_notification(
                portal,
                session_id,
                format!(
                    "{} needs input: {}",
                    agent_display_name(kind),
                    terminal_name
                ),
                "Portal is waiting for your response.",
            );
        }
        TerminalAgentTitleState::Ready(kind) => {
            let started_turn = portal
                .sessions
                .get(session_id)
                .and_then(|session| session.terminal_agent_turn_started_at.clone());
            let kind = kind
                .or_else(|| tab_agent_kind(portal, session_id))
                .or_else(|| {
                    started_turn
                        .as_ref()
                        .and_then(|(name, _)| agent_kind_from_name(name))
                })
                .unwrap_or(TabAgentKind::Codex);
            set_tab_agent_status(portal, session_id, kind, TabAgentActivity::Ready);
            let Some((agent_name, started_at)) = portal
                .sessions
                .get_mut(session_id)
                .and_then(|session| session.terminal_agent_turn_started_at.take())
            else {
                return;
            };
            let duration = started_at.elapsed();
            if duration < TERMINAL_AGENT_TITLE_NOTIFICATION_THRESHOLD {
                return;
            }

            mark_terminal_attention(portal, session_id);
            let terminal_name = terminal_notification_name(portal, session_id);
            send_terminal_desktop_notification(
                portal,
                session_id,
                format!("{} ready: {}", agent_name, terminal_name),
                format!("Finished after {}s.", duration.as_secs().max(1)),
            );
        }
        TerminalAgentTitleState::Clear => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                session.terminal_agent_turn_started_at = None;
            }
            if let Some(tab) = portal.tabs.iter_mut().find(|tab| tab.id == session_id) {
                tab.agent_status = None;
            }
        }
    }
}

fn set_tab_agent_status(
    portal: &mut Portal,
    session_id: SessionId,
    kind: TabAgentKind,
    activity: TabAgentActivity,
) {
    if let Some(tab) = portal.tabs.iter_mut().find(|tab| tab.id == session_id) {
        tab.agent_status = Some(TabAgentStatus { kind, activity });
    }
}

fn tab_agent_kind(portal: &Portal, session_id: SessionId) -> Option<TabAgentKind> {
    portal
        .tabs
        .iter()
        .find(|tab| tab.id == session_id)
        .and_then(|tab| tab.agent_status.map(|status| status.kind))
}

fn agent_display_name(kind: TabAgentKind) -> &'static str {
    match kind {
        TabAgentKind::Codex => "Codex",
        TabAgentKind::Claude => "Claude Code",
    }
}

fn agent_kind_from_name(name: &str) -> Option<TabAgentKind> {
    let lower = name.to_ascii_lowercase();
    if lower.contains("codex") {
        Some(TabAgentKind::Codex)
    } else if lower.contains("claude") {
        Some(TabAgentKind::Claude)
    } else {
        None
    }
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

    if let Some(next_attempt) = session.reconnect_next_attempt
        && next_attempt > Instant::now()
    {
        return Task::none();
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

fn paste_text_into_session(
    portal: &mut Portal,
    session_id: SessionId,
    text: String,
) -> Task<Message> {
    let bytes = portal
        .sessions
        .get(session_id)
        .map(|session| {
            let term = session.terminal.term();
            let term = term.lock();
            paste_bytes_for_mode(&text, term.mode())
        })
        .unwrap_or_else(|| text.into_bytes());

    handle_session(portal, SessionMessage::Input(session_id, bytes))
}

async fn upload_clipboard_image_via_sftp(
    ssh_session: Arc<crate::ssh::SshSession>,
    filename: String,
    png: Vec<u8>,
) -> Result<String, String> {
    let sftp = SftpSession::from_ssh_session(&ssh_session)
        .await
        .map_err(|error| error.to_string())?;
    let remote_dir = terminal_paste::remote_paste_dir(sftp.home_dir());
    sftp.ensure_dir_all(&remote_dir)
        .await
        .map_err(|error| error.to_string())?;

    let portal_dir = sftp.home_dir().join(".cache").join("portal");
    if let Err(error) = sftp.set_permissions(&portal_dir, 0o700).await {
        tracing::warn!(
            "Failed to set remote Portal paste directory permissions on {}: {}",
            portal_dir.display(),
            error
        );
    }
    if let Err(error) = sftp.set_permissions(&remote_dir, 0o700).await {
        tracing::warn!(
            "Failed to set remote Portal paste directory permissions on {}: {}",
            remote_dir.display(),
            error
        );
    }

    let remote_path = remote_dir.join(filename);
    sftp.upload_bytes(&png, &remote_path)
        .await
        .map_err(|error| error.to_string())?;
    Ok(remote_path.to_string_lossy().to_string())
}

async fn save_clipboard_image_locally(filename: String, png: Vec<u8>) -> Result<String, String> {
    let dir = std::env::temp_dir().join("portal-pastes");
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|error| format!("Failed to create local paste directory: {error}"))?;
    let path = dir.join(filename);
    let mut options = tokio::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        options.mode(0o600);
    }
    use tokio::io::AsyncWriteExt;
    let mut file = options
        .open(&path)
        .await
        .map_err(|error| format!("Failed to create local paste image: {error}"))?;
    file.write_all(&png)
        .await
        .map_err(|error| format!("Failed to write local paste image: {error}"))?;
    file.flush()
        .await
        .map_err(|error| format!("Failed to flush local paste image: {error}"))?;
    Ok(path.to_string_lossy().to_string())
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
            let existing_session = portal.sessions.contains(session_id);
            if !existing_session && !portal.finish_pending_connect_for(session_id) {
                tracing::warn!("Ignoring stale SSH connection for session {}", session_id);
                return Task::none();
            }

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
                TerminalSessionStart::new(history_entry_id, Instant::now()),
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
                TerminalSessionStart::new(history_entry_id, Instant::now()),
            )
        }
        SessionMessage::ProxyConnected {
            session_id,
            proxy_session,
            host_name,
            host_id,
            session_started_at,
            resume_preview,
        } => {
            tracing::info!("Portal Hub connected");
            let existing_session = portal.sessions.contains(session_id);
            if !existing_session && !portal.finish_pending_connect_for(session_id) {
                tracing::warn!(
                    "Ignoring stale Portal Hub connection for session {}",
                    session_id
                );
                return Task::none();
            }
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
                    Some(("Reattached via Portal Hub".to_string(), Instant::now()));
                start_session_logger(portal, session_id);
                return Task::none();
            }

            if let Some(host_id) = host_id
                && let Some(host) = portal.config.hosts.find_host_mut(host_id)
            {
                host.last_connected = Some(chrono::Utc::now());
                host.updated_at = chrono::Utc::now();
                if let Err(e) = portal.config.hosts.save() {
                    tracing::error!("Failed to save host connection time: {}", e);
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
                TerminalSessionStart::with_resume_preview(
                    history_entry_id,
                    proxy_session_start,
                    resume_preview,
                ),
            )
        }
        SessionMessage::ProxyOsDetected {
            host_id,
            detected_os,
        } => {
            let Some(host_id) = host_id else {
                return Task::none();
            };
            let Some(host) = portal.config.hosts.find_host_mut(host_id) else {
                return Task::none();
            };
            if host.detected_os.as_ref() == Some(&detected_os) {
                return Task::none();
            }

            tracing::info!(
                host_id = %host_id,
                os = ?detected_os,
                "Detected target OS through Portal Hub"
            );
            host.detected_os = Some(detected_os);
            host.updated_at = chrono::Utc::now();
            if let Err(error) = portal.config.hosts.save() {
                tracing::error!("Failed to save Portal Hub detected OS: {}", error);
            }
            Task::none()
        }
        SessionMessage::Data(session_id, data) => {
            let now = Instant::now();

            let Some(session) = portal.sessions.get_mut(session_id) else {
                buffer_pre_session_terminal_output(portal, session_id, data);
                return Task::none();
            };

            if !data.is_empty() {
                if should_drop_resume_attach_redraw(session, &data, now) {
                    return Task::none();
                }
                if let Some(logger) = session.logger.as_ref() {
                    logger.write(&data);
                }
                queue_terminal_output(session, data, now);
            }
            Task::none()
        }
        SessionMessage::ProcessOutputTick => {
            let now = Instant::now();
            for session in portal.sessions.values_mut() {
                process_terminal_output_tick(session, now);
                // New output shifts buffer lines; recompute match positions.
                refresh_search_if_stale(session);
            }

            Task::none()
        }
        SessionMessage::Search(msg) => handle_search(portal, msg),
        SessionMessage::Disconnected { session_id, clean } => {
            tracing::info!("Terminal session disconnected (clean: {})", clean);
            if !portal.sessions.contains(session_id) {
                tracing::debug!(
                    "Ignoring stale terminal disconnect for closed session {}",
                    session_id
                );
                return Task::none();
            }
            let close_task = close_session_logger(portal, session_id);
            if let Some(session) = portal.sessions.get(session_id) {
                if matches!(session.backend, SessionBackend::Proxy(_)) {
                    if clean {
                        if history::mark_entry_disconnected(
                            &mut portal.config.history,
                            session.history_entry_id,
                        ) && let Err(e) = portal.config.history.save()
                        {
                            tracing::error!("Failed to save history config: {}", e);
                        }
                        if let Some(session) = portal.sessions.get_mut(session_id) {
                            session.status_message =
                                Some(("Portal Hub session ended".to_string(), Instant::now()));
                        }
                        notify_terminal_finished(portal, session_id);
                        finalize_disconnection(portal, session_id);
                        return close_task;
                    }

                    if let Some(session) = portal.sessions.get_mut(session_id) {
                        session.status_message = Some((
                            "Portal Hub session disconnected".to_string(),
                            Instant::now(),
                        ));
                    }
                    if portal.prefs.auto_reconnect {
                        let reconnect_task = schedule_reconnect(portal, session_id);
                        return Task::batch([close_task, reconnect_task]);
                    }
                    return close_task;
                }

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
            if clean {
                notify_terminal_finished(portal, session_id);
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

            // Password and keyboard-interactive hosts need the user present
            // to re-authenticate; abort auto-reconnect so they can reconnect
            // manually (which re-prompts).
            if matches!(
                host.auth,
                AuthMethod::Password | AuthMethod::KeyboardInteractive
            ) {
                session.reconnect_next_attempt = None;
                portal
                    .toast_manager
                    .push(Toast::error("Reconnect failed (authentication required)"));
                finalize_disconnection(portal, session_id);
                return Task::none();
            }

            let use_proxy =
                is_proxy && connection::should_use_portal_hub(&portal.prefs.portal_hub, &host);
            let terminal_size = session.last_terminal_size;
            let host = Arc::new(host);
            if use_proxy {
                return connection::proxy_connect_tasks(
                    portal.prefs.portal_hub.clone(),
                    host,
                    session_id,
                    host_id,
                    terminal_size,
                );
            }

            // Re-resolve the jump chain so reconnects re-establish the full
            // tunnel path with the current configuration.
            let Some(jump_chain) = portal.resolved_jump_chain(&host) else {
                finalize_disconnection(portal, session_id);
                return Task::none();
            };

            let should_detect_os = connection::should_detect_os(host.detected_os.as_ref());
            connection::ssh_connect_tasks(
                host,
                session_id,
                host_id,
                terminal_size,
                should_detect_os,
                portal.prefs.allow_agent_forwarding,
                jump_chain,
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
            if portal.sessions.contains(session_id) {
                return schedule_reconnect(portal, session_id);
            }
            if !portal.finish_pending_connect_for(session_id) {
                tracing::warn!(
                    "Ignoring stale connection failure for session {}: {}",
                    session_id,
                    error
                );
                return Task::none();
            }
            portal.pre_session_terminal_output.remove(&session_id);
            portal.toast_manager.push(Toast::error(error));
            Task::none()
        }
        SessionMessage::TerminalEvent(session_id, event) => match event {
            TerminalEvent::Title(title) => {
                if !portal.sessions.contains(session_id) {
                    return Task::none();
                }
                handle_terminal_agent_title_state(portal, session_id, &title);
                Task::none()
            }
            TerminalEvent::Bell => {
                if !portal.sessions.contains(session_id) {
                    return Task::none();
                }
                mark_terminal_attention(portal, session_id);
                notify_terminal_bell(portal, session_id);
                portal
                    .toast_manager
                    .push_or_refresh(Toast::warning("Terminal bell"));
                Task::none()
            }
            TerminalEvent::ClipboardStore(contents) => {
                if !portal.sessions.contains(session_id) {
                    return Task::none();
                }
                clipboard::write::<Message>(contents)
            }
            TerminalEvent::ClipboardLoad => {
                if !portal.sessions.contains(session_id) {
                    return Task::none();
                }
                clipboard::read().map(move |contents| {
                    Message::Session(SessionMessage::ClipboardLoaded(session_id, contents))
                })
            }
            TerminalEvent::Notification { title, body } => {
                if !portal.sessions.contains(session_id) {
                    return Task::none();
                }
                mark_terminal_attention(portal, session_id);
                notify_terminal_osc(portal, session_id, title, body);
                Task::none()
            }
            TerminalEvent::CommandFinished {
                exit_status,
                duration,
            } => {
                if !portal.sessions.contains(session_id) {
                    return Task::none();
                }
                mark_terminal_attention(portal, session_id);
                notify_command_finished(portal, session_id, exit_status, duration);
                Task::none()
            }
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
                return paste_text_into_session(portal, session_id, text);
            }
            Task::none()
        }
        SessionMessage::Paste(session_id) => {
            if !portal.sessions.contains(session_id) {
                return Task::none();
            }
            Task::perform(
                async { terminal_paste::read_clipboard_payload() },
                move |result| {
                    Message::Session(SessionMessage::PasteClipboardLoaded(session_id, result))
                },
            )
        }
        SessionMessage::PasteClipboardLoaded(session_id, result) => {
            let payload = match result {
                Ok(payload) => payload,
                Err(error) => {
                    tracing::debug!(
                        "Native clipboard paste failed, falling back to Iced text clipboard: {}",
                        error
                    );
                    let native_error = error;
                    return clipboard::read().map(move |contents| {
                        Message::Session(SessionMessage::PasteTextFallbackLoaded(
                            session_id,
                            contents,
                            native_error.clone(),
                        ))
                    });
                }
            };

            match payload {
                TerminalPastePayload::Text(text) => {
                    paste_text_into_session(portal, session_id, text)
                }
                TerminalPastePayload::ImagePng {
                    filename,
                    png,
                    width,
                    height,
                } => {
                    let upload_target = match portal.sessions.get(session_id) {
                        Some(session) => match &session.backend {
                            SessionBackend::Ssh(ssh_session) => {
                                ClipboardImageUploadTarget::Ssh(ssh_session.clone())
                            }
                            SessionBackend::Proxy(proxy_session) => {
                                ClipboardImageUploadTarget::Proxy(proxy_session.clone())
                            }
                            SessionBackend::Local(_) => ClipboardImageUploadTarget::Local,
                        },
                        None => return Task::none(),
                    };

                    let status = format!("Uploading pasted image ({}x{})...", width, height);
                    if let Some(session) = portal.sessions.get_mut(session_id) {
                        session.status_message = Some((status, Instant::now()));
                    }

                    match upload_target {
                        ClipboardImageUploadTarget::Ssh(ssh_session) => Task::perform(
                            upload_clipboard_image_via_sftp(ssh_session, filename, png),
                            move |result| {
                                Message::Session(SessionMessage::PasteImageUploaded(
                                    session_id, result,
                                ))
                            },
                        ),
                        ClipboardImageUploadTarget::Proxy(proxy_session) => Task::perform(
                            async move { proxy_session.upload_file(filename, png).await },
                            move |result| {
                                Message::Session(SessionMessage::PasteImageUploaded(
                                    session_id, result,
                                ))
                            },
                        ),
                        ClipboardImageUploadTarget::Local => Task::perform(
                            save_clipboard_image_locally(filename, png),
                            move |result| {
                                Message::Session(SessionMessage::PasteImageUploaded(
                                    session_id, result,
                                ))
                            },
                        ),
                    }
                }
            }
        }
        SessionMessage::PasteTextFallbackLoaded(session_id, contents, native_error) => {
            if let Some(text) = contents {
                return paste_text_into_session(portal, session_id, text);
            }
            portal.toast_manager.push(Toast::error(native_error));
            Task::none()
        }
        SessionMessage::PasteImageUploaded(session_id, result) => match result {
            Ok(path) => {
                if let Some(session) = portal.sessions.get_mut(session_id) {
                    session.status_message =
                        Some(("Pasted uploaded image path".to_string(), Instant::now()));
                }
                paste_text_into_session(
                    portal,
                    session_id,
                    terminal_paste::paste_text_for_uploaded_path(&path),
                )
            }
            Err(error) => {
                if let Some(session) = portal.sessions.get_mut(session_id) {
                    session.status_message =
                        Some(("Image paste failed".to_string(), Instant::now()));
                }
                portal.toast_manager.push(Toast::error(format!(
                    "Failed to paste clipboard image: {}",
                    error
                )));
                Task::none()
            }
        },
        SessionMessage::Input(session_id, bytes) => {
            tracing::debug!("Terminal input ({} bytes)", bytes.len());
            let Some(session) = portal.sessions.get_mut(session_id) else {
                return Task::none();
            };
            session.resume_snapshot_protected_until = None;
            match &session.backend {
                SessionBackend::Ssh(ssh_session) => {
                    let ssh_session = ssh_session.clone();
                    Task::perform(
                        async move {
                            if let Err(e) = ssh_session.send(&bytes).await {
                                tracing::error!("Failed to send to SSH: {}", e);
                            }
                        },
                        |_| Message::Noop,
                    )
                }
                SessionBackend::Local(local_session) => {
                    let local_session = local_session.clone();
                    Task::perform(
                        async move {
                            if let Err(e) = local_session.send(&bytes).await {
                                tracing::error!("Failed to send to local PTY: {}", e);
                            }
                        },
                        |_| Message::Noop,
                    )
                }
                SessionBackend::Proxy(proxy_session) => {
                    let proxy_session = proxy_session.clone();
                    Task::perform(
                        async move {
                            if let Err(e) = proxy_session.send(&bytes).await {
                                tracing::error!("Failed to send to Portal Hub: {}", e);
                            }
                        },
                        |_| Message::Noop,
                    )
                }
            }
        }
        SessionMessage::Resize(session_id, cols, rows) => {
            tracing::debug!("Terminal resize: {}x{}", cols, rows);
            if let Some(session) = portal.sessions.get_mut(session_id) {
                if !session.terminal.resize(cols, rows) {
                    return Task::none();
                }
                session.last_terminal_size = (cols, rows);
                // Reflowed lines invalidate search match positions.
                refresh_search_if_stale(session);
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
                                    tracing::error!("Failed to resize Portal Hub PTY: {}", e);
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

/// How to pick the active match after recomputing the match list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchSelection {
    /// Jump to the match nearest the current viewport (query changed).
    Reset,
    /// Keep the previously active match if it still exists (buffer changed).
    Preserve,
}

/// Recompute matches for the session's current query and select a match.
///
/// When `scroll` is set, the viewport jumps so the active match is visible.
fn recompute_search(session: &mut ActiveSession, selection: SearchSelection, scroll: bool) {
    let backend = &session.terminal.backend;
    let search = &mut session.search;

    let previous_match = search.current_match().cloned();
    let previous_index = search.current;

    search.matches = if search.query.is_empty() {
        Vec::new()
    } else {
        backend.search_matches(
            &search.query,
            search.case_sensitive,
            terminal_search::MAX_SEARCH_MATCHES,
        )
    };
    search.last_epoch = backend.current_epoch();

    search.current = if search.matches.is_empty() {
        None
    } else {
        match selection {
            SearchSelection::Reset => terminal_search::initial_match_index(
                &search.matches,
                backend.viewport_bottom_line(),
            ),
            SearchSelection::Preserve => previous_match
                .and_then(|previous| search.matches.iter().position(|m| *m == previous))
                .or_else(|| previous_index.map(|index| index.min(search.matches.len() - 1)))
                .or(Some(0)),
        }
    };
    search.bump_version();

    if scroll && let Some(current) = search.current_match() {
        backend.scroll_to_line(current.start().line);
    }
}

/// Re-run the search when terminal output/resize invalidated match positions.
fn refresh_search_if_stale(session: &mut ActiveSession) {
    if session.search.open
        && !session.search.query.is_empty()
        && session.search.last_epoch != session.terminal.backend.current_epoch()
    {
        // Preserve the active match and never move the viewport: this runs on
        // live output, and yanking the scroll position around would be jarring.
        recompute_search(session, SearchSelection::Preserve, false);
    }
}

/// Handle terminal scrollback search (find-in-buffer) messages.
fn handle_search(portal: &mut Portal, msg: SearchMessage) -> Task<Message> {
    match msg {
        SearchMessage::Open(session_id) => {
            let Some(session) = portal.sessions.get_mut(session_id) else {
                return Task::none();
            };
            session.search.open = true;
            session.search.bump_version();
            // Reopening with a previous query restores its highlights.
            if !session.search.query.is_empty() {
                recompute_search(session, SearchSelection::Reset, true);
            }
            iced::widget::operation::focus(crate::views::terminal_view::terminal_search_input_id())
        }
        SearchMessage::Close(session_id) => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                session.search.open = false;
                session.search.matches.clear();
                session.search.current = None;
                session.search.bump_version();
            }
            // Return keyboard focus to the terminal widget.
            portal.ui.terminal_focus_token = portal.ui.terminal_focus_token.wrapping_add(1);
            Task::none()
        }
        SearchMessage::QueryChanged(session_id, query) => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                session.search.query = query;
                recompute_search(session, SearchSelection::Reset, true);
            }
            Task::none()
        }
        SearchMessage::NextMatch(session_id) => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                refresh_search_if_stale(session);
                if session.search.select_next().is_some() {
                    session.search.bump_version();
                    if let Some(current) = session.search.current_match() {
                        session
                            .terminal
                            .backend
                            .scroll_to_line(current.start().line);
                    }
                }
            }
            Task::none()
        }
        SearchMessage::PreviousMatch(session_id) => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                refresh_search_if_stale(session);
                if session.search.select_previous().is_some() {
                    session.search.bump_version();
                    if let Some(current) = session.search.current_match() {
                        session
                            .terminal
                            .backend
                            .scroll_to_line(current.start().line);
                    }
                }
            }
            Task::none()
        }
        SearchMessage::CaseSensitiveToggled(session_id) => {
            if let Some(session) = portal.sessions.get_mut(session_id) {
                session.search.case_sensitive = !session.search.case_sensitive;
                recompute_search(session, SearchSelection::Reset, true);
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
        let terminal_size = terminal.size();
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
            last_terminal_size: terminal_size,
            pending_output: VecDeque::new(),
            pending_output_bytes: 0,
            last_data_received_at: None,
            pending_output_started_at: None,
            max_pending_output_bytes: 0,
            dropped_output_bytes: 0,
            last_output_process_duration: None,
            last_backlog_warning_at: None,
            terminal_agent_turn_started_at: None,
            last_terminal_notification_at: None,
            resume_snapshot_protected_until: None,
            logger: None,
            search: TerminalSearchState::default(),
        }
    }

    #[test]
    fn recompute_search_selects_nearest_match_and_survives_new_output() {
        let mut session = create_test_session();
        session.search.open = true;
        session.search.query = "target".to_string();
        session
            .terminal
            .process_output(b"target one\r\ntarget two\r\n");

        recompute_search(&mut session, SearchSelection::Reset, false);
        assert_eq!(session.search.matches.len(), 2);
        // Reset picks the match nearest the (bottom) viewport.
        assert_eq!(session.search.current, Some(1));

        // Move to the first match, then let new output arrive: the stale
        // refresh must re-run the search and keep the same active match.
        session.search.current = Some(0);
        let active_before = session.search.current_match().cloned().unwrap();
        session.terminal.process_output(b"target three\r\n");
        refresh_search_if_stale(&mut session);

        assert_eq!(session.search.matches.len(), 3);
        assert_eq!(session.search.current_match(), Some(&active_before));
        assert_eq!(
            session.search.last_epoch,
            session.terminal.backend.current_epoch()
        );
    }

    #[test]
    fn recompute_search_clears_matches_for_empty_query() {
        let mut session = create_test_session();
        session.search.open = true;
        session.search.query = "hit".to_string();
        session.terminal.process_output(b"hit hit");

        recompute_search(&mut session, SearchSelection::Reset, false);
        assert_eq!(session.search.matches.len(), 2);

        session.search.query.clear();
        recompute_search(&mut session, SearchSelection::Reset, false);
        assert!(session.search.matches.is_empty());
        assert_eq!(session.search.current, None);
    }

    #[test]
    fn terminal_agent_title_state_detects_ready_and_active_titles() {
        assert_eq!(
            terminal_agent_title_state("Codex - Working", false),
            Some(TerminalAgentTitleState::Active(TabAgentKind::Codex))
        );
        assert_eq!(
            terminal_agent_title_state("Codex - Thinking", false),
            Some(TerminalAgentTitleState::Active(TabAgentKind::Codex))
        );
        assert_eq!(
            terminal_agent_title_state("Codex - Ready", true),
            Some(TerminalAgentTitleState::Ready(Some(TabAgentKind::Codex)))
        );
        assert_eq!(
            terminal_agent_title_state("Codex - Needs input", true),
            Some(TerminalAgentTitleState::NeedsInput(TabAgentKind::Codex))
        );
        assert_eq!(
            terminal_agent_title_state("\u{2839} portal", false),
            Some(TerminalAgentTitleState::Active(TabAgentKind::Codex))
        );
        assert_eq!(
            terminal_agent_title_state("\u{2802} Claude Code", false),
            Some(TerminalAgentTitleState::Active(TabAgentKind::Claude))
        );
        assert_eq!(
            terminal_agent_title_state("\u{2810} Respond with confirmation message", false),
            Some(TerminalAgentTitleState::Active(TabAgentKind::Claude))
        );
        assert_eq!(
            terminal_agent_title_state("portal", true),
            Some(TerminalAgentTitleState::Ready(None))
        );
        assert_eq!(
            terminal_agent_title_state("\u{2733} Respond with confirmation message", true),
            Some(TerminalAgentTitleState::Ready(None))
        );
        assert_eq!(
            terminal_agent_title_state("\u{2733} Claude Code", false),
            None
        );
        assert_eq!(
            terminal_agent_title_state("bash", false),
            Some(TerminalAgentTitleState::Clear)
        );
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
    fn seeded_resume_preview_allows_live_output_to_continue_from_snapshot() {
        let mut session = create_test_session();
        let now = Instant::now();
        session
            .terminal
            .replace_with_rendered_snapshot(b"snapshot output");
        queue_terminal_output(&mut session, b"live output".to_vec(), now);

        process_terminal_output_tick(&mut session, now + OUTPUT_COALESCE_DELAY);

        assert_eq!(session.pending_output_bytes, 0);
        assert!(session.pending_output.is_empty());
    }

    #[test]
    fn resume_snapshot_protection_drops_initial_clear_redraw() {
        let mut session = create_test_session();
        let now = Instant::now();
        session.resume_snapshot_protected_until = Some(now + PROXY_RESUME_SNAPSHOT_PROTECTION);

        assert!(should_drop_resume_attach_redraw(
            &mut session,
            b"\x1b[H\x1b[Jredrawn attach screen",
            now
        ));
        assert!(session.resume_snapshot_protected_until.is_none());
    }

    #[test]
    fn resume_snapshot_protection_keeps_regular_live_output() {
        let mut session = create_test_session();
        let now = Instant::now();
        session.resume_snapshot_protected_until = Some(now + PROXY_RESUME_SNAPSHOT_PROTECTION);

        assert!(!should_drop_resume_attach_redraw(
            &mut session,
            b"regular live output",
            now
        ));
        assert!(session.resume_snapshot_protected_until.is_some());
    }

    #[test]
    fn resume_snapshot_protection_expires_before_clear_redraw() {
        let mut session = create_test_session();
        let now = Instant::now();
        session.resume_snapshot_protected_until = Some(now);

        assert!(!should_drop_resume_attach_redraw(
            &mut session,
            b"\x1b[2Jlater clear",
            now + Duration::from_millis(1)
        ));
        assert!(session.resume_snapshot_protected_until.is_none());
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
