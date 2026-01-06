//! Host management message handlers

use iced::Task;
use uuid::Uuid;

use crate::app::Portal;
use crate::config::{AuthMethod, Host};
use crate::message::{HostMessage, Message};
use crate::views::dialogs::host_dialog::HostDialogState;
use crate::views::toast::Toast;

/// Handle host management messages
pub fn handle_host(portal: &mut Portal, msg: HostMessage) -> Task<Message> {
    match msg {
        HostMessage::Connect(id) => {
            tracing::info!("Connect to host: {}", id);
            if let Some(host) = portal.hosts_config.find_host(id).cloned() {
                return portal.connect_to_host(&host);
            }
            Task::none()
        }
        HostMessage::Add => {
            portal.dialogs.open_host(HostDialogState::new_host());
            Task::none()
        }
        HostMessage::Edit(id) => {
            if let Some(host) = portal.hosts_config.find_host(id) {
                portal.dialogs.open_host(HostDialogState::from_host(host));
            }
            Task::none()
        }
        HostMessage::Hover(id) => {
            portal.hovered_host = id;
            Task::none()
        }
        HostMessage::QuickConnect => {
            // Parse search query as [ssh] [user@]hostname[:port]
            let query = portal.search_query.trim();
            if query.is_empty() {
                portal
                    .toast_manager
                    .push(Toast::warning("Enter a hostname to connect"));
                return Task::none();
            }

            // Strip optional "ssh " prefix
            let query = query.strip_prefix("ssh ").unwrap_or(query);

            // Parse user@hostname:port
            let (user_part, host_part) = if let Some(at_pos) = query.rfind('@') {
                (Some(&query[..at_pos]), &query[at_pos + 1..])
            } else {
                (None, query)
            };

            let (hostname, port) = if let Some(colon_pos) = host_part.rfind(':') {
                let port_str = &host_part[colon_pos + 1..];
                if let Ok(port) = port_str.parse::<u16>() {
                    (&host_part[..colon_pos], port)
                } else {
                    (host_part, 22)
                }
            } else {
                (host_part, 22)
            };

            // Get current username as default
            let username = user_part.map(|s| s.to_string()).unwrap_or_else(|| {
                std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "root".to_string())
            });

            let now = chrono::Utc::now();
            let temp_host = Host {
                id: Uuid::new_v4(),
                name: format!("{}@{}", username, hostname),
                hostname: hostname.to_string(),
                port,
                username,
                auth: AuthMethod::Agent,
                group_id: None,
                notes: None,
                tags: vec![],
                created_at: now,
                updated_at: now,
                detected_os: None,
                last_connected: None,
            };

            tracing::info!(
                "Quick connect to: {}@{}:{}",
                temp_host.username,
                temp_host.hostname,
                temp_host.port
            );
            portal.connect_to_host(&temp_host)
        }
        HostMessage::LocalTerminal => {
            tracing::info!("Spawning local terminal");
            portal.spawn_local_terminal()
        }
    }
}
