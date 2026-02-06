//! VNC session message handlers

use iced::Task;

use crate::app::Portal;
use crate::app::managers::session_manager::VncActiveSession;
use crate::config::settings::VncScalingMode;
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
            portal.dialogs.close_connecting();

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
            let mut history_entry_id = uuid::Uuid::nil();
            if let Some(host) = portal.config.hosts.find_host(host_id) {
                let entry = crate::config::HistoryEntry::new(
                    host.id,
                    host.name.clone(),
                    host.hostname.clone(),
                    host.effective_username(),
                    crate::config::SessionType::Vnc,
                );
                history_entry_id = entry.id;
                portal.config.history.add_entry(entry);
                if let Err(e) = portal.config.history.save() {
                    tracing::error!("Failed to save history config: {}", e);
                }
            }

            // Store VNC session
            let now = std::time::Instant::now();
            portal.vnc_sessions.insert(
                session_id,
                VncActiveSession {
                    session: vnc_session,
                    host_name: host_name.clone(),
                    session_start: now,
                    frame_count: 0,
                    fps_last_check: now,
                    current_fps: 0.0,
                    fullscreen: false,
                    keyboard_passthrough: false,
                    quality_level: crate::message::QualityLevel::High,
                    monitors: Vec::new(),
                    selected_monitor: None,
                    history_entry_id,
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
            // Update FPS counter for the active VNC session
            if let crate::app::View::VncViewer(session_id) = portal.ui.active_view {
                if let Some(vnc) = portal.vnc_sessions.get_mut(&session_id) {
                    vnc.frame_count += 1;
                    let elapsed = vnc.fps_last_check.elapsed();
                    if elapsed.as_secs_f32() >= 1.0 {
                        vnc.current_fps = vnc.frame_count as f32 / elapsed.as_secs_f32();
                        vnc.frame_count = 0;
                        vnc.fps_last_check = std::time::Instant::now();
                    }
                }
            }
            Task::none()
        }
        VncMessage::KeyEvent {
            session_id,
            keysym,
            pressed,
        } => {
            if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                tracing::debug!(
                    "VNC key event: keysym=0x{:04X} pressed={} char={:?}",
                    keysym,
                    pressed,
                    char::from_u32(keysym).filter(|c| c.is_ascii_graphic())
                );
                vnc.session.try_send_key(keysym, pressed);
                vnc.session.try_request_refresh();
            }
            Task::none()
        }
        VncMessage::Disconnected(session_id) => {
            tracing::info!("VNC disconnected: {}", session_id);
            if let Some(vnc) = portal.vnc_sessions.remove(&session_id) {
                portal
                    .config
                    .history
                    .mark_disconnected(vnc.history_entry_id);
                if let Err(e) = portal.config.history.save() {
                    tracing::error!("Failed to save history config: {}", e);
                }
            }
            portal.close_tab(session_id);
            portal
                .toast_manager
                .push(Toast::success("VNC session disconnected"));
            Task::none()
        }
        VncMessage::Error(err) => {
            tracing::error!("VNC error: {}", err);
            portal.dialogs.close_connecting();
            portal
                .toast_manager
                .push(Toast::error(format!("VNC: {}", err)));
            Task::none()
        }
        VncMessage::ClipboardReceived(_session_id, text) => {
            if portal.prefs.vnc_settings.clipboard_sharing {
                // Write to system clipboard via iced
                return iced::clipboard::write(text);
            }
            Task::none()
        }
        VncMessage::ClipboardSend(session_id, text) => {
            if portal.prefs.vnc_settings.clipboard_sharing {
                if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                    let session = vnc.session.clone();
                    return Task::perform(
                        async move { session.send_clipboard(text).await },
                        |_| Message::Noop,
                    );
                }
            }
            Task::none()
        }
        VncMessage::SendSpecialKeys {
            session_id,
            keysyms,
        } => {
            if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                // Press all keys, then release in reverse order
                for &keysym in &keysyms {
                    vnc.session.try_send_key(keysym, true);
                }
                for &keysym in keysyms.iter().rev() {
                    vnc.session.try_send_key(keysym, false);
                }
                vnc.session.try_request_refresh();
            }
            Task::none()
        }
        VncMessage::ToggleFullscreen => {
            if let crate::app::View::VncViewer(session_id) = portal.ui.active_view {
                if let Some(vnc) = portal.vnc_sessions.get_mut(&session_id) {
                    vnc.fullscreen = !vnc.fullscreen;
                    if vnc.fullscreen {
                        portal.ui.sidebar_state_before_session = Some(portal.ui.sidebar_state);
                        portal.ui.sidebar_state = crate::app::SidebarState::Hidden;
                    } else {
                        portal.restore_sidebar_after_session();
                    }
                }
            }
            Task::none()
        }
        VncMessage::CaptureScreenshot(session_id) => {
            if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                let fb = vnc.session.framebuffer.clone();
                let host_name = vnc.host_name.clone();
                return Task::perform(
                    async move {
                        let (width, height, pixels) = {
                            let fb = fb.lock();
                            (fb.width, fb.height, fb.pixels.clone())
                        };
                        if width == 0 || height == 0 {
                            return Err("Empty framebuffer".to_string());
                        }
                        // Convert BGRA to RGBA for PNG encoding
                        let mut rgba = pixels;
                        for chunk in rgba.chunks_exact_mut(4) {
                            chunk.swap(0, 2);
                        }

                        // Save to user's pictures directory or home
                        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                        let filename = format!("vnc_{}_{}.png", host_name, timestamp);
                        let dir = directories::UserDirs::new()
                            .and_then(|d| d.picture_dir().map(|p| p.to_path_buf()))
                            .unwrap_or_else(|| {
                                directories::UserDirs::new()
                                    .map(|d| d.home_dir().to_path_buf())
                                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                            });
                        let path = dir.join(&filename);

                        image::save_buffer(&path, &rgba, width, height, image::ColorType::Rgba8)
                            .map_err(|e| format!("Failed to save screenshot: {}", e))?;

                        Ok(path.display().to_string())
                    },
                    |result| match result {
                        Ok(path) => Message::Vnc(VncMessage::ScreenshotSaved(path)),
                        Err(e) => Message::Vnc(VncMessage::Error(e)),
                    },
                );
            }
            Task::none()
        }
        VncMessage::ScreenshotSaved(path) => {
            portal
                .toast_manager
                .push(Toast::success(format!("Screenshot saved: {}", path)));
            Task::none()
        }
        VncMessage::MouseEvent {
            session_id,
            x,
            y,
            buttons,
        } => {
            if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                vnc.session.try_send_mouse(x, y, buttons);
            }
            Task::none()
        }
        VncMessage::ToggleKeyboardPassthrough => {
            if let crate::app::View::VncViewer(session_id) = portal.ui.active_view {
                if let Some(vnc) = portal.vnc_sessions.get_mut(&session_id) {
                    vnc.keyboard_passthrough = !vnc.keyboard_passthrough;
                }
            }
            Task::none()
        }
        VncMessage::QualityChanged(session_id, level) => {
            if let Some(vnc) = portal.vnc_sessions.get_mut(&session_id) {
                vnc.quality_level = level;
            }
            Task::none()
        }
        VncMessage::MonitorsDiscovered(session_id, screens) => {
            if let Some(vnc) = portal.vnc_sessions.get_mut(&session_id) {
                vnc.monitors = screens;
            }
            Task::none()
        }
        VncMessage::SelectMonitor(session_id, monitor_idx) => {
            if let Some(vnc) = portal.vnc_sessions.get_mut(&session_id) {
                vnc.selected_monitor = monitor_idx;
            }
            Task::none()
        }
        VncMessage::CycleScalingMode => {
            portal.prefs.vnc_settings.scaling_mode = match portal.prefs.vnc_settings.scaling_mode {
                VncScalingMode::Fit => VncScalingMode::Actual,
                VncScalingMode::Actual => VncScalingMode::Stretch,
                VncScalingMode::Stretch => VncScalingMode::Fit,
            };
            portal.save_settings();
            Task::none()
        }
    }
}
