//! History management message handlers

use iced::Task;

use crate::app::Portal;
use crate::message::{HistoryMessage, Message};

/// Handle history messages
pub fn handle_history(portal: &mut Portal, msg: HistoryMessage) -> Task<Message> {
    match msg {
        HistoryMessage::Clear => {
            portal.history_config.clear();
            if let Err(e) = portal.history_config.save() {
                tracing::error!("Failed to save history: {}", e);
            }
            Task::none()
        }
        HistoryMessage::Reconnect(entry_id) => {
            if let Some(entry) = portal.history_config.find_entry(entry_id) {
                let host_id = entry.host_id;
                if let Some(host) = portal.hosts_config.find_host(host_id).cloned() {
                    return portal.connect_to_host(&host);
                }
            }
            Task::none()
        }
    }
}
