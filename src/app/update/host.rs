//! Host management message handlers

use iced::Task;
use uuid::Uuid;

use crate::app::Portal;
use crate::app::services::connection;
use crate::config::{Host, Protocol};
use crate::message::{HostMessage, Message};
use crate::proxy;
use crate::proxy::ListedProxySession;
use crate::views::dialogs::host_dialog::HostDialogState;
use crate::views::dialogs::session_choice_dialog::{
    DetachedProxySessionChoice, LocalSessionChoice, SessionChoiceDialogState, SessionThumbnail,
};
use crate::views::toast::Toast;

/// Handle host management messages
pub fn handle_host(portal: &mut Portal, msg: HostMessage) -> Task<Message> {
    match msg {
        HostMessage::Connect(id) => {
            tracing::info!("Connect to host");
            if let Some(host) = portal.config.hosts.find_host(id).cloned() {
                return match host.protocol {
                    Protocol::Vnc => portal.connect_vnc_host(&host),
                    Protocol::Ssh => choose_or_connect_ssh_host(portal, &host),
                };
            }
            Task::none()
        }
        HostMessage::CreateNewSession(id) => {
            portal.dialogs.close();
            if let Some(host) = portal.config.hosts.find_host(id).cloned() {
                return match host.protocol {
                    Protocol::Vnc => portal.connect_vnc_host(&host),
                    Protocol::Ssh => portal.connect_to_host_new_session(&host),
                };
            }
            Task::none()
        }
        HostMessage::OpenExistingSession(session_id) => {
            portal.dialogs.close();
            portal.set_active_tab(session_id);
            Task::none()
        }
        HostMessage::OpenDetachedProxySession(session_id) => {
            let Some(choice) = portal
                .dialogs
                .session_choice()
                .and_then(|state| state.proxy_choice(session_id))
                .cloned()
            else {
                portal
                    .toast_manager
                    .push(Toast::error("Portal Hub session is no longer available"));
                portal.dialogs.close();
                return Task::none();
            };

            let host_id = portal.dialogs.session_choice().map(|state| state.host_id);
            let terminal_size = portal.terminal_initial_size();
            portal.dialogs.close();
            connection::proxy_resume_tasks(
                portal.prefs.portal_hub.clone(),
                choice.session,
                host_id,
                choice.display_name,
                terminal_size,
            )
        }
        HostMessage::DetachedProxySessionsLoaded { host_id, result } => {
            handle_detached_proxy_sessions_loaded(portal, host_id, result)
        }
        HostMessage::Add => {
            portal
                .dialogs
                .open_host(HostDialogState::new_host_with_proxy_default(
                    portal.prefs.portal_hub.default_for_new_ssh_hosts,
                ));
            Task::none()
        }
        HostMessage::Edit(id) => {
            if let Some(host) = portal.config.hosts.find_host(id) {
                portal.dialogs.open_host(HostDialogState::from_host(host));
            }
            Task::none()
        }
        HostMessage::Hover(id) => {
            portal.ui.hovered_host = id;
            Task::none()
        }
        HostMessage::DetailsOpen(id) => {
            portal.ui.host_details_sheet = Some(id);
            Task::none()
        }
        HostMessage::DetailsClose => {
            portal.ui.host_details_sheet = None;
            Task::none()
        }
        HostMessage::QuickConnect => {
            portal.dialogs.open_quick_connect();
            Task::none()
        }
        HostMessage::LocalTerminal => {
            tracing::info!("Spawning local terminal");
            portal.spawn_local_terminal()
        }
    }
}

fn choose_or_connect_ssh_host(portal: &mut Portal, host: &Host) -> Task<Message> {
    let local_sessions = local_session_choices(portal, host.id);
    let should_load_proxy = connection::should_use_portal_hub(&portal.prefs.portal_hub, host);

    if local_sessions.is_empty() && !should_load_proxy {
        return portal.connect_to_host(host);
    }

    portal
        .dialogs
        .open_session_choice(SessionChoiceDialogState::new(
            host.id,
            host.name.clone(),
            local_sessions,
            should_load_proxy,
        ));

    if should_load_proxy {
        let settings = portal.prefs.portal_hub.clone();
        let host_id = host.id;
        return Task::perform(
            async move { proxy::list_active_sessions(&settings).await },
            move |result| {
                Message::Host(HostMessage::DetachedProxySessionsLoaded { host_id, result })
            },
        );
    }

    Task::none()
}

fn handle_detached_proxy_sessions_loaded(
    portal: &mut Portal,
    host_id: Uuid,
    result: Result<Vec<ListedProxySession>, String>,
) -> Task<Message> {
    let Some(host) = portal.config.hosts.find_host(host_id).cloned() else {
        return Task::none();
    };
    let result = result.map(|sessions| {
        sessions
            .into_iter()
            .filter(|session| !portal.sessions.contains(session.session_id))
            .filter(|session| proxy_session_matches_host(session, &host))
            .map(|session| DetachedProxySessionChoice {
                thumbnail: SessionThumbnail::from_preview(host.name.clone(), &session.preview),
                session,
                display_name: host.name.clone(),
            })
            .collect::<Vec<_>>()
    });

    let mut connect_new = false;
    {
        let Some(state) = portal.dialogs.session_choice_mut() else {
            return Task::none();
        };

        if state.host_id != host_id {
            return Task::none();
        }

        state.proxy_loading = false;
        match result {
            Ok(proxy_sessions) => {
                state.proxy_error = None;
                state.proxy_sessions = proxy_sessions;
                connect_new = !state.has_choices();
            }
            Err(error) => {
                state.proxy_error = Some(format!("Could not load Portal Hub sessions: {}", error));
            }
        }
    }

    if connect_new {
        portal.dialogs.close();
        return portal.connect_to_host_new_session(&host);
    }

    Task::none()
}

fn local_session_choices(portal: &Portal, host_id: Uuid) -> Vec<LocalSessionChoice> {
    portal
        .sessions
        .sessions_for_host(host_id)
        .into_iter()
        .map(|(session_id, title, term)| LocalSessionChoice {
            session_id,
            title,
            thumbnail: SessionThumbnail::from_terminal(term),
        })
        .collect()
}

fn proxy_session_matches_host(session: &ListedProxySession, host: &Host) -> bool {
    session.target_host.trim() == host.hostname.trim()
        && session.target_port == host.port
        && session.target_user == host.effective_username()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use uuid::Uuid;

    use super::*;
    use crate::config::AuthMethod;

    fn ssh_host() -> Host {
        let now = Utc::now();
        Host {
            id: Uuid::new_v4(),
            name: "Pulse".to_string(),
            hostname: "192.0.2.6".to_string(),
            port: 22,
            username: "root".to_string(),
            auth: AuthMethod::Agent,
            protocol: Protocol::Ssh,
            vnc_port: None,
            vnc_password_id: None,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            portal_hub_enabled: true,
            group_id: None,
            notes: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        }
    }

    fn proxy_session(target_host: &str, target_port: u16, target_user: &str) -> ListedProxySession {
        let now = Utc::now();
        ListedProxySession {
            session_id: Uuid::new_v4(),
            target_host: target_host.to_string(),
            target_port,
            target_user: target_user.to_string(),
            created_at: now,
            updated_at: now,
            last_output_at: None,
            preview: Vec::new(),
            preview_truncated: false,
        }
    }

    #[test]
    fn proxy_session_matching_uses_host_endpoint_and_user() {
        let host = ssh_host();

        assert!(proxy_session_matches_host(
            &proxy_session("192.0.2.6", 22, "root"),
            &host
        ));
        assert!(!proxy_session_matches_host(
            &proxy_session("192.0.2.7", 22, "root"),
            &host
        ));
        assert!(!proxy_session_matches_host(
            &proxy_session("192.0.2.6", 2222, "root"),
            &host
        ));
        assert!(!proxy_session_matches_host(
            &proxy_session("192.0.2.6", 22, "john"),
            &host
        ));
    }
}
