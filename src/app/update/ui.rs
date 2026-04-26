//! UI state message handlers

mod keyboard;
mod settings;

use iced::Task;

use crate::app::{Portal, SIDEBAR_AUTO_COLLAPSE_THRESHOLD, View};
use crate::message::{Message, ProxySessionsMessage, SftpMessage, SidebarMenuItem, UiMessage};

/// Handle UI state messages.
pub fn handle_ui(portal: &mut Portal, msg: UiMessage) -> Task<Message> {
    match msg {
        UiMessage::SearchChanged(query) => {
            portal.ui.search_query = query;
            Task::none()
        }
        UiMessage::FolderToggle(id) => {
            if let Some(group) = portal.config.hosts.find_group_mut(id) {
                group.collapsed = !group.collapsed;
                if let Err(e) = portal.config.hosts.save() {
                    tracing::error!("Failed to save config: {}", e);
                }
            }
            Task::none()
        }
        UiMessage::SidebarItemSelect(item) => handle_sidebar_item_select(portal, item),
        UiMessage::SidebarToggleCollapse => {
            portal.ui.sidebar_state = portal.ui.sidebar_state.next();
            portal.ui.sidebar_manually_set = true;
            tracing::info!("Sidebar state updated (manual)");
            Task::none()
        }
        UiMessage::SettingsTabSelected(tab) => {
            portal.ui.settings_tab = tab;
            Task::none()
        }
        msg @ (UiMessage::ThemeChange(_)
        | UiMessage::FontChange(_)
        | UiMessage::FontSizeChange(_)
        | UiMessage::TerminalScrollSpeedChange(_)
        | UiMessage::UiScaleChange(_)
        | UiMessage::UiScaleReset
        | UiMessage::SnippetHistoryEnabled(_)
        | UiMessage::SnippetHistoryStoreCommand(_)
        | UiMessage::SnippetHistoryStoreOutput(_)
        | UiMessage::SnippetHistoryRedactOutput(_)
        | UiMessage::SessionLoggingEnabled(_)
        | UiMessage::AllowAgentForwarding(_)
        | UiMessage::AutoReconnectEnabled(_)
        | UiMessage::ReconnectMaxAttemptsChanged(_)
        | UiMessage::ReconnectBaseDelayChanged(_)
        | UiMessage::ReconnectMaxDelayChanged(_)
        | UiMessage::CredentialTimeoutChange(_)
        | UiMessage::SecurityAuditLoggingEnabled(_)
        | UiMessage::VncQualityPresetChanged(_)
        | UiMessage::VncScalingModeChanged(_)
        | UiMessage::VncEncodingPreferenceChanged(_)
        | UiMessage::VncColorDepthChanged(_)
        | UiMessage::VncRefreshFpsChanged(_)
        | UiMessage::VncPointerIntervalChanged(_)
        | UiMessage::VncRemoteResizeChanged(_)
        | UiMessage::VncClipboardSharingChanged(_)
        | UiMessage::VncViewOnlyChanged(_)
        | UiMessage::VncShowCursorDotChanged(_)
        | UiMessage::VncShowStatsOverlayChanged(_)
        | UiMessage::PortalHubEnabled(_)
        | UiMessage::PortalHubDefaultForNewHosts(_)
        | UiMessage::PortalHubHostChanged(_)
        | UiMessage::PortalHubPortChanged(_)
        | UiMessage::PortalHubUsernameChanged(_)
        | UiMessage::PortalHubIdentityFileChanged(_)
        | UiMessage::PortalHubWebUrlChanged(_)
        | UiMessage::PortalHubCheckStatus
        | UiMessage::PortalHubStatusLoaded(_)
        | UiMessage::PortalHubAuthenticate
        | UiMessage::PortalHubAuthenticated(_)
        | UiMessage::PortalHubUploadLocalProfile
        | UiMessage::PortalHubUploadLocalProfileDone(_)
        | UiMessage::PortalHubPullProfile
        | UiMessage::PortalHubPullProfileDone(_)) => settings::handle_settings_message(portal, msg),
        UiMessage::WindowResized(size) => {
            portal.ui.window_size = size;
            if !portal.ui.sidebar_manually_set {
                use crate::app::SidebarState;
                portal.ui.sidebar_state = if size.width < SIDEBAR_AUTO_COLLAPSE_THRESHOLD {
                    SidebarState::IconsOnly
                } else {
                    SidebarState::Expanded
                };
            }
            if portal.prefs.vnc_settings.remote_resize {
                if let View::VncViewer(session_id) = portal.ui.active_view {
                    if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                        if let Some((w, h)) = portal.vnc_target_size() {
                            vnc.session.try_request_desktop_size(w, h);
                        }
                    }
                }
            }
            Task::none()
        }
        UiMessage::WindowUnfocused => {
            portal.ui.window_focused = false;
            if let View::VncViewer(session_id) = portal.ui.active_view {
                if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                    vnc.session.release_all_keys();
                }
            }
            Task::none()
        }
        UiMessage::WindowFocused => {
            portal.ui.window_focused = true;
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
            keyboard::handle_keyboard_event(portal, key, modifiers)
        }
        UiMessage::KeyReleased(key, modifiers) => {
            keyboard::handle_key_released(portal, key, modifiers)
        }
    }
}

fn handle_sidebar_item_select(portal: &mut Portal, item: SidebarMenuItem) -> Task<Message> {
    // Auto-close pristine SFTP tab when navigating away (not when staying on SFTP).
    if item != SidebarMenuItem::Sftp {
        if let View::DualSftp(tab_id) = portal.ui.active_view {
            if let Some(state) = portal.sftp.get_tab(tab_id) {
                if state.is_pristine() {
                    portal.close_tab(tab_id);
                }
            }
        }
    }

    portal.ui.terminal_captured = false;
    portal.ui.sidebar_selection = item;
    tracing::info!("Sidebar item selected");

    match item {
        SidebarMenuItem::Hosts => {
            portal.restore_sidebar_after_session();
            portal.enter_host_grid();
            iced::widget::operation::focus(crate::app::search_input_id())
        }
        SidebarMenuItem::History => {
            portal.restore_sidebar_after_session();
            portal.enter_host_grid();
            Task::none()
        }
        SidebarMenuItem::Sftp => {
            if let Some(tab_id) = portal.sftp.first_tab_id() {
                portal.set_active_tab(tab_id);
                Task::none()
            } else {
                portal.update(Message::Sftp(SftpMessage::Open))
            }
        }
        SidebarMenuItem::Sessions => {
            portal.restore_sidebar_after_session();
            portal.ui.active_view = View::ProxySessions;
            portal.update(Message::ProxySessions(ProxySessionsMessage::Refresh))
        }
        SidebarMenuItem::Settings => {
            portal.ui.active_view = View::Settings;
            Task::none()
        }
        SidebarMenuItem::Snippets => {
            portal.ui.active_view = View::Snippets;
            portal.snippets.editing = None;
            portal.restore_sidebar_after_session();
            Task::none()
        }
        SidebarMenuItem::About => {
            portal.dialogs.open_about();
            Task::none()
        }
    }
}
