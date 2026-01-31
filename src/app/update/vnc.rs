//! VNC session message handlers

use iced::Task;

use crate::app::managers::session_manager::VncActiveSession;
use crate::app::Portal;
use crate::message::{Message, VncMessage};
use crate::views::tabs::Tab;
use crate::views::toast::Toast;

/// Handle VNC session messages
pub fn handle_vnc(portal: &mut Portal, msg: VncMessage) -> Task<Message> {
    match msg {
        VncMessage::Connected {
            session_id,
            host_name,
            vnc_session,
            host_id,
            detected_os,
        } => {
            tracing::info!("VNC connected to {}", host_name);

            // Update host with detected OS and last_connected
            if let Some(host) = portal.config.hosts.find_host_mut(host_id) {
                if let Some(os) = detected_os {
                    host.detected_os = Some(os);
                }
                host.last_connected = Some(chrono::Utc::now());
                host.updated_at = chrono::Utc::now();
                if let Err(e) = portal.config.hosts.save() {
                    tracing::error!("Failed to save host config: {}", e);
                }
            }

            // Create history entry
            if let Some(host) = portal.config.hosts.find_host(host_id) {
                let entry = crate::config::HistoryEntry::new(
                    host.id,
                    host.name.clone(),
                    host.hostname.clone(),
                    host.username.clone(),
                    crate::config::SessionType::Ssh, // Reuse SSH type for now
                );
                portal.config.history.add_entry(entry);
                if let Err(e) = portal.config.history.save() {
                    tracing::error!("Failed to save history config: {}", e);
                }
            }

            // Store VNC session
            portal.vnc_sessions.insert(
                session_id,
                VncActiveSession {
                    session: vnc_session,
                    host_name: host_name.clone(),
                    session_start: std::time::Instant::now(),
                },
            );

            if portal.prefs.vnc_settings.remote_resize {
                if let Some((w, h)) = portal.vnc_target_size() {
                    if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                        vnc.session.try_request_desktop_size(w, h);
                    }
                }
            }

            // Create tab
            let tab = Tab::new_vnc(session_id, host_name, Some(host_id));
            portal.tabs.push(tab);
            portal.enter_vnc_view(session_id);

            Task::none()
        }
        VncMessage::RenderTick => {
            // The shader widget reads the framebuffer directly in draw().
            // This tick just triggers a UI refresh so the shader re-renders.
            Task::none()
        }
        VncMessage::KeyEvent {
            session_id,
            keysym,
            pressed,
        } => {
            if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                vnc.session.try_send_key(keysym, pressed);
                vnc.session.try_request_refresh();
            }
            Task::none()
        }
        VncMessage::Disconnected(session_id) => {
            tracing::info!("VNC disconnected: {}", session_id);
            portal.vnc_sessions.remove(&session_id);
            portal.close_tab(session_id);
            portal
                .toast_manager
                .push(Toast::success("VNC session disconnected"));
            Task::none()
        }
        VncMessage::Error(err) => {
            tracing::error!("VNC error: {}", err);
            portal
                .toast_manager
                .push(Toast::error(format!("VNC: {}", err)));
            Task::none()
        }
    }
}
