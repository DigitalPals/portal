//! Host management message handlers

use iced::Task;

use crate::app::Portal;
use crate::config::Protocol;
use crate::message::{HostMessage, Message};
use crate::views::dialogs::host_dialog::HostDialogState;

/// Handle host management messages
pub fn handle_host(portal: &mut Portal, msg: HostMessage) -> Task<Message> {
    match msg {
        HostMessage::Connect(id) => {
            tracing::info!("Connect to host");
            if let Some(host) = portal.config.hosts.find_host(id).cloned() {
                return match host.protocol {
                    Protocol::Vnc => portal.connect_vnc_host(&host),
                    Protocol::Ssh => portal.connect_to_host(&host),
                };
            }
            Task::none()
        }
        HostMessage::Add => {
            portal.dialogs.open_host(HostDialogState::new_host());
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
