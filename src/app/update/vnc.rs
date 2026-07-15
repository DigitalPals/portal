//! VNC session message handlers

use iced::Task;

use crate::app::Portal;
use crate::app::managers::session_manager::VncActiveSession;
use crate::config::settings::VncScalingMode;
use crate::fs_utils;
use crate::message::{Message, VncMessage};
use crate::views::tabs::{Tab, promote_connection_tab};
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
            if portal.vnc_sessions.contains_key(&session_id) {
                tracing::warn!(
                    "Ignoring duplicate VNC connection for session {}",
                    session_id
                );
                vnc_session.disconnect();
                return Task::none();
            }
            let draft_tab_id = portal.pending_connect_draft_for(session_id);
            if !portal.finish_pending_connect_for(session_id) {
                tracing::warn!("Ignoring stale VNC connection for session {}", session_id);
                vnc_session.disconnect();
                return Task::none();
            }

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

            // Human-readable tunnel description for the toolbar ("via ...")
            let via = portal
                .config
                .hosts
                .find_host(host_id)
                .and_then(|host| host.vnc_via_ssh_host_id)
                .and_then(|ssh_id| portal.config.hosts.find_host(ssh_id))
                .map(|ssh_host| ssh_host.name.clone());

            // Store VNC session
            let now = std::time::Instant::now();
            portal.vnc_sessions.insert(
                session_id,
                VncActiveSession {
                    session: vnc_session,
                    host_name: host_name.clone(),
                    via,
                    session_start: now,
                    frame_count: 0,
                    fps_last_check: now,
                    current_fps: 0.0,
                    first_frame_received: false,
                    status_text: "connecting".to_string(),
                    fullscreen: false,
                    keyboard_passthrough: false,
                    view_only: portal.prefs.vnc_settings.view_only,
                    show_cursor_dot: portal.prefs.vnc_settings.show_cursor_dot,
                    show_stats_overlay: portal.prefs.vnc_settings.show_stats_overlay,
                    history_entry_id,
                },
            );

            if portal.prefs.vnc_settings.remote_resize
                && let Some((w, h)) = portal.vnc_target_size()
                && let Some(vnc) = portal.vnc_sessions.get(&session_id)
            {
                vnc.session.try_request_desktop_size(w, h);
            }

            // Create tab
            let tab = Tab::new_vnc(session_id, host_name, Some(host_id));
            if !draft_tab_id.is_some_and(|draft_tab_id| {
                promote_connection_tab(&mut portal.tabs, draft_tab_id, tab.clone())
            }) {
                portal.tabs.push(tab);
            }
            portal.enter_vnc_view(session_id);

            Task::none()
        }
        VncMessage::FrameReady(session_id) => {
            // New framebuffer data arrived (coalesced by the session's frame
            // notifier). Receiving this message makes Iced rebuild the view,
            // which uploads the dirty framebuffer region and repaints.
            let max_fps = portal.prefs.vnc_settings.effective_refresh_fps() as f32;
            if let Some(vnc) = portal.vnc_sessions.get_mut(&session_id) {
                vnc.frame_count += 1;
                let elapsed = vnc.fps_last_check.elapsed();
                if elapsed.as_secs_f32() >= 1.0 {
                    vnc.frame_count = 0;
                    vnc.fps_last_check = std::time::Instant::now();
                }
                refresh_session_status(vnc, max_fps);
            }
            Task::none()
        }
        VncMessage::StatusTick => {
            // Low-frequency housekeeping for the active VNC session: keeps the
            // idle counter and FPS decay updating while no frames arrive.
            let max_fps = portal.prefs.vnc_settings.effective_refresh_fps() as f32;
            if let crate::app::View::VncViewer(session_id) = portal.ui.active_view
                && let Some(vnc) = portal.vnc_sessions.get_mut(&session_id)
            {
                refresh_session_status(vnc, max_fps);
            }
            Task::none()
        }
        VncMessage::KeyEvent {
            session_id,
            keysym,
            pressed,
        } => {
            if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                if vnc.view_only {
                    return Task::none();
                }
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
            let Some(vnc) = portal.vnc_sessions.remove(&session_id) else {
                tracing::debug!("Ignoring stale VNC disconnect for session {}", session_id);
                return Task::none();
            };
            portal
                .config
                .history
                .mark_disconnected(vnc.history_entry_id);
            if let Err(e) = portal.config.history.save() {
                tracing::error!("Failed to save history config: {}", e);
            }
            portal.close_tab(session_id);
            portal
                .toast_manager
                .push(Toast::success("VNC session disconnected"));
            Task::none()
        }
        VncMessage::ConnectFailed { session_id, error } => {
            tracing::error!("VNC connection failed: {}", error);
            if !portal.finish_pending_connect_for(session_id) {
                tracing::warn!(
                    "Ignoring stale VNC connection failure for session {}: {}",
                    session_id,
                    error
                );
                return Task::none();
            }
            portal
                .toast_manager
                .push(Toast::error(format!("VNC: {}", error)));
            Task::none()
        }
        VncMessage::Error(err) => {
            tracing::error!("VNC error: {}", err);
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
            if portal.prefs.vnc_settings.clipboard_sharing
                && let Some(vnc) = portal.vnc_sessions.get(&session_id)
            {
                let session = vnc.session.clone();
                return Task::perform(async move { session.send_clipboard(text).await }, |_| {
                    Message::Noop
                });
            }
            Task::none()
        }
        VncMessage::SendSpecialKeys {
            session_id,
            keysyms,
        } => {
            if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                if vnc.view_only {
                    return Task::none();
                }
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
            if let crate::app::View::VncViewer(session_id) = portal.ui.active_view
                && let Some(vnc) = portal.vnc_sessions.get_mut(&session_id)
            {
                vnc.fullscreen = !vnc.fullscreen;
                if vnc.fullscreen {
                    portal.ui.sidebar_state_before_session = Some(portal.ui.sidebar_state);
                    portal.ui.sidebar_state = crate::app::SidebarState::Hidden;
                } else {
                    portal.restore_sidebar_after_session();
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
                        let host_name = safe_filename_component(&host_name, "session");
                        let filename = format!("vnc_{}_{}.png", host_name, timestamp);
                        let dir = directories::UserDirs::new()
                            .and_then(|d| d.picture_dir().map(|p| p.to_path_buf()))
                            .unwrap_or_else(|| {
                                directories::UserDirs::new()
                                    .map(|d| d.home_dir().to_path_buf())
                                    .unwrap_or_else(|| std::path::PathBuf::from("."))
                            });
                        tokio::fs::create_dir_all(&dir)
                            .await
                            .map_err(|e| format!("Failed to create screenshot directory: {}", e))?;
                        let path = save_rgba_png_unique(&dir, &filename, &rgba, width, height)?;

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
                if vnc.view_only {
                    return Task::none();
                }
                vnc.session.try_send_mouse(x, y, buttons);
            }
            Task::none()
        }
        VncMessage::ToggleKeyboardPassthrough => {
            if let crate::app::View::VncViewer(session_id) = portal.ui.active_view
                && let Some(vnc) = portal.vnc_sessions.get_mut(&session_id)
            {
                vnc.session.release_all_keys();
                vnc.keyboard_passthrough = !vnc.keyboard_passthrough;
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
            super::ui::settings::queue_portal_hub_local_sync(portal);
            Task::none()
        }
        VncMessage::ManualRefresh(session_id) => {
            if let Some(vnc) = portal.vnc_sessions.get(&session_id) {
                vnc.session.try_request_full_refresh();
            }
            Task::none()
        }
        VncMessage::ToggleViewOnly => {
            if let crate::app::View::VncViewer(session_id) = portal.ui.active_view
                && let Some(vnc) = portal.vnc_sessions.get_mut(&session_id)
            {
                vnc.session.release_all_keys();
                vnc.view_only = !vnc.view_only;
                vnc.keyboard_passthrough = false;
            }
            Task::none()
        }
        VncMessage::ToggleCursorDot => {
            if let crate::app::View::VncViewer(session_id) = portal.ui.active_view
                && let Some(vnc) = portal.vnc_sessions.get_mut(&session_id)
            {
                vnc.show_cursor_dot = !vnc.show_cursor_dot;
            }
            Task::none()
        }
        VncMessage::ToggleStatsOverlay => {
            if let crate::app::View::VncViewer(session_id) = portal.ui.active_view
                && let Some(vnc) = portal.vnc_sessions.get_mut(&session_id)
            {
                vnc.show_stats_overlay = !vnc.show_stats_overlay;
            }
            Task::none()
        }
        VncMessage::CycleQualityPreset => {
            use crate::config::settings::VncQualityPreset;
            portal.prefs.vnc_settings.quality_preset =
                match portal.prefs.vnc_settings.quality_preset {
                    VncQualityPreset::Auto => VncQualityPreset::Speed,
                    VncQualityPreset::Speed => VncQualityPreset::Balanced,
                    VncQualityPreset::Balanced => VncQualityPreset::Quality,
                    VncQualityPreset::Quality => VncQualityPreset::Lossless,
                    VncQualityPreset::Lossless => VncQualityPreset::Auto,
                };
            portal.save_settings();
            super::ui::settings::queue_portal_hub_local_sync(portal);
            portal.toast_manager.push(Toast::success(format!(
                "VNC quality applies to new sessions: {}",
                portal.prefs.vnc_settings.quality_preset.label()
            )));
            Task::none()
        }
        VncMessage::Bell(session_id) => {
            if portal.vnc_sessions.contains_key(&session_id) {
                portal.toast_manager.push(Toast::success("Remote bell"));
            }
            Task::none()
        }
    }
}

/// Refresh the toolbar status text and FPS estimate from the session stats.
fn refresh_session_status(vnc: &mut VncActiveSession, max_fps: f32) {
    let stats = vnc.session.stats_snapshot();
    vnc.first_frame_received = stats.first_frame_received;
    vnc.current_fps = if let Some(last_update_at) = stats.last_update_at {
        if last_update_at.elapsed().as_millis() <= 1_000 {
            stats.update_fps.min(max_fps)
        } else {
            0.0
        }
    } else {
        0.0
    };
    vnc.status_text = if let Some(last_update_at) = stats.last_update_at {
        let age_ms = last_update_at.elapsed().as_millis();
        if age_ms > 2_000 {
            format!("idle {}s", age_ms / 1000)
        } else {
            "live".to_string()
        }
    } else {
        "waiting for frame".to_string()
    };
}

fn safe_filename_component(value: &str, fallback: &str) -> String {
    let mut safe = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
            safe.push(ch);
        } else if ch.is_whitespace() {
            safe.push('_');
        }
    }

    let safe = safe.trim_matches(['.', '_', '-']);
    if safe.is_empty() || safe == ".." {
        fallback.to_string()
    } else {
        safe.chars().take(80).collect()
    }
}

fn save_rgba_png_new(
    path: &std::path::Path,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<(), String> {
    use image::ImageEncoder;

    let expected_len = usize::try_from(width)
        .ok()
        .and_then(|width| {
            usize::try_from(height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| "Screenshot dimensions are too large".to_string())?;
    if rgba.len() != expected_len {
        return Err(format!(
            "Invalid screenshot buffer length: expected {} bytes, got {}",
            expected_len,
            rgba.len()
        ));
    }

    let mut file = fs_utils::create_new_regular_file_no_follow(path)
        .map_err(|e| format!("Failed to create screenshot {}: {}", path.display(), e))?;
    let encoder = image::codecs::png::PngEncoder::new(&mut file);
    if let Err(error) = encoder.write_image(rgba, width, height, image::ColorType::Rgba8.into()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(format!("Failed to save screenshot: {}", error));
    }
    file.sync_all()
        .map_err(|e| format!("Failed to sync screenshot {}: {}", path.display(), e))?;
    fs_utils::sync_parent_dir(path);
    Ok(())
}

fn save_rgba_png_unique(
    dir: &std::path::Path,
    filename: &str,
    rgba: &[u8],
    width: u32,
    height: u32,
) -> Result<std::path::PathBuf, String> {
    for index in 0..1000 {
        let path = screenshot_candidate_path(dir, filename, index);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => continue,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                save_rgba_png_new(&path, rgba, width, height)?;
                return Ok(path);
            }
            Err(error) => {
                return Err(format!(
                    "Failed to inspect screenshot target {}: {}",
                    path.display(),
                    error
                ));
            }
        }
    }

    Err(format!(
        "No available screenshot filename for {}",
        dir.join(filename).display()
    ))
}

fn screenshot_candidate_path(
    dir: &std::path::Path,
    filename: &str,
    index: usize,
) -> std::path::PathBuf {
    if index == 0 {
        return dir.join(filename);
    }

    let path = std::path::Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(filename);

    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) => dir.join(format!("{}_{}.{}", stem, index, extension)),
        None => dir.join(format!("{}_{}", filename, index)),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        safe_filename_component, save_rgba_png_new, save_rgba_png_unique, screenshot_candidate_path,
    };

    #[test]
    fn safe_filename_component_strips_path_separators() {
        assert_eq!(
            safe_filename_component("../prod/web:5900", "session"),
            "prodweb5900"
        );
    }

    #[test]
    fn safe_filename_component_uses_fallback_for_empty_names() {
        assert_eq!(safe_filename_component("../", "session"), "session");
    }

    #[test]
    fn save_rgba_png_new_rejects_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("screenshot.png");
        std::fs::write(&path, "original").unwrap();

        let error = save_rgba_png_new(&path, &[0, 0, 0, 255], 1, 1)
            .expect_err("existing screenshot target should not be overwritten");

        assert!(error.contains("Failed to create screenshot"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn save_rgba_png_new_rejects_symlink_without_writing_target() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.png");
        let link = temp.path().join("screenshot.png");
        std::fs::write(&target, "original").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = save_rgba_png_new(&link, &[0, 0, 0, 255], 1, 1)
            .expect_err("screenshot should not be saved through symlink");

        assert!(error.contains("Failed to create screenshot"));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "original");
    }

    #[test]
    fn save_rgba_png_new_writes_png() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("screenshot.png");

        save_rgba_png_new(&path, &[0, 0, 0, 255], 1, 1).unwrap();

        let bytes = std::fs::read(path).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn save_rgba_png_new_removes_file_on_encode_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("screenshot.png");

        let error = save_rgba_png_new(&path, &[0, 0, 0], 1, 1)
            .expect_err("invalid RGBA buffer should fail");

        assert!(error.contains("Invalid screenshot buffer length"));
        assert!(!path.exists());
    }

    #[test]
    fn screenshot_candidate_path_adds_index_before_extension() {
        let temp = tempfile::tempdir().unwrap();

        assert_eq!(
            screenshot_candidate_path(temp.path(), "vnc_host.png", 2),
            temp.path().join("vnc_host_2.png")
        );
    }

    #[test]
    fn save_rgba_png_unique_skips_existing_file() {
        let temp = tempfile::tempdir().unwrap();
        let existing = temp.path().join("screenshot.png");
        std::fs::write(&existing, "original").unwrap();

        let path =
            save_rgba_png_unique(temp.path(), "screenshot.png", &[0, 0, 0, 255], 1, 1).unwrap();

        assert_eq!(path, temp.path().join("screenshot_1.png"));
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "original");
        assert!(
            std::fs::read(path)
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_rgba_png_unique_skips_socket_target() {
        let temp = tempfile::tempdir().unwrap();
        let socket = temp.path().join("screenshot.png");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();

        let path =
            save_rgba_png_unique(temp.path(), "screenshot.png", &[0, 0, 0, 255], 1, 1).unwrap();

        assert_eq!(path, temp.path().join("screenshot_1.png"));
        assert!(
            std::fs::read(path)
                .unwrap()
                .starts_with(b"\x89PNG\r\n\x1a\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_rgba_png_unique_skips_symlink_without_writing_target() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.png");
        let link = temp.path().join("screenshot.png");
        std::fs::write(&target, "original").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let path =
            save_rgba_png_unique(temp.path(), "screenshot.png", &[0, 0, 0, 255], 1, 1).unwrap();

        assert_eq!(path, temp.path().join("screenshot_1.png"));
        assert_eq!(std::fs::read_to_string(target).unwrap(), "original");
    }
}
