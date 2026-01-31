//! Tab management message handlers

use iced::Task;

use crate::app::Portal;
use crate::message::{Message, TabMessage};

/// Handle tab management messages
pub fn handle_tab(portal: &mut Portal, msg: TabMessage) -> Task<Message> {
    match msg {
        TabMessage::Select(tab_id) => {
            tracing::info!("Tab selected");
            portal.set_active_tab(tab_id);
            Task::none()
        }
        TabMessage::Close(tab_id) => {
            tracing::info!("Tab closed");
            portal.close_tab(tab_id);
            Task::none()
        }
        TabMessage::New => {
            tracing::info!("New tab requested");
            portal.restore_sidebar_after_session();
            portal.enter_host_grid();
            Task::none()
        }
        TabMessage::Hover(id) => {
            portal.ui.hovered_tab = id;
            Task::none()
        }
    }
}
