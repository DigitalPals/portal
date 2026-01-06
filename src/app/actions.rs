use std::sync::Arc;
use std::time::Duration;

use futures::stream;
use iced::Task;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::{DetectedOs, Host};
use crate::fs_utils::{copy_dir_recursive, count_items_in_dir};
use crate::local_fs::list_local_dir;
use crate::message::{
    DialogMessage, Message, SessionId, SessionMessage, SftpMessage, VerificationRequestWrapper,
};
use crate::sftp::SftpClient;
use crate::ssh::{SshClient, SshEvent};
use crate::views::sftp::{ContextMenuAction, PaneId, PaneSource, PermissionBits, SftpDialogType};

use super::{Portal, View};

impl Portal {
    pub(super) fn set_active_tab(&mut self, tab_id: Uuid) {
        self.active_tab = Some(tab_id);
        if self.sessions.contains(tab_id) {
            self.active_view = View::Terminal(tab_id);
        } else if self.sftp.contains_tab(tab_id) {
            self.active_view = View::DualSftp(tab_id);
        }
    }

    pub(super) fn close_tab(&mut self, tab_id: Uuid) {
        let sftp_sessions_to_close = self
            .sftp
            .get_tab(tab_id)
            .map(|state| {
                let mut ids = Vec::new();
                for pane in [&state.left_pane, &state.right_pane] {
                    if let PaneSource::Remote { session_id, .. } = pane.source {
                        if !ids.contains(&session_id) {
                            ids.push(session_id);
                        }
                    }
                }
                ids
            })
            .unwrap_or_default();

        self.tabs.retain(|t| t.id != tab_id);
        self.sessions.remove(tab_id);
        self.sftp.remove_tab(tab_id);

        let mut history_changed = false;
        for session_id in sftp_sessions_to_close {
            let still_used = self.sftp.is_connection_in_use(session_id);
            if !still_used {
                self.sftp.remove_connection(session_id);
                if let Some(entry_id) = self.sftp.remove_history_entry(session_id) {
                    self.history_config.mark_disconnected(entry_id);
                    history_changed = true;
                }
            }
        }

        if history_changed {
            if let Err(e) = self.history_config.save() {
                tracing::error!("Failed to save history config: {}", e);
            }
        }

        if self.active_tab == Some(tab_id) {
            if let Some(last_tab) = self.tabs.last() {
                self.set_active_tab(last_tab.id);
            } else {
                self.active_tab = None;
                self.active_view = View::HostGrid;
                self.sidebar_selection = crate::message::SidebarMenuItem::Hosts;
                // Reset keyboard navigation state when returning to host grid
                self.terminal_captured = false;
                self.focus_section = crate::app::FocusSection::Content;
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

    pub(super) fn connect_to_host(&mut self, host: &Host) -> Task<Message> {
        // Use Arc to avoid multiple deep clones of Host data
        let host = Arc::new(host.clone());
        let session_id = Uuid::new_v4();
        let host_id = host.id;

        // Detect OS if not already detected, or if it's generic Linux (re-detect to get specific distro)
        let should_detect_os = match &host.detected_os {
            None => true,
            Some(DetectedOs::Linux) => true, // Re-detect generic Linux to get specific distro
            Some(_) => false,
        };

        // Create two channels:
        // 1. For sending events during connection (verification requests)
        // 2. For ongoing session data after connection
        let (event_tx, event_rx) = mpsc::unbounded_channel::<SshEvent>();

        // Start listening for events immediately - this allows us to receive
        // HostKeyVerification events during the connection handshake
        let event_listener = Task::run(
            stream::unfold(event_rx, |mut rx| async move {
                rx.recv().await.map(|event| (event, rx))
            }),
            move |event| match event {
                SshEvent::Data(data) => Message::Session(SessionMessage::Data(session_id, data)),
                SshEvent::Disconnected => Message::Session(SessionMessage::Disconnected(session_id)),
                SshEvent::HostKeyVerification(request) => {
                    Message::Dialog(DialogMessage::HostKeyVerification(VerificationRequestWrapper(Some(request))))
                }
                SshEvent::Connected => Message::Noop,
            },
        );

        let ssh_client = SshClient::default();
        let host_for_task = Arc::clone(&host);

        // Connection task
        let connect_task = Task::perform(
            async move {
                let result = ssh_client
                    .connect(
                        &host_for_task,
                        (80, 24),
                        event_tx,
                        Duration::from_secs(30),
                        None,
                        should_detect_os,
                    )
                    .await;

                (session_id, host_id, host_for_task.name.clone(), result)
            },
            |(session_id, host_id, host_name, result)| match result {
                Ok((ssh_session, detected_os)) => {
                    Message::Session(SessionMessage::Connected {
                        session_id,
                        host_name,
                        ssh_session,
                        host_id,
                        detected_os,
                    })
                }
                Err(e) => Message::Session(SessionMessage::Error(format!("Connection failed: {}", e))),
            },
        );

        // Run both tasks: listener starts immediately, connection proceeds in parallel
        Task::batch([event_listener, connect_task])
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
                    Task::perform(
                        async move { list_local_dir(&path).await },
                        move |result| Message::Sftp(SftpMessage::PaneListResult(tab_id, pane_id, result)),
                    )
                }
                PaneSource::Remote { session_id, .. } => {
                    // Load remote directory via SFTP
                    if let Some(sftp) = self.sftp.get_connection(*session_id) {
                        let sftp = sftp.clone();
                        Task::perform(
                            async move { sftp.list_dir(&path).await },
                            move |result| {
                                Message::Sftp(SftpMessage::PaneListResult(
                                    tab_id,
                                    pane_id,
                                    result.map_err(|e| e.to_string()),
                                ))
                            },
                        )
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
        // Use Arc to avoid multiple deep clones of Host data
        let host = Arc::new(host.clone());
        let sftp_session_id = Uuid::new_v4();
        let host_id = host.id;

        // Store pending connection info for host key verification
        self.sftp.set_pending_connection(Some((tab_id, pane_id, host_id)));

        // Create event channel for SSH events (including host key verification)
        let (event_tx, event_rx) = mpsc::unbounded_channel::<SshEvent>();

        let sftp_client = SftpClient::default();

        // Start listening for SSH events (host key verification)
        let event_listener = Task::run(
            futures::stream::unfold(event_rx, |mut rx| async move {
                rx.recv().await.map(|event| (event, rx))
            }),
            move |event| match event {
                SshEvent::HostKeyVerification(request) => {
                    Message::Dialog(DialogMessage::HostKeyVerification(VerificationRequestWrapper(Some(request))))
                }
                _ => Message::Noop,
            },
        );

        // Connection task
        let host_for_task = Arc::clone(&host);
        let connect_task = Task::perform(
            async move {
                let result = sftp_client
                    .connect(&host_for_task, event_tx, Duration::from_secs(30), None)
                    .await;

                (tab_id, pane_id, sftp_session_id, host_for_task.name.clone(), result)
            },
            move |(tab_id, pane_id, sftp_session_id, host_name, result)| match result {
                Ok(sftp_session) => Message::Sftp(SftpMessage::Connected {
                    tab_id,
                    pane_id,
                    sftp_session_id,
                    host_id,
                    host_name,
                    sftp_session,
                }),
                Err(e) => Message::Session(SessionMessage::Error(format!("SFTP connection failed: {}", e))),
            },
        );

        Task::batch([event_listener, connect_task])
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
                // Open file with default application
                if let Some(entry) = selected_entries.first() {
                    if !entry.is_dir && !entry.is_parent() {
                        let file_name = entry.name.clone();
                        let remote_path = entry.path.clone();

                        match &pane.source {
                            PaneSource::Local => {
                                // For local files, open directly
                                return Task::perform(
                                    async move {
                                        open::that(&remote_path)
                                            .map_err(|e| format!("Failed to open file: {}", e))
                                    },
                                    |result| Message::Sftp(SftpMessage::OpenWithResult(result)),
                                );
                            }
                            PaneSource::Remote { session_id, .. } => {
                                // For remote files, download to temp and open
                                if let Some(sftp) = self.sftp.get_connection(*session_id) {
                                    let sftp = sftp.clone();
                                    return Task::perform(
                                        async move {
                                            // Create temp directory for this file
                                            let temp_dir = std::env::temp_dir()
                                                .join("portal_open")
                                                .join(format!("{}", uuid::Uuid::new_v4()));

                                            tokio::fs::create_dir_all(&temp_dir).await
                                                .map_err(|e| format!("Failed to create temp directory: {}", e))?;

                                            let local_path = temp_dir.join(&file_name);

                                            // Download the file
                                            sftp.download(&remote_path, &local_path).await
                                                .map_err(|e| format!("Failed to download file: {}", e))?;

                                            // Open with default application
                                            open::that(&local_path)
                                                .map_err(|e| format!("Failed to open file: {}", e))
                                        },
                                        |result| Message::Sftp(SftpMessage::OpenWithResult(result)),
                                    );
                                }
                            }
                        }
                    }
                }
            }
            ContextMenuAction::OpenWith => {
                // Show the Open With dialog for single selection
                if let Some(entry) = selected_entries.first() {
                    if !entry.is_dir && !entry.is_parent() {
                        let file_name = entry.name.clone();
                        let file_path = entry.path.clone();
                        let is_remote = matches!(pane.source, PaneSource::Remote { .. });

                        if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                            tab_state.show_open_with_dialog(file_name, file_path, is_remote);
                        }
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
                                match std::fs::create_dir(&new_folder_path) {
                                    Ok(()) => Ok(()),
                                    Err(e) => Err(e.to_string()),
                                }
                            },
                            move |result| Message::Sftp(SftpMessage::NewFolderResult(tab_id, pane_id, result)),
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
                                move |result| Message::Sftp(SftpMessage::NewFolderResult(tab_id, pane_id, result)),
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
                                std::fs::rename(&old_path, &new_path).map_err(|e| e.to_string())
                            },
                            move |result| Message::Sftp(SftpMessage::RenameResult(tab_id, pane_id, result)),
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
                                move |result| Message::Sftp(SftpMessage::RenameResult(tab_id, pane_id, result)),
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
                                            ))
                                        }
                                    }
                                }
                                Ok(deleted_count)
                            },
                            move |result| Message::Sftp(SftpMessage::DeleteResult(tab_id, pane_id, result)),
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
                                                ))
                                            }
                                        }
                                    }
                                    Ok(deleted_count)
                                },
                                move |result| Message::Sftp(SftpMessage::DeleteResult(tab_id, pane_id, result)),
                            )
                        } else {
                            Task::none()
                        }
                    }
                }
            }
            SftpDialogType::EditPermissions { path, permissions, .. } => {
                let path = path.clone();
                let mode = permissions.to_mode();

                match &pane.source {
                    PaneSource::Local => {
                        // Set local file permissions
                        Task::perform(
                            async move {
                                #[cfg(unix)]
                                {
                                    use std::os::unix::fs::PermissionsExt;
                                    let permissions = std::fs::Permissions::from_mode(mode);
                                    std::fs::set_permissions(&path, permissions)
                                        .map_err(|e| format!("Failed to set permissions: {}", e))
                                }
                                #[cfg(not(unix))]
                                {
                                    let _ = (path, mode);
                                    Err("Permissions are only supported on Unix systems".to_string())
                                }
                            },
                            move |result| Message::Sftp(SftpMessage::PermissionsResult(tab_id, pane_id, result)),
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
                                move |result| Message::Sftp(SftpMessage::PermissionsResult(tab_id, pane_id, result)),
                            )
                        } else {
                            Task::none()
                        }
                    }
                }
            }
            SftpDialogType::OpenWith { path, is_remote, .. } => {
                let path = path.clone();
                let is_remote = *is_remote;
                let command = input_value;
                let pane_source = pane.source.clone();

                // Close dialog immediately
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.close_dialog();
                }

                if is_remote {
                    // For remote files, download to temp first, then open with command
                    if let PaneSource::Remote { session_id, .. } = &pane_source {
                        if let Some(sftp) = self.sftp.get_connection(*session_id) {
                            let sftp = sftp.clone();
                            let file_name = path.file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            return Task::perform(
                                async move {
                                    // Create temp directory for this file
                                    let temp_dir = std::env::temp_dir()
                                        .join("portal_open")
                                        .join(format!("{}", uuid::Uuid::new_v4()));

                                    tokio::fs::create_dir_all(&temp_dir).await
                                        .map_err(|e| format!("Failed to create temp directory: {}", e))?;

                                    let local_path = temp_dir.join(&file_name);

                                    // Download the file
                                    sftp.download(&path, &local_path).await
                                        .map_err(|e| format!("Failed to download file: {}", e))?;

                                    // Open with specified command
                                    let status = std::process::Command::new(&command)
                                        .arg(&local_path)
                                        .spawn();

                                    match status {
                                        Ok(_) => Ok(()),
                                        Err(e) => Err(format!("Failed to run '{}': {}", command, e)),
                                    }
                                },
                                |result| Message::Sftp(SftpMessage::OpenWithResult(result)),
                            );
                        }
                    }
                    Task::none()
                } else {
                    // For local files, just run the command directly
                    Task::perform(
                        async move {
                            let status = std::process::Command::new(&command)
                                .arg(&path)
                                .spawn();

                            match status {
                                Ok(_) => Ok(()),
                                Err(e) => Err(format!("Failed to run '{}': {}", command, e)),
                            }
                        },
                        |result| Message::Sftp(SftpMessage::OpenWithResult(result)),
                    )
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
                    },
                    move |result| Message::Sftp(SftpMessage::CopyResult(tab_id, target_pane_id, result)),
                )
            }
            (PaneSource::Local, PaneSource::Remote { session_id, .. }) => {
                // Local to Remote upload
                let sftp_session_id = *session_id;
                if let Some(sftp) = self.sftp.get_connection(sftp_session_id) {
                    let sftp = sftp.clone();
                    Task::perform(
                        async move {
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
                            Ok(count)
                        },
                        move |result| Message::Sftp(SftpMessage::CopyResult(tab_id, target_pane_id, result)),
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
                            Ok(count)
                        },
                        move |result| Message::Sftp(SftpMessage::CopyResult(tab_id, target_pane_id, result)),
                    )
                } else {
                    Task::none()
                }
            }
            (
                PaneSource::Remote { session_id: source_session_id, .. },
                PaneSource::Remote { session_id: target_session_id, .. },
            ) => {
                // Remote to Remote copy (via local temp)
                let source_sftp_id = *source_session_id;
                let target_sftp_id = *target_session_id;

                let source_sftp = self.sftp.get_connection(source_sftp_id).cloned();
                let target_sftp = self.sftp.get_connection(target_sftp_id).cloned();

                if let (Some(source_sftp), Some(target_sftp)) = (source_sftp, target_sftp) {
                    Task::perform(
                        async move {
                            let mut count = 0;
                            let temp_dir = std::env::temp_dir().join(format!("portal_copy_{}", uuid::Uuid::new_v4()));
                            tokio::fs::create_dir_all(&temp_dir).await.map_err(|e| {
                                format!("Failed to create temp directory: {}", e)
                            })?;

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

                            // Clean up temp directory
                            let _ = tokio::fs::remove_dir_all(&temp_dir).await;

                            Ok(count)
                        },
                        move |result| Message::Sftp(SftpMessage::CopyResult(tab_id, target_pane_id, result)),
                    )
                } else {
                    Task::none()
                }
            }
        }
    }
}
