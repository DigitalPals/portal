mod actions;
pub mod managers;
mod services;
mod update;
mod view_model;

use iced::widget::{column, row, stack, text};
use iced::{Element, Fill, Subscription, Task, Theme as IcedTheme, event, time, window};
use std::time::Duration;
use uuid::Uuid;

use iced::keyboard;

use crate::config::{
    HistoryConfig, HostsConfig, SettingsConfig, SnippetHistoryConfig, SnippetsConfig,
};
use crate::message::{Message, SessionId, SessionMessage, SidebarMenuItem, UiMessage, VncMessage};
use crate::theme::{ScaledFonts, ThemeId, get_theme};
use crate::views::dialogs::about_dialog::about_dialog_view;
use crate::views::dialogs::host_dialog::host_dialog_view;
use crate::views::dialogs::host_key_dialog::host_key_dialog_view;
use crate::views::dialogs::passphrase_dialog::passphrase_dialog_view;
use crate::views::dialogs::password_dialog::password_dialog_view;
use crate::views::dialogs::quick_connect_dialog::quick_connect_dialog_view;
use crate::views::file_viewer::file_viewer_view;
use crate::views::history_view::history_view;
use crate::views::host_grid::{calculate_columns, host_grid_view, search_input_id};
use crate::views::settings_page::{SettingsPageContext, settings_page_view};
use crate::views::sftp::{
    dual_pane_sftp_view, has_actions_menu_open, sftp_actions_menu_dismiss_overlay,
    sftp_context_menu_overlay,
};
use crate::views::sidebar::sidebar_view;
use crate::views::snippet_grid::{SnippetPageContext, snippet_page_view};
use crate::views::tabs::{Tab, tab_bar_view};
use crate::views::terminal_view::terminal_view_with_status;
use crate::views::toast::{ToastManager, toast_overlay_view};
use crate::views::vnc_view::vnc_viewer_view;

pub use self::managers::ActiveSession;
use self::managers::{
    ActiveDialog, DialogManager, FileViewerManager, SessionManager, SftpManager,
    SnippetExecutionManager, VncActiveSession,
};
use self::view_model::{filter_group_cards, filter_host_cards, group_cards, host_cards};

/// Threshold for auto-collapsing sidebar (in pixels)
pub const SIDEBAR_AUTO_COLLAPSE_THRESHOLD: f32 = 800.0;

/// The active view in the main content area
#[derive(Debug, Clone, Default)]
pub enum View {
    #[default]
    HostGrid,
    Terminal(SessionId),
    DualSftp(SessionId),   // Dual-pane SFTP browser
    FileViewer(SessionId), // In-app file viewer
    VncViewer(SessionId),  // VNC remote desktop viewer
    Settings,              // Full-page settings view
    Snippets,              // Snippets page with execution
}

/// Major UI sections that can receive keyboard focus
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusSection {
    #[default]
    Content, // Main content area (host grid, SFTP, history)
    Sidebar, // Sidebar menu
    TabBar,  // Tab navigation bar
}

/// Sidebar visibility state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarState {
    Hidden,    // Completely hidden (0 width)
    IconsOnly, // Collapsed to icons only
    #[default]
    Expanded, // Full width with labels
}

impl SidebarState {
    /// Cycle to the next state (for toggle button)
    pub fn next(self) -> Self {
        match self {
            SidebarState::Hidden => SidebarState::IconsOnly,
            SidebarState::IconsOnly => SidebarState::Expanded,
            SidebarState::Expanded => SidebarState::Hidden,
        }
    }
}

/// State for editing a snippet (name, command, description, selected hosts)
#[derive(Debug, Clone)]
pub struct SnippetEditState {
    /// ID of snippet being edited (None = creating new)
    pub snippet_id: Option<Uuid>,
    /// Snippet name
    pub name: String,
    /// Command to execute
    pub command: String,
    /// Optional description
    pub description: String,
    /// Selected host IDs for execution
    pub selected_hosts: std::collections::HashSet<Uuid>,
}

impl SnippetEditState {
    /// Create a new empty edit state (for creating a new snippet)
    pub fn new() -> Self {
        Self {
            snippet_id: None,
            name: String::new(),
            command: String::new(),
            description: String::new(),
            selected_hosts: std::collections::HashSet::new(),
        }
    }

    /// Create edit state from an existing snippet
    pub fn from_snippet(snippet: &crate::config::Snippet) -> Self {
        Self {
            snippet_id: Some(snippet.id),
            name: snippet.name.clone(),
            command: snippet.command.clone(),
            description: snippet.description.clone().unwrap_or_default(),
            selected_hosts: snippet.host_ids.iter().copied().collect(),
        }
    }

    /// Check if the form has valid required fields
    pub fn is_valid(&self) -> bool {
        !self.name.trim().is_empty() && !self.command.trim().is_empty()
    }
}

impl Default for SnippetEditState {
    fn default() -> Self {
        Self::new()
    }
}

/// Main application state
pub struct Portal {
    // UI state
    active_view: View,
    search_query: String,
    hovered_host: Option<Uuid>,
    hovered_tab: Option<Uuid>,

    // Sidebar state
    sidebar_state: SidebarState,
    sidebar_state_before_session: Option<SidebarState>, // Saved state before hiding for terminal
    sidebar_selection: SidebarMenuItem,

    // Tab management
    tabs: Vec<Tab>,
    active_tab: Option<Uuid>,

    // Domain managers
    sessions: SessionManager,
    sftp: SftpManager,
    file_viewers: FileViewerManager,
    dialogs: DialogManager,

    // VNC sessions (separate from terminal sessions)
    pub(crate) vnc_sessions: std::collections::HashMap<SessionId, VncActiveSession>,

    // Theme preference
    theme_id: ThemeId,

    // UI scale settings
    system_ui_scale: f32,           // Detected at startup, read-only
    ui_scale_override: Option<f32>, // User override from settings

    // Terminal settings
    terminal_font_size: f32,
    terminal_font: crate::fonts::TerminalFont,

    // SFTP settings
    sftp_column_widths: crate::views::sftp::ColumnWidths,

    // VNC settings
    vnc_settings: crate::config::settings::VncSettings,

    // Data from config
    hosts_config: HostsConfig,
    snippets_config: SnippetsConfig,
    history_config: HistoryConfig,
    snippet_history: SnippetHistoryConfig,

    // Toast notifications
    toast_manager: ToastManager,

    // Responsive layout
    window_size: iced::Size,
    sidebar_manually_set: bool, // True if user manually changed sidebar state

    // Keyboard navigation focus state
    focus_section: FocusSection,
    sidebar_focus_index: usize,
    tab_focus_index: usize,
    host_grid_focus_index: Option<usize>,
    history_focus_index: Option<usize>,
    terminal_captured: bool,

    // Snippets page state
    snippet_executions: SnippetExecutionManager,
    snippet_search_query: String,
    snippet_editing: Option<SnippetEditState>,
    hovered_snippet: Option<Uuid>,
    selected_snippet: Option<Uuid>,
    /// Currently viewed history entry (None = show current execution)
    viewed_history_entry: Option<Uuid>,
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
                tracing::info!(
                    "Loaded {} history entries from config",
                    config.entries.len()
                );
                config
            }
            Err(e) => {
                tracing::warn!("Failed to load history config: {}, using empty config", e);
                HistoryConfig::default()
            }
        };

        // Load snippet execution history from config file
        let snippet_history = match SnippetHistoryConfig::load() {
            Ok(config) => {
                tracing::info!(
                    "Loaded {} snippet execution history entries",
                    config.entries.len()
                );
                config
            }
            Err(e) => {
                tracing::warn!("Failed to load snippet history: {}, using empty", e);
                SnippetHistoryConfig::default()
            }
        };

        // Load settings from config file
        let settings_config = match SettingsConfig::load() {
            Ok(config) => {
                tracing::info!(
                    "Loaded settings: font_size={}, theme={:?}",
                    config.terminal_font_size,
                    config.theme
                );
                config
            }
            Err(e) => {
                tracing::warn!("Failed to load settings config: {}, using defaults", e);
                SettingsConfig::default()
            }
        };

        // Detect system UI scale at startup
        let system_ui_scale = crate::platform::detect_system_ui_scale();
        tracing::info!("System UI scale: {}", system_ui_scale);

        let app = Self {
            active_view: View::HostGrid,
            search_query: String::new(),
            hovered_host: None,
            hovered_tab: None,
            sidebar_state: SidebarState::Expanded,
            sidebar_state_before_session: None,
            sidebar_selection: SidebarMenuItem::Hosts,
            tabs: Vec::new(),
            active_tab: None,
            sessions: SessionManager::new(),
            sftp: SftpManager::new(),
            file_viewers: FileViewerManager::new(),
            dialogs: DialogManager::new(),
            vnc_sessions: std::collections::HashMap::new(),
            theme_id: settings_config.theme,
            system_ui_scale,
            ui_scale_override: settings_config.ui_scale,
            terminal_font_size: settings_config.terminal_font_size,
            terminal_font: settings_config.terminal_font,
            sftp_column_widths: settings_config.sftp_column_widths,
            vnc_settings: settings_config.vnc.apply_env_overrides(),
            hosts_config,
            snippets_config,
            history_config,
            snippet_history,
            toast_manager: ToastManager::new(),
            window_size: iced::Size::new(1200.0, 800.0),
            sidebar_manually_set: false,
            // Focus navigation state
            focus_section: FocusSection::Content,
            sidebar_focus_index: 0,
            tab_focus_index: 0,
            host_grid_focus_index: None,
            history_focus_index: None,
            terminal_captured: false,

            // Snippets page state
            snippet_executions: SnippetExecutionManager::new(),
            snippet_search_query: String::new(),
            snippet_editing: None,
            hovered_snippet: None,
            selected_snippet: None,
            viewed_history_entry: None,
        };

        // Focus the search input on startup
        let focus_task = iced::widget::operation::focus(search_input_id());
        (app, focus_task)
    }

    /// Handle messages - dispatches to specialized handlers
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Session(msg) => update::handle_session(self, msg),
            Message::Sftp(msg) => update::handle_sftp(self, msg),
            Message::FileViewer(msg) => update::handle_file_viewer(self, msg),
            Message::Dialog(msg) => update::handle_dialog(self, msg),
            Message::Tab(msg) => update::handle_tab(self, msg),
            Message::Host(msg) => update::handle_host(self, msg),
            Message::History(msg) => update::handle_history(self, msg),
            Message::Snippet(msg) => update::handle_snippet(self, msg),
            Message::Vnc(msg) => update::handle_vnc(self, msg),
            Message::Ui(msg) => update::handle_ui(self, msg),
            Message::Noop => Task::none(),
        }
    }

    /// Build the view
    pub fn view(&self) -> Element<'_, Message> {
        let all_cards = host_cards(&self.hosts_config);
        let all_groups = group_cards(&self.hosts_config);

        // Filter based on search
        let filtered_cards = filter_host_cards(&self.search_query, all_cards);
        let filtered_groups = filter_group_cards(&self.search_query, all_groups);

        let theme = get_theme(self.theme_id);
        let fonts = ScaledFonts::new(self.effective_ui_scale());

        // Sidebar (new collapsible icon menu)
        let sidebar = sidebar_view(
            theme,
            fonts,
            self.sidebar_state,
            self.sidebar_selection,
            self.focus_section,
            self.sidebar_focus_index,
        );

        // Main content - prioritize active sessions over sidebar selection
        let main_content: Element<'_, Message> = match &self.active_view {
            View::Settings => settings_page_view(
                SettingsPageContext {
                    current_theme: self.theme_id,
                    terminal_font_size: self.terminal_font_size,
                    terminal_font: self.terminal_font,
                    snippet_history_enabled: self.snippet_history.enabled,
                    snippet_store_command: self.snippet_history.store_command,
                    snippet_store_output: self.snippet_history.store_output,
                    snippet_redact_output: self.snippet_history.redact_output,
                    ui_scale: self.effective_ui_scale(),
                    system_ui_scale: self.system_ui_scale,
                    has_ui_scale_override: self.has_ui_scale_override(),
                },
                theme,
                fonts,
            ),
            View::Terminal(session_id) => {
                if let Some(session) = self.sessions.get(*session_id) {
                    let session_id = *session_id;

                    // Get transient status message if not expired (show for 3 seconds)
                    let status_message = session
                        .status_message
                        .as_ref()
                        .filter(|(_, shown_at)| shown_at.elapsed() < Duration::from_secs(3))
                        .map(|(msg, _)| msg.as_str());

                    terminal_view_with_status(
                        theme,
                        fonts,
                        &session.terminal,
                        session.session_start,
                        &session.host_name,
                        status_message,
                        self.terminal_font_size,
                        self.terminal_font,
                        move |_sid, bytes| {
                            Message::Session(SessionMessage::Input(session_id, bytes))
                        },
                        move |_sid, cols, rows| {
                            Message::Session(SessionMessage::Resize(session_id, cols, rows))
                        },
                    )
                } else {
                    text("Session not found").into()
                }
            }
            View::DualSftp(tab_id) => {
                if let Some(state) = self.sftp.get_tab(*tab_id) {
                    // Build available sources list for dropdown
                    let available_hosts: Vec<_> = self
                        .hosts_config
                        .hosts
                        .iter()
                        .map(|h| (h.id, h.name.clone()))
                        .collect();
                    dual_pane_sftp_view(state, available_hosts, theme, fonts)
                } else {
                    text("File browser not found").into()
                }
            }
            View::FileViewer(viewer_id) => {
                if let Some(state) = self.file_viewers.get(*viewer_id) {
                    file_viewer_view(state, theme)
                } else {
                    text("File viewer not found").into()
                }
            }
            View::VncViewer(session_id) => {
                if let Some(vnc) = self.vnc_sessions.get(session_id) {
                    vnc_viewer_view(*session_id, vnc, theme, fonts)
                } else {
                    text("VNC session not found").into()
                }
            }
            View::Snippets => {
                // Check if results panel will be visible
                let results_panel_visible = self
                    .selected_snippet
                    .map(|id| {
                        self.snippet_executions.get_active(id).is_some()
                            || self.snippet_executions.get_last_result(id).is_some()
                    })
                    .unwrap_or(false);

                // Calculate responsive column count for snippet grid
                let column_count = crate::views::snippet_grid::calculate_columns(
                    self.window_size.width,
                    self.sidebar_state,
                    results_panel_visible,
                );

                // Collect available hosts for the edit form
                let hosts: Vec<_> = self
                    .hosts_config
                    .hosts
                    .iter()
                    .map(|h| (h.id, h.name.clone(), h.detected_os.clone()))
                    .collect();

                snippet_page_view(SnippetPageContext {
                    snippets: &self.snippets_config.snippets,
                    search_query: &self.snippet_search_query,
                    editing: self.snippet_editing.as_ref(),
                    hosts: &hosts,
                    executions: &self.snippet_executions,
                    snippet_history: &self.snippet_history,
                    column_count,
                    theme,
                    fonts,
                    hovered_snippet: self.hovered_snippet,
                    selected_snippet: self.selected_snippet,
                    viewed_history_entry: self.viewed_history_entry,
                })
            }
            View::HostGrid => {
                // Calculate responsive column count
                let column_count = calculate_columns(self.window_size.width, self.sidebar_state);

                // Show content based on sidebar selection
                match self.sidebar_selection {
                    SidebarMenuItem::Hosts | SidebarMenuItem::Sftp => {
                        // SFTP now opens directly into dual-pane view, so show hosts grid as fallback
                        host_grid_view(
                            &self.search_query,
                            filtered_groups,
                            filtered_cards,
                            column_count,
                            theme,
                            fonts,
                            self.focus_section,
                            self.host_grid_focus_index,
                            self.hovered_host,
                        )
                    }
                    SidebarMenuItem::History => history_view(
                        &self.history_config,
                        &self.hosts_config,
                        theme,
                        fonts,
                        self.focus_section,
                        self.history_focus_index,
                    ),
                    SidebarMenuItem::Snippets
                    | SidebarMenuItem::Settings
                    | SidebarMenuItem::About => {
                        // These open dialogs or pages, show hosts grid as fallback
                        host_grid_view(
                            &self.search_query,
                            filtered_groups,
                            filtered_cards,
                            column_count,
                            theme,
                            fonts,
                            self.focus_section,
                            self.host_grid_focus_index,
                            self.hovered_host,
                        )
                    }
                }
            }
        };

        // Tab bar - always visible at full width (Termius-style)
        // Uses terminal background color when in terminal/sftp/file viewer for seamless look
        let header: Element<'_, Message> = tab_bar_view(
            &self.tabs,
            self.active_tab,
            self.sidebar_state,
            theme,
            fonts,
            self.focus_section,
            self.tab_focus_index,
            &self.active_view,
            &self.hosts_config,
            self.hovered_tab,
        );

        // Content row: sidebar | main content
        let content_row = row![sidebar, main_content].width(Fill).height(Fill);

        // Full layout: tab bar on top, then sidebar+content below (Termius-style)
        let main_layout: Element<'_, Message> =
            column![header, content_row].width(Fill).height(Fill).into();

        // Overlay dialog if open - host key dialog takes priority as it's connection-critical
        let with_dialog: Element<'_, Message> = match self.dialogs.active() {
            ActiveDialog::HostKey(host_key_state) => {
                let dialog = host_key_dialog_view(host_key_state, theme);
                stack![main_layout, dialog].into()
            }
            ActiveDialog::Host(dialog_state) => {
                let dialog = host_dialog_view(dialog_state, theme);
                stack![main_layout, dialog].into()
            }
            ActiveDialog::About(about_state) => {
                let dialog = about_dialog_view(about_state, theme, fonts);
                stack![main_layout, dialog].into()
            }
            ActiveDialog::PasswordPrompt(password_state) => {
                let dialog = password_dialog_view(password_state, theme);
                stack![main_layout, dialog].into()
            }
            ActiveDialog::PassphrasePrompt(passphrase_state) => {
                let dialog = passphrase_dialog_view(passphrase_state, theme);
                stack![main_layout, dialog].into()
            }
            ActiveDialog::QuickConnect(quick_connect_state) => {
                let dialog = quick_connect_dialog_view(quick_connect_state, theme);
                stack![main_layout, dialog].into()
            }
            ActiveDialog::None => main_layout,
        };

        // Overlay SFTP actions menu dismiss background if visible (rendered at app level for window-wide dismissal)
        let with_actions_dismiss: Element<'_, Message> = if let Some(tab_id) = self.active_tab {
            if let Some(sftp_state) = self.sftp.get_tab(tab_id) {
                if has_actions_menu_open(sftp_state) {
                    stack![with_dialog, sftp_actions_menu_dismiss_overlay(sftp_state)].into()
                } else {
                    with_dialog
                }
            } else {
                with_dialog
            }
        } else {
            with_dialog
        };

        // Overlay SFTP context menu if visible (rendered at app level for correct window positioning)
        let with_context_menu: Element<'_, Message> = if let Some(tab_id) = self.active_tab {
            if let Some(sftp_state) = self.sftp.get_tab(tab_id) {
                if sftp_state.context_menu.visible {
                    stack![
                        with_actions_dismiss,
                        sftp_context_menu_overlay(sftp_state, theme, fonts, self.window_size)
                    ]
                    .into()
                } else {
                    with_actions_dismiss
                }
            } else {
                with_actions_dismiss
            }
        } else {
            with_actions_dismiss
        };

        // Overlay toast notifications on top of everything
        let final_content = if self.toast_manager.has_toasts() {
            stack![
                with_context_menu,
                toast_overlay_view(&self.toast_manager, theme, fonts)
            ]
            .into()
        } else {
            with_context_menu
        };

        // Wrap everything in a container with our background color
        iced::widget::container(final_content)
            .width(Fill)
            .height(Fill)
            .style(move |_| iced::widget::container::Style {
                background: Some(theme.background.into()),
                ..Default::default()
            })
            .into()
    }

    /// Theme based on theme_id preference
    pub fn theme(&self) -> IcedTheme {
        let theme = get_theme(self.theme_id);
        if self.theme_id.is_dark() {
            let palette = iced::theme::Palette {
                background: theme.background,
                text: theme.text_primary,
                primary: theme.accent,
                success: iced::Color::from_rgb8(0x40, 0xa0, 0x2b),
                warning: iced::Color::from_rgb8(0xdf, 0x8e, 0x1d),
                danger: iced::Color::from_rgb8(0xd2, 0x0f, 0x39),
            };
            IcedTheme::custom_with_fn("Portal Dark".to_string(), palette, |p| {
                iced::theme::palette::Extended::generate(p)
            })
        } else {
            IcedTheme::Light
        }
    }

    /// Get the effective UI scale (user override or system default)
    pub fn effective_ui_scale(&self) -> f32 {
        self.ui_scale_override.unwrap_or(self.system_ui_scale)
    }

    /// Get the system-detected UI scale
    pub fn system_ui_scale(&self) -> f32 {
        self.system_ui_scale
    }

    /// Check if user has overridden the UI scale
    pub fn has_ui_scale_override(&self) -> bool {
        self.ui_scale_override.is_some()
    }

    /// Compute a best-effort target size for VNC remote resize.
    pub fn vnc_target_size(&self) -> Option<(u16, u16)> {
        let scale = self.effective_ui_scale();
        let width = (self.window_size.width * scale).round();
        let height = ((self.window_size.height - 32.0).max(1.0) * scale).round();

        let width = width.clamp(320.0, 8192.0) as u16;
        let height = height.clamp(240.0, 8192.0) as u16;

        Some((width, height))
    }

    /// Save settings to config file
    pub(crate) fn save_settings(&self) {
        let mut settings = SettingsConfig::default();
        settings.terminal_font_size = self.terminal_font_size;
        settings.terminal_font = self.terminal_font;
        settings.theme = self.theme_id;
        settings.ui_scale = self.ui_scale_override;
        settings.vnc = self.vnc_settings.clone();
        if let Err(e) = settings.save() {
            tracing::error!("Failed to save settings: {}", e);
        }
    }

    pub(crate) fn save_snippet_history(&self) {
        if let Err(e) = self.snippet_history.save() {
            tracing::error!("Failed to save snippet history: {}", e);
        }
    }

    /// Keyboard subscription for shortcuts
    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            // Keyboard events
            event::listen_with(|event, _status, _id| match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                    Some(Message::Ui(UiMessage::KeyboardEvent(key, modifiers)))
                }
                iced::Event::Keyboard(keyboard::Event::KeyReleased { key, .. }) => {
                    Some(Message::Ui(UiMessage::KeyReleased(key)))
                }
                _ => None,
            }),
            // Window resize events
            window::resize_events().map(|(_id, size)| Message::Ui(UiMessage::WindowResized(size))),
        ];

        // Toast tick timer (only when toasts are visible)
        if self.toast_manager.has_toasts() {
            subscriptions.push(
                time::every(Duration::from_millis(100)).map(|_| Message::Ui(UiMessage::ToastTick)),
            );
        }

        // VNC render tick (~30fps, only when viewing VNC)
        if matches!(self.active_view, View::VncViewer(_)) && !self.vnc_sessions.is_empty() {
            let fps = self.vnc_settings.refresh_fps.clamp(1, 60);
            let interval_ms = (1000u64 / fps as u64).max(1);
            subscriptions.push(
                time::every(Duration::from_millis(interval_ms))
                    .map(|_| Message::Vnc(VncMessage::RenderTick)),
            );
        }

        // Session duration tick (only when viewing a terminal)
        if matches!(self.active_view, View::Terminal(_)) && !self.sessions.is_empty() {
            subscriptions.push(
                time::every(Duration::from_secs(1))
                    .map(|_| Message::Session(SessionMessage::DurationTick)),
            );
        }

        if self.sessions.has_pending_output() {
            subscriptions.push(
                time::every(Duration::from_millis(16))
                    .map(|_| Message::Session(SessionMessage::ProcessOutputTick)),
            );
        }

        Subscription::batch(subscriptions)
    }
}
