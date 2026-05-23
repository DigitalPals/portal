//! Agent notification center message handlers.

use iced::Task;

use crate::app::Portal;
use crate::message::{AgentNotificationMessage, Message};
use crate::views::toast::Toast;

pub fn handle_notification(portal: &mut Portal, msg: AgentNotificationMessage) -> Task<Message> {
    match msg {
        AgentNotificationMessage::Jump(id) => {
            let Some((session_id, host_name)) = portal
                .config
                .agent_notifications
                .find_entry(id)
                .map(|entry| (entry.session_id, entry.host_name.clone()))
            else {
                return Task::none();
            };

            let changed = portal.config.agent_notifications.mark_read(id);
            if changed {
                portal.save_agent_notifications();
            }

            if portal.sessions.contains(session_id) {
                portal.set_active_tab(session_id);
            } else {
                portal
                    .toast_manager
                    .push(Toast::warning(format!("{} is no longer open", host_name)));
            }
            Task::none()
        }
        AgentNotificationMessage::JumpLatestUnread => {
            let Some(id) = portal
                .config
                .agent_notifications
                .latest_unread()
                .map(|entry| entry.id)
            else {
                portal
                    .toast_manager
                    .push(Toast::success("No unread agent notifications"));
                return Task::none();
            };

            handle_notification(portal, AgentNotificationMessage::Jump(id))
        }
        AgentNotificationMessage::MarkRead(id) => {
            let session_id = portal
                .config
                .agent_notifications
                .find_entry(id)
                .map(|entry| entry.session_id);
            if portal.config.agent_notifications.mark_read(id) {
                if let Some(session_id) = session_id {
                    portal.sync_tab_attention_from_notifications(session_id);
                }
                portal.save_agent_notifications();
            }
            Task::none()
        }
        AgentNotificationMessage::MarkUnread(id) => {
            if portal.config.agent_notifications.mark_unread(id) {
                if let Some(entry) = portal.config.agent_notifications.find_entry(id) {
                    portal.mark_terminal_attention(entry.session_id);
                }
                portal.save_agent_notifications();
            }
            Task::none()
        }
        AgentNotificationMessage::MarkAllRead => {
            if portal.config.agent_notifications.mark_all_read() {
                let session_ids: Vec<_> = portal.tabs.iter().map(|tab| tab.id).collect();
                for session_id in session_ids {
                    portal.sync_tab_attention_from_notifications(session_id);
                }
                portal.save_agent_notifications();
            }
            Task::none()
        }
        AgentNotificationMessage::Clear(id) => {
            let session_id = portal
                .config
                .agent_notifications
                .find_entry(id)
                .map(|entry| entry.session_id);
            if portal.config.agent_notifications.clear(id) {
                if let Some(session_id) = session_id {
                    portal.sync_tab_attention_from_notifications(session_id);
                }
                portal.save_agent_notifications();
            }
            Task::none()
        }
        AgentNotificationMessage::ClearRead => {
            if portal.config.agent_notifications.clear_read() {
                portal.save_agent_notifications();
            }
            Task::none()
        }
        AgentNotificationMessage::ClearAll => {
            if portal.config.agent_notifications.clear_all() {
                let session_ids: Vec<_> = portal.tabs.iter().map(|tab| tab.id).collect();
                for session_id in session_ids {
                    portal.sync_tab_attention_from_notifications(session_id);
                }
                portal.save_agent_notifications();
            }
            Task::none()
        }
    }
}
