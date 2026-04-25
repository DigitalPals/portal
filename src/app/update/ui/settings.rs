use iced::Task;

use crate::app::Portal;
use crate::app::services;
use crate::config::settings::{TERMINAL_SCROLL_SPEED_MAX, TERMINAL_SCROLL_SPEED_MIN};
use crate::message::{Message, UiMessage};

pub(super) fn handle_settings_message(portal: &mut Portal, msg: UiMessage) -> Task<Message> {
    match msg {
        UiMessage::ThemeChange(theme_id) => {
            portal.prefs.theme_id = theme_id;
            portal.save_settings();
        }
        UiMessage::FontChange(font) => {
            tracing::info!("Font changed");
            portal.prefs.terminal_font = font;
            portal.save_settings();
        }
        UiMessage::FontSizeChange(size) => {
            portal.prefs.terminal_font_size = size;
            portal.save_settings();
        }
        UiMessage::TerminalScrollSpeedChange(speed) => {
            portal.prefs.terminal_scroll_speed =
                speed.clamp(TERMINAL_SCROLL_SPEED_MIN, TERMINAL_SCROLL_SPEED_MAX);
            portal.save_settings();
        }
        UiMessage::UiScaleChange(scale) => {
            portal.prefs.ui_scale_override = Some(scale.clamp(0.8, 1.5));
            portal.save_settings();
        }
        UiMessage::UiScaleReset => {
            portal.prefs.ui_scale_override = None;
            portal.save_settings();
        }
        UiMessage::SnippetHistoryEnabled(enabled) => {
            portal.config.snippet_history.enabled = enabled;
            portal.save_snippet_history();
        }
        UiMessage::SnippetHistoryStoreCommand(store_command) => {
            portal.config.snippet_history.store_command = store_command;
            portal.save_snippet_history();
        }
        UiMessage::SnippetHistoryStoreOutput(store_output) => {
            portal.config.snippet_history.store_output = store_output;
            portal.save_snippet_history();
        }
        UiMessage::SnippetHistoryRedactOutput(redact_output) => {
            portal.config.snippet_history.redact_output = redact_output;
            portal.save_snippet_history();
        }
        UiMessage::SessionLoggingEnabled(enabled) => {
            portal.prefs.session_logging_enabled = enabled;
            portal.save_settings();
        }
        UiMessage::AllowAgentForwarding(enabled) => {
            portal.prefs.allow_agent_forwarding = enabled;
            portal.save_settings();
        }
        UiMessage::AutoReconnectEnabled(enabled) => {
            portal.prefs.auto_reconnect = enabled;
            portal.save_settings();
        }
        UiMessage::ReconnectMaxAttemptsChanged(attempts) => {
            portal.prefs.reconnect_max_attempts = attempts.clamp(1, 20);
            portal.save_settings();
        }
        UiMessage::ReconnectBaseDelayChanged(delay_ms) => {
            let base_delay = delay_ms.clamp(500, 10_000);
            portal.prefs.reconnect_base_delay_ms = base_delay;
            if portal.prefs.reconnect_max_delay_ms < base_delay {
                portal.prefs.reconnect_max_delay_ms = base_delay;
            }
            portal.save_settings();
        }
        UiMessage::ReconnectMaxDelayChanged(delay_ms) => {
            portal.prefs.reconnect_max_delay_ms =
                delay_ms.clamp(portal.prefs.reconnect_base_delay_ms.max(500), 120_000);
            portal.save_settings();
        }
        UiMessage::CredentialTimeoutChange(timeout_seconds) => {
            let clamped = timeout_seconds.min(3600);
            portal.prefs.credential_timeout = clamped;
            portal.save_settings();
            services::connection::init_passphrase_cache(clamped);
        }
        UiMessage::SecurityAuditLoggingEnabled(enabled) => {
            portal.prefs.security_audit_enabled = enabled;

            if enabled {
                if portal.prefs.security_audit_dir.is_none() {
                    portal.prefs.security_audit_dir = crate::config::paths::config_dir()
                        .map(|dir| dir.join("logs").join("security"));
                }

                portal.save_settings();
                let audit_path = portal.prefs.security_audit_dir.as_ref().map(|dir| {
                    let _ = std::fs::create_dir_all(dir);
                    dir.join("audit.log")
                });
                crate::security_log::init_audit_log(audit_path);
                tracing::info!("Security audit logging enabled");
            } else {
                portal.save_settings();
                crate::security_log::init_audit_log(None);
                tracing::info!("Security audit logging disabled");
            }
        }
        UiMessage::VncQualityPresetChanged(preset) => {
            portal.prefs.vnc_settings.quality_preset = preset;
            portal.save_settings();
        }
        UiMessage::VncScalingModeChanged(mode) => {
            portal.prefs.vnc_settings.scaling_mode = mode;
            portal.save_settings();
        }
        UiMessage::VncEncodingPreferenceChanged(encoding) => {
            portal.prefs.vnc_settings.encoding = encoding;
            portal.save_settings();
        }
        UiMessage::VncColorDepthChanged(depth) => {
            if matches!(depth, 16 | 32) {
                portal.prefs.vnc_settings.color_depth = depth;
                portal.save_settings();
            }
        }
        UiMessage::VncRefreshFpsChanged(fps) => {
            portal.prefs.vnc_settings.refresh_fps = fps.clamp(1, 20);
            portal.save_settings();
        }
        UiMessage::VncPointerIntervalChanged(interval_ms) => {
            portal.prefs.vnc_settings.pointer_interval_ms = interval_ms.min(1000);
            portal.save_settings();
        }
        UiMessage::VncRemoteResizeChanged(enabled) => {
            portal.prefs.vnc_settings.remote_resize = enabled;
            portal.save_settings();
        }
        UiMessage::VncClipboardSharingChanged(enabled) => {
            portal.prefs.vnc_settings.clipboard_sharing = enabled;
            portal.save_settings();
        }
        UiMessage::VncViewOnlyChanged(enabled) => {
            portal.prefs.vnc_settings.view_only = enabled;
            portal.save_settings();
        }
        UiMessage::VncShowCursorDotChanged(enabled) => {
            portal.prefs.vnc_settings.show_cursor_dot = enabled;
            portal.save_settings();
        }
        UiMessage::VncShowStatsOverlayChanged(enabled) => {
            portal.prefs.vnc_settings.show_stats_overlay = enabled;
            portal.save_settings();
        }
        UiMessage::PortalProxyEnabled(enabled) => {
            portal.prefs.portal_proxy.enabled = enabled;
            clear_portal_proxy_status(portal);
            portal.save_settings();
        }
        UiMessage::PortalProxyDefaultForNewHosts(enabled) => {
            portal.prefs.portal_proxy.default_for_new_ssh_hosts = enabled;
            portal.save_settings();
        }
        UiMessage::PortalProxyHostChanged(host) => {
            portal.prefs.portal_proxy.host = host;
            clear_portal_proxy_status(portal);
            portal.save_settings();
        }
        UiMessage::PortalProxyPortChanged(port) => {
            if let Ok(parsed) = port.trim().parse::<u16>() {
                if parsed > 0 {
                    portal.prefs.portal_proxy.port = parsed;
                    clear_portal_proxy_status(portal);
                    portal.save_settings();
                }
            }
        }
        UiMessage::PortalProxyUsernameChanged(username) => {
            portal.prefs.portal_proxy.username = username;
            clear_portal_proxy_status(portal);
            portal.save_settings();
        }
        UiMessage::PortalProxyIdentityFileChanged(path) => {
            let trimmed = path.trim();
            portal.prefs.portal_proxy.identity_file = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.into())
            };
            clear_portal_proxy_status(portal);
            portal.save_settings();
        }
        UiMessage::PortalProxyCheckStatus => {
            if !portal.prefs.portal_proxy.is_configured() {
                portal.ui.portal_proxy_status = None;
                portal.ui.portal_proxy_status_error =
                    Some("Portal Proxy is not fully configured".to_string());
                portal.ui.portal_proxy_status_loading = false;
                return Task::none();
            }

            portal.ui.portal_proxy_status = None;
            portal.ui.portal_proxy_status_error = None;
            portal.ui.portal_proxy_status_loading = true;
            let settings = portal.prefs.portal_proxy.clone();
            return Task::perform(
                async move { crate::proxy::check_proxy_status(&settings).await },
                |result| Message::Ui(UiMessage::PortalProxyStatusLoaded(result)),
            );
        }
        UiMessage::PortalProxyStatusLoaded(result) => {
            portal.ui.portal_proxy_status_loading = false;
            match result {
                Ok(status) => {
                    portal.ui.portal_proxy_status = Some(status);
                    portal.ui.portal_proxy_status_error = None;
                }
                Err(error) => {
                    portal.ui.portal_proxy_status = None;
                    portal.ui.portal_proxy_status_error = Some(error);
                }
            }
        }
        _ => {}
    }

    Task::none()
}

fn clear_portal_proxy_status(portal: &mut Portal) {
    portal.ui.portal_proxy_status = None;
    portal.ui.portal_proxy_status_error = None;
    portal.ui.portal_proxy_status_loading = false;
}
