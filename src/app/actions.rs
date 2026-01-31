use std::sync::Arc;

use futures::stream;
use iced::Task;
use std::time::Instant;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::{AuthMethod, Host};
use crate::fs_utils::{cleanup_temp_dir, copy_dir_recursive, count_items_in_dir};
use crate::local::{LocalEvent, LocalSession};
use crate::local_fs::list_local_dir;
use crate::message::{Message, SessionId, SessionMessage, SftpMessage};
use crate::views::dialogs::password_dialog::PasswordDialogState;
use crate::views::file_viewer::{FileSource, FileType};
use crate::views::sftp::{ContextMenuAction, PaneId, PaneSource, PermissionBits, SftpDialogType};
use crate::views::tabs::Tab;
use crate::views::toast::Toast;

use super::services::{connection, file_viewer, history};
use super::{Portal, View};

impl Portal {
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
        self.ui.active_view = View::Terminal(tab_id);
        self.ui.terminal_captured = true;
        if auto_hide_sidebar {
            self.hide_sidebar_for_session();
        }
    }

    pub(super) fn enter_sftp_view(&mut self, tab_id: Uuid) {
        self.active_tab = Some(tab_id);
        self.ui.active_view = View::DualSftp(tab_id);
        self.ui.terminal_captured = false;
    }

    pub(super) fn enter_file_viewer_view(&mut self, tab_id: Uuid) {
        self.active_tab = Some(tab_id);
        self.ui.active_view = View::FileViewer(tab_id);
        self.ui.terminal_captured = false;
    }

    pub(super) fn enter_vnc_view(&mut self, tab_id: Uuid) {
        self.active_tab = Some(tab_id);
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
        let sftp_sessions_to_close = self.sftp.remove_tab_and_collect_sessions(tab_id);

        self.tabs.retain(|t| t.id != tab_id);
        self.sessions.remove(tab_id);
        if let Some(vnc) = self.vnc_sessions.remove(&tab_id) {
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

        let mut history_changed = false;
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

    /// Connect to a VNC host
    pub(super) fn connect_vnc_host(&mut self, host: &Host) -> Task<Message> {
        let port = host.effective_vnc_port();

        // VNC (especially macOS ARD) always needs a password — prompt for it
        let password_dialog = PasswordDialogState::new_vnc(
            host.name.clone(),
            host.hostname.clone(),
            port,
            host.username.clone(),
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
        let hostname = host.hostname.clone();
        let port = host.effective_vnc_port();
        let host_name = host.name.clone();
        let host_id = host.id;
        let username = Some(host.username.clone()).filter(|u| !u.is_empty());
        let password_str = Some(password.expose_secret().to_string());

        let (msg_tx, msg_rx) = mpsc::channel::<Message>(256);
        let vnc_settings = self.prefs.vnc_settings.clone();

        let connect_task = Task::perform(
            async move {
                match VncSession::connect(
                    &hostname,
                    port,
                    username,
                    password_str,
                    host_name.clone(),
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
                                VncSessionEvent::Bell => continue,
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
                        let _ = msg_tx.send(Message::Vnc(VncMessage::Error(e))).await;
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

        Task::batch([connect_task, event_listener])
    }

    pub(super) fn connect_to_host(&mut self, host: &Host) -> Task<Message> {
        // Check if password authentication is configured
        if matches!(host.auth, AuthMethod::Password) {
            // Show password dialog
            let password_dialog = PasswordDialogState::new_ssh(
                host.name.clone(),
                host.hostname.clone(),
                host.port,
                host.username.clone(),
                host.id,
            );
            self.dialogs.open_password(password_dialog);
            return Task::none();
        }

        // Use Arc to avoid multiple deep clones of Host data
        let host = Arc::new(host.clone());
        let session_id = Uuid::new_v4();
        let host_id = host.id;

        let should_detect_os = connection::should_detect_os(host.detected_os.as_ref());

        connection::ssh_connect_tasks(host, session_id, host_id, should_detect_os)
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
                LocalEvent::Disconnected => {
                    Message::Session(SessionMessage::Disconnected(session_id))
                }
            },
        );

        // Spawn the local terminal
        // Use default terminal size (80x24), will be resized on first render
        match LocalSession::spawn(80, 24, event_tx) {
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
                    Task::perform(async move { list_local_dir(&path).await }, move |result| {
                        Message::Sftp(SftpMessage::PaneListResult(tab_id, pane_id, result))
                    })
                }
                PaneSource::Remote { session_id, .. } => {
                    // Load remote directory via SFTP
                    if let Some(sftp) = self.sftp.get_connection(*session_id) {
                        let sftp = sftp.clone();
                        Task::perform(async move { sftp.list_dir(&path).await }, move |result| {
                            Message::Sftp(SftpMessage::PaneListResult(
                                tab_id,
                                pane_id,
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
            // Show password dialog for SFTP
            let password_dialog = PasswordDialogState::new_sftp(
                host.name.clone(),
                host.hostname.clone(),
                host.port,
                host.username.clone(),
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
                                file_path.clone(),
                                file_type.clone(),
                            ),
                            PaneSource::Remote { session_id, .. } => {
                                if let Some(sftp) = self.sftp.get_connection(*session_id) {
                                    file_viewer::build_remote_viewer(
                                        viewer_id,
                                        file_name.clone(),
                                        file_path.clone(),
                                        *session_id,
                                        sftp.clone(),
                                        file_type.clone(),
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
                                        let result = if is_dir {
                                            std::fs::remove_dir_all(&path)
                                        } else {
                                            std::fs::remove_file(&path)
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
            .map(|e| (e.name.clone(), e.path.clone(), e.is_dir))
            .collect();

        if entries_to_copy.is_empty() {
            return Task::none();
        }

        let target_dir = target_pane.current_path.clone();
        let source = source_pane.source.clone();
        let target = target_pane.source.clone();

        match (&source, &target) {
            (PaneSource::Local, PaneSource::Local) => {
                // Local to Local copy
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            let mut count = 0;
                            for (name, source_path, is_dir) in entries_to_copy {
                                let target_path = target_dir.join(&name);
                                if is_dir {
                                    copy_dir_recursive(&source_path, &target_path)?;
                                    count += count_items_in_dir(&source_path)?;
                                } else {
                                    std::fs::copy(&source_path, &target_path).map_err(|e| {
                                        format!("Failed to copy {}: {}", source_path.display(), e)
                                    })?;
                                    count += 1;
                                }
                            }
                            Ok(count)
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    move |result| {
                        Message::Sftp(SftpMessage::CopyResult(tab_id, target_pane_id, result))
                    },
                )
            }
            (PaneSource::Local, PaneSource::Remote { session_id, .. }) => {
                // Local to Remote upload
                let sftp_session_id = *session_id;
                if let Some(sftp) = self.sftp.get_connection(sftp_session_id) {
                    let sftp = sftp.clone();
                    Task::perform(
                        async move {
                            let start = Instant::now();
                            let mut count = 0;
                            for (name, source_path, is_dir) in entries_to_copy {
                                let target_path = target_dir.join(&name);
                                if is_dir {
                                    count += sftp
                                        .upload_recursive(&source_path, &target_path)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                } else {
                                    sftp.upload(&source_path, &target_path)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                    count += 1;
                                }
                            }
                            tracing::info!(
                                "SFTP upload completed: {} item(s) in {:?}",
                                count,
                                start.elapsed()
                            );
                            Ok(count)
                        },
                        move |result| {
                            Message::Sftp(SftpMessage::CopyResult(tab_id, target_pane_id, result))
                        },
                    )
                } else {
                    Task::none()
                }
            }
            (PaneSource::Remote { session_id, .. }, PaneSource::Local) => {
                // Remote to Local download
                let sftp_session_id = *session_id;
                if let Some(sftp) = self.sftp.get_connection(sftp_session_id) {
                    let sftp = sftp.clone();
                    Task::perform(
                        async move {
                            let start = Instant::now();
                            let mut count = 0;
                            for (name, source_path, is_dir) in entries_to_copy {
                                let target_path = target_dir.join(&name);
                                if is_dir {
                                    count += sftp
                                        .download_recursive(&source_path, &target_path)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                } else {
                                    sftp.download(&source_path, &target_path)
                                        .await
                                        .map_err(|e| e.to_string())?;
                                    count += 1;
                                }
                            }
                            tracing::info!(
                                "SFTP download completed: {} item(s) in {:?}",
                                count,
                                start.elapsed()
                            );
                            Ok(count)
                        },
                        move |result| {
                            Message::Sftp(SftpMessage::CopyResult(tab_id, target_pane_id, result))
                        },
                    )
                } else {
                    Task::none()
                }
            }
            (
                PaneSource::Remote {
                    session_id: source_session_id,
                    ..
                },
                PaneSource::Remote {
                    session_id: target_session_id,
                    ..
                },
            ) => {
                // Remote to Remote copy (via local temp)
                let source_sftp_id = *source_session_id;
                let target_sftp_id = *target_session_id;

                let source_sftp = self.sftp.get_connection(source_sftp_id).cloned();
                let target_sftp = self.sftp.get_connection(target_sftp_id).cloned();

                if let (Some(source_sftp), Some(target_sftp)) = (source_sftp, target_sftp) {
                    Task::perform(
                        async move {
                            let start = Instant::now();
                            let mut count = 0;
                            let temp_dir = std::env::temp_dir()
                                .join(format!("portal_copy_{}", uuid::Uuid::new_v4()));
                            tokio::fs::create_dir_all(&temp_dir)
                                .await
                                .map_err(|e| format!("Failed to create temp directory: {}", e))?;

                            let result = async {
                                for (name, source_path, is_dir) in entries_to_copy {
                                    let temp_path = temp_dir.join(&name);
                                    let target_path = target_dir.join(&name);

                                    // Download to temp
                                    if is_dir {
                                        source_sftp
                                            .download_recursive(&source_path, &temp_path)
                                            .await
                                            .map_err(|e| e.to_string())?;
                                    } else {
                                        source_sftp
                                            .download(&source_path, &temp_path)
                                            .await
                                            .map_err(|e| e.to_string())?;
                                    }

                                    // Upload from temp to target
                                    if is_dir {
                                        count += target_sftp
                                            .upload_recursive(&temp_path, &target_path)
                                            .await
                                            .map_err(|e| e.to_string())?;
                                    } else {
                                        target_sftp
                                            .upload(&temp_path, &target_path)
                                            .await
                                            .map_err(|e| e.to_string())?;
                                        count += 1;
                                    }
                                }

                                Ok(count)
                            }
                            .await;

                            let _ = cleanup_temp_dir(&temp_dir).await;
                            if result.is_ok() {
                                tracing::info!(
                                    "SFTP remote copy completed: {} item(s) in {:?}",
                                    count,
                                    start.elapsed()
                                );
                            }

                            result
                        },
                        move |result| {
                            Message::Sftp(SftpMessage::CopyResult(tab_id, target_pane_id, result))
                        },
                    )
                } else {
                    Task::none()
                }
            }
        }
    }
}
