//! History management message handlers

use iced::Task;

use crate::app::Portal;
use crate::message::{HistoryMessage, Message};

/// Handle history messages
pub fn handle_history(portal: &mut Portal, msg: HistoryMessage) -> Task<Message> {
    match msg {
        HistoryMessage::Clear => {
            portal.config.history.clear();
            if let Err(e) = portal.config.history.save() {
                tracing::error!("Failed to save history: {}", e);
            }
            Task::none()
        }
        HistoryMessage::Reconnect(entry_id) => {
            if let Some(entry) = portal.config.history.find_entry(entry_id) {
                let host_id = entry.host_id;
                if let Some(host) = portal.config.hosts.find_host(host_id).cloned() {
                    return portal.connect_to_host(&host);
                }
            }
            Task::none()
        }
    }
}
