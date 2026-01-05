mod actions;
mod view_model;

use std::collections::HashMap;
use std::sync::Arc;
use iced::keyboard::{self, Key};
use iced::widget::{button, column, container, row, text, stack};
use iced::{event, window, Element, Fill, Subscription, Task, Theme as IcedTheme};
use uuid::Uuid;

use crate::config::{AuthMethod, HistoryConfig, Host, HostsConfig, Snippet, SnippetsConfig};
use crate::message::{HostDialogField, Message, SessionId, SidebarMenuItem, SnippetField};
use crate::sftp::SharedSftpSession;
use crate::ssh::SshSession;
use crate::theme::{SIDEBAR_AUTO_COLLAPSE_THRESHOLD, THEME};
use crate::ssh::host_key_verification::HostKeyVerificationResponse;
use crate::views::dialogs::host_dialog::{
    host_dialog_view, AuthMethodChoice, HostDialogState,
};
use crate::views::dialogs::host_key_dialog::{host_key_dialog_view, HostKeyDialogState};
use crate::views::dialogs::settings_dialog::{settings_dialog_view, SettingsDialogState};
use crate::views::dialogs::sftp_dialogs::{
    delete_confirm_dialog_view, mkdir_dialog_view, DeleteConfirmDialogState, MkdirDialogState,
};
use crate::views::dialogs::snippets_dialog::{snippets_dialog_view, SnippetsDialogState};
use crate::views::history_view::history_view;
use crate::views::host_grid::{calculate_columns, host_grid_view};
use crate::views::sftp_picker::{sftp_picker_view, SftpHostCard};
use crate::views::sftp_view::{sftp_browser_view, SftpBrowserState};
use crate::views::sidebar::sidebar_view;
use crate::views::tabs::{tab_bar_view, Tab};
use crate::views::terminal_view::{terminal_view, TerminalSession};

use self::view_model::{filter_group_cards, filter_host_cards, group_cards, host_cards};

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

    // Sidebar state
    sidebar_collapsed: bool,
    sidebar_selection: SidebarMenuItem,

    // Tab management
    tabs: Vec<Tab>,
    active_tab: Option<Uuid>,

    // Dialog state
    dialog: Option<HostDialogState>,
    mkdir_dialog: Option<MkdirDialogState>,
    delete_dialog: Option<DeleteConfirmDialogState>,
    settings_dialog: Option<SettingsDialogState>,
    snippets_dialog: Option<SnippetsDialogState>,
    host_key_dialog: Option<HostKeyDialogState>,

    // Theme preference
    dark_mode: bool,

    // Data from config
    hosts_config: HostsConfig,
    snippets_config: SnippetsConfig,
    history_config: HistoryConfig,

    // Demo terminal session
    demo_terminal: Option<TerminalSession>,

    // Active sessions
    sessions: HashMap<SessionId, ActiveSession>,

    // SFTP sessions
    sftp_sessions: HashMap<SessionId, SftpSessionState>,

    // Connection status message
    status_message: Option<String>,

    // Responsive layout
    window_size: iced::Size,
    sidebar_manually_collapsed: bool,
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

        // Load history from config file
        let history_config = match HistoryConfig::load() {
            Ok(config) => {
                tracing::info!("Loaded {} history entries from config", config.entries.len());
                config
            }
            Err(e) => {
                tracing::warn!("Failed to load history config: {}, using empty config", e);
                HistoryConfig::default()
            }
        };

        // Create demo terminal session and add some test content
        let demo_terminal = TerminalSession::new("Demo Terminal");
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
            sidebar_collapsed: false,
            sidebar_selection: SidebarMenuItem::Hosts,
            tabs: Vec::new(),
            active_tab: None,
            dialog: None,
            mkdir_dialog: None,
            delete_dialog: None,
            settings_dialog: None,
            snippets_dialog: None,
            host_key_dialog: None,
            dark_mode: true,
            hosts_config,
            snippets_config,
            history_config,
            demo_terminal: Some(demo_terminal),
            sessions: HashMap::new(),
            sftp_sessions: HashMap::new(),
            status_message: None,
            window_size: iced::Size::new(1200.0, 800.0),
            sidebar_manually_collapsed: false,
        };

        (app, Task::none())
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
            Message::SidebarItemSelect(item) => {
                self.sidebar_selection = item;
                tracing::info!("Sidebar item selected: {:?}", item);
                // Handle view changes based on selection
                match item {
                    SidebarMenuItem::Hosts => {
                        if self.active_tab.is_none() {
                            self.active_view = View::HostGrid;
                        }
                    }
                    SidebarMenuItem::Settings => {
                        self.settings_dialog = Some(SettingsDialogState {
                            dark_mode: self.dark_mode,
                        });
                    }
                    SidebarMenuItem::Snippets => {
                        self.snippets_dialog = Some(SnippetsDialogState::new(
                            self.snippets_config.snippets.clone(),
                        ));
                    }
                    _ => {}
                }
            }
            Message::SidebarToggleCollapse => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
                // Track manual collapse state to prevent auto-collapse override
                self.sidebar_manually_collapsed = self.sidebar_collapsed;
                tracing::info!("Sidebar collapsed: {} (manual)", self.sidebar_collapsed);
            }
            Message::OsDetectionResult(host_id, result) => {
                match result {
                    Ok(detected_os) => {
                        tracing::info!("OS detected for host {}: {:?}", host_id, detected_os);
                        if let Some(host) = self.hosts_config.find_host_mut(host_id) {
                            host.detected_os = Some(detected_os);
                            host.updated_at = chrono::Utc::now();
                            if let Err(e) = self.hosts_config.save() {
                                tracing::error!("Failed to save hosts config: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to detect OS for host {}: {}", host_id, e);
                    }
                }
            }
            Message::HistoryClear => {
                self.history_config.clear();
                if let Err(e) = self.history_config.save() {
                    tracing::error!("Failed to save history: {}", e);
                }
            }
            Message::HistoryReconnect(entry_id) => {
                if let Some(entry) = self.history_config.find_entry(entry_id) {
                    let host_id = entry.host_id;
                    if let Some(host) = self.hosts_config.find_host(host_id).cloned() {
                        return self.connect_to_host(&host);
                    }
                }
            }
            Message::HostAdd => {
                self.dialog = Some(HostDialogState::new_host());
            }
            Message::HostEdit(id) => {
                if let Some(host) = self.hosts_config.find_host(id) {
                    self.dialog = Some(HostDialogState::edit_host(host));
                }
            }
            Message::QuickConnect => {
                // Parse search query as [ssh] [user@]hostname[:port]
                let query = self.search_query.trim();
                if query.is_empty() {
                    self.status_message = Some("Enter a hostname to connect".to_string());
                    return Task::none();
                }

                // Strip optional "ssh " prefix
                let query = query.strip_prefix("ssh ").unwrap_or(query);

                // Parse user@hostname:port
                let (user_part, host_part) = if let Some(at_pos) = query.rfind('@') {
                    (Some(&query[..at_pos]), &query[at_pos + 1..])
                } else {
                    (None, query)
                };

                let (hostname, port) = if let Some(colon_pos) = host_part.rfind(':') {
                    let port_str = &host_part[colon_pos + 1..];
                    if let Ok(port) = port_str.parse::<u16>() {
                        (&host_part[..colon_pos], port)
                    } else {
                        (host_part, 22)
                    }
                } else {
                    (host_part, 22)
                };

                // Get current username as default
                let username = user_part
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        std::env::var("USER")
                            .or_else(|_| std::env::var("USERNAME"))
                            .unwrap_or_else(|_| "root".to_string())
                    });

                let now = chrono::Utc::now();
                let temp_host = Host {
                    id: Uuid::new_v4(),
                    name: format!("{}@{}", username, hostname),
                    hostname: hostname.to_string(),
                    port,
                    username,
                    auth: AuthMethod::Agent,
                    group_id: None,
                    notes: None,
                    tags: vec![],
                    created_at: now,
                    updated_at: now,
                    detected_os: None,
                    last_connected: None,
                };

                tracing::info!("Quick connect to: {}@{}:{}", temp_host.username, temp_host.hostname, temp_host.port);
                return self.connect_to_host(&temp_host);
            }
            Message::LocalTerminal => {
                // Stub for now - local terminal support coming later
                self.status_message = Some("Local terminal coming soon".to_string());
                tracing::info!("Local terminal requested (not yet implemented)");
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
                    match field {
                        HostDialogField::Name => dialog_state.name = value,
                        HostDialogField::Hostname => dialog_state.hostname = value,
                        HostDialogField::Port => dialog_state.port = value,
                        HostDialogField::Username => dialog_state.username = value,
                        HostDialogField::KeyPath => dialog_state.key_path = value,
                        HostDialogField::Tags => dialog_state.tags = value,
                        HostDialogField::Notes => dialog_state.notes = value,
                        HostDialogField::AuthMethod => {
                            dialog_state.auth_method = match value.as_str() {
                                "Agent" => AuthMethodChoice::Agent,
                                "Password" => AuthMethodChoice::Password,
                                "PublicKey" => AuthMethodChoice::PublicKey,
                                _ => dialog_state.auth_method,
                            };
                        }
                        HostDialogField::GroupId => {
                            dialog_state.group_id = if value.is_empty() {
                                None
                            } else {
                                Uuid::parse_str(&value).ok()
                            };
                        }
                    }
                }
            }
            Message::Noop => {}
            // Window resize for responsive layout
            Message::WindowResized(size) => {
                self.window_size = size;

                // Auto-collapse/expand sidebar (unless manually collapsed)
                if !self.sidebar_manually_collapsed {
                    self.sidebar_collapsed = size.width < SIDEBAR_AUTO_COLLAPSE_THRESHOLD;
                }
            }
            // Host key verification
            Message::HostKeyVerification(mut wrapper) => {
                if let Some(request) = wrapper.0.take() {
                    self.host_key_dialog = Some(HostKeyDialogState::from_request(*request));
                    tracing::info!("Host key verification dialog opened");
                }
            }
            Message::HostKeyVerificationAccept => {
                if let Some(ref mut dialog) = self.host_key_dialog {
                    dialog.respond(HostKeyVerificationResponse::Accept);
                    tracing::info!("Host key accepted for {}:{}", dialog.host, dialog.port);
                }
                self.host_key_dialog = None;
            }
            Message::HostKeyVerificationReject => {
                if let Some(ref mut dialog) = self.host_key_dialog {
                    dialog.respond(HostKeyVerificationResponse::Reject);
                    tracing::info!("Host key rejected for {}:{}", dialog.host, dialog.port);
                    self.status_message = Some(format!(
                        "Connection rejected: host key verification failed for {}",
                        dialog.host
                    ));
                }
                self.host_key_dialog = None;
            }
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
            Message::TerminalResize(session_id, cols, rows) => {
                tracing::debug!(
                    "Terminal resize for session {}: {}x{}",
                    session_id,
                    cols,
                    rows
                );
                // Resize the terminal backend and notify SSH server
                if let Some(session) = self.sessions.get_mut(&session_id) {
                    session.terminal.resize(cols, rows);
                    // Send window change to SSH server
                    if let Err(e) = session.ssh_session.window_change(cols, rows) {
                        tracing::error!("Failed to send window change: {}", e);
                    }
                }
            }
            Message::SshConnected {
                session_id,
                host_name,
                ssh_session,
                event_rx: _, // Event listener already running from connect_to_host
                host_id,
                detected_os,
            } => {
                tracing::info!("SSH connected to {}", host_name);
                self.status_message = Some(format!("Connected to {}", host_name));

                // Update host with detected OS if available
                if let Some(os) = detected_os {
                    if let Some(host) = self.hosts_config.find_host_mut(host_id) {
                        host.detected_os = Some(os);
                        host.last_connected = Some(chrono::Utc::now());
                        host.updated_at = chrono::Utc::now();
                        if let Err(e) = self.hosts_config.save() {
                            tracing::error!("Failed to save hosts config with detected OS: {}", e);
                        }
                    }
                } else {
                    // Just update last_connected
                    if let Some(host) = self.hosts_config.find_host_mut(host_id) {
                        host.last_connected = Some(chrono::Utc::now());
                        host.updated_at = chrono::Utc::now();
                        let _ = self.hosts_config.save();
                    }
                }

                // Create terminal session for this connection
                let terminal = TerminalSession::new(&host_name);

                // Store the active session
                self.sessions.insert(
                    session_id,
                    ActiveSession {
                        ssh_session,
                        terminal,
                    },
                );

                // Create a new tab for this session
                let tab = Tab::new_terminal(session_id, host_name.clone());
                self.tabs.push(tab);
                self.active_tab = Some(session_id);

                // Switch to terminal view
                self.active_view = View::Terminal(session_id);

                // Event listener is already running from connect_to_host
            }
            Message::SshData(session_id, data) => {
                if let Some(session) = self.sessions.get_mut(&session_id) {
                    session.terminal.process_output(&data);
                }
            }
            Message::SshDisconnected(session_id) => {
                tracing::info!("SSH disconnected: {}", session_id);
                self.status_message = Some("Disconnected".to_string());
                self.close_tab(session_id);
            }
            Message::SshError(error) => {
                tracing::error!("SSH error: {}", error);
                self.status_message = Some(error);
            }
            Message::TabSelect(tab_id) => {
                tracing::info!("Tab selected: {}", tab_id);
                self.set_active_tab(tab_id);
            }
            Message::TabClose(tab_id) => {
                tracing::info!("Tab closed: {}", tab_id);
                self.close_tab(tab_id);
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
            Message::KeyboardEvent(key, modifiers) => {
                // Handle global keyboard shortcuts
                match (key, modifiers.control(), modifiers.shift()) {
                    // Escape - close dialogs
                    (Key::Named(keyboard::key::Named::Escape), _, _) => {
                        // Host key dialog - Escape means reject
                        if let Some(ref mut dialog) = self.host_key_dialog {
                            dialog.respond(HostKeyVerificationResponse::Reject);
                            self.status_message = Some("Connection cancelled".to_string());
                            self.host_key_dialog = None;
                        } else if self.dialog.is_some() || self.mkdir_dialog.is_some()
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
                        self.close_active_tab();
                    }
                    // Ctrl+Tab - next tab
                    (Key::Named(keyboard::key::Named::Tab), true, false) => {
                        self.select_next_tab();
                    }
                    // Ctrl+Shift+Tab - previous tab
                    (Key::Named(keyboard::key::Named::Tab), true, true) => {
                        self.select_prev_tab();
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
                    match field {
                        SnippetField::Name => dialog.edit_name = value,
                        SnippetField::Command => dialog.edit_command = value,
                        SnippetField::Description => dialog.edit_description = value,
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

    /// Build the view
    pub fn view(&self) -> Element<'_, Message> {
        let all_cards = host_cards(&self.hosts_config);
        let all_groups = group_cards(&self.hosts_config);

        // Filter based on search
        let filtered_cards = filter_host_cards(&self.search_query, all_cards);
        let filtered_groups = filter_group_cards(&self.search_query, all_groups);

        // Sidebar (new collapsible icon menu)
        let sidebar = sidebar_view(
            self.sidebar_collapsed,
            self.sidebar_selection,
        );

        // Main content - prioritize active sessions over sidebar selection
        let main_content: Element<'_, Message> = match &self.active_view {
            View::Terminal(session_id) => {
                if let Some(session) = self.sessions.get(session_id) {
                    let session_id = *session_id;
                    terminal_view(
                        &session.terminal,
                        move |_sid, bytes| Message::TerminalInput(session_id, bytes),
                        move |_sid, cols, rows| Message::TerminalResize(session_id, cols, rows),
                    )
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
            View::TerminalDemo => {
                if let Some(ref session) = self.demo_terminal {
                    terminal_view(
                        session,
                        |session_id, bytes| Message::TerminalInput(session_id, bytes),
                        |session_id, cols, rows| Message::TerminalResize(session_id, cols, rows),
                    )
                } else {
                    text("No terminal session").into()
                }
            }
            View::HostGrid => {
                // Calculate responsive column count
                let column_count = calculate_columns(self.window_size.width, self.sidebar_collapsed);

                // Show content based on sidebar selection
                match self.sidebar_selection {
                    SidebarMenuItem::Hosts => {
                        host_grid_view(&self.search_query, filtered_groups, filtered_cards, column_count)
                    }
                    SidebarMenuItem::Sftp => {
                        // Show SFTP host picker
                        let sftp_hosts: Vec<SftpHostCard> = self.hosts_config.hosts.iter()
                            .map(|h| SftpHostCard {
                                id: h.id,
                                name: h.name.clone(),
                            })
                            .collect();
                        sftp_picker_view(&self.search_query, sftp_hosts)
                    }
                    SidebarMenuItem::History => {
                        history_view(&self.history_config)
                    }
                    SidebarMenuItem::Snippets | SidebarMenuItem::Settings => {
                        // These open dialogs, show hosts grid as fallback
                        host_grid_view(&self.search_query, filtered_groups, filtered_cards, column_count)
                    }
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

        // Overlay dialog if open - host key dialog takes priority as it's connection-critical
        if let Some(ref host_key_state) = self.host_key_dialog {
            let dialog = host_key_dialog_view(host_key_state);
            stack![main_layout, dialog].into()
        } else if let Some(ref dialog_state) = self.dialog {
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
        Subscription::batch([
            // Keyboard events
            event::listen_with(|event, _status, _id| {
                if let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) = event {
                    Some(Message::KeyboardEvent(key, modifiers))
                } else {
                    None
                }
            }),
            // Window resize events
            window::resize_events().map(|(_id, size)| Message::WindowResized(size)),
        ])
    }
}
