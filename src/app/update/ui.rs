//! UI state message handlers

use iced::keyboard::{self, Key};
use iced::Task;

use crate::app::{Portal, View, SIDEBAR_AUTO_COLLAPSE_THRESHOLD};
use crate::message::{Message, SessionMessage, SftpMessage, SidebarMenuItem, UiMessage};
use crate::ssh::host_key_verification::HostKeyVerificationResponse;
use crate::views::toast::Toast;
use crate::views::dialogs::settings_dialog::SettingsDialogState;
use crate::views::dialogs::snippets_dialog::SnippetsDialogState;

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
                    portal.dialogs.open_settings(SettingsDialogState {
                        dark_mode: portal.dark_mode,
                        terminal_font_size: portal.terminal_font_size,
                    });
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
        UiMessage::ThemeToggle(enabled) => {
            portal.dark_mode = enabled;
            if let Some(dialog) = portal.dialogs.settings_mut() {
                dialog.dark_mode = enabled;
            }
            portal.save_settings();
            Task::none()
        }
        UiMessage::FontSizeChange(size) => {
            portal.terminal_font_size = size;
            if let Some(dialog) = portal.dialogs.settings_mut() {
                dialog.terminal_font_size = size;
            }
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
    }
}

/// Handle keyboard shortcuts
fn handle_keyboard_event(
    portal: &mut Portal,
    key: Key,
    modifiers: keyboard::Modifiers,
) -> Task<Message> {
    match (key, modifiers.control(), modifiers.shift()) {
        // Escape - close dialogs and context menus
        (Key::Named(keyboard::key::Named::Escape), _, _) => {
            if let Some(dialog) = portal.dialogs.host_key_mut() {
                dialog.respond(HostKeyVerificationResponse::Reject);
                portal.toast_manager.push(Toast::warning("Connection cancelled"));
                portal.dialogs.close();
            } else if portal.dialogs.is_open() {
                portal.dialogs.close();
            }
            // Close any open SFTP context menu or dialog
            for tab_state in portal.sftp.tab_values_mut() {
                tab_state.hide_context_menu();
                tab_state.close_dialog();
            }
        }
        // Ctrl+N - new tab / go to host grid
        (Key::Character(c), true, false) if c.as_str() == "n" => {
            portal.active_view = View::HostGrid;
        }
        // Ctrl+W - close current tab
        (Key::Character(c), true, false) if c.as_str() == "w" => {
            portal.close_active_tab();
        }
        // Ctrl+Tab - next tab
        (Key::Named(keyboard::key::Named::Tab), true, false) => {
            portal.select_next_tab();
        }
        // Ctrl+Shift+Tab - previous tab
        (Key::Named(keyboard::key::Named::Tab), true, true) => {
            portal.select_prev_tab();
        }
        // Ctrl+Shift+K - Install SSH key on remote server
        (Key::Character(c), true, true) if c.as_str() == "k" || c.as_str() == "K" => {
            if let View::Terminal(session_id) = portal.active_view {
                if portal.sessions.contains(session_id) {
                    return portal.update(Message::Session(SessionMessage::InstallKey(session_id)));
                }
            }
        }
        _ => {}
    }
    Task::none()
}
