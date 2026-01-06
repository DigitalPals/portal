//! UI state message handlers

use iced::keyboard::{self, Key};
use iced::Task;

use crate::app::{FocusSection, Portal, View, SIDEBAR_AUTO_COLLAPSE_THRESHOLD};
use crate::message::{Message, SessionMessage, SftpMessage, SidebarMenuItem, TabMessage, HistoryMessage, HostMessage, UiMessage};
use crate::ssh::host_key_verification::HostKeyVerificationResponse;
use crate::views::toast::Toast;
use crate::views::dialogs::snippets_dialog::SnippetsDialogState;
use crate::views::sftp::PaneId;

/// Handle UI state messages
pub fn handle_ui(portal: &mut Portal, msg: UiMessage) -> Task<Message> {
    match msg {
        UiMessage::SearchChanged(query) => {
            portal.search_query = query;
            Task::none()
        }
        UiMessage::FolderToggle(id) => {
            if let Some(group) = portal.hosts_config.find_group_mut(id) {
                group.collapsed = !group.collapsed;
                if let Err(e) = portal.hosts_config.save() {
                    tracing::error!("Failed to save config: {}", e);
                }
            }
            Task::none()
        }
        UiMessage::SidebarItemSelect(item) => {
            portal.sidebar_selection = item;
            tracing::info!("Sidebar item selected: {:?}", item);
            match item {
                SidebarMenuItem::Hosts => {
                    portal.active_view = View::HostGrid;
                    return iced::widget::text_input::focus(crate::app::search_input_id());
                }
                SidebarMenuItem::History => {
                    portal.active_view = View::HostGrid;
                }
                SidebarMenuItem::Sftp => {
                    if let Some(tab_id) = portal.sftp.first_tab_id() {
                        portal.set_active_tab(tab_id);
                    } else {
                        return portal.update(Message::Sftp(SftpMessage::Open));
                    }
                }
                SidebarMenuItem::Settings => {
                    // Open settings page view instead of dialog
                    portal.active_view = View::Settings;
                }
                SidebarMenuItem::Snippets => {
                    portal.dialogs.open_snippets(SnippetsDialogState::new(
                        portal.snippets_config.snippets.clone(),
                    ));
                }
            }
            Task::none()
        }
        UiMessage::SidebarToggleCollapse => {
            portal.sidebar_collapsed = !portal.sidebar_collapsed;
            portal.sidebar_manually_collapsed = portal.sidebar_collapsed;
            tracing::info!("Sidebar collapsed: {} (manual)", portal.sidebar_collapsed);
            Task::none()
        }
        UiMessage::ThemeChange(theme_id) => {
            portal.theme_id = theme_id;
            portal.save_settings();
            Task::none()
        }
        UiMessage::FontSizeChange(size) => {
            portal.terminal_font_size = size;
            portal.save_settings();
            Task::none()
        }
        UiMessage::WindowResized(size) => {
            portal.window_size = size;
            if !portal.sidebar_manually_collapsed {
                portal.sidebar_collapsed = size.width < SIDEBAR_AUTO_COLLAPSE_THRESHOLD;
            }
            Task::none()
        }
        UiMessage::ToastDismiss(id) => {
            portal.toast_manager.dismiss(id);
            Task::none()
        }
        UiMessage::ToastTick => {
            portal.toast_manager.cleanup_expired();
            Task::none()
        }
        UiMessage::KeyboardEvent(key, modifiers) => {
            handle_keyboard_event(portal, key, modifiers)
        }
        UiMessage::FocusSectionChange(section) => {
            portal.focus_section = section;
            // Reset content focus when switching sections
            if section == FocusSection::Content {
                portal.host_grid_focus_index = None;
                portal.history_focus_index = None;
            }
            Task::none()
        }
        UiMessage::TerminalCaptureChange(captured) => {
            portal.terminal_captured = captured;
            if !captured {
                portal.focus_section = FocusSection::Content;
            }
            Task::none()
        }
    }
}

/// Handle keyboard shortcuts
fn handle_keyboard_event(
    portal: &mut Portal,
    key: Key,
    modifiers: keyboard::Modifiers,
) -> Task<Message> {
    // Priority 1: Dialog open - only handle Escape to close
    if portal.dialogs.is_open() {
        if let Key::Named(keyboard::key::Named::Escape) = key {
            if let Some(dialog) = portal.dialogs.host_key_mut() {
                dialog.respond(HostKeyVerificationResponse::Reject);
                portal.toast_manager.push(Toast::warning("Connection cancelled"));
            }
            portal.dialogs.close();
        }
        return Task::none();
    }

    // Priority 2: Terminal captured - only Ctrl+Escape exits
    if portal.terminal_captured {
        if let Key::Named(keyboard::key::Named::Escape) = &key {
            if modifiers.control() {
                portal.terminal_captured = false;
                portal.focus_section = FocusSection::Content;
                return Task::none();
            }
        }
        // All other keys go to terminal (handled by terminal widget)
        return Task::none();
    }

    // Priority 3: Global shortcuts (always work unless terminal captured)
    match (&key, modifiers.control(), modifiers.shift()) {
        // F1 - Focus Sidebar
        (Key::Named(keyboard::key::Named::F1), false, false) => {
            portal.focus_section = FocusSection::Sidebar;
            return Task::none();
        }
        // F2 - Focus Tab Bar
        (Key::Named(keyboard::key::Named::F2), false, false) => {
            portal.focus_section = FocusSection::TabBar;
            return Task::none();
        }
        // F3 - Focus Content
        (Key::Named(keyboard::key::Named::F3), false, false) => {
            portal.focus_section = FocusSection::Content;
            return Task::none();
        }
        // Escape - close context menus, or exit terminal capture indication
        (Key::Named(keyboard::key::Named::Escape), _, _) => {
            // Close any open SFTP context menu or dialog
            for tab_state in portal.sftp.tab_values_mut() {
                tab_state.hide_context_menu();
                tab_state.close_dialog();
            }
            return Task::none();
        }
        // Ctrl+N - new tab / go to host grid
        (Key::Character(c), true, false) if c.as_str() == "n" => {
            portal.active_view = View::HostGrid;
            portal.focus_section = FocusSection::Content;
            return Task::none();
        }
        // Ctrl+W - close current tab
        (Key::Character(c), true, false) if c.as_str() == "w" => {
            portal.close_active_tab();
            return Task::none();
        }
        // Ctrl+Tab - next tab
        (Key::Named(keyboard::key::Named::Tab), true, false) => {
            portal.select_next_tab();
            return Task::none();
        }
        // Ctrl+Shift+Tab - previous tab
        (Key::Named(keyboard::key::Named::Tab), true, true) => {
            portal.select_prev_tab();
            return Task::none();
        }
        // Ctrl+Shift+K - Install SSH key on remote server
        (Key::Character(c), true, true) if c.as_str() == "k" || c.as_str() == "K" => {
            if let View::Terminal(session_id) = portal.active_view {
                if portal.sessions.contains(session_id) {
                    return portal.update(Message::Session(SessionMessage::InstallKey(session_id)));
                }
            }
            return Task::none();
        }
        _ => {}
    }

    // Priority 4: Section-specific navigation
    match portal.focus_section {
        FocusSection::Sidebar => handle_sidebar_keyboard(portal, &key, &modifiers),
        FocusSection::TabBar => handle_tabbar_keyboard(portal, &key, &modifiers),
        FocusSection::Content => handle_content_keyboard(portal, &key, &modifiers),
    }
}

/// Number of sidebar menu items
const SIDEBAR_MENU_COUNT: usize = 5;

/// Handle keyboard navigation in sidebar
fn handle_sidebar_keyboard(
    portal: &mut Portal,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    match key {
        Key::Named(keyboard::key::Named::ArrowUp) => {
            portal.sidebar_focus_index = portal.sidebar_focus_index.saturating_sub(1);
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            portal.sidebar_focus_index = (portal.sidebar_focus_index + 1).min(SIDEBAR_MENU_COUNT - 1);
        }
        Key::Named(keyboard::key::Named::Home) => {
            portal.sidebar_focus_index = 0;
        }
        Key::Named(keyboard::key::Named::End) => {
            portal.sidebar_focus_index = SIDEBAR_MENU_COUNT - 1;
        }
        Key::Named(keyboard::key::Named::ArrowRight) => {
            portal.focus_section = FocusSection::Content;
        }
        Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space) => {
            let item = match portal.sidebar_focus_index {
                0 => SidebarMenuItem::Hosts,
                1 => SidebarMenuItem::Sftp,
                2 => SidebarMenuItem::Snippets,
                3 => SidebarMenuItem::History,
                4 => SidebarMenuItem::Settings,
                _ => return Task::none(),
            };
            return portal.update(Message::Ui(UiMessage::SidebarItemSelect(item)));
        }
        _ => {}
    }
    Task::none()
}

/// Handle keyboard navigation in tab bar
fn handle_tabbar_keyboard(
    portal: &mut Portal,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    let tab_count = portal.tabs.len();
    if tab_count == 0 {
        // No tabs - switch to content
        portal.focus_section = FocusSection::Content;
        return Task::none();
    }

    match key {
        Key::Named(keyboard::key::Named::ArrowLeft) => {
            portal.tab_focus_index = portal.tab_focus_index.saturating_sub(1);
        }
        Key::Named(keyboard::key::Named::ArrowRight) => {
            portal.tab_focus_index = (portal.tab_focus_index + 1).min(tab_count.saturating_sub(1));
        }
        Key::Named(keyboard::key::Named::Home) => {
            portal.tab_focus_index = 0;
        }
        Key::Named(keyboard::key::Named::End) => {
            portal.tab_focus_index = tab_count.saturating_sub(1);
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            portal.focus_section = FocusSection::Content;
        }
        Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space) => {
            if let Some(tab) = portal.tabs.get(portal.tab_focus_index) {
                let tab_id = tab.id;
                return portal.update(Message::Tab(TabMessage::Select(tab_id)));
            }
        }
        Key::Named(keyboard::key::Named::Delete) => {
            if let Some(tab) = portal.tabs.get(portal.tab_focus_index) {
                let tab_id = tab.id;
                return portal.update(Message::Tab(TabMessage::Close(tab_id)));
            }
        }
        _ => {}
    }
    Task::none()
}

/// Handle keyboard navigation in content area
fn handle_content_keyboard(
    portal: &mut Portal,
    key: &Key,
    modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    match &portal.active_view {
        View::Settings => {
            // Settings page - arrow left goes back to sidebar
            if let Key::Named(keyboard::key::Named::ArrowLeft) = key {
                portal.focus_section = FocusSection::Sidebar;
            }
            Task::none()
        }
        View::HostGrid => {
            match portal.sidebar_selection {
                SidebarMenuItem::Hosts | SidebarMenuItem::Sftp | SidebarMenuItem::Snippets | SidebarMenuItem::Settings => {
                    handle_host_grid_keyboard(portal, key, modifiers)
                }
                SidebarMenuItem::History => {
                    handle_history_keyboard(portal, key, modifiers)
                }
            }
        }
        View::Terminal(_session_id) => {
            // Enter key or any character captures terminal
            match key {
                Key::Named(keyboard::key::Named::Enter) | Key::Character(_) => {
                    portal.terminal_captured = true;
                }
                Key::Named(keyboard::key::Named::ArrowUp) => {
                    portal.focus_section = FocusSection::TabBar;
                }
                Key::Named(keyboard::key::Named::ArrowLeft) => {
                    portal.focus_section = FocusSection::Sidebar;
                }
                _ => {}
            }
            Task::none()
        }
        View::DualSftp(tab_id) => {
            handle_sftp_keyboard(portal, *tab_id, key, modifiers)
        }
    }
}

/// Handle keyboard navigation in host grid
fn handle_host_grid_keyboard(
    portal: &mut Portal,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    // Count total items (groups + hosts)
    let group_count = portal.hosts_config.groups.len();
    let host_count = portal.hosts_config.hosts.len();
    let total_items = group_count + host_count;

    if total_items == 0 {
        // "/" focuses search even when empty
        if let Key::Character(c) = key {
            if c.as_str() == "/" {
                portal.host_grid_focus_index = None; // Clear grid focus when focusing search
                return iced::widget::text_input::focus(crate::views::host_grid::search_input_id());
            }
        }
        return Task::none();
    }

    // Calculate column count for 2D navigation
    let columns = crate::views::host_grid::calculate_columns(portal.window_size.width, portal.sidebar_collapsed);

    match key {
        Key::Named(keyboard::key::Named::ArrowUp) => {
            if let Some(idx) = portal.host_grid_focus_index {
                if idx >= columns {
                    portal.host_grid_focus_index = Some(idx - columns);
                } else {
                    // At top row - move focus to tabs
                    portal.focus_section = FocusSection::TabBar;
                }
            } else {
                portal.host_grid_focus_index = Some(0);
            }
            // Unfocus search input when navigating with arrows
            return iced::widget::text_input::focus(iced::widget::text_input::Id::unique());
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            if let Some(idx) = portal.host_grid_focus_index {
                let new_idx = idx + columns;
                if new_idx < total_items {
                    portal.host_grid_focus_index = Some(new_idx);
                }
            } else {
                portal.host_grid_focus_index = Some(0);
            }
            // Unfocus search input when navigating with arrows
            return iced::widget::text_input::focus(iced::widget::text_input::Id::unique());
        }
        Key::Named(keyboard::key::Named::ArrowLeft) => {
            if let Some(idx) = portal.host_grid_focus_index {
                if idx > 0 {
                    portal.host_grid_focus_index = Some(idx - 1);
                } else {
                    portal.focus_section = FocusSection::Sidebar;
                }
            } else {
                portal.focus_section = FocusSection::Sidebar;
            }
            // Unfocus search input when navigating with arrows
            return iced::widget::text_input::focus(iced::widget::text_input::Id::unique());
        }
        Key::Named(keyboard::key::Named::ArrowRight) => {
            if let Some(idx) = portal.host_grid_focus_index {
                if idx + 1 < total_items {
                    portal.host_grid_focus_index = Some(idx + 1);
                }
            } else {
                portal.host_grid_focus_index = Some(0);
            }
            // Unfocus search input when navigating with arrows
            return iced::widget::text_input::focus(iced::widget::text_input::Id::unique());
        }
        Key::Named(keyboard::key::Named::Home) => {
            portal.host_grid_focus_index = Some(0);
            return iced::widget::text_input::focus(iced::widget::text_input::Id::unique());
        }
        Key::Named(keyboard::key::Named::End) => {
            portal.host_grid_focus_index = Some(total_items.saturating_sub(1));
            return iced::widget::text_input::focus(iced::widget::text_input::Id::unique());
        }
        Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space) => {
            // Only activate if we have a focused card (not the search input)
            if let Some(idx) = portal.host_grid_focus_index {
                // First come groups, then hosts
                if idx < group_count {
                    // Toggle group
                    if let Some(group) = portal.hosts_config.groups.get(idx) {
                        let group_id = group.id;
                        return portal.update(Message::Ui(UiMessage::FolderToggle(group_id)));
                    }
                } else {
                    // Connect to host
                    let host_idx = idx - group_count;
                    if let Some(host) = portal.hosts_config.hosts.get(host_idx) {
                        let host_id = host.id;
                        return portal.update(Message::Host(HostMessage::Connect(host_id)));
                    }
                }
            }
            // No card focused - don't handle Enter (let search input handle it)
        }
        Key::Character(c) if c.as_str() == "/" => {
            portal.host_grid_focus_index = None; // Clear grid focus when focusing search
            return iced::widget::text_input::focus(crate::views::host_grid::search_input_id());
        }
        _ => {}
    }
    Task::none()
}

/// Handle keyboard navigation in history view
fn handle_history_keyboard(
    portal: &mut Portal,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    let entry_count = portal.history_config.entries.len();
    if entry_count == 0 {
        return Task::none();
    }

    match key {
        Key::Named(keyboard::key::Named::ArrowUp) => {
            if let Some(idx) = portal.history_focus_index {
                portal.history_focus_index = Some(idx.saturating_sub(1));
            } else {
                portal.history_focus_index = Some(0);
            }
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            if let Some(idx) = portal.history_focus_index {
                portal.history_focus_index = Some((idx + 1).min(entry_count - 1));
            } else {
                portal.history_focus_index = Some(0);
            }
        }
        Key::Named(keyboard::key::Named::Home) => {
            portal.history_focus_index = Some(0);
        }
        Key::Named(keyboard::key::Named::End) => {
            portal.history_focus_index = Some(entry_count.saturating_sub(1));
        }
        Key::Named(keyboard::key::Named::ArrowLeft) => {
            portal.focus_section = FocusSection::Sidebar;
        }
        Key::Named(keyboard::key::Named::Enter | keyboard::key::Named::Space) => {
            if let Some(idx) = portal.history_focus_index {
                if let Some(entry) = portal.history_config.entries.get(idx) {
                    let host_id = entry.host_id;
                    return portal.update(Message::History(HistoryMessage::Reconnect(host_id)));
                }
            }
        }
        _ => {}
    }
    Task::none()
}

/// Handle keyboard navigation in SFTP dual-pane view
fn handle_sftp_keyboard(
    portal: &mut Portal,
    tab_id: uuid::Uuid,
    key: &Key,
    _modifiers: &keyboard::Modifiers,
) -> Task<Message> {
    let Some(state) = portal.sftp.get_tab_mut(tab_id) else {
        return Task::none();
    };

    let active_pane = state.active_pane;
    let pane_state = match active_pane {
        PaneId::Left => &mut state.left_pane,
        PaneId::Right => &mut state.right_pane,
    };

    let file_count = pane_state.entries.len();

    match key {
        // Tab switches between panes
        Key::Named(keyboard::key::Named::Tab) => {
            state.active_pane = match state.active_pane {
                PaneId::Left => PaneId::Right,
                PaneId::Right => PaneId::Left,
            };
        }
        Key::Named(keyboard::key::Named::ArrowUp) => {
            if file_count > 0 {
                let current = pane_state.last_selected_index.unwrap_or(0);
                let new_idx = current.saturating_sub(1);
                pane_state.selected_indices.clear();
                pane_state.selected_indices.insert(new_idx);
                pane_state.last_selected_index = Some(new_idx);
            }
        }
        Key::Named(keyboard::key::Named::ArrowDown) => {
            if file_count > 0 {
                let current = pane_state.last_selected_index.unwrap_or(0);
                let new_idx = (current + 1).min(file_count - 1);
                pane_state.selected_indices.clear();
                pane_state.selected_indices.insert(new_idx);
                pane_state.last_selected_index = Some(new_idx);
            }
        }
        Key::Named(keyboard::key::Named::Home) => {
            if file_count > 0 {
                pane_state.selected_indices.clear();
                pane_state.selected_indices.insert(0);
                pane_state.last_selected_index = Some(0);
            }
        }
        Key::Named(keyboard::key::Named::End) => {
            if file_count > 0 {
                let last = file_count - 1;
                pane_state.selected_indices.clear();
                pane_state.selected_indices.insert(last);
                pane_state.last_selected_index = Some(last);
            }
        }
        Key::Named(keyboard::key::Named::ArrowLeft) => {
            // Need to re-borrow state since we're not inside the match
            let Some(state) = portal.sftp.get_tab_mut(tab_id) else {
                return Task::none();
            };
            if active_pane == PaneId::Left {
                portal.focus_section = FocusSection::Sidebar;
            } else {
                state.active_pane = PaneId::Left;
            }
            return Task::none();
        }
        Key::Named(keyboard::key::Named::ArrowRight) => {
            let Some(state) = portal.sftp.get_tab_mut(tab_id) else {
                return Task::none();
            };
            if active_pane != PaneId::Right {
                state.active_pane = PaneId::Right;
            }
            return Task::none();
        }
        Key::Named(keyboard::key::Named::Enter) => {
            // Navigate into directory or activate file
            if let Some(idx) = pane_state.last_selected_index {
                if let Some(entry) = pane_state.entries.get(idx) {
                    if entry.is_dir {
                        let path = entry.path.clone();
                        return portal.update(Message::Sftp(SftpMessage::PaneNavigate(tab_id, active_pane, path)));
                    }
                }
            }
        }
        Key::Named(keyboard::key::Named::Backspace) => {
            // Navigate to parent
            return portal.update(Message::Sftp(SftpMessage::PaneNavigateUp(tab_id, active_pane)));
        }
        Key::Named(keyboard::key::Named::F5) => {
            // Refresh
            return portal.update(Message::Sftp(SftpMessage::PaneRefresh(tab_id, active_pane)));
        }
        _ => {}
    }
    Task::none()
}
