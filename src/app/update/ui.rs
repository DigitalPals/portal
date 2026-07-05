//! UI state message handlers

mod keyboard;
pub(crate) mod settings;

use iced::Task;

use crate::app::{Portal, SIDEBAR_AUTO_COLLAPSE_THRESHOLD, SidebarState, View};
use crate::message::{
    CommandAction, HostMessage, Message, ProxySessionsMessage, SessionMessage, SftpMessage,
    SidebarMenuItem, SnippetMessage, UiMessage,
};

/// Handle UI state messages.
pub fn handle_ui(portal: &mut Portal, msg: UiMessage) -> Task<Message> {
    match msg {
        UiMessage::SearchChanged(query) => {
            portal.ui.search_query = query;
            Task::none()
        }
        UiMessage::SearchSubmitted => handle_search_submitted(portal),
        UiMessage::HostCardHovered(host_id) => {
            portal.ui.hovered_host_card = host_id;
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
        UiMessage::CommandPaletteToggle => {
            portal.ui.command_palette_open = !portal.ui.command_palette_open;
            if portal.ui.command_palette_open {
                portal.ui.command_palette_query.clear();
                iced::widget::operation::focus(crate::views::command_palette::command_input_id())
            } else {
                Task::none()
            }
        }
        UiMessage::CommandPaletteClose => {
            portal.ui.command_palette_open = false;
            Task::none()
        }
        UiMessage::CommandPaletteChanged(query) => {
            portal.ui.command_palette_query = query;
            Task::none()
        }
        UiMessage::CommandPaletteRun(action) => run_command_action(portal, action),
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
        | UiMessage::PortalHubWebPortChanged(_)
        | UiMessage::PortalHubPortChanged(_)
        | UiMessage::PortalHubUsernameChanged(_)
        | UiMessage::PortalHubIdentityFileChanged(_)
        | UiMessage::PortalHubWebUrlChanged(_)
        | UiMessage::PortalHubHostsSyncChanged(_)
        | UiMessage::PortalHubSettingsSyncChanged(_)
        | UiMessage::PortalHubSnippetsSyncChanged(_)
        | UiMessage::PortalHubKeyVaultChanged(_)
        | UiMessage::PortalHubDisableSyncRequested(_)
        | UiMessage::PortalHubDisableSyncKeepData(_)
        | UiMessage::PortalHubDisableSyncDeleteData(_)
        | UiMessage::PortalHubDisableSyncDeleteDone(_, _)
        | UiMessage::PortalHubPreferVaultKeys(_)
        | UiMessage::PortalHubWizardToggleHost(_)
        | UiMessage::PortalHubWizardToggleAdvanced
        | UiMessage::PortalHubWizardRouteDefault(_)
        | UiMessage::PortalHubWizardPreferVault(_)
        | UiMessage::PortalHubWizardSyncAll(_)
        | UiMessage::PortalHubWizardApply
        | UiMessage::PortalHubWizardSkip
        | UiMessage::PortalHubOpenDefaultsReview
        | UiMessage::PortalHubDefaultsPromptDismiss
        | UiMessage::PortalHubOpenOnboarding
        | UiMessage::PortalHubOpenGithub
        | UiMessage::PortalHubCheckStatus
        | UiMessage::PortalHubStatusLoaded(_)
        | UiMessage::PortalHubRunDiagnostics
        | UiMessage::PortalHubDiagnosticsDone(_)
        | UiMessage::PortalHubAuthenticate
        | UiMessage::PortalHubAuthenticated(_)
        | UiMessage::PortalHubLogout
        | UiMessage::PortalHubLoggedOut(_)
        | UiMessage::PortalHubUploadLocalProfile
        | UiMessage::PortalHubUploadLocalProfileDone(_)
        | UiMessage::PortalHubPullProfile
        | UiMessage::PortalHubPullProfileDone(_)
        | UiMessage::PortalHubSyncNow
        | UiMessage::PortalHubLocalSyncDue
        | UiMessage::PortalHubRemoteRevisions(_)
        | UiMessage::PortalHubSyncDone(_, _)
        | UiMessage::PortalHubConflictChoiceChanged(_, _)
        | UiMessage::PortalHubResolveConflicts
        | UiMessage::PortalHubResolveConflictsDone(_)) => {
            settings::handle_settings_message(portal, msg)
        }
        UiMessage::WindowResized(size) => {
            portal.ui.window_size = size;
            if should_apply_responsive_sidebar_state(
                portal.ui.sidebar_manually_set,
                &portal.ui.active_view,
                portal.ui.sidebar_state,
                portal.ui.sidebar_state_before_session,
            ) {
                portal.ui.sidebar_state = if size.width < SIDEBAR_AUTO_COLLAPSE_THRESHOLD {
                    SidebarState::IconsOnly
                } else {
                    SidebarState::Expanded
                };
            }
            if portal.prefs.vnc_settings.remote_resize
                && let View::VncViewer(session_id) = portal.ui.active_view
                && let Some(vnc) = portal.vnc_sessions.get(&session_id)
                && let Some((w, h)) = portal.vnc_target_size()
            {
                vnc.session.try_request_desktop_size(w, h);
            }
            reconcile_active_terminal_size(portal)
        }
        UiMessage::WindowUnfocused => {
            portal.ui.window_focused = false;
            if let View::VncViewer(session_id) = portal.ui.active_view
                && let Some(vnc) = portal.vnc_sessions.get(&session_id)
            {
                vnc.session.release_all_keys();
            }
            Task::none()
        }
        UiMessage::WindowFocused => {
            portal.ui.window_focused = true;
            reconcile_active_terminal_size(portal)
        }
        UiMessage::ToastDismiss(id) => {
            portal.toast_manager.dismiss(id);
            Task::none()
        }
        UiMessage::ToastAction(id, action) => {
            portal.toast_manager.dismiss(id);
            match action {
                crate::views::toast::ToastAction::OpenVaultApprovals => {
                    let open_vault = handle_sidebar_item_select(portal, SidebarMenuItem::Vault);
                    let refresh = Task::done(Message::Vault(
                        crate::message::VaultMessage::EnrollmentRefresh,
                    ));
                    Task::batch([open_vault, refresh])
                }
            }
        }
        UiMessage::ToastTick => {
            portal.toast_manager.cleanup_expired();
            Task::none()
        }
        UiMessage::AgentStatusTick => {
            // No-op: drives animated tab agent indicators.
            Task::none()
        }
        UiMessage::KeyboardEvent(key, modifiers, shortcut_key) => {
            keyboard::handle_keyboard_event(portal, key, modifiers, shortcut_key)
        }
        UiMessage::KeyReleased(key, modifiers) => {
            keyboard::handle_key_released(portal, key, modifiers)
        }
    }
}

/// Omnibox submit: `user@host[:port]` connects directly; otherwise, when the
/// filter matches exactly one host, connect to it.
fn handle_search_submitted(portal: &mut Portal) -> Task<Message> {
    let query = portal.ui.search_query.trim().to_string();
    if query.is_empty() {
        return Task::none();
    }

    if let Some((user, rest)) = query.split_once('@') {
        let (hostname, port) = match rest.rsplit_once(':') {
            Some((host, port_str)) => match port_str.parse::<u16>() {
                Ok(port) => (host, port),
                Err(_) => (rest, 22),
            },
            None => (rest, 22),
        };
        if user.is_empty() || hostname.is_empty() {
            return Task::none();
        }

        let now = chrono::Utc::now();
        let temp_host = crate::config::Host {
            id: uuid::Uuid::new_v4(),
            name: format!("{}@{}", user, hostname),
            hostname: hostname.to_string(),
            port,
            username: user.to_string(),
            protocol: crate::config::Protocol::Ssh,
            vnc_port: None,
            vnc_password_id: None,
            auth: crate::config::AuthMethod::Agent,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            hub_routing: crate::config::hosts::HubRouting::Auto,
            jump_host_id: None,
            group_id: None,
            notes: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        };
        portal.ui.search_query.clear();
        tracing::info!("Omnibox quick connect requested");
        return portal.connect_to_host(&temp_host);
    }

    let query_lower = query.to_lowercase();
    let matches: Vec<uuid::Uuid> = portal
        .config
        .hosts
        .hosts
        .iter()
        .filter(|host| {
            host.name.to_lowercase().contains(&query_lower)
                || host.hostname.to_lowercase().contains(&query_lower)
        })
        .map(|host| host.id)
        .collect();
    if let [host_id] = matches.as_slice() {
        return Task::done(Message::Host(HostMessage::Connect(*host_id)));
    }

    Task::none()
}

fn should_apply_responsive_sidebar_state(
    sidebar_manually_set: bool,
    active_view: &View,
    sidebar_state: SidebarState,
    sidebar_state_before_session: Option<SidebarState>,
) -> bool {
    !sidebar_manually_set
        && !matches!(
            (active_view, sidebar_state, sidebar_state_before_session),
            (
                View::Terminal(_) | View::VncViewer(_),
                SidebarState::Hidden,
                Some(_)
            )
        )
}

pub(super) fn reconcile_active_terminal_size(portal: &Portal) -> Task<Message> {
    let View::Terminal(session_id) = portal.ui.active_view else {
        return Task::none();
    };
    let Some(session) = portal.sessions.get(session_id) else {
        return Task::none();
    };

    let (cols, rows) = portal.terminal_initial_size();
    if session.terminal.size() == (cols, rows) {
        return Task::none();
    }

    Task::done(Message::Session(SessionMessage::Resize(
        session_id, cols, rows,
    )))
}

fn run_command_action(portal: &mut Portal, action: CommandAction) -> Task<Message> {
    portal.ui.command_palette_open = false;
    portal.ui.command_palette_query.clear();

    match action {
        CommandAction::Hosts => handle_sidebar_item_select(portal, SidebarMenuItem::Hosts),
        CommandAction::Sftp => handle_sidebar_item_select(portal, SidebarMenuItem::Sftp),
        CommandAction::Snippets => handle_sidebar_item_select(portal, SidebarMenuItem::Snippets),
        CommandAction::Vault => handle_sidebar_item_select(portal, SidebarMenuItem::Vault),
        CommandAction::History => handle_sidebar_item_select(portal, SidebarMenuItem::History),
        CommandAction::Settings => handle_sidebar_item_select(portal, SidebarMenuItem::Settings),
        CommandAction::QuickConnect => portal.update(Message::Host(HostMessage::QuickConnect)),
        CommandAction::NewHost => portal.update(Message::Host(HostMessage::Add)),
        CommandAction::LocalTerminal => portal.update(Message::Host(HostMessage::LocalTerminal)),
        CommandAction::ConnectHost(id) => portal.update(Message::Host(HostMessage::Connect(id))),
        CommandAction::RunSnippet(id) => portal.update(Message::Snippet(SnippetMessage::Run(id))),
        CommandAction::PortalHubSync => portal.update(Message::Ui(UiMessage::PortalHubSyncNow)),
    }
}

fn handle_sidebar_item_select(portal: &mut Portal, item: SidebarMenuItem) -> Task<Message> {
    // Auto-close pristine SFTP tab when navigating away (not when staying on SFTP).
    if item != SidebarMenuItem::Sftp
        && let View::DualSftp(tab_id) = portal.ui.active_view
        && let Some(state) = portal.sftp.get_tab(tab_id)
        && state.is_pristine()
    {
        portal.close_tab(tab_id);
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
        SidebarMenuItem::Vault => {
            portal.restore_sidebar_after_session();
            portal.ui.active_view = View::Vault;
            Task::none()
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn responsive_resize_preserves_auto_hidden_terminal_sidebar() {
        assert!(!should_apply_responsive_sidebar_state(
            false,
            &View::Terminal(Uuid::new_v4()),
            SidebarState::Hidden,
            Some(SidebarState::Expanded)
        ));
    }

    #[test]
    fn responsive_resize_still_updates_regular_views() {
        assert!(should_apply_responsive_sidebar_state(
            false,
            &View::HostGrid,
            SidebarState::Expanded,
            None
        ));
    }

    #[test]
    fn responsive_resize_respects_manual_sidebar_choice() {
        assert!(!should_apply_responsive_sidebar_state(
            true,
            &View::HostGrid,
            SidebarState::Expanded,
            None
        ));
    }
}
