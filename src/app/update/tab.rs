//! Tab management message handlers

use iced::Task;

use crate::app::{Portal, View};
use crate::message::{Message, TabMessage};

/// Handle tab management messages
pub fn handle_tab(portal: &mut Portal, msg: TabMessage) -> Task<Message> {
    match msg {
        TabMessage::Select(tab_id) => {
            tracing::info!("Tab selected: {}", tab_id);
            portal.set_active_tab(tab_id);
            Task::none()
        }
        TabMessage::Close(tab_id) => {
            tracing::info!("Tab closed: {}", tab_id);
            portal.close_tab(tab_id);
            Task::none()
        }
        TabMessage::New => {
            tracing::info!("New tab requested");
            portal.active_view = View::HostGrid;
            // Restore sidebar state if returning from terminal
            if let Some(saved_state) = portal.sidebar_state_before_session.take() {
                portal.sidebar_state = saved_state;
            }
            Task::none()
        }
        TabMessage::Hover(id) => {
            portal.hovered_tab = id;
            Task::none()
        }
    }
}
