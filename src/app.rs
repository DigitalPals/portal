use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use futures::stream;
use iced::keyboard::{self, Key, Modifiers};
use iced::widget::{button, column, container, row, text, stack};
use iced::{event, Element, Fill, Subscription, Task, Theme as IcedTheme};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::{Host, HostsConfig, Snippet, SnippetsConfig};
use crate::message::{DialogType, EventReceiver, Message, SessionId};
use crate::sftp::{SharedSftpSession, SftpClient};
use crate::ssh::{SshClient, SshEvent, SshSession};
use crate::theme::THEME;
use crate::views::dialogs::host_dialog::{
    host_dialog_view, AuthMethodChoice, HostDialogState,
};
use crate::views::dialogs::settings_dialog::{settings_dialog_view, SettingsDialogState};
use crate::views::dialogs::sftp_dialogs::{
    delete_confirm_dialog_view, mkdir_dialog_view, DeleteConfirmDialogState, MkdirDialogState,
};
use crate::views::dialogs::snippets_dialog::{snippets_dialog_view, SnippetsDialogState};
use crate::views::host_grid::{host_grid_view, HostCard};
use crate::views::sftp_view::{sftp_browser_view, SftpBrowserState};
use crate::views::sidebar::{sidebar_view, SidebarFolder, SidebarHost};
use crate::views::tabs::{tab_bar_view, Tab, TabType};
use crate::views::terminal_view::{terminal_view, TerminalSession};

/// The active view in the main content area
#[derive(Debug, Clone, Default)]
pub enum View {
    #[default]
    HostGrid,
    TerminalDemo,
    Terminal(SessionId),
    Sftp(SessionId),
}

/// Active SSH session with its terminal
pub struct ActiveSession {
    pub ssh_session: Arc<SshSession>,
    pub terminal: TerminalSession,
    pub host_name: String,
}

/// Active SFTP session
pub struct SftpSessionState {
    pub sftp_session: SharedSftpSession,
    pub browser_state: SftpBrowserState,
}

/// Main application state
pub struct Portal {
    // UI state
    active_view: View,
    search_query: String,
    selected_host: Option<Uuid>,

    // Tab management
    tabs: Vec<Tab>,
    active_tab: Option<Uuid>,

    // Dialog state
    dialog: Option<HostDialogState>,
    mkdir_dialog: Option<MkdirDialogState>,
    delete_dialog: Option<DeleteConfirmDialogState>,
    settings_dialog: Option<SettingsDialogState>,
    snippets_dialog: Option<SnippetsDialogState>,

    // Theme preference
    dark_mode: bool,

    // Data from config
    hosts_config: HostsConfig,
    snippets_config: SnippetsConfig,

    // Demo terminal session
    demo_terminal: Option<TerminalSession>,

    // SSH client and active sessions
    ssh_client: SshClient,
    sessions: HashMap<SessionId, ActiveSession>,

    // SFTP client and sessions
    sftp_client: SftpClient,
    sftp_sessions: HashMap<SessionId, SftpSessionState>,

    // Connection status message
    status_message: Option<String>,
}

impl Portal {
    /// Create new application with initial state
    pub fn new() -> (Self, Task<Message>) {
        // Load hosts from config file
        let hosts_config = match HostsConfig::load() {
            Ok(config) => {
                tracing::info!("Loaded {} hosts from config", config.hosts.len());
                config
            }
            Err(e) => {
                tracing::warn!("Failed to load hosts config: {}, using empty config", e);
                HostsConfig::default()
            }
        };

        // Load snippets from config file
        let snippets_config = match SnippetsConfig::load() {
            Ok(config) => {
                tracing::info!("Loaded {} snippets from config", config.snippets.len());
                config
            }
            Err(e) => {
                tracing::warn!("Failed to load snippets config: {}, using empty config", e);
                SnippetsConfig::default()
            }
        };

        // Create demo terminal session and add some test content
        let mut demo_terminal = TerminalSession::new("Demo Terminal");
        // Add some demo output to show the terminal is working
        let demo_content = b"\x1b[1;32mWelcome to Portal Terminal!\x1b[0m\r\n\r\n\
            This is a \x1b[1;34mtest\x1b[0m of the terminal widget.\r\n\r\n\
            \x1b[33mFeatures:\x1b[0m\r\n\
            - ANSI color support\r\n\
            - Cursor rendering\r\n\
            - Keyboard input\r\n\r\n\
            \x1b[36mType something to test input:\x1b[0m \r\n\
            $ ";
        demo_terminal.process_output(demo_content);

        let app = Self {
            active_view: View::HostGrid,
            search_query: String::new(),
            selected_host: None,
            tabs: Vec::new(),
            active_tab: None,
            dialog: None,
            mkdir_dialog: None,
            delete_dialog: None,
            settings_dialog: None,
            snippets_dialog: None,
            dark_mode: true,
            hosts_config,
            snippets_config,
            demo_terminal: Some(demo_terminal),
            ssh_client: SshClient::default(),
            sessions: HashMap::new(),
            sftp_client: SftpClient::default(),
            sftp_sessions: HashMap::new(),
            status_message: None,
        };

        (app, Task::none())
    }

    /// Start an SSH connection to a host
    fn connect_to_host(&mut self, host: &Host) -> Task<Message> {
        let host = host.clone();
        let session_id = Uuid::new_v4();

        self.status_message = Some(format!("Connecting to {}...", host.name));

        // Create channel for SSH events
        let (event_tx, mut event_rx) = mpsc::unbounded_channel::<SshEvent>();

        // Clone what we need for the async task
        let ssh_client = SshClient::default();
        let host_clone = host.clone();

        // Spawn the connection task
        Task::perform(
            async move {
                let result = ssh_client
                    .connect(
                        &host_clone,
                        (80, 24), // Default terminal size
                        event_tx,
                        Duration::from_secs(30),
                        None, // No password for now (use agent/key)
                    )
                    .await;

                (session_id, host_clone.name.clone(), result, event_rx)
            },
            |(session_id, host_name, result, event_rx)| match result {
                Ok(ssh_session) => Message::SshConnected {
                    session_id,
                    host_name,
                    ssh_session,
                    event_rx: EventReceiver(Some(event_rx)),
                },
                Err(e) => Message::SshError(format!("Connection failed: {}", e)),
            },
        )
    }

    /// Convert config hosts to sidebar display format
    fn get_sidebar_hosts(&self) -> Vec<SidebarHost> {
        self.hosts_config
            .hosts
            .iter()
            .map(|host| {
                let folder_name = host.group_id.and_then(|gid| {
                    self.hosts_config
                        .find_group(gid)
                        .map(|g| g.name.clone())
                });
                SidebarHost {
                    id: host.id,
                    name: host.name.clone(),
                    hostname: host.hostname.clone(),
                    folder: folder_name,
                }
            })
            .collect()
    }

    /// Convert config groups to sidebar display format
    fn get_sidebar_folders(&self) -> Vec<SidebarFolder> {
        self.hosts_config
            .groups
            .iter()
            .map(|group| SidebarFolder {
                id: group.id,
                name: group.name.clone(),
                expanded: !group.collapsed,
            })
            .collect()
    }

    /// Convert config hosts to card display format
    fn get_host_cards(&self) -> Vec<HostCard> {
        self.hosts_config
            .hosts
            .iter()
            .map(|host| HostCard {
                id: host.id,
                name: host.name.clone(),
                hostname: host.hostname.clone(),
                username: host.username.clone(),
                tags: host.tags.clone(),
                last_connected: None, // TODO: Load from history
            })
            .collect()
    }

    /// Handle messages
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::HostSelected(id) => {
                self.selected_host = Some(id);
                tracing::info!("Host selected: {}", id);
            }
            Message::HostConnect(id) => {
                tracing::info!("Connect to host: {}", id);
                if let Some(host) = self.hosts_config.find_host(id).cloned() {
                    return self.connect_to_host(&host);
                }
            }
            Message::SearchChanged(query) => {
                self.search_query = query;
            }
            Message::FolderToggle(id) => {
                if let Some(group) = self.hosts_config.find_group_mut(id) {
                    group.collapsed = !group.collapsed;
                    if let Err(e) = self.hosts_config.save() {
                        tracing::error!("Failed to save config: {}", e);
                    }
                }
            }
            Message::ToggleTerminalDemo => {
                self.active_view = match self.active_view {
                    View::TerminalDemo => View::HostGrid,
                    _ => View::TerminalDemo,
                };
                tracing::info!("Switched to view: {:?}", self.active_view);
            }
            Message::HostAdd => {
                self.dialog = Some(HostDialogState::new_host());
            }
            Message::HostEdit(id) => {
                if let Some(host) = self.hosts_config.find_host(id) {
                    self.dialog = Some(HostDialogState::edit_host(host));
                }
            }
            Message::DialogOpen(dialog_type) => {
                match dialog_type {
                    DialogType::AddHost => {
                        self.dialog = Some(HostDialogState::new_host());
                    }
                    DialogType::EditHost(id) => {
                        if let Some(host) = self.hosts_config.find_host(id) {
                            self.dialog = Some(HostDialogState::edit_host(host));
                        }
                    }
                    DialogType::SftpMkdir(session_id, parent_path) => {
                        self.mkdir_dialog = Some(MkdirDialogState::new(session_id, parent_path));
                    }
                    DialogType::SftpDeleteConfirm(session_id, path, is_dir) => {
                        self.delete_dialog = Some(DeleteConfirmDialogState::new(session_id, path, is_dir));
                    }
                }
            }
            Message::DialogClose => {
                self.dialog = None;
                self.mkdir_dialog = None;
                self.delete_dialog = None;
                self.settings_dialog = None;
                self.snippets_dialog = None;
            }
            Message::DialogSubmit => {
                if let Some(ref dialog_state) = self.dialog {
                    if let Some(host) = dialog_state.to_host() {
                        // Preserve created_at for edits
                        let host = if let Some(existing_id) = dialog_state.editing_id {
                            if let Some(existing) = self.hosts_config.find_host(existing_id) {
                                Host {
                                    created_at: existing.created_at,
                                    ..host
                                }
                            } else {
                                host
                            }
                        } else {
                            host
                        };

                        if dialog_state.editing_id.is_some() {
                            if let Err(e) = self.hosts_config.update_host(host.clone()) {
                                tracing::error!("Failed to update host: {}", e);
                            } else {
                                tracing::info!("Updated host: {}", host.name);
                            }
                        } else {
                            self.hosts_config.add_host(host.clone());
                            tracing::info!("Added host: {}", host.name);
                        }

                        if let Err(e) = self.hosts_config.save() {
                            tracing::error!("Failed to save config: {}", e);
                        }
                        self.dialog = None;
                    }
                }
            }
            Message::DialogFieldChanged(field, value) => {
                if let Some(ref mut dialog_state) = self.dialog {
                    match field.as_str() {
                        "name" => dialog_state.name = value,
                        "hostname" => dialog_state.hostname = value,
                        "port" => dialog_state.port = value,
                        "username" => dialog_state.username = value,
                        "key_path" => dialog_state.key_path = value,
                        "tags" => dialog_state.tags = value,
                        "notes" => dialog_state.notes = value,
                        "auth_method" => {
                            dialog_state.auth_method = match value.as_str() {
                                "Agent" => AuthMethodChoice::Agent,
                                "Password" => AuthMethodChoice::Password,
                                "PublicKey" => AuthMethodChoice::PublicKey,
                                _ => dialog_state.auth_method,
                            };
                        }
                        "group_id" => {
                            dialog_state.group_id = if value.is_empty() {
                                None
                            } else {
                                Uuid::parse_str(&value).ok()
                            };
                        }
                        _ => {}
                    }
                }
            }
            Message::HostSave(host) => {
                if self.hosts_config.find_host(host.id).is_some() {
                    if let Err(e) = self.hosts_config.update_host(host) {
                        tracing::error!("Failed to update host: {}", e);
                    }
                } else {
                    self.hosts_config.add_host(host);
                }
                if let Err(e) = self.hosts_config.save() {
                    tracing::error!("Failed to save config: {}", e);
                }
            }
            Message::HostDelete(id) => {
                match self.hosts_config.delete_host(id) {
                    Ok(host) => {
                        tracing::info!("Deleted host: {}", host.name);
                        if let Err(e) = self.hosts_config.save() {
                            tracing::error!("Failed to save config: {}", e);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to delete host: {}", e);
                    }
                }
            }
            Message::Noop => {}
            // Terminal messages
            Message::TerminalInput(session_id, bytes) => {
                tracing::debug!("Terminal input for session {}: {:?}", session_id, bytes);
                // Send to SSH channel
                if let Some(session) = self.sessions.get(&session_id) {
                    let ssh_session = session.ssh_session.clone();
                    return Task::perform(
                        async move {
                            if let Err(e) = ssh_session.send(&bytes).await {
                                tracing::error!("Failed to send to SSH: {}", e);
                            }
                        },
                        |_| Message::Noop,
                    );
                }
            }
            Message::TerminalOutput(session_id, bytes) => {
                tracing::debug!("Terminal output for session {}: {} bytes", session_id, bytes.len());
                // TODO: Process through terminal backend
            }
            Message::SessionCreated(session_id) => {
                tracing::info!("Session created: {}", session_id);
            }
            Message::SessionClosed(session_id) => {
                tracing::info!("Session closed: {}", session_id);
                self.sessions.remove(&session_id);
                // If this was the active view, go back to host grid
                if matches!(self.active_view, View::Terminal(id) if id == session_id) {
                    self.active_view = View::HostGrid;
                }
            }
            Message::SshConnected {
                session_id,
                host_name,
                ssh_session,
                mut event_rx,
            } => {
                tracing::info!("SSH connected to {}", host_name);
                self.status_message = Some(format!("Connected to {}", host_name));

                // Create terminal session for this connection
                let terminal = TerminalSession::new(&host_name);

                // Store the active session
                self.sessions.insert(
                    session_id,
                    ActiveSession {
                        ssh_session,
                        terminal,
                        host_name: host_name.clone(),
                    },
                );

                // Create a new tab for this session
                let tab = Tab::new_terminal(session_id, host_name.clone());
                self.tabs.push(tab);
                self.active_tab = Some(session_id);

                // Switch to terminal view
                self.active_view = View::Terminal(session_id);

                // Start listening for SSH events
                if let Some(rx) = event_rx.0.take() {
                    return Task::run(
                        stream::unfold(rx, |mut rx| async move {
                            rx.recv().await.map(|event| (event, rx))
                        }),
                        move |event| match event {
                            SshEvent::Data(data) => Message::SshData(session_id, data),
                            SshEvent::Disconnected => Message::SshDisconnected(session_id),
                            SshEvent::Error(e) => Message::SshError(e),
                            _ => Message::Noop,
                        },
                    );
                }
            }
            Message::SshData(session_id, data) => {
                if let Some(session) = self.sessions.get_mut(&session_id) {
                    session.terminal.process_output(&data);
                }
            }
            Message::SshDisconnected(session_id) => {
                tracing::info!("SSH disconnected: {}", session_id);
                self.status_message = Some("Disconnected".to_string());
                self.sessions.remove(&session_id);
                self.tabs.retain(|t| t.id != session_id);

                // If this was the active tab, switch to another or go to host grid
                if self.active_tab == Some(session_id) {
                    if let Some(last_tab) = self.tabs.last() {
                        self.active_tab = Some(last_tab.id);
                        self.active_view = View::Terminal(last_tab.id);
                    } else {
                        self.active_tab = None;
                        self.active_view = View::HostGrid;
                    }
                } else if matches!(self.active_view, View::Terminal(id) if id == session_id) {
                    self.active_view = View::HostGrid;
                }
            }
            Message::SshError(error) => {
                tracing::error!("SSH error: {}", error);
                self.status_message = Some(error);
            }
            Message::TabSelect(tab_id) => {
                tracing::info!("Tab selected: {}", tab_id);
                self.active_tab = Some(tab_id);
                // Switch view to show this tab's content
                if self.sessions.contains_key(&tab_id) {
                    self.active_view = View::Terminal(tab_id);
                } else if self.sftp_sessions.contains_key(&tab_id) {
                    self.active_view = View::Sftp(tab_id);
                }
            }
            Message::TabClose(tab_id) => {
                tracing::info!("Tab closed: {}", tab_id);
                // Remove the tab
                self.tabs.retain(|t| t.id != tab_id);
                // Remove sessions (both terminal and SFTP)
                self.sessions.remove(&tab_id);
                self.sftp_sessions.remove(&tab_id);

                // If this was the active tab, switch to another or go to host grid
                if self.active_tab == Some(tab_id) {
                    if let Some(last_tab) = self.tabs.last() {
                        self.active_tab = Some(last_tab.id);
                        // Determine view type based on what sessions exist
                        if self.sessions.contains_key(&last_tab.id) {
                            self.active_view = View::Terminal(last_tab.id);
                        } else if self.sftp_sessions.contains_key(&last_tab.id) {
                            self.active_view = View::Sftp(last_tab.id);
                        }
                    } else {
                        self.active_tab = None;
                        self.active_view = View::HostGrid;
                    }
                }
            }
            Message::TabNew => {
                tracing::info!("New tab requested");
                // Go to host grid to select a new connection
                self.active_view = View::HostGrid;
            }
            // SFTP messages
            Message::SftpOpen(host_id) => {
                tracing::info!("Opening SFTP for host: {}", host_id);
                if let Some(host) = self.hosts_config.find_host(host_id).cloned() {
                    return self.connect_sftp(&host);
                }
            }
            Message::SftpConnected {
                session_id,
                host_name,
                sftp_session,
            } => {
                tracing::info!("SFTP connected to {}", host_name);
                let home_dir = sftp_session.home_dir().to_path_buf();

                // Create browser state
                let browser_state = SftpBrowserState::new(session_id, host_name.clone(), home_dir.clone());

                // Store SFTP session
                self.sftp_sessions.insert(
                    session_id,
                    SftpSessionState {
                        sftp_session: sftp_session.clone(),
                        browser_state,
                    },
                );

                // Create tab
                let tab = Tab::new_sftp(session_id, host_name);
                self.tabs.push(tab);
                self.active_tab = Some(session_id);
                self.active_view = View::Sftp(session_id);

                // Start loading directory
                return self.load_sftp_directory(session_id, home_dir);
            }
            Message::SftpNavigate(session_id, path) => {
                if let Some(state) = self.sftp_sessions.get_mut(&session_id) {
                    state.browser_state.current_path = path.clone();
                    state.browser_state.loading = true;
                    return self.load_sftp_directory(session_id, path);
                }
            }
            Message::SftpNavigateUp(session_id) => {
                if let Some(state) = self.sftp_sessions.get_mut(&session_id) {
                    if let Some(parent) = state.browser_state.current_path.parent() {
                        let path = parent.to_path_buf();
                        state.browser_state.current_path = path.clone();
                        state.browser_state.loading = true;
                        return self.load_sftp_directory(session_id, path);
                    }
                }
            }
            Message::SftpRefresh(session_id) => {
                if let Some(state) = self.sftp_sessions.get_mut(&session_id) {
                    let path = state.browser_state.current_path.clone();
                    state.browser_state.loading = true;
                    return self.load_sftp_directory(session_id, path);
                }
            }
            Message::SftpSelect(session_id, index) => {
                if let Some(state) = self.sftp_sessions.get_mut(&session_id) {
                    state.browser_state.selected_index = Some(index);
                }
            }
            Message::SftpListResult(session_id, result) => {
                if let Some(state) = self.sftp_sessions.get_mut(&session_id) {
                    match result {
                        Ok(entries) => {
                            state.browser_state.set_entries(entries);
                        }
                        Err(e) => {
                            state.browser_state.set_error(e);
                        }
                    }
                }
            }
            Message::SftpDownload(session_id, path) => {
                tracing::info!("Download requested: {:?}", path);
                if let Some(state) = self.sftp_sessions.get(&session_id) {
                    let sftp = state.sftp_session.clone();
                    let file_name = path.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "download".to_string());

                    return Task::perform(
                        async move {
                            // Open native save dialog
                            let file_handle = rfd::AsyncFileDialog::new()
                                .set_file_name(&file_name)
                                .save_file()
                                .await;

                            if let Some(handle) = file_handle {
                                let local_path = handle.path().to_path_buf();
                                match sftp.download(&path, &local_path).await {
                                    Ok(_) => Ok(local_path),
                                    Err(e) => Err(e.to_string()),
                                }
                            } else {
                                Err("Download cancelled".to_string())
                            }
                        },
                        move |result| Message::SftpDownloadComplete(session_id, result),
                    );
                }
            }
            Message::SftpUpload(session_id) => {
                tracing::info!("Upload requested for session: {}", session_id);
                if let Some(state) = self.sftp_sessions.get(&session_id) {
                    let sftp = state.sftp_session.clone();
                    let current_path = state.browser_state.current_path.clone();

                    return Task::perform(
                        async move {
                            // Open native file picker
                            let file_handle = rfd::AsyncFileDialog::new()
                                .pick_file()
                                .await;

                            if let Some(handle) = file_handle {
                                let local_path = handle.path().to_path_buf();
                                let file_name = local_path.file_name()
                                    .map(|n| n.to_os_string())
                                    .unwrap_or_default();
                                let remote_path = current_path.join(file_name);

                                match sftp.upload(&local_path, &remote_path).await {
                                    Ok(_) => Ok(()),
                                    Err(e) => Err(e.to_string()),
                                }
                            } else {
                                Err("Upload cancelled".to_string())
                            }
                        },
                        move |result| Message::SftpUploadComplete(session_id, result),
                    );
                }
            }
            Message::SftpMkdir(session_id) => {
                tracing::info!("Mkdir requested for session: {}", session_id);
                if let Some(state) = self.sftp_sessions.get(&session_id) {
                    let current_path = state.browser_state.current_path.clone();
                    self.mkdir_dialog = Some(MkdirDialogState::new(session_id, current_path));
                }
            }
            Message::SftpMkdirNameChanged(name) => {
                if let Some(ref mut dialog) = self.mkdir_dialog {
                    dialog.folder_name = name;
                }
            }
            Message::SftpMkdirSubmit => {
                if let Some(dialog) = self.mkdir_dialog.take() {
                    if dialog.is_valid() {
                        let session_id = dialog.session_id;
                        let full_path = dialog.full_path();

                        if let Some(state) = self.sftp_sessions.get(&session_id) {
                            let sftp = state.sftp_session.clone();

                            return Task::perform(
                                async move {
                                    match sftp.create_dir(&full_path).await {
                                        Ok(()) => Ok(full_path),
                                        Err(e) => Err(e.to_string()),
                                    }
                                },
                                move |result| Message::SftpMkdirResult(session_id, result),
                            );
                        }
                    }
                }
            }
            Message::SftpMkdirResult(session_id, result) => {
                match result {
                    Ok(path) => {
                        tracing::info!("Created directory: {:?}", path);
                        // Refresh the directory listing
                        if let Some(state) = self.sftp_sessions.get(&session_id) {
                            let current_path = state.browser_state.current_path.clone();
                            return self.load_sftp_directory(session_id, current_path);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to create directory: {}", e);
                        if let Some(state) = self.sftp_sessions.get_mut(&session_id) {
                            state.browser_state.set_error(format!("Failed to create folder: {}", e));
                        }
                    }
                }
            }
            Message::SftpDelete(session_id, path) => {
                tracing::info!("Delete requested: {:?}", path);
                // Check if it's a directory
                let is_dir = if let Some(state) = self.sftp_sessions.get(&session_id) {
                    state.browser_state.selected_entry()
                        .map(|e| e.is_dir)
                        .unwrap_or(false)
                } else {
                    false
                };
                self.delete_dialog = Some(DeleteConfirmDialogState::new(session_id, path, is_dir));
            }
            Message::SftpDeleteConfirm => {
                if let Some(dialog) = self.delete_dialog.take() {
                    let session_id = dialog.session_id;
                    let path = dialog.path.clone();
                    let is_dir = dialog.is_directory;

                    if let Some(state) = self.sftp_sessions.get(&session_id) {
                        let sftp = state.sftp_session.clone();

                        return Task::perform(
                            async move {
                                let result = if is_dir {
                                    sftp.remove_recursive(&path).await
                                } else {
                                    sftp.remove_file(&path).await
                                };
                                match result {
                                    Ok(()) => Ok(path),
                                    Err(e) => Err(e.to_string()),
                                }
                            },
                            move |result| Message::SftpDeleteResult(session_id, result),
                        );
                    }
                }
            }
            Message::SftpDeleteResult(session_id, result) => {
                match result {
                    Ok(path) => {
                        tracing::info!("Deleted: {:?}", path);
                        // Refresh the directory listing
                        if let Some(state) = self.sftp_sessions.get(&session_id) {
                            let current_path = state.browser_state.current_path.clone();
                            return self.load_sftp_directory(session_id, current_path);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to delete: {}", e);
                        if let Some(state) = self.sftp_sessions.get_mut(&session_id) {
                            state.browser_state.set_error(format!("Failed to delete: {}", e));
                        }
                    }
                }
            }
            Message::SftpDownloadComplete(session_id, result) => {
                match result {
                    Ok(path) => {
                        tracing::info!("Download complete: {:?}", path);
                        self.status_message = Some(format!("Downloaded to {}", path.display()));
                    }
                    Err(e) => {
                        if e != "Download cancelled" {
                            tracing::error!("Download failed: {}", e);
                            if let Some(state) = self.sftp_sessions.get_mut(&session_id) {
                                state.browser_state.set_error(format!("Download failed: {}", e));
                            }
                        }
                    }
                }
            }
            Message::SftpUploadComplete(session_id, result) => {
                match result {
                    Ok(()) => {
                        tracing::info!("Upload complete");
                        self.status_message = Some("Upload complete".to_string());
                        // Refresh the directory listing
                        if let Some(state) = self.sftp_sessions.get(&session_id) {
                            let current_path = state.browser_state.current_path.clone();
                            return self.load_sftp_directory(session_id, current_path);
                        }
                    }
                    Err(e) => {
                        if e != "Upload cancelled" {
                            tracing::error!("Upload failed: {}", e);
                            if let Some(state) = self.sftp_sessions.get_mut(&session_id) {
                                state.browser_state.set_error(format!("Upload failed: {}", e));
                            }
                        }
                    }
                }
            }
            Message::SftpError(session_id, error) => {
                tracing::error!("SFTP error for {}: {}", session_id, error);
                if let Some(state) = self.sftp_sessions.get_mut(&session_id) {
                    state.browser_state.set_error(error);
                }
            }
            Message::KeyboardEvent(key, modifiers) => {
                // Handle global keyboard shortcuts
                match (key, modifiers.control(), modifiers.shift()) {
                    // Escape - close dialogs
                    (Key::Named(keyboard::key::Named::Escape), _, _) => {
                        if self.dialog.is_some() || self.mkdir_dialog.is_some()
                            || self.delete_dialog.is_some() || self.settings_dialog.is_some()
                            || self.snippets_dialog.is_some() {
                            self.dialog = None;
                            self.mkdir_dialog = None;
                            self.delete_dialog = None;
                            self.settings_dialog = None;
                            self.snippets_dialog = None;
                        }
                    }
                    // Ctrl+N - new tab / go to host grid
                    (Key::Character(c), true, false) if c.as_str() == "n" => {
                        self.active_view = View::HostGrid;
                    }
                    // Ctrl+W - close current tab
                    (Key::Character(c), true, false) if c.as_str() == "w" => {
                        if let Some(tab_id) = self.active_tab {
                            // Close the active tab
                            self.tabs.retain(|t| t.id != tab_id);
                            self.sessions.remove(&tab_id);
                            self.sftp_sessions.remove(&tab_id);

                            // Select next available tab or go to host grid
                            if let Some(next_tab) = self.tabs.first() {
                                let next_id = next_tab.id;
                                self.active_tab = Some(next_id);
                                if self.sessions.contains_key(&next_id) {
                                    self.active_view = View::Terminal(next_id);
                                } else if self.sftp_sessions.contains_key(&next_id) {
                                    self.active_view = View::Sftp(next_id);
                                }
                            } else {
                                self.active_tab = None;
                                self.active_view = View::HostGrid;
                            }
                        }
                    }
                    // Ctrl+Tab - next tab
                    (Key::Named(keyboard::key::Named::Tab), true, false) => {
                        if !self.tabs.is_empty() {
                            let current_idx = self.active_tab
                                .and_then(|id| self.tabs.iter().position(|t| t.id == id))
                                .unwrap_or(0);
                            let next_idx = (current_idx + 1) % self.tabs.len();
                            let next_tab = &self.tabs[next_idx];
                            let next_id = next_tab.id;
                            self.active_tab = Some(next_id);
                            if self.sessions.contains_key(&next_id) {
                                self.active_view = View::Terminal(next_id);
                            } else if self.sftp_sessions.contains_key(&next_id) {
                                self.active_view = View::Sftp(next_id);
                            }
                        }
                    }
                    // Ctrl+Shift+Tab - previous tab
                    (Key::Named(keyboard::key::Named::Tab), true, true) => {
                        if !self.tabs.is_empty() {
                            let current_idx = self.active_tab
                                .and_then(|id| self.tabs.iter().position(|t| t.id == id))
                                .unwrap_or(0);
                            let prev_idx = if current_idx == 0 {
                                self.tabs.len() - 1
                            } else {
                                current_idx - 1
                            };
                            let prev_tab = &self.tabs[prev_idx];
                            let prev_id = prev_tab.id;
                            self.active_tab = Some(prev_id);
                            if self.sessions.contains_key(&prev_id) {
                                self.active_view = View::Terminal(prev_id);
                            } else if self.sftp_sessions.contains_key(&prev_id) {
                                self.active_view = View::Sftp(prev_id);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Message::SettingsOpen => {
                self.settings_dialog = Some(SettingsDialogState {
                    dark_mode: self.dark_mode,
                });
            }
            Message::SettingsThemeToggle(enabled) => {
                self.dark_mode = enabled;
                if let Some(ref mut dialog) = self.settings_dialog {
                    dialog.dark_mode = enabled;
                }
            }
            Message::SnippetsOpen => {
                self.snippets_dialog = Some(SnippetsDialogState::new(
                    self.snippets_config.snippets.clone(),
                ));
            }
            Message::SnippetSelect(id) => {
                if let Some(ref mut dialog) = self.snippets_dialog {
                    dialog.selected_id = Some(id);
                }
            }
            Message::SnippetNew => {
                if let Some(ref mut dialog) = self.snippets_dialog {
                    dialog.start_new();
                }
            }
            Message::SnippetEdit(id) => {
                if let Some(ref mut dialog) = self.snippets_dialog {
                    if let Some(snippet) = dialog.snippets.iter().find(|s| s.id == id).cloned() {
                        dialog.start_edit(&snippet);
                    }
                }
            }
            Message::SnippetDelete(id) => {
                // Remove from dialog and config
                if let Some(ref mut dialog) = self.snippets_dialog {
                    dialog.snippets.retain(|s| s.id != id);
                    dialog.selected_id = None;
                }
                let _ = self.snippets_config.delete_snippet(id);
                let _ = self.snippets_config.save();
            }
            Message::SnippetInsert(id) => {
                // Insert snippet into active terminal
                if let Some(snippet) = self.snippets_config.find_snippet(id) {
                    let command = snippet.command.clone();
                    if let Some(session_id) = self.active_tab {
                        if let Some(session) = self.sessions.get(&session_id) {
                            let data = command.into_bytes();
                            let ssh = session.ssh_session.clone();
                            return Task::perform(
                                async move {
                                    let _ = ssh.send(&data).await;
                                },
                                move |_| Message::Noop,
                            );
                        }
                    }
                }
                self.snippets_dialog = None;
            }
            Message::SnippetFieldChanged(field, value) => {
                if let Some(ref mut dialog) = self.snippets_dialog {
                    match field.as_str() {
                        "name" => dialog.edit_name = value,
                        "command" => dialog.edit_command = value,
                        "description" => dialog.edit_description = value,
                        _ => {}
                    }
                }
            }
            Message::SnippetEditCancel => {
                if let Some(ref mut dialog) = self.snippets_dialog {
                    dialog.cancel_edit();
                }
            }
            Message::SnippetSave => {
                if let Some(ref mut dialog) = self.snippets_dialog {
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
                            if let Some(snippet) = self.snippets_config.find_snippet_mut(id) {
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
                            self.snippets_config.add_snippet(snippet);
                        }
                        let _ = self.snippets_config.save();
                        dialog.cancel_edit();
                    }
                }
            }
        }

        Task::none()
    }

    /// Connect to a host via SFTP
    fn connect_sftp(&self, host: &Host) -> Task<Message> {
        let host = host.clone();
        let session_id = Uuid::new_v4();

        // Create dummy event channel (SFTP doesn't need it for data)
        let (event_tx, _event_rx) = mpsc::unbounded_channel::<SshEvent>();

        let sftp_client = SftpClient::default();
        let host_clone = host.clone();

        Task::perform(
            async move {
                let result = sftp_client
                    .connect(
                        &host_clone,
                        event_tx,
                        Duration::from_secs(30),
                        None,
                    )
                    .await;

                (session_id, host_clone.name.clone(), result)
            },
            |(session_id, host_name, result)| match result {
                Ok(sftp_session) => Message::SftpConnected {
                    session_id,
                    host_name,
                    sftp_session,
                },
                Err(e) => Message::SshError(format!("SFTP connection failed: {}", e)),
            },
        )
    }

    /// Load SFTP directory listing
    fn load_sftp_directory(&self, session_id: SessionId, path: std::path::PathBuf) -> Task<Message> {
        if let Some(state) = self.sftp_sessions.get(&session_id) {
            let sftp = state.sftp_session.clone();
            Task::perform(
                async move {
                    sftp.list_dir(&path).await
                },
                move |result| {
                    Message::SftpListResult(
                        session_id,
                        result.map_err(|e| e.to_string()),
                    )
                },
            )
        } else {
            Task::none()
        }
    }

    /// Build the view
    pub fn view(&self) -> Element<'_, Message> {
        let all_hosts = self.get_sidebar_hosts();
        let all_cards = self.get_host_cards();
        let folders = self.get_sidebar_folders();

        // Filter hosts based on search
        let filtered_hosts: Vec<_> = if self.search_query.is_empty() {
            all_hosts
        } else {
            let query = self.search_query.to_lowercase();
            all_hosts
                .into_iter()
                .filter(|h| {
                    h.name.to_lowercase().contains(&query)
                        || h.hostname.to_lowercase().contains(&query)
                })
                .collect()
        };

        let filtered_cards: Vec<_> = if self.search_query.is_empty() {
            all_cards
        } else {
            let query = self.search_query.to_lowercase();
            all_cards
                .into_iter()
                .filter(|h| {
                    h.name.to_lowercase().contains(&query)
                        || h.hostname.to_lowercase().contains(&query)
                })
                .collect()
        };

        // Sidebar
        let sidebar = sidebar_view(
            &self.search_query,
            folders,
            filtered_hosts,
            self.selected_host,
        );

        // Main content
        let main_content: Element<'_, Message> = match &self.active_view {
            View::HostGrid => host_grid_view(filtered_cards),
            View::TerminalDemo => {
                if let Some(ref session) = self.demo_terminal {
                    terminal_view(session, |session_id, bytes| {
                        Message::TerminalInput(session_id, bytes)
                    })
                } else {
                    text("No terminal session").into()
                }
            }
            View::Terminal(session_id) => {
                if let Some(session) = self.sessions.get(session_id) {
                    terminal_view(&session.terminal, |sid, bytes| {
                        Message::TerminalInput(sid, bytes)
                    })
                } else {
                    text("Session not found").into()
                }
            }
            View::Sftp(session_id) => {
                if let Some(state) = self.sftp_sessions.get(session_id) {
                    sftp_browser_view(&state.browser_state)
                } else {
                    text("SFTP session not found").into()
                }
            }
        };

        // Tab bar - show actual tabs if we have any sessions, otherwise show title bar
        let header: Element<'_, Message> = if !self.tabs.is_empty() {
            // Show tab bar with session tabs
            tab_bar_view(&self.tabs, self.active_tab)
        } else {
            // Show simple title bar with terminal demo button
            let terminal_btn_text = match self.active_view {
                View::TerminalDemo => "Hide Terminal",
                _ => "Terminal Demo",
            };
            container(
                row![
                    text("Portal").size(14).color(THEME.text_primary),
                    container(text("")).width(Fill),
                    button(text(terminal_btn_text).size(12))
                        .style(|_theme, status| {
                            let bg = match status {
                                button::Status::Hovered => Some(THEME.hover.into()),
                                _ => Some(THEME.surface.into()),
                            };
                            button::Style {
                                background: bg,
                                text_color: THEME.text_primary,
                                border: iced::Border {
                                    color: THEME.border,
                                    width: 1.0,
                                    radius: 4.0.into(),
                                },
                                ..Default::default()
                            }
                        })
                        .padding([4, 12])
                        .on_press(Message::ToggleTerminalDemo),
                ]
                .spacing(8)
                .padding([8, 16])
                .align_y(iced::Alignment::Center),
            )
            .width(Fill)
            .style(|_theme| container::Style {
                background: Some(THEME.surface.into()),
                border: iced::Border {
                    color: THEME.border,
                    width: 1.0,
                    radius: 0.0.into(),
                },
                ..Default::default()
            })
            .into()
        };

        // Main layout with content below header
        let content_area = column![header, main_content];

        // Full layout: sidebar | content
        let main_layout: Element<'_, Message> = row![sidebar, content_area]
            .width(Fill)
            .height(Fill)
            .into();

        // Overlay dialog if open
        if let Some(ref dialog_state) = self.dialog {
            let dialog = host_dialog_view(dialog_state, &self.hosts_config.groups);
            stack![main_layout, dialog].into()
        } else if let Some(ref mkdir_state) = self.mkdir_dialog {
            let dialog = mkdir_dialog_view(mkdir_state);
            stack![main_layout, dialog].into()
        } else if let Some(ref delete_state) = self.delete_dialog {
            let dialog = delete_confirm_dialog_view(delete_state);
            stack![main_layout, dialog].into()
        } else if let Some(ref settings_state) = self.settings_dialog {
            let dialog = settings_dialog_view(settings_state);
            stack![main_layout, dialog].into()
        } else if let Some(ref snippets_state) = self.snippets_dialog {
            let dialog = snippets_dialog_view(snippets_state);
            stack![main_layout, dialog].into()
        } else {
            main_layout
        }
    }

    /// Theme based on dark_mode preference
    pub fn theme(&self) -> IcedTheme {
        if self.dark_mode {
            IcedTheme::Dark
        } else {
            IcedTheme::Light
        }
    }

    /// Keyboard subscription for shortcuts
    pub fn subscription(&self) -> Subscription<Message> {
        event::listen_with(|event, _status, _id| {
            if let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
                Some(Message::KeyboardEvent(key, modifiers))
            } else {
                None
            }
        })
    }
}
