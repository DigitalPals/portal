mod actions;
pub mod managers;
mod view_model;

use std::time::{Duration, Instant};
use iced::keyboard::{self, Key};
use iced::widget::{column, row, text, stack, Space};
use iced::{event, time, window, Element, Fill, Subscription, Task, Theme as IcedTheme};
use uuid::Uuid;

use crate::config::{AuthMethod, HistoryConfig, Host, HostsConfig, SettingsConfig, Snippet, SnippetsConfig};
use crate::message::{HostDialogField, Message, SessionId, SidebarMenuItem, SnippetField};
use crate::theme::{theme_for, SIDEBAR_AUTO_COLLAPSE_THRESHOLD};
use crate::ssh::host_key_verification::HostKeyVerificationResponse;
use crate::views::dialogs::host_dialog::{
    host_dialog_view, AuthMethodChoice, HostDialogState,
};
use crate::views::dialogs::host_key_dialog::{host_key_dialog_view, HostKeyDialogState};
use crate::views::dialogs::settings_dialog::{settings_dialog_view, SettingsDialogState};
use crate::views::dialogs::snippets_dialog::{snippets_dialog_view, SnippetsDialogState};
use crate::views::history_view::history_view;
use crate::views::host_grid::{calculate_columns, host_grid_view, search_input_id};
use iced::widget::text_input;
use crate::views::sftp::{
    dual_pane_sftp_view, sftp_context_menu_overlay, DualPaneSftpState, PaneId, PaneSource,
};
use crate::views::sidebar::sidebar_view;
use crate::views::tabs::{tab_bar_view, Tab};
use crate::views::terminal_view::{terminal_view_with_status, TerminalSession};
use crate::views::toast::{toast_overlay_view, Toast, ToastManager};

use self::managers::{ActiveDialog, DialogManager, SessionManager, SftpManager};
pub use self::managers::ActiveSession;
use self::view_model::{filter_group_cards, filter_host_cards, group_cards, host_cards};

/// The active view in the main content area
#[derive(Debug, Clone, Default)]
pub enum View {
    #[default]
    HostGrid,
    Terminal(SessionId),
    DualSftp(SessionId),  // Dual-pane SFTP browser
}

/// Main application state
pub struct Portal {
    // UI state
    active_view: View,
    search_query: String,

    // Sidebar state
    sidebar_collapsed: bool,
    sidebar_selection: SidebarMenuItem,

    // Tab management
    tabs: Vec<Tab>,
    active_tab: Option<Uuid>,

    // Domain managers
    sessions: SessionManager,
    sftp: SftpManager,
    dialogs: DialogManager,

    // Theme preference
    dark_mode: bool,

    // Terminal settings
    terminal_font_size: f32,

    // Data from config
    hosts_config: HostsConfig,
    snippets_config: SnippetsConfig,
    history_config: HistoryConfig,

    // Toast notifications
    toast_manager: ToastManager,

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

        // Load settings from config file
        let settings_config = match SettingsConfig::load() {
            Ok(config) => {
                tracing::info!("Loaded settings: font_size={}", config.terminal_font_size);
                config
            }
            Err(e) => {
                tracing::warn!("Failed to load settings config: {}, using defaults", e);
                SettingsConfig::default()
            }
        };

        let app = Self {
            active_view: View::HostGrid,
            search_query: String::new(),
            sidebar_collapsed: false,
            sidebar_selection: SidebarMenuItem::Hosts,
            tabs: Vec::new(),
            active_tab: None,
            sessions: SessionManager::new(),
            sftp: SftpManager::new(),
            dialogs: DialogManager::new(),
            dark_mode: settings_config.dark_mode,
            terminal_font_size: settings_config.terminal_font_size,
            hosts_config,
            snippets_config,
            history_config,
            toast_manager: ToastManager::new(),
            window_size: iced::Size::new(1200.0, 800.0),
            sidebar_manually_collapsed: false,
        };

        // Focus the search input on startup
        let focus_task = text_input::focus(search_input_id());
        (app, focus_task)
    }

    /// Handle messages
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
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
            Message::SidebarItemSelect(item) => {
                self.sidebar_selection = item;
                tracing::info!("Sidebar item selected: {:?}", item);
                // Handle view changes based on selection
                match item {
                    SidebarMenuItem::Hosts => {
                        // Switch to HostGrid view and focus the search input
                        self.active_view = View::HostGrid;
                        return text_input::focus(search_input_id());
                    }
                    SidebarMenuItem::History => {
                        // Switch to HostGrid view (history uses the same view container)
                        self.active_view = View::HostGrid;
                    }
                    SidebarMenuItem::Sftp => {
                        // Open dual-pane SFTP browser directly
                        // Check if we already have a dual sftp tab open
                        if let Some(tab_id) = self.sftp.first_tab_id() {
                            // Switch to existing dual sftp tab
                            self.set_active_tab(tab_id);
                        } else {
                            // Create a new dual-pane SFTP tab
                            return self.update(Message::DualSftpOpen);
                        }
                    }
                    SidebarMenuItem::Settings => {
                        self.dialogs.open_settings(SettingsDialogState {
                            dark_mode: self.dark_mode,
                            terminal_font_size: self.terminal_font_size,
                        });
                    }
                    SidebarMenuItem::Snippets => {
                        self.dialogs.open_snippets(SnippetsDialogState::new(
                            self.snippets_config.snippets.clone(),
                        ));
                    }
                }
            }
            Message::SidebarToggleCollapse => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
                // Track manual collapse state to prevent auto-collapse override
                self.sidebar_manually_collapsed = self.sidebar_collapsed;
                tracing::info!("Sidebar collapsed: {} (manual)", self.sidebar_collapsed);
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
                self.dialogs.open_host(HostDialogState::new_host());
            }
            Message::QuickConnect => {
                // Parse search query as [ssh] [user@]hostname[:port]
                let query = self.search_query.trim();
                if query.is_empty() {
                    self.toast_manager.push(Toast::warning("Enter a hostname to connect"));
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
                tracing::info!("Local terminal requested (not yet implemented)");
            }
            Message::DialogClose => {
                self.dialogs.close();
            }
            Message::DialogSubmit => {
                if let Some(dialog_state) = self.dialogs.host() {
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

                        let is_edit = dialog_state.editing_id.is_some();
                        if is_edit {
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
                        self.dialogs.close();
                    }
                }
            }
            Message::DialogFieldChanged(field, value) => {
                if let Some(dialog_state) = self.dialogs.host_mut() {
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
            // Toast notifications
            Message::ToastDismiss(id) => {
                self.toast_manager.dismiss(id);
            }
            Message::ToastTick => {
                self.toast_manager.cleanup_expired();
            }
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
                    self.dialogs.open_host_key(HostKeyDialogState::from_request(*request));
                    tracing::info!("Host key verification dialog opened");
                }
            }
            Message::HostKeyVerificationAccept => {
                if let Some(dialog) = self.dialogs.host_key_mut() {
                    dialog.respond(HostKeyVerificationResponse::Accept);
                    tracing::info!("Host key accepted for {}:{}", dialog.host, dialog.port);
                }
                self.dialogs.close();
            }
            Message::HostKeyVerificationReject => {
                if let Some(dialog) = self.dialogs.host_key_mut() {
                    dialog.respond(HostKeyVerificationResponse::Reject);
                    tracing::info!("Host key rejected for {}:{}", dialog.host, dialog.port);
                    self.toast_manager.push(Toast::error(format!(
                        "Connection rejected: host key verification failed for {}",
                        dialog.host
                    )));
                }
                self.dialogs.close();
            }
            // Terminal messages
            Message::TerminalInput(session_id, bytes) => {
                tracing::debug!("Terminal input for session {}: {:?}", session_id, bytes);
                // Send to SSH channel
                if let Some(session) = self.sessions.get(session_id) {
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
                if let Some(session) = self.sessions.get_mut(session_id) {
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
                host_id,
                detected_os,
            } => {
                tracing::info!("SSH connected to {}", host_name);

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

                // Create history entry for this connection
                let history_entry_id = if let Some(host) = self.hosts_config.find_host(host_id) {
                    let entry = crate::config::HistoryEntry::new(
                        host.id,
                        host.name.clone(),
                        host.hostname.clone(),
                        host.username.clone(),
                        crate::config::SessionType::Ssh,
                    );
                    let entry_id = entry.id;
                    self.history_config.add_entry(entry);
                    if let Err(e) = self.history_config.save() {
                        tracing::error!("Failed to save history config: {}", e);
                    }
                    entry_id
                } else {
                    Uuid::new_v4()
                };

                // Create terminal session for this connection
                let terminal = TerminalSession::new(&host_name);

                // Store the active session
                self.sessions.insert(
                    session_id,
                    ActiveSession {
                        ssh_session,
                        terminal,
                        session_start: Instant::now(),
                        host_name: host_name.clone(),
                        history_entry_id,
                        status_message: None,
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
                if let Some(session) = self.sessions.get_mut(session_id) {
                    session.terminal.process_output(&data);
                }
            }
            Message::SshDisconnected(session_id) => {
                tracing::info!("SSH disconnected: {}", session_id);
                if let Some(session) = self.sessions.get(session_id) {
                    self.history_config.mark_disconnected(session.history_entry_id);
                    if let Err(e) = self.history_config.save() {
                        tracing::error!("Failed to save history config: {}", e);
                    }
                }
                self.close_tab(session_id);
            }
            Message::SshError(error) => {
                tracing::error!("SSH error: {}", error);
                self.toast_manager.push(Toast::error(error));
            }
            Message::SessionDurationTick => {
                // No-op: triggers a re-render to update duration display
            }
            Message::InstallSshKey(session_id) => {
                if let Some(session) = self.sessions.get_mut(session_id) {
                    // Set status message to show we're installing
                    session.status_message = Some(("Installing key...".to_string(), Instant::now()));

                    let ssh_session = session.ssh_session.clone();
                    return Task::perform(
                        async move {
                            crate::ssh::install_ssh_key(&ssh_session).await
                        },
                        move |result| Message::InstallSshKeyResult(session_id, result.map_err(|e| e.to_string())),
                    );
                }
            }
            Message::InstallSshKeyResult(session_id, result) => {
                if let Some(session) = self.sessions.get_mut(session_id) {
                    // Clear the "Installing..." status message
                    session.status_message = None;
                }
                match result {
                    Ok(true) => {
                        self.toast_manager.push(Toast::success("SSH key installed on remote server"));
                    }
                    Ok(false) => {
                        self.toast_manager.push(Toast::success("SSH key already installed"));
                    }
                    Err(e) => {
                        self.toast_manager.push(Toast::error(format!("Failed to install key: {}", e)));
                    }
                }
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

            // Dual-pane SFTP browser messages
            Message::DualSftpOpen => {
                // Create a new dual-pane SFTP tab with both panes starting as Local
                let tab_id = Uuid::new_v4();
                let dual_state = DualPaneSftpState::new(tab_id);
                self.sftp.insert_tab(tab_id, dual_state);

                // Create tab and switch view
                let tab = Tab::new_sftp(tab_id, "File Browser".to_string());
                self.tabs.push(tab);
                self.active_tab = Some(tab_id);
                self.active_view = View::DualSftp(tab_id);

                // Load both panes (both start as Local)
                let left_task = self.load_dual_pane_directory(tab_id, PaneId::Left);
                let right_task = self.load_dual_pane_directory(tab_id, PaneId::Right);
                return Task::batch([left_task, right_task]);
            }
            Message::DualSftpPaneSourceChanged(tab_id, pane_id, new_source) => {
                // First, get the new path based on source (to avoid borrow conflicts)
                let new_path = match &new_source {
                    PaneSource::Local => {
                        directories::BaseDirs::new()
                            .map(|d| d.home_dir().to_path_buf())
                            .unwrap_or_else(|| std::path::PathBuf::from("/"))
                    }
                    PaneSource::Remote { session_id, .. } => {
                        // Get home dir from existing connection if available
                        if let Some(sftp) = self.sftp.get_connection(*session_id) {
                            sftp.home_dir().to_path_buf()
                        } else {
                            // Connection not available - shouldn't happen with proper UI
                            tracing::warn!("SFTP connection {} not found", session_id);
                            return Task::none();
                        }
                    }
                };

                // Now we can mutably borrow the tab
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    let pane = tab_state.pane_mut(pane_id);
                    pane.source = new_source;
                    pane.current_path = new_path;
                    pane.loading = true;
                    pane.entries.clear();

                    return self.load_dual_pane_directory(tab_id, pane_id);
                }
            }
            Message::DualSftpPaneNavigate(tab_id, pane_id, path) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.active_pane = pane_id;
                    let pane = tab_state.pane_mut(pane_id);
                    pane.current_path = path;
                    pane.loading = true;
                    return self.load_dual_pane_directory(tab_id, pane_id);
                }
            }
            Message::DualSftpPaneNavigateUp(tab_id, pane_id) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.active_pane = pane_id;
                    let pane = tab_state.pane_mut(pane_id);
                    if let Some(parent) = pane.current_path.parent() {
                        pane.current_path = parent.to_path_buf();
                        pane.loading = true;
                        return self.load_dual_pane_directory(tab_id, pane_id);
                    }
                }
            }
            Message::DualSftpPaneRefresh(tab_id, pane_id) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.active_pane = pane_id;
                    tab_state.pane_mut(pane_id).loading = true;
                    return self.load_dual_pane_directory(tab_id, pane_id);
                }
            }
            Message::DualSftpPaneSelect(tab_id, pane_id, index) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.active_pane = pane_id;
                    tab_state.pane_mut(pane_id).select(index);
                }
            }
            Message::DualSftpShowContextMenu(tab_id, pane_id, x, y, index) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.active_pane = pane_id;
                    // Select the item if index provided and not already selected
                    if let Some(idx) = index {
                        if !tab_state.pane(pane_id).is_selected(idx) {
                            tab_state.pane_mut(pane_id).select(idx);
                        }
                    }
                    tab_state.show_context_menu(pane_id, x, y);
                }
            }
            Message::DualSftpHideContextMenu(tab_id) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.hide_context_menu();
                }
            }
            Message::DualSftpContextMenuAction(tab_id, action) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.hide_context_menu();
                    // Handle context menu actions - implemented below
                    return self.handle_sftp_context_action(tab_id, action);
                }
            }
            Message::DualSftpDialogInputChanged(tab_id, value) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    if let Some(ref mut dialog) = tab_state.dialog {
                        dialog.input_value = value;
                        dialog.error = None; // Clear error when user types
                    }
                }
            }
            Message::DualSftpDialogCancel(tab_id) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    tab_state.close_dialog();
                }
            }
            Message::DualSftpDialogSubmit(tab_id) => {
                if let Some(tab_state) = self.sftp.get_tab(tab_id) {
                    if let Some(ref dialog) = tab_state.dialog {
                        if dialog.is_valid() {
                            return self.handle_sftp_dialog_submit(tab_id);
                        }
                    }
                }
            }
            Message::DualSftpNewFolderResult(tab_id, pane_id, result) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    match result {
                        Ok(()) => {
                            self.toast_manager.push(Toast::success("Folder created"));
                            tab_state.close_dialog();
                            tab_state.pane_mut(pane_id).loading = true;
                            return self.load_dual_pane_directory(tab_id, pane_id);
                        }
                        Err(error) => {
                            if let Some(ref mut dialog) = tab_state.dialog {
                                dialog.error = Some(error);
                            }
                        }
                    }
                }
            }
            Message::DualSftpRenameResult(tab_id, pane_id, result) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    match result {
                        Ok(()) => {
                            self.toast_manager.push(Toast::success("Renamed successfully"));
                            tab_state.close_dialog();
                            tab_state.pane_mut(pane_id).loading = true;
                            return self.load_dual_pane_directory(tab_id, pane_id);
                        }
                        Err(error) => {
                            if let Some(ref mut dialog) = tab_state.dialog {
                                dialog.error = Some(error);
                            }
                        }
                    }
                }
            }
            Message::DualSftpDeleteResult(tab_id, pane_id, result) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    match result {
                        Ok(count) => {
                            tracing::info!("Deleted {} item(s)", count);
                            let msg = if count == 1 {
                                "Deleted 1 item".to_string()
                            } else {
                                format!("Deleted {} items", count)
                            };
                            self.toast_manager.push(Toast::success(msg));
                            tab_state.close_dialog();
                            tab_state.pane_mut(pane_id).loading = true;
                            return self.load_dual_pane_directory(tab_id, pane_id);
                        }
                        Err(error) => {
                            if let Some(ref mut dialog) = tab_state.dialog {
                                dialog.error = Some(error);
                            }
                        }
                    }
                }
            }
            Message::DualSftpCopyToTarget(tab_id) => {
                return self.handle_copy_to_target(tab_id);
            }
            Message::DualSftpCopyResult(tab_id, target_pane_id, result) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    match result {
                        Ok(count) => {
                            tracing::info!("Copied {} item(s)", count);
                            let msg = if count == 1 {
                                "Copied 1 item".to_string()
                            } else {
                                format!("Copied {} items", count)
                            };
                            self.toast_manager.push(Toast::success(msg));
                            // Refresh the target pane to show newly copied files
                            tab_state.pane_mut(target_pane_id).loading = true;
                            return self.load_dual_pane_directory(tab_id, target_pane_id);
                        }
                        Err(error) => {
                            tracing::error!("Copy failed: {}", error);
                            self.toast_manager.push(Toast::error(format!("Copy failed: {}", error)));
                        }
                    }
                }
            }
            Message::DualSftpPermissionToggle(tab_id, bit, value) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    if let Some(ref mut dialog) = tab_state.dialog {
                        dialog.set_permission(bit, value);
                    }
                }
            }
            Message::DualSftpPermissionsResult(tab_id, pane_id, result) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    match result {
                        Ok(()) => {
                            tracing::info!("Permissions updated successfully");
                            self.toast_manager.push(Toast::success("Permissions updated"));
                            tab_state.close_dialog();
                            tab_state.pane_mut(pane_id).loading = true;
                            return self.load_dual_pane_directory(tab_id, pane_id);
                        }
                        Err(error) => {
                            if let Some(ref mut dialog) = tab_state.dialog {
                                dialog.error = Some(error);
                            }
                        }
                    }
                }
            }
            Message::DualSftpOpenWithResult(result) => {
                if let Err(error) = result {
                    self.toast_manager.push(Toast::error(error));
                }
            }
            Message::DualSftpPaneListResult(tab_id, pane_id, result) => {
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    let pane = tab_state.pane_mut(pane_id);
                    match result {
                        Ok(entries) => pane.set_entries(entries),
                        Err(e) => pane.set_error(e),
                    }
                }
            }
            Message::DualSftpConnectHost(tab_id, pane_id, host_id) => {
                // Connect to a remote host for a specific pane
                tracing::info!("Connecting to host {} for pane {:?}", host_id, pane_id);
                if let Some(host) = self.hosts_config.find_host(host_id).cloned() {
                    return self.connect_sftp_for_pane(tab_id, pane_id, &host);
                }
            }
            Message::DualSftpConnected {
                tab_id,
                pane_id,
                sftp_session_id,
                host_id,
                host_name,
                sftp_session,
            } => {
                tracing::info!("SFTP connected to {} for pane {:?}", host_name, pane_id);
                self.sftp.clear_pending_connection();

                if let Some(host) = self.hosts_config.find_host(host_id) {
                    let entry = crate::config::HistoryEntry::new(
                        host.id,
                        host.name.clone(),
                        host.hostname.clone(),
                        host.username.clone(),
                        crate::config::SessionType::Sftp,
                    );
                    let entry_id = entry.id;
                    self.history_config.add_entry(entry);
                    self.sftp.insert_history_entry(sftp_session_id, entry_id);
                    if let Err(e) = self.history_config.save() {
                        tracing::error!("Failed to save history config: {}", e);
                    }
                }

                // Store the SFTP connection in the pool
                let home_dir = sftp_session.home_dir().to_path_buf();
                self.sftp.insert_connection(sftp_session_id, sftp_session);

                // Update the pane source to point to this connection
                if let Some(tab_state) = self.sftp.get_tab_mut(tab_id) {
                    let pane = tab_state.pane_mut(pane_id);
                    pane.source = PaneSource::Remote {
                        session_id: sftp_session_id,
                        host_name,
                    };
                    pane.current_path = home_dir;
                    pane.loading = true;
                    pane.entries.clear();

                    // Load directory for the newly connected pane
                    return self.load_dual_pane_directory(tab_id, pane_id);
                }
            }

            Message::KeyboardEvent(key, modifiers) => {
                // Handle global keyboard shortcuts
                match (key, modifiers.control(), modifiers.shift()) {
                    // Escape - close dialogs and context menus
                    (Key::Named(keyboard::key::Named::Escape), _, _) => {
                        // Host key dialog - Escape means reject
                        if let Some(dialog) = self.dialogs.host_key_mut() {
                            dialog.respond(HostKeyVerificationResponse::Reject);
                            self.toast_manager.push(Toast::warning("Connection cancelled"));
                            self.dialogs.close();
                        } else if self.dialogs.is_open() {
                            self.dialogs.close();
                        }
                        // Close any open SFTP context menu or dialog
                        for tab_state in self.sftp.tab_values_mut() {
                            tab_state.hide_context_menu();
                            tab_state.close_dialog();
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
                    // Ctrl+Shift+K - Install SSH key on remote server
                    (Key::Character(c), true, true) if c.as_str() == "k" || c.as_str() == "K" => {
                        if let View::Terminal(session_id) = self.active_view {
                            if self.sessions.contains(session_id) {
                                return self.update(Message::InstallSshKey(session_id));
                            }
                        }
                    }
                    _ => {}
                }
            }
            Message::SettingsThemeToggle(enabled) => {
                self.dark_mode = enabled;
                if let Some(dialog) = self.dialogs.settings_mut() {
                    dialog.dark_mode = enabled;
                }
                self.save_settings();
            }
            Message::SettingsFontSizeChange(size) => {
                self.terminal_font_size = size;
                if let Some(dialog) = self.dialogs.settings_mut() {
                    dialog.terminal_font_size = size;
                }
                self.save_settings();
            }
            Message::SnippetSelect(id) => {
                if let Some(dialog) = self.dialogs.snippets_mut() {
                    dialog.selected_id = Some(id);
                }
            }
            Message::SnippetNew => {
                if let Some(dialog) = self.dialogs.snippets_mut() {
                    dialog.start_new();
                }
            }
            Message::SnippetEdit(id) => {
                if let Some(dialog) = self.dialogs.snippets_mut() {
                    if let Some(snippet) = dialog.snippets.iter().find(|s| s.id == id).cloned() {
                        dialog.start_edit(&snippet);
                    }
                }
            }
            Message::SnippetDelete(id) => {
                // Remove from dialog and config
                if let Some(dialog) = self.dialogs.snippets_mut() {
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
                        if let Some(session) = self.sessions.get(session_id) {
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
                self.dialogs.close();
            }
            Message::SnippetFieldChanged(field, value) => {
                if let Some(dialog) = self.dialogs.snippets_mut() {
                    match field {
                        SnippetField::Name => dialog.edit_name = value,
                        SnippetField::Command => dialog.edit_command = value,
                        SnippetField::Description => dialog.edit_description = value,
                    }
                }
            }
            Message::SnippetEditCancel => {
                if let Some(dialog) = self.dialogs.snippets_mut() {
                    dialog.cancel_edit();
                }
            }
            Message::SnippetSave => {
                if let Some(dialog) = self.dialogs.snippets_mut() {
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

        let theme = theme_for(self.dark_mode);

        // Sidebar (new collapsible icon menu)
        let sidebar = sidebar_view(
            theme,
            self.sidebar_collapsed,
            self.sidebar_selection,
        );

        // Main content - prioritize active sessions over sidebar selection
        let main_content: Element<'_, Message> = match &self.active_view {
            View::Terminal(session_id) => {
                if let Some(session) = self.sessions.get(*session_id) {
                    let session_id = *session_id;

                    // Get transient status message if not expired (show for 3 seconds)
                    let status_message = session.status_message.as_ref()
                        .filter(|(_, shown_at)| shown_at.elapsed() < Duration::from_secs(3))
                        .map(|(msg, _)| msg.as_str());

                    terminal_view_with_status(
                        theme,
                        &session.terminal,
                        session.session_start,
                        &session.host_name,
                        status_message,
                        self.terminal_font_size,
                        move |_sid, bytes| Message::TerminalInput(session_id, bytes),
                        move |_sid, cols, rows| Message::TerminalResize(session_id, cols, rows),
                    )
                } else {
                    text("Session not found").into()
                }
            }
            View::DualSftp(tab_id) => {
                if let Some(state) = self.sftp.get_tab(*tab_id) {
                    // Build available sources list for dropdown
                    let available_hosts: Vec<_> = self.hosts_config.hosts.iter()
                        .map(|h| (h.id, h.name.clone()))
                        .collect();
                    dual_pane_sftp_view(state, available_hosts, theme)
                } else {
                    text("File browser not found").into()
                }
            }
            View::HostGrid => {
                // Calculate responsive column count
                let column_count = calculate_columns(self.window_size.width, self.sidebar_collapsed);

                // Show content based on sidebar selection
                match self.sidebar_selection {
                    SidebarMenuItem::Hosts | SidebarMenuItem::Sftp => {
                        // SFTP now opens directly into dual-pane view, so show hosts grid as fallback
                        host_grid_view(&self.search_query, filtered_groups, filtered_cards, column_count, theme)
                    }
                    SidebarMenuItem::History => {
                        history_view(&self.history_config, theme)
                    }
                    SidebarMenuItem::Snippets | SidebarMenuItem::Settings => {
                        // These open dialogs, show hosts grid as fallback
                        host_grid_view(&self.search_query, filtered_groups, filtered_cards, column_count, theme)
                    }
                }
            }
        };

        // Tab bar - only show when there are tabs
        let header: Element<'_, Message> = if !self.tabs.is_empty() {
            tab_bar_view(&self.tabs, self.active_tab, theme)
        } else {
            Space::with_height(0).into()
        };

        // Main layout with content below header
        let content_area = column![header, main_content];

        // Full layout: sidebar | content
        let main_layout: Element<'_, Message> = row![sidebar, content_area]
            .width(Fill)
            .height(Fill)
            .into();

        // Overlay dialog if open - host key dialog takes priority as it's connection-critical
        let with_dialog: Element<'_, Message> = match self.dialogs.active() {
            ActiveDialog::HostKey(host_key_state) => {
                let dialog = host_key_dialog_view(host_key_state, theme);
                stack![main_layout, dialog].into()
            }
            ActiveDialog::Host(dialog_state) => {
                let dialog = host_dialog_view(dialog_state, &self.hosts_config.groups, theme);
                stack![main_layout, dialog].into()
            }
            ActiveDialog::Settings(settings_state) => {
                let dialog = settings_dialog_view(settings_state, theme);
                stack![main_layout, dialog].into()
            }
            ActiveDialog::Snippets(snippets_state) => {
                let dialog = snippets_dialog_view(snippets_state, theme);
                stack![main_layout, dialog].into()
            }
            ActiveDialog::None => main_layout,
        };

        // Overlay SFTP context menu if visible (rendered at app level for correct window positioning)
        let with_context_menu: Element<'_, Message> = if let Some(tab_id) = self.active_tab {
            if let Some(sftp_state) = self.sftp.get_tab(tab_id) {
                if sftp_state.context_menu.visible {
                    stack![with_dialog, sftp_context_menu_overlay(sftp_state, theme)].into()
                } else {
                    with_dialog
                }
            } else {
                with_dialog
            }
        } else {
            with_dialog
        };

        // Overlay toast notifications on top of everything
        if self.toast_manager.has_toasts() {
            stack![with_context_menu, toast_overlay_view(&self.toast_manager, theme)].into()
        } else {
            with_context_menu
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

    /// Save settings to config file
    fn save_settings(&self) {
        let settings = SettingsConfig {
            terminal_font_size: self.terminal_font_size,
            dark_mode: self.dark_mode,
        };
        if let Err(e) = settings.save() {
            tracing::error!("Failed to save settings: {}", e);
        }
    }

    /// Keyboard subscription for shortcuts
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
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
        ];

        // Toast tick timer (only when toasts are visible)
        if self.toast_manager.has_toasts() {
            subscriptions.push(
                time::every(Duration::from_millis(100)).map(|_| Message::ToastTick)
            );
        }

        // Session duration tick (only when viewing a terminal)
        if matches!(self.active_view, View::Terminal(_)) && !self.sessions.is_empty() {
            subscriptions.push(
                time::every(Duration::from_secs(1)).map(|_| Message::SessionDurationTick)
            );
        }

        Subscription::batch(subscriptions)
    }
}
