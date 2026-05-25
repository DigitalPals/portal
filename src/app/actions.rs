use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use futures::stream;
use iced::Task;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::{AuthMethod, Host};
use crate::fs_utils::{
    cleanup_temp_dir, copy_dir_recursive, count_items_in_dir, ensure_not_same_path,
    ensure_not_symlink,
};
use crate::keybindings::AppAction;
use crate::local::{LocalEvent, LocalSession};
use crate::local_fs::list_local_dir;
use crate::message::{Message, SessionId, SessionMessage, SftpMessage, VncMessage};
use crate::sftp::SharedSftpSession;
use crate::views::dialogs::password_dialog::PasswordDialogState;
use crate::views::file_viewer::{FileSource, FileType};
use crate::views::sftp::{ContextMenuAction, PaneId, PaneSource, PermissionBits, SftpDialogType};
use crate::views::tabs::Tab;
use crate::views::toast::Toast;

use super::managers::{
    SessionBackend, TransferDirection, TransferItem, TransferItemInit, TransferProgress,
};
use super::services::{connection, file_viewer, history};
use super::{FocusSection, Portal, View};

const TRANSFER_PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectionLaunchMode {
    Default,
    FreshSession,
}

#[derive(Debug, Clone)]
struct SftpTransferEntry {
    name: String,
    path: std::path::PathBuf,
    is_dir: bool,
    is_symlink: bool,
    size: u64,
}

#[derive(Debug, Clone)]
enum SftpTransferEndpoint {
    Local,
    Remote(SharedSftpSession),
}

#[derive(Debug, Clone)]
struct SftpTransferRequest {
    tab_id: SessionId,
    target_pane_id: PaneId,
    target_dir: std::path::PathBuf,
    source: SftpTransferEndpoint,
    target: SftpTransferEndpoint,
    entries: Vec<SftpTransferEntry>,
}

impl SftpTransferRequest {
    fn direction(&self) -> TransferDirection {
        match (&self.source, &self.target) {
            (SftpTransferEndpoint::Local, SftpTransferEndpoint::Local) => {
                TransferDirection::LocalToLocal
            }
            (SftpTransferEndpoint::Local, SftpTransferEndpoint::Remote(_)) => {
                TransferDirection::LocalToRemote
            }
            (SftpTransferEndpoint::Remote(_), SftpTransferEndpoint::Local) => {
                TransferDirection::RemoteToLocal
            }
            (SftpTransferEndpoint::Remote(_), SftpTransferEndpoint::Remote(_)) => {
                TransferDirection::RemoteToRemote
            }
        }
    }

    fn total_bytes(&self) -> Option<u64> {
        let total = self
            .entries
            .iter()
            .filter(|entry| !entry.is_dir)
            .map(|entry| entry.size)
            .sum::<u64>();
        (total > 0).then_some(total)
    }

    fn label(&self) -> String {
        match self.entries.as_slice() {
            [entry] => entry.name.clone(),
            entries => format!("{} items", entries.len()),
        }
    }
}

fn sftp_transfer_task(
    transfer_id: Uuid,
    request: SftpTransferRequest,
    cancel_requested: Arc<AtomicBool>,
) -> Task<Message> {
    let total_files = request.entries.len();
    let total_bytes = request.total_bytes();
    let tab_id = request.tab_id;
    let target_pane_id = request.target_pane_id;

    Task::run(
        async_stream::stream! {
            let mut completed_files = 0usize;
            let mut completed_bytes = 0u64;
            let mut copied_items = 0usize;
            let mut result: Result<usize, String> = Ok(0);
            let temp_dir = if matches!(
                (&request.source, &request.target),
                (SftpTransferEndpoint::Remote(_), SftpTransferEndpoint::Remote(_))
            ) {
                let path = std::env::temp_dir()
                    .join(format!("portal_copy_{}", uuid::Uuid::new_v4()));
                match tokio::fs::create_dir_all(&path).await {
                    Ok(()) => Some(path),
                    Err(error) => {
                        yield Message::Sftp(SftpMessage::TransferFinished {
                            transfer_id,
                            tab_id,
                            target_pane_id,
                            result: Err(format!("Failed to create temp directory: {}", error)),
                        });
                        return;
                    }
                }
            } else {
                None
            };

            yield Message::Sftp(SftpMessage::TransferProgress(TransferProgress {
                transfer_id,
                current_item: None,
                completed_files,
                total_files,
                completed_bytes,
                total_bytes,
            }));

            for entry in request.entries.iter().cloned() {
                if cancel_requested.load(Ordering::Relaxed) {
                    result = Err("Transfer cancelled".to_string());
                    break;
                }

                yield Message::Sftp(SftpMessage::TransferProgress(TransferProgress {
                    transfer_id,
                    current_item: Some(entry.name.clone()),
                    completed_files,
                    total_files,
                    completed_bytes,
                    total_bytes,
                }));

                let target_path = request.target_dir.join(&entry.name);
                let (progress_tx, mut progress_rx) = mpsc::unbounded_channel::<u64>();
                let source = request.source.clone();
                let target = request.target.clone();
                let temp_dir_for_task = temp_dir.clone();
                let entry_for_task = entry.clone();
                let target_path_for_task = target_path.clone();
                let cancel_for_task = cancel_requested.clone();
                let mut item_task = tokio::spawn(async move {
                    transfer_one_sftp_entry(
                        source,
                        target,
                        temp_dir_for_task,
                        entry_for_task,
                        target_path_for_task,
                        cancel_for_task,
                        move |bytes| {
                            let _ = progress_tx.send(bytes);
                        },
                    )
                    .await
                });

                let mut progress_open = true;
                let mut last_progress_emit = Instant::now()
                    .checked_sub(TRANSFER_PROGRESS_EMIT_INTERVAL)
                    .unwrap_or_else(Instant::now);
                let item_result = loop {
                    tokio::select! {
                        progress = progress_rx.recv(), if progress_open => {
                            if let Some(item_bytes) = progress {
                                let item_bytes = item_bytes.min(entry.size);
                                let now = Instant::now();
                                if now.duration_since(last_progress_emit) >= TRANSFER_PROGRESS_EMIT_INTERVAL
                                    || item_bytes >= entry.size
                                {
                                    last_progress_emit = now;
                                    yield Message::Sftp(SftpMessage::TransferProgress(TransferProgress {
                                        transfer_id,
                                        current_item: Some(entry.name.clone()),
                                        completed_files,
                                        total_files,
                                        completed_bytes: completed_bytes.saturating_add(item_bytes),
                                        total_bytes,
                                    }));
                                }
                            } else {
                                progress_open = false;
                            }
                        }
                        result = &mut item_task => {
                            break match result {
                                Ok(item_result) => item_result,
                                Err(error) => Err(error.to_string()),
                            };
                        }
                    }
                };

                match item_result {
                    Ok(count) => {
                        copied_items = copied_items.saturating_add(count);
                        completed_files = completed_files.saturating_add(1);
                        if !entry.is_dir {
                            completed_bytes = completed_bytes.saturating_add(entry.size);
                        }
                        yield Message::Sftp(SftpMessage::TransferProgress(TransferProgress {
                            transfer_id,
                            current_item: Some(entry.name),
                            completed_files,
                            total_files,
                            completed_bytes,
                            total_bytes,
                        }));
                    }
                    Err(error) => {
                        result = Err(error);
                        break;
                    }
                }
            }

            if let Some(temp_dir) = temp_dir {
                let _ = cleanup_temp_dir(&temp_dir).await;
            }

            if result.is_ok() {
                result = Ok(copied_items);
            }

            yield Message::Sftp(SftpMessage::TransferFinished {
                transfer_id,
                tab_id,
                target_pane_id,
                result,
            });
        },
        |message| message,
    )
}

async fn transfer_one_sftp_entry<P>(
    source: SftpTransferEndpoint,
    target: SftpTransferEndpoint,
    temp_dir: Option<std::path::PathBuf>,
    entry: SftpTransferEntry,
    target_path: std::path::PathBuf,
    cancel_requested: Arc<AtomicBool>,
    mut on_progress: P,
) -> Result<usize, String>
where
    P: FnMut(u64) + Send + 'static,
{
    reject_symlink_copy(&entry.name, entry.is_symlink)?;

    if cancel_requested.load(Ordering::Relaxed) {
        return Err("Transfer cancelled".to_string());
    }

    match (source, target) {
        (SftpTransferEndpoint::Local, SftpTransferEndpoint::Local) => {
            let source_path = entry.path.clone();
            let is_dir = entry.is_dir;
            tokio::task::spawn_blocking(move || {
                if is_dir {
                    copy_dir_recursive(&source_path, &target_path)?;
                    count_items_in_dir(&source_path)
                } else {
                    ensure_not_symlink(&source_path)?;
                    ensure_not_same_path(&source_path, &target_path)?;
                    std::fs::copy(&source_path, &target_path)
                        .map_err(|error| {
                            format!("Failed to copy {}: {}", source_path.display(), error)
                        })
                        .map(|_| 1)
                }
            })
            .await
            .map_err(|error| error.to_string())?
        }
        (SftpTransferEndpoint::Local, SftpTransferEndpoint::Remote(target_sftp)) => {
            if entry.is_dir {
                target_sftp
                    .upload_recursive(&entry.path, &target_path)
                    .await
                    .map_err(|error| error.to_string())
            } else {
                target_sftp
                    .upload_with_progress(&entry.path, &target_path, &mut on_progress, || {
                        cancel_requested.load(Ordering::Relaxed)
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .map(|_| 1)
            }
        }
        (SftpTransferEndpoint::Remote(source_sftp), SftpTransferEndpoint::Local) => {
            if entry.is_dir {
                source_sftp
                    .download_recursive(&entry.path, &target_path)
                    .await
                    .map_err(|error| error.to_string())
            } else {
                source_sftp
                    .download_with_progress(&entry.path, &target_path, &mut on_progress, || {
                        cancel_requested.load(Ordering::Relaxed)
                    })
                    .await
                    .map_err(|error| error.to_string())
                    .map(|_| 1)
            }
        }
        (SftpTransferEndpoint::Remote(source_sftp), SftpTransferEndpoint::Remote(target_sftp)) => {
            let temp_dir =
                temp_dir.ok_or_else(|| "Remote copy temp directory is unavailable".to_string())?;
            let temp_path = temp_dir.join(&entry.name);
            if entry.is_dir {
                source_sftp
                    .download_recursive(&entry.path, &temp_path)
                    .await
                    .map_err(|error| error.to_string())?;
                target_sftp
                    .upload_recursive(&temp_path, &target_path)
                    .await
                    .map_err(|error| error.to_string())
            } else {
                let first_phase_bytes = entry.size / 2;
                source_sftp
                    .download_with_progress(
                        &entry.path,
                        &temp_path,
                        |bytes| on_progress(bytes.min(entry.size) / 2),
                        || cancel_requested.load(Ordering::Relaxed),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                target_sftp
                    .upload_with_progress(
                        &temp_path,
                        &target_path,
                        |bytes| {
                            on_progress(first_phase_bytes.saturating_add(bytes.min(entry.size) / 2))
                        },
                        || cancel_requested.load(Ordering::Relaxed),
                    )
                    .await
                    .map_err(|error| error.to_string())
                    .map(|_| 1)
            }
        }
    }
}

impl Portal {
    pub(super) fn begin_connecting(
        &mut self,
        host_name: String,
        protocol: &str,
        session_id: SessionId,
        task: Task<Message>,
    ) -> Task<Message> {
        self.dialogs.open_connecting(host_name, protocol);
        self.track_pending_connect(session_id, task)
    }

    pub(super) fn track_pending_connect(
        &mut self,
        session_id: SessionId,
        task: Task<Message>,
    ) -> Task<Message> {
        if let Some(pending) = self.pending_connect.take() {
            pending.handle.abort();
        }

        let (task, handle) = task.abortable();
        self.pending_connect = Some(crate::app::PendingConnect::new(session_id, handle));
        task
    }

    pub(super) fn finish_pending_connect(&mut self) {
        self.pending_connect = None;
        self.dialogs.close_connecting();
    }

    pub(super) fn finish_pending_connect_for(&mut self, session_id: SessionId) -> bool {
        if self
            .pending_connect
            .as_ref()
            .is_some_and(|pending| pending.is_for(session_id))
        {
            self.pending_connect = None;
            self.dialogs.close_connecting();
            return true;
        }
        false
    }

    pub(super) fn cancel_pending_connect(&mut self) {
        if let Some(pending) = self.pending_connect.take() {
            pending.handle.abort();
            self.toast_manager
                .push(Toast::warning("Connection cancelled"));
        }
        self.dialogs.close_connecting();
    }

    fn hide_sidebar_for_session(&mut self) {
        if self.ui.sidebar_state != crate::app::SidebarState::Hidden {
            self.ui.sidebar_state_before_session = Some(self.ui.sidebar_state);
        }
        self.ui.sidebar_state = crate::app::SidebarState::Hidden;
    }

    pub(super) fn restore_sidebar_after_session(&mut self) {
        if let Some(saved_state) = self.ui.sidebar_state_before_session.take() {
            self.ui.sidebar_state = saved_state;
        }
    }

    pub(super) fn enter_host_grid(&mut self) {
        self.ui.active_view = View::HostGrid;
        self.ui.terminal_captured = false;
    }

    pub(super) fn enter_terminal_view(&mut self, tab_id: Uuid, auto_hide_sidebar: bool) {
        self.active_tab = Some(tab_id);
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.needs_attention = false;
        }
        self.ui.active_view = View::Terminal(tab_id);
        self.ui.terminal_captured = true;
        self.ui.terminal_focus_token = self.ui.terminal_focus_token.wrapping_add(1);
        self.ui.focus_section = crate::app::FocusSection::Content;
        if auto_hide_sidebar {
            self.hide_sidebar_for_session();
        }
    }

    pub(super) fn enter_sftp_view(&mut self, tab_id: Uuid) {
        self.active_tab = Some(tab_id);
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.needs_attention = false;
        }
        self.ui.active_view = View::DualSftp(tab_id);
        self.ui.terminal_captured = false;
    }

    pub(super) fn enter_file_viewer_view(&mut self, tab_id: Uuid) {
        self.active_tab = Some(tab_id);
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.needs_attention = false;
        }
        self.ui.active_view = View::FileViewer(tab_id);
        self.ui.terminal_captured = false;
    }

    pub(super) fn enter_vnc_view(&mut self, tab_id: Uuid) {
        self.active_tab = Some(tab_id);
        if let Some(tab) = self.tabs.iter_mut().find(|tab| tab.id == tab_id) {
            tab.needs_attention = false;
        }
        self.ui.active_view = View::VncViewer(tab_id);
        self.ui.terminal_captured = false;
        self.hide_sidebar_for_session();
    }

    pub(super) fn set_active_tab(&mut self, tab_id: Uuid) {
        if self.sessions.contains(tab_id) {
            self.enter_terminal_view(tab_id, false);
        } else if self.sftp.contains_tab(tab_id) {
            self.enter_sftp_view(tab_id);
        } else if self.file_viewers.contains(tab_id) {
            self.enter_file_viewer_view(tab_id);
        } else if self.vnc_sessions.contains_key(&tab_id) {
            self.enter_vnc_view(tab_id);
        }
    }

    pub(super) fn close_tab(&mut self, tab_id: Uuid) {
        self.transfers.cancel_for_tab(tab_id);
        let sftp_sessions_to_close = self.sftp.remove_tab_and_collect_sessions(tab_id);
        let mut history_changed = false;

        self.tabs.retain(|t| t.id != tab_id);
        if let Some(session) = self.sessions.remove(tab_id) {
            if history::mark_entry_disconnected(&mut self.config.history, session.history_entry_id)
            {
                history_changed = true;
            }

            let ssh_session_to_cleanup = match &session.backend {
                SessionBackend::Ssh(ssh_session) => Some(ssh_session.clone()),
                SessionBackend::Local(_) => None,
                SessionBackend::Proxy(_) => None,
            };

            if let Some(logger) = session.logger {
                tokio::spawn(async move {
                    logger.shutdown().await;
                });
            }

            if let Some(ssh_session) = ssh_session_to_cleanup {
                tokio::spawn(async move {
                    ssh_session.stop_all_forwards().await;
                });
            }
        }
        if let Some(vnc) = self.vnc_sessions.remove(&tab_id) {
            if history::mark_entry_disconnected(&mut self.config.history, vnc.history_entry_id) {
                history_changed = true;
            }
            vnc.session.disconnect();
        }
        if let Some(viewer_state) = self.file_viewers.remove(tab_id) {
            if let FileSource::Remote { temp_path, .. } = viewer_state.file_source {
                if let Some(temp_dir) = temp_path.parent().map(|path| path.to_path_buf()) {
                    tokio::spawn(async move {
                        let _ = cleanup_temp_dir(&temp_dir).await;
                    });
                }
            }
        }

        for session_id in sftp_sessions_to_close {
            let still_used = self.sftp.is_connection_in_use(session_id);
            if !still_used {
                self.sftp.remove_connection(session_id);
                if let Some(entry_id) = self.sftp.remove_history_entry(session_id) {
                    if history::mark_entry_disconnected(&mut self.config.history, entry_id) {
                        history_changed = true;
                    }
                }
            }
        }

        if history_changed {
            if let Err(e) = self.config.history.save() {
                tracing::error!("Failed to save history config: {}", e);
            }
        }

        if self.active_tab == Some(tab_id) {
            if let Some(last_tab) = self.tabs.last() {
                self.set_active_tab(last_tab.id);
            } else {
                self.active_tab = None;
                self.ui.sidebar_selection = crate::message::SidebarMenuItem::Hosts;
                self.restore_sidebar_after_session();
                self.enter_host_grid();
                self.ui.focus_section = crate::app::FocusSection::Content;
            }
        }
    }

    pub(super) fn close_active_tab(&mut self) {
        if let Some(tab_id) = self.active_tab {
            self.close_tab(tab_id);
        }
    }

    pub(super) fn select_next_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }

        let current_idx = self
            .active_tab
            .and_then(|id| self.tabs.iter().position(|t| t.id == id))
            .unwrap_or(0);
        let next_idx = (current_idx + 1) % self.tabs.len();
        let next_id = self.tabs[next_idx].id;
        self.set_active_tab(next_id);
    }

    pub(super) fn select_prev_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }

        let current_idx = self
            .active_tab
            .and_then(|id| self.tabs.iter().position(|t| t.id == id))
            .unwrap_or(0);
        let prev_idx = if current_idx == 0 {
            self.tabs.len() - 1
        } else {
            current_idx - 1
        };
        let prev_id = self.tabs[prev_idx].id;
        self.set_active_tab(prev_id);
    }

    pub(super) fn handle_keybinding_action(&mut self, action: AppAction) -> Task<Message> {
        match action {
            AppAction::NewWindow => {
                match std::env::current_exe()
                    .map_err(|e| e.to_string())
                    .and_then(|exe| {
                        std::process::Command::new(exe)
                            .spawn()
                            .map(|_| ())
                            .map_err(|e| e.to_string())
                    }) {
                    Ok(()) => {}
                    Err(error) => self.toast_manager.push(Toast::error(format!(
                        "Failed to open new window: {}",
                        error
                    ))),
                }
                Task::none()
            }
            AppAction::NewConnection => {
                self.dialogs.open_quick_connect();
                Task::none()
            }
            AppAction::CloseSession => {
                self.close_active_tab();
                Task::none()
            }
            AppAction::NewTab => {
                self.restore_sidebar_after_session();
                self.enter_host_grid();
                self.ui.focus_section = FocusSection::Content;
                Task::none()
            }
            AppAction::NextSession => {
                self.select_next_tab();
                Task::none()
            }
            AppAction::PreviousSession => {
                self.select_prev_tab();
                Task::none()
            }
            AppAction::Copy | AppAction::Paste => Task::none(),
            AppAction::ToggleFullscreen => match self.ui.active_view {
                View::VncViewer(_) => Task::done(Message::Vnc(VncMessage::ToggleFullscreen)),
                _ => Task::none(),
            },
        }
    }

    /// Connect to a VNC host
    pub(super) fn connect_vnc_host(&mut self, host: &Host) -> Task<Message> {
        let port = host.effective_vnc_port();

        if let Some(password_id) = host.vnc_password_id {
            match crate::hub::vault::load_decrypted_secret(password_id) {
                Ok(password) => return self.connect_vnc_host_with_password(host, password),
                Err(error) => {
                    self.toast_manager.push(Toast::warning(format!(
                        "Could not load saved VNC password: {}",
                        error
                    )));
                }
            }
        }

        // VNC (especially macOS ARD) always needs a password — prompt for it
        let password_dialog = PasswordDialogState::new_vnc(
            host.name.clone(),
            host.hostname.clone(),
            port,
            host.effective_username(),
            host.id,
        );
        self.dialogs.open_password(password_dialog);
        Task::none()
    }

    pub(super) fn connect_vnc_host_with_password(
        &mut self,
        host: &Host,
        password: secrecy::SecretString,
    ) -> Task<Message> {
        use crate::message::VncMessage;
        use crate::vnc::VncSession;
        use crate::vnc::session::VncSessionEvent;
        use secrecy::ExposeSecret;

        let session_id = Uuid::new_v4();
        let dialog_host_name = host.name.clone();
        let host = Arc::new(host.clone());
        let port = host.effective_vnc_port();
        let host_name = host.name.clone();
        let host_id = host.id;
        let username = Some(host.effective_username());
        let password_str = Some(password.expose_secret().to_string());

        let (msg_tx, msg_rx) = mpsc::channel::<Message>(256);
        let vnc_settings = self.prefs.vnc_settings.clone();

        let connect_task = Task::perform(
            async move {
                match VncSession::connect(
                    host.hostname.as_str(),
                    port,
                    username,
                    password_str,
                    host_name,
                    vnc_settings,
                )
                .await
                {
                    Ok((vnc_session, mut event_rx, detected_os)) => {
                        let _ = msg_tx
                            .send(Message::Vnc(VncMessage::Connected {
                                session_id,
                                host_name: vnc_session.host_name.clone(),
                                vnc_session,
                                host_id,
                                detected_os,
                            }))
                            .await;

                        while let Some(event) = event_rx.recv().await {
                            let msg = match event {
                                VncSessionEvent::ResolutionChanged(_, _) => continue,
                                VncSessionEvent::Disconnected => {
                                    Message::Vnc(VncMessage::Disconnected(session_id))
                                }
                                VncSessionEvent::Bell => Message::Vnc(VncMessage::Bell(session_id)),
                                VncSessionEvent::ClipboardText(text) => {
                                    Message::Vnc(VncMessage::ClipboardReceived(session_id, text))
                                }
                            };
                            if msg_tx.send(msg).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = msg_tx
                            .send(Message::Vnc(VncMessage::ConnectFailed {
                                session_id,
                                error: e,
                            }))
                            .await;
                    }
                }
            },
            |_| Message::Noop,
        );

        let event_listener = Task::run(
            stream::unfold(msg_rx, |mut rx| async move {
                rx.recv().await.map(|msg| (msg, rx))
            }),
            |msg| msg,
        );

        self.begin_connecting(
            dialog_host_name,
            "VNC",
            session_id,
            Task::batch([connect_task, event_listener]),
        )
    }

    pub(super) fn connect_to_host(&mut self, host: &Host) -> Task<Message> {
        self.connect_to_host_with_mode(host, ConnectionLaunchMode::Default)
    }

    pub(super) fn connect_to_host_new_session(&mut self, host: &Host) -> Task<Message> {
        self.connect_to_host_with_mode(host, ConnectionLaunchMode::FreshSession)
    }

    fn connect_to_host_with_mode(
        &mut self,
        host: &Host,
        mode: ConnectionLaunchMode,
    ) -> Task<Message> {
        if mode == ConnectionLaunchMode::FreshSession {
            tracing::debug!("Starting explicit fresh session for host {}", host.name);
        }

        if connection::should_use_portal_hub(&self.prefs.portal_hub, host) {
            let dialog_host_name = host.name.clone();
            let host = Arc::new(host.clone());
            let session_id = Uuid::new_v4();
            let host_id = host.id;
            let terminal_size = self.terminal_initial_size();
            let task = connection::proxy_connect_tasks(
                self.prefs.portal_hub.clone(),
                host,
                session_id,
                host_id,
                terminal_size,
            );

            return self.begin_connecting(dialog_host_name, "Portal Hub", session_id, task);
        }

        // Check if password authentication is configured
        if matches!(host.auth, AuthMethod::Password) {
            // Show password dialog
            let password_dialog = PasswordDialogState::new_ssh(
                host.name.clone(),
                host.hostname.clone(),
                host.port,
                host.effective_username(),
                host.id,
            );
            self.dialogs.open_password(password_dialog);
            return Task::none();
        }

        // Use Arc to avoid multiple deep clones of Host data
        let dialog_host_name = host.name.clone();
        let host = Arc::new(host.clone());
        let session_id = Uuid::new_v4();
        let host_id = host.id;

        let should_detect_os = connection::should_detect_os(host.detected_os.as_ref());
        let terminal_size = self.terminal_initial_size();

        let task = connection::ssh_connect_tasks(
            host,
            session_id,
            host_id,
            terminal_size,
            should_detect_os,
            self.prefs.allow_agent_forwarding,
        );

        self.begin_connecting(dialog_host_name, "SSH", session_id, task)
    }

    /// Spawn a local terminal session
    pub(super) fn spawn_local_terminal(&mut self) -> Task<Message> {
        let session_id = Uuid::new_v4();

        // Create event channel for local PTY events
        let (event_tx, event_rx) = mpsc::channel::<LocalEvent>(1024);

        // Start listening for events
        let event_listener = Task::run(
            stream::unfold(event_rx, |mut rx| async move {
                rx.recv().await.map(|event| (event, rx))
            }),
            move |event| match event {
                LocalEvent::Data(data) => Message::Session(SessionMessage::Data(session_id, data)),
                LocalEvent::Disconnected => Message::Session(SessionMessage::Disconnected {
                    session_id,
                    clean: true,
                }),
            },
        );

        // Spawn the local terminal with a best-effort size. The first render
        // still sends the exact grid size.
        let (cols, rows) = self.terminal_initial_size();
        match LocalSession::spawn(cols, rows, event_tx) {
            Ok(local_session) => {
                let local_session = Arc::new(local_session);
                let spawn_task = Task::done(Message::Session(SessionMessage::LocalConnected {
                    session_id,
                    local_session,
                }));

                Task::batch([event_listener, spawn_task])
            }
            Err(e) => {
                tracing::error!("Failed to spawn local terminal: {}", e);
                self.toast_manager.push(Toast::error(format!(
                    "Failed to spawn local terminal: {}",
                    e
                )));
                Task::none()
            }
        }
    }

    /// Load directory contents for a dual-pane SFTP browser pane
    pub(super) fn load_dual_pane_directory(
        &self,
        tab_id: SessionId,
        pane_id: PaneId,
    ) -> Task<Message> {
        if let Some(tab_state) = self.sftp.get_tab(tab_id) {
            let pane = tab_state.pane(pane_id);
            let path = pane.current_path.clone();

            match &pane.source {
                PaneSource::Local => {
                    // Load local directory
                    let requested_source = pane.source.clone();
                    let requested_path = path.clone();
                    Task::perform(async move { list_local_dir(&path).await }, move |result| {
                        Message::Sftp(SftpMessage::PaneListResult(
                            tab_id,
                            pane_id,
                            requested_source.clone(),
                            requested_path.clone(),
                            result,
                        ))
                    })
                }
                PaneSource::Remote { session_id, .. } => {
                    // Load remote directory via SFTP
                    if let Some(sftp) = self.sftp.get_connection(*session_id) {
                        let sftp = sftp.clone();
                        let requested_source = pane.source.clone();
                        let requested_path = path.clone();
                        Task::perform(async move { sftp.list_dir(&path).await }, move |result| {
                            Message::Sftp(SftpMessage::PaneListResult(
                                tab_id,
                                pane_id,
                                requested_source.clone(),
                                requested_path.clone(),
                                result.map_err(|e| e.to_string()),
                            ))
                        })
                    } else {
                        Task::none()
                    }
                }
            }
        } else {
            Task::none()
        }
    }

    /// Connect to an SFTP host for use in a dual-pane browser
    pub(super) fn connect_sftp_for_pane(
        &mut self,
        tab_id: SessionId,
        pane_id: PaneId,
        host: &Host,
    ) -> Task<Message> {
        // Check if password authentication is configured
        if matches!(host.auth, AuthMethod::Password) {
            self.sftp
                .set_pending_connection(Some((tab_id, pane_id, host.id)));
            // Show password dialog for SFTP
            let password_dialog = PasswordDialogState::new_sftp(
                host.name.clone(),
                host.hostname.clone(),
                host.port,
                host.effective_username(),
                host.id,
                tab_id,
                pane_id,
            );
            self.dialogs.open_password(password_dialog);
            return Task::none();
        }

        // Use Arc to avoid multiple deep clones of Host data
        let host = Arc::new(host.clone());
        let sftp_session_id = Uuid::new_v4();
        let host_id = host.id;

        // Store pending connection info for host key verification
        self.sftp
            .set_pending_connection(Some((tab_id, pane_id, host_id)));

        connection::sftp_connect_tasks(host, tab_id, pane_id, sftp_session_id, host_id)
    }

    /// Handle context menu actions for SFTP panes
    pub(super) fn handle_sftp_context_action(
        &mut self,
        tab_id: SessionId,
        action: ContextMenuAction,
    ) -> Task<Message> {
        let Some(tab_state) = self.sftp.get_tab(tab_id) else {
            return Task::none();
        };

        let active_pane = tab_state.active_pane;
        let pane = tab_state.pane(active_pane);
        let selected_entries: Vec<_> = pane.selected_entries();

        match action {
            ContextMenuAction::Open => {
                // Open file in the in-app file viewer
                if let Some(entry) = selected_entries.first() {
                    if !entry.is_dir && !entry.is_parent() {
                        let file_name = entry.name.clone();
                        let file_path = entry.path.clone();
                        let file_type = FileType::from_path(&file_path);

                        // Check if file type is viewable
                        if !file_type.is_viewable() {
                            self.toast_manager.push(Toast::warning(
                                "Binary files are not supported in the viewer.",
                            ));
                            return Task::none();
                        }

                        // Create a new file viewer
                        let viewer_id = Uuid::new_v4();

                        let (viewer_state, load_task) = match &pane.source {
                            PaneSource::Local => file_viewer::build_local_viewer(
                                viewer_id,
                                file_name.clone(),
                                file_path,
                                file_type,
                            ),
                            PaneSource::Remote { session_id, .. } => {
                                if let Some(sftp) = self.sftp.get_connection(*session_id) {
                                    file_viewer::build_remote_viewer(
                                        viewer_id,
                                        file_name.clone(),
                                        file_path,
                                        *session_id,
                                        sftp.clone(),
                                        file_type,
                                    )
                                } else {
                                    return Task::none();
                                }
                            }
                        };

                        // Add viewer to manager
                        self.file_viewers.insert(viewer_state);

                        // Create tab
                        let tab = Tab::new_file_viewer(viewer_id, file_name);
                        self.tabs.push(tab);
                        self.enter_file_viewer_view(viewer_id);

                        return load_task;
                    }
                }
            }
            ContextMenuAction::CopyToTarget => {
                // Copy selected files to the target (other) pane
                return Task::done(Message::Sftp(SftpMessage::CopyToTarget(tab_id)));
            }
            ContextMenuAction::Rename => {
                // Show the Rename dialog for single selection
                if let Some(entry) = selected_entries.first() {
                    if !entry.is_parent() {
                        let original_name = entry.name.clone();
                        if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                            tab_state.show_rename_dialog(original_name);
                        }
                    }
                }
            }
            ContextMenuAction::Delete => {
                // Show delete confirmation dialog for selected entries
                let entries_to_delete: Vec<_> = selected_entries
                    .iter()
                    .filter(|e| !e.is_parent())
                    .map(|e| (e.name.clone(), e.path.clone(), e.is_dir))
                    .collect();

                if !entries_to_delete.is_empty() {
                    if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                        tab_state.show_delete_dialog(entries_to_delete);
                    }
                }
            }
            ContextMenuAction::Refresh => {
                // Refresh the current pane
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.pane_mut(active_pane).loading = true;
                }
                return self.load_dual_pane_directory(tab_id, active_pane);
            }
            ContextMenuAction::NewFolder => {
                // Show the New Folder dialog
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.show_new_folder_dialog();
                }
            }
            ContextMenuAction::EditPermissions => {
                // Show permissions dialog for single selection
                if let Some(entry) = selected_entries.first() {
                    if !entry.is_parent() {
                        let name = entry.name.clone();
                        let path = entry.path.clone();

                        // Get current permissions
                        let permissions = match &pane.source {
                            PaneSource::Local => {
                                // Read permissions from local file
                                #[cfg(unix)]
                                {
                                    use std::os::unix::fs::PermissionsExt;
                                    std::fs::metadata(&path)
                                        .map(|m| PermissionBits::from_mode(m.permissions().mode()))
                                        .unwrap_or_default()
                                }
                                #[cfg(not(unix))]
                                {
                                    PermissionBits::default()
                                }
                            }
                            PaneSource::Remote { .. } => {
                                // For remote files, we'll use the mode from FileEntry if available
                                // For now, use default permissions (644 for files, 755 for directories)
                                if entry.is_dir {
                                    PermissionBits::from_mode(0o755)
                                } else {
                                    PermissionBits::from_mode(0o644)
                                }
                            }
                        };

                        if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                            tab_state.show_permissions_dialog(name, path, permissions);
                        }
                    }
                }
            }
        }

        Task::none()
    }

    /// Handle dialog submission (New Folder or Rename)
    pub(super) fn handle_sftp_dialog_submit(&mut self, tab_id: SessionId) -> Task<Message> {
        let Some(tab_state) = self.sftp.get_tab(tab_id) else {
            return Task::none();
        };

        let Some(ref dialog) = tab_state.dialog else {
            return Task::none();
        };

        let pane_id = dialog.target_pane;
        let pane = tab_state.pane(pane_id);
        let current_path = pane.current_path.clone();
        let input_value = dialog.input_value.trim().to_string();

        match &dialog.dialog_type {
            SftpDialogType::NewFolder => {
                let new_folder_path = current_path.join(&input_value);

                match &pane.source {
                    PaneSource::Local => {
                        // Create local folder
                        Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    std::fs::create_dir(&new_folder_path).map_err(|e| e.to_string())
                                })
                                .await
                                .map_err(|e| e.to_string())?
                            },
                            move |result| {
                                Message::Sftp(SftpMessage::NewFolderResult(tab_id, pane_id, result))
                            },
                        )
                    }
                    PaneSource::Remote { session_id, .. } => {
                        // Create remote folder via SFTP
                        if let Some(sftp) = self.sftp.get_connection(*session_id) {
                            let sftp = sftp.clone();
                            Task::perform(
                                async move {
                                    sftp.create_dir(&new_folder_path)
                                        .await
                                        .map_err(|e| e.to_string())
                                },
                                move |result| {
                                    Message::Sftp(SftpMessage::NewFolderResult(
                                        tab_id, pane_id, result,
                                    ))
                                },
                            )
                        } else {
                            Task::none()
                        }
                    }
                }
            }
            SftpDialogType::Rename { original_name } => {
                let old_path = current_path.join(original_name);
                let new_path = current_path.join(&input_value);

                match &pane.source {
                    PaneSource::Local => {
                        // Rename local file/folder
                        Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    std::fs::rename(&old_path, &new_path).map_err(|e| e.to_string())
                                })
                                .await
                                .map_err(|e| e.to_string())?
                            },
                            move |result| {
                                Message::Sftp(SftpMessage::RenameResult(tab_id, pane_id, result))
                            },
                        )
                    }
                    PaneSource::Remote { session_id, .. } => {
                        // Rename remote file/folder via SFTP
                        if let Some(sftp) = self.sftp.get_connection(*session_id) {
                            let sftp = sftp.clone();
                            Task::perform(
                                async move {
                                    sftp.rename(&old_path, &new_path)
                                        .await
                                        .map_err(|e| e.to_string())
                                },
                                move |result| {
                                    Message::Sftp(SftpMessage::RenameResult(
                                        tab_id, pane_id, result,
                                    ))
                                },
                            )
                        } else {
                            Task::none()
                        }
                    }
                }
            }
            SftpDialogType::Delete { entries } => {
                let entries = entries.clone();

                match &pane.source {
                    PaneSource::Local => {
                        // Delete local files/folders
                        Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    let mut deleted_count = 0;
                                    for (_, path, is_dir) in entries {
                                        let result = delete_local_path(&path, is_dir);
                                        match result {
                                            Ok(()) => deleted_count += 1,
                                            Err(e) => {
                                                return Err(format!(
                                                    "Failed to delete {}: {}",
                                                    path.display(),
                                                    e
                                                ));
                                            }
                                        }
                                    }
                                    Ok(deleted_count)
                                })
                                .await
                                .map_err(|e| e.to_string())?
                            },
                            move |result| {
                                Message::Sftp(SftpMessage::DeleteResult(tab_id, pane_id, result))
                            },
                        )
                    }
                    PaneSource::Remote { session_id, .. } => {
                        // Delete remote files/folders via SFTP
                        if let Some(sftp) = self.sftp.get_connection(*session_id) {
                            let sftp = sftp.clone();
                            Task::perform(
                                async move {
                                    let mut deleted_count = 0;
                                    for (_, path, is_dir) in entries {
                                        let result = if is_dir {
                                            sftp.remove_recursive(&path).await
                                        } else {
                                            sftp.remove_file(&path).await
                                        };
                                        match result {
                                            Ok(()) => deleted_count += 1,
                                            Err(e) => {
                                                return Err(format!(
                                                    "Failed to delete {}: {}",
                                                    path.display(),
                                                    e
                                                ));
                                            }
                                        }
                                    }
                                    Ok(deleted_count)
                                },
                                move |result| {
                                    Message::Sftp(SftpMessage::DeleteResult(
                                        tab_id, pane_id, result,
                                    ))
                                },
                            )
                        } else {
                            Task::none()
                        }
                    }
                }
            }
            SftpDialogType::EditPermissions {
                path, permissions, ..
            } => {
                let path = path.clone();
                let mode = permissions.to_mode();

                match &pane.source {
                    PaneSource::Local => {
                        // Set local file permissions
                        Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    #[cfg(unix)]
                                    {
                                        use std::os::unix::fs::PermissionsExt;
                                        let permissions = std::fs::Permissions::from_mode(mode);
                                        std::fs::set_permissions(&path, permissions).map_err(|e| {
                                            format!("Failed to set permissions: {}", e)
                                        })
                                    }
                                    #[cfg(not(unix))]
                                    {
                                        let _ = (path, mode);
                                        Err("Permissions are only supported on Unix systems"
                                            .to_string())
                                    }
                                })
                                .await
                                .map_err(|e| e.to_string())?
                            },
                            move |result| {
                                Message::Sftp(SftpMessage::PermissionsResult(
                                    tab_id, pane_id, result,
                                ))
                            },
                        )
                    }
                    PaneSource::Remote { session_id, .. } => {
                        // Set remote file permissions via SFTP
                        if let Some(sftp) = self.sftp.get_connection(*session_id) {
                            let sftp = sftp.clone();
                            Task::perform(
                                async move {
                                    sftp.set_permissions(&path, mode)
                                        .await
                                        .map_err(|e| e.to_string())
                                },
                                move |result| {
                                    Message::Sftp(SftpMessage::PermissionsResult(
                                        tab_id, pane_id, result,
                                    ))
                                },
                            )
                        } else {
                            Task::none()
                        }
                    }
                }
            }
        }
    }

    /// Handle copying selected files from active pane to target pane
    pub(super) fn handle_copy_to_target(&mut self, tab_id: SessionId) -> Task<Message> {
        let Some(tab_state) = self.sftp.get_tab(tab_id) else {
            return Task::none();
        };

        let source_pane_id = tab_state.active_pane;
        let target_pane_id = match source_pane_id {
            PaneId::Left => PaneId::Right,
            PaneId::Right => PaneId::Left,
        };

        let source_pane = tab_state.pane(source_pane_id);
        let target_pane = tab_state.pane(target_pane_id);

        // Collect entries to copy (exclude ".." parent entry)
        let entries_to_copy: Vec<_> = source_pane
            .selected_entries()
            .into_iter()
            .filter(|e| !e.is_parent())
            .map(|e| SftpTransferEntry {
                name: e.name.clone(),
                path: e.path.clone(),
                is_dir: e.is_dir,
                is_symlink: e.is_symlink,
                size: e.size,
            })
            .collect();

        if entries_to_copy.is_empty() {
            return Task::none();
        }

        let source = match &source_pane.source {
            PaneSource::Local => SftpTransferEndpoint::Local,
            PaneSource::Remote { session_id, .. } => {
                let Some(sftp) = self.sftp.get_connection(*session_id).cloned() else {
                    return Task::none();
                };
                SftpTransferEndpoint::Remote(sftp)
            }
        };
        let target = match &target_pane.source {
            PaneSource::Local => SftpTransferEndpoint::Local,
            PaneSource::Remote { session_id, .. } => {
                let Some(sftp) = self.sftp.get_connection(*session_id).cloned() else {
                    return Task::none();
                };
                SftpTransferEndpoint::Remote(sftp)
            }
        };

        let request = SftpTransferRequest {
            tab_id,
            target_pane_id,
            target_dir: target_pane.current_path.clone(),
            source,
            target,
            entries: entries_to_copy,
        };
        let transfer_id = Uuid::new_v4();
        let cancel_requested = Arc::new(AtomicBool::new(false));
        let transfer = TransferItem::new(TransferItemInit {
            id: transfer_id,
            tab_id,
            target_pane: target_pane_id,
            direction: request.direction(),
            label: request.label(),
            total_files: request.entries.len(),
            total_bytes: request.total_bytes(),
            cancel_requested: cancel_requested.clone(),
        });
        self.transfers.insert(transfer);

        let task = sftp_transfer_task(transfer_id, request, cancel_requested);
        let (task, _handle) = task.abortable();
        task
    }
}

fn reject_symlink_copy(name: &str, is_symlink: bool) -> Result<(), String> {
    if is_symlink {
        Err(format!("Cannot copy symbolic link {}", name))
    } else {
        Ok(())
    }
}

fn delete_local_path(path: &std::path::Path, listed_is_dir: bool) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => std::fs::remove_file(path),
        Ok(metadata) if metadata.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(_) if listed_is_dir => std::fs::remove_dir_all(path),
        Err(_) => std::fs::remove_file(path),
    }
}

#[cfg(test)]
mod tests {
    use super::delete_local_path;
    use crate::app::PendingConnect;
    use crate::message::Message;
    use iced::Task;
    use uuid::Uuid;

    #[test]
    fn pending_connect_matches_only_its_session() {
        let session_id = Uuid::new_v4();
        let (_task, handle) = Task::<Message>::none().abortable();
        let pending = PendingConnect::new(session_id, handle);

        assert!(pending.is_for(session_id));
        assert!(!pending.is_for(Uuid::new_v4()));
    }

    #[cfg(unix)]
    #[test]
    fn delete_local_path_removes_directory_symlink_without_deleting_target() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let symlink = temp.path().join("link");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("file.txt"), "content").unwrap();
        std::os::unix::fs::symlink(&target, &symlink).unwrap();

        delete_local_path(&symlink, true).unwrap();

        assert!(!symlink.exists());
        assert!(target.join("file.txt").exists());
    }
}
