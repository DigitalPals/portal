use std::time::Duration;

use iced::Task;

use crate::app::services::connection;
use crate::app::{Portal, View};
use crate::message::{Message, ProxySessionsMessage};
use crate::proxy;
use crate::views::toast::Toast;

pub fn handle_proxy_sessions(portal: &mut Portal, msg: ProxySessionsMessage) -> Task<Message> {
    match msg {
        ProxySessionsMessage::RefreshDue(generation) => {
            if generation != portal.proxy_sessions.refresh_generation {
                return Task::none();
            }

            portal.update(Message::ProxySessions(ProxySessionsMessage::Refresh))
        }
        ProxySessionsMessage::Refresh => {
            if !matches!(portal.ui.active_view, View::ProxySessions)
                || !portal.prefs.portal_hub.is_configured()
                || portal.proxy_sessions.loading
            {
                return Task::none();
            }

            portal.proxy_sessions.start_loading();
            let settings = portal.prefs.portal_hub.clone();
            Task::perform(
                async move { proxy::list_active_sessions(&settings).await },
                |result| Message::ProxySessions(ProxySessionsMessage::Loaded(result)),
            )
        }
        ProxySessionsMessage::Loaded(result) => {
            match result {
                Ok(sessions) => portal
                    .proxy_sessions
                    .set_sessions(sessions, &portal.config.hosts),
                Err(error) => portal.proxy_sessions.set_error(error),
            }
            schedule_next_refresh(portal)
        }
        ProxySessionsMessage::Resume(session_id) => {
            if portal.sessions.contains(session_id) {
                portal.set_active_tab(session_id);
                return Task::none();
            }

            let Some(session) = portal.proxy_sessions.get(session_id) else {
                portal
                    .toast_manager
                    .push(Toast::error("Portal Hub session is no longer available"));
                return Task::none();
            };

            let listed_session = crate::proxy::ListedProxySession {
                session_id: session.session_id,
                target_host: session.target_host.clone(),
                target_port: session.target_port,
                target_user: session.target_user.clone(),
                created_at: session.created_at,
                updated_at: session.updated_at,
                last_output_at: session.last_output_at,
                preview: Vec::new(),
                preview_truncated: session.preview_truncated,
            };

            connection::proxy_resume_tasks(
                portal.prefs.portal_hub.clone(),
                listed_session,
                session.host_id,
                session.display_name.clone(),
                portal.terminal_initial_size(),
            )
        }
        ProxySessionsMessage::KillRequested(session_id) => {
            if portal.proxy_sessions.get(session_id).is_none() {
                portal
                    .toast_manager
                    .push(Toast::error("Portal Hub session is no longer available"));
                return Task::none();
            }

            portal.proxy_sessions.kill_requested = Some(session_id);
            Task::none()
        }
        ProxySessionsMessage::KillCanceled => {
            portal.proxy_sessions.kill_requested = None;
            Task::none()
        }
        ProxySessionsMessage::KillConfirmed(session_id) => {
            if portal.proxy_sessions.get(session_id).is_none() {
                portal.proxy_sessions.kill_requested = None;
                portal
                    .toast_manager
                    .push(Toast::error("Portal Hub session is no longer available"));
                return Task::none();
            }

            portal.proxy_sessions.start_action();
            let settings = portal.prefs.portal_hub.clone();
            Task::perform(
                async move { proxy::kill_session(&settings, session_id).await },
                move |result| {
                    Message::ProxySessions(ProxySessionsMessage::KillFinished(session_id, result))
                },
            )
        }
        ProxySessionsMessage::KillFinished(_session_id, result) => {
            portal.proxy_sessions.finish_action();
            match result {
                Ok(()) => {
                    portal
                        .toast_manager
                        .push(Toast::success("Killed Portal Hub session"));
                    portal.update(Message::ProxySessions(ProxySessionsMessage::Refresh))
                }
                Err(error) => {
                    portal
                        .toast_manager
                        .push(Toast::error(format!("Session kill failed: {}", error)));
                    Task::none()
                }
            }
        }
    }
}

fn schedule_next_refresh(portal: &Portal) -> Task<Message> {
    if !matches!(portal.ui.active_view, View::ProxySessions)
        || !portal.prefs.portal_hub.is_configured()
    {
        return Task::none();
    }

    let generation = portal.proxy_sessions.refresh_generation;
    Task::perform(
        async move {
            tokio::time::sleep(Duration::from_secs(3)).await;
            generation
        },
        |generation| Message::ProxySessions(ProxySessionsMessage::RefreshDue(generation)),
    )
}
