mod actions;
pub mod managers;
mod services;
mod update;
mod view_model;

use std::time::Duration;
use iced::widget::{column, row, text, stack};
use iced::{event, time, window, Element, Fill, Subscription, Task, Theme as IcedTheme};
use uuid::Uuid;

use iced::keyboard;

use crate::config::{HistoryConfig, HostsConfig, SettingsConfig, SnippetsConfig};
use crate::message::{Message, SessionId, SessionMessage, SidebarMenuItem, UiMessage};
use crate::theme::{get_theme, ThemeId};
use crate::views::dialogs::host_dialog::host_dialog_view;
use crate::views::dialogs::host_key_dialog::host_key_dialog_view;
use crate::views::dialogs::snippets_dialog::snippets_dialog_view;
use crate::views::settings_page::settings_page_view;
use crate::views::history_view::history_view;
use crate::views::host_grid::{calculate_columns, host_grid_view, search_input_id};
use crate::views::file_viewer::file_viewer_view;
use crate::views::sftp::{dual_pane_sftp_view, sftp_context_menu_overlay};
use crate::views::sidebar::sidebar_view;
use crate::views::tabs::{tab_bar_view, Tab};
use crate::views::terminal_view::terminal_view_with_status;
use crate::views::toast::{toast_overlay_view, ToastManager};

use self::managers::{ActiveDialog, DialogManager, FileViewerManager, SessionManager, SftpManager};
#[allow(unused_imports)]
pub use self::managers::ActiveSession;
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
    Settings,              // Full-page settings view
}

/// Major UI sections that can receive keyboard focus
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusSection {
    #[default]
    Content,   // Main content area (host grid, SFTP, history)
    Sidebar,   // Sidebar menu
    TabBar,    // Tab navigation bar
}

/// Sidebar visibility state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarState {
    Hidden,     // Completely hidden (0 width)
    IconsOnly,  // Collapsed to icons only
    #[default]
    Expanded,   // Full width with labels
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

/// Main application state
pub struct Portal {
    // UI state
    active_view: View,
    search_query: String,

    // Sidebar state
    sidebar_state: SidebarState,
    sidebar_state_before_session: Option<SidebarState>,  // Saved state before hiding for terminal
    sidebar_selection: SidebarMenuItem,

    // Tab management
    tabs: Vec<Tab>,
    active_tab: Option<Uuid>,

    // Domain managers
    sessions: SessionManager,
    sftp: SftpManager,
    file_viewers: FileViewerManager,
    dialogs: DialogManager,

    // Theme preference
    theme_id: ThemeId,

    // Terminal settings
    terminal_font_size: f32,
    terminal_font: crate::fonts::TerminalFont,

    // Data from config
    hosts_config: HostsConfig,
    snippets_config: SnippetsConfig,
    history_config: HistoryConfig,

    // Toast notifications
    toast_manager: ToastManager,

    // Responsive layout
    window_size: iced::Size,
    sidebar_manually_set: bool,  // True if user manually changed sidebar state

    // Keyboard navigation focus state
    focus_section: FocusSection,
    sidebar_focus_index: usize,
    tab_focus_index: usize,
    host_grid_focus_index: Option<usize>,
    history_focus_index: Option<usize>,
    terminal_captured: bool,
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
                tracing::info!("Loaded settings: font_size={}, theme={:?}", config.terminal_font_size, config.theme);
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
            sidebar_state: SidebarState::Expanded,
            sidebar_state_before_session: None,
            sidebar_selection: SidebarMenuItem::Hosts,
            tabs: Vec::new(),
            active_tab: None,
            sessions: SessionManager::new(),
            sftp: SftpManager::new(),
            file_viewers: FileViewerManager::new(),
            dialogs: DialogManager::new(),
            theme_id: settings_config.theme,
            terminal_font_size: settings_config.terminal_font_size,
            terminal_font: settings_config.terminal_font,
            hosts_config,
            snippets_config,
            history_config,
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

        // Sidebar (new collapsible icon menu)
        let sidebar = sidebar_view(
            theme,
            self.sidebar_state,
            self.sidebar_selection,
            self.focus_section,
            self.sidebar_focus_index,
        );

        // Main content - prioritize active sessions over sidebar selection
        let main_content: Element<'_, Message> = match &self.active_view {
            View::Settings => {
                settings_page_view(self.theme_id, self.terminal_font_size, self.terminal_font, theme)
            }
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
                        self.terminal_font,
                        move |_sid, bytes| Message::Session(SessionMessage::Input(session_id, bytes)),
                        move |_sid, cols, rows| Message::Session(SessionMessage::Resize(session_id, cols, rows)),
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
            View::FileViewer(viewer_id) => {
                if let Some(state) = self.file_viewers.get(*viewer_id) {
                    file_viewer_view(state, theme)
                } else {
                    text("File viewer not found").into()
                }
            }
            View::HostGrid => {
                // Calculate responsive column count
                let column_count = calculate_columns(self.window_size.width, self.sidebar_state);

                // Show content based on sidebar selection
                match self.sidebar_selection {
                    SidebarMenuItem::Hosts | SidebarMenuItem::Sftp => {
                        // SFTP now opens directly into dual-pane view, so show hosts grid as fallback
                        host_grid_view(&self.search_query, filtered_groups, filtered_cards, column_count, theme, self.focus_section, self.host_grid_focus_index)
                    }
                    SidebarMenuItem::History => {
                        history_view(&self.history_config, theme, self.focus_section, self.history_focus_index)
                    }
                    SidebarMenuItem::Snippets | SidebarMenuItem::Settings => {
                        // These open dialogs, show hosts grid as fallback
                        host_grid_view(&self.search_query, filtered_groups, filtered_cards, column_count, theme, self.focus_section, self.host_grid_focus_index)
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
            self.focus_section,
            self.tab_focus_index,
            &self.active_view,
        );

        // Content row: sidebar | main content
        let content_row = row![sidebar, main_content]
            .width(Fill)
            .height(Fill);

        // Full layout: tab bar on top, then sidebar+content below (Termius-style)
        let main_layout: Element<'_, Message> = column![header, content_row]
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

    /// Theme based on theme_id preference
    pub fn theme(&self) -> IcedTheme {
        if self.theme_id.is_dark() {
            IcedTheme::Dark
        } else {
            IcedTheme::Light
        }
    }

    /// Save settings to config file
    pub(crate) fn save_settings(&self) {
        let mut settings = SettingsConfig::default();
        settings.terminal_font_size = self.terminal_font_size;
        settings.terminal_font = self.terminal_font;
        settings.theme = self.theme_id;
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
                    Some(Message::Ui(UiMessage::KeyboardEvent(key, modifiers)))
                } else {
                    None
                }
            }),
            // Window resize events
            window::resize_events().map(|(_id, size)| Message::Ui(UiMessage::WindowResized(size))),
        ];

        // Toast tick timer (only when toasts are visible)
        if self.toast_manager.has_toasts() {
            subscriptions.push(
                time::every(Duration::from_millis(100)).map(|_| Message::Ui(UiMessage::ToastTick))
            );
        }

        // Session duration tick (only when viewing a terminal)
        if matches!(self.active_view, View::Terminal(_)) && !self.sessions.is_empty() {
            subscriptions.push(
                time::every(Duration::from_secs(1)).map(|_| Message::Session(SessionMessage::DurationTick))
            );
        }

        Subscription::batch(subscriptions)
    }
}
