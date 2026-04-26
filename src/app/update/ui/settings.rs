use iced::Task;

use crate::app::Portal;
use crate::app::services;
use crate::config::settings::{
    SettingsConfig, TERMINAL_SCROLL_SPEED_MAX, TERMINAL_SCROLL_SPEED_MIN,
};
use crate::hub::vault::HubVaultConfig;
use crate::message::{Message, UiMessage};
use crate::views::toast::Toast;

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
        UiMessage::PortalHubEnabled(enabled) => {
            portal.prefs.portal_hub.enabled = enabled;
            clear_portal_hub_status(portal);
            portal.save_settings();
        }
        UiMessage::PortalHubDefaultForNewHosts(enabled) => {
            portal.prefs.portal_hub.default_for_new_ssh_hosts = enabled;
            portal.save_settings();
        }
        UiMessage::PortalHubHostChanged(host) => {
            portal.prefs.portal_hub.host = host;
            clear_portal_hub_status(portal);
            portal.save_settings();
        }
        UiMessage::PortalHubPortChanged(port) => {
            if let Ok(parsed) = port.trim().parse::<u16>() {
                if parsed > 0 {
                    portal.prefs.portal_hub.port = parsed;
                    clear_portal_hub_status(portal);
                    portal.save_settings();
                }
            }
        }
        UiMessage::PortalHubUsernameChanged(username) => {
            portal.prefs.portal_hub.username = username;
            clear_portal_hub_status(portal);
            portal.save_settings();
        }
        UiMessage::PortalHubIdentityFileChanged(path) => {
            let trimmed = path.trim();
            portal.prefs.portal_hub.identity_file = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.into())
            };
            clear_portal_hub_status(portal);
            portal.save_settings();
        }
        UiMessage::PortalHubWebUrlChanged(url) => {
            portal.prefs.portal_hub.web_url = url;
            portal.ui.portal_hub_auth_user = None;
            portal.ui.portal_hub_auth_error = None;
            portal.save_settings();
        }
        UiMessage::PortalHubCheckStatus => {
            if !portal.prefs.portal_hub.is_configured() {
                portal.ui.portal_hub_status = None;
                portal.ui.portal_hub_status_error =
                    Some("Portal Hub is not fully configured".to_string());
                portal.ui.portal_hub_status_loading = false;
                return Task::none();
            }

            portal.ui.portal_hub_status = None;
            portal.ui.portal_hub_status_error = None;
            portal.ui.portal_hub_status_loading = true;
            let settings = portal.prefs.portal_hub.clone();
            return Task::perform(
                async move { crate::proxy::check_proxy_status(&settings).await },
                |result| Message::Ui(UiMessage::PortalHubStatusLoaded(result)),
            );
        }
        UiMessage::PortalHubStatusLoaded(result) => {
            portal.ui.portal_hub_status_loading = false;
            match result {
                Ok(status) => {
                    portal.ui.portal_hub_status = Some(status);
                    portal.ui.portal_hub_status_error = None;
                }
                Err(error) => {
                    portal.ui.portal_hub_status = None;
                    portal.ui.portal_hub_status_error = Some(error);
                }
            }
        }
        UiMessage::PortalHubAuthenticate => {
            if portal.prefs.portal_hub.web_url.trim().is_empty() {
                portal.ui.portal_hub_auth_user = None;
                portal.ui.portal_hub_auth_error =
                    Some("Portal Hub web URL is not configured".to_string());
                portal.ui.portal_hub_auth_loading = false;
                return Task::none();
            }

            portal.ui.portal_hub_auth_user = None;
            portal.ui.portal_hub_auth_error = None;
            portal.ui.portal_hub_auth_loading = true;
            let settings = portal.prefs.portal_hub.clone();
            return Task::perform(
                async move { crate::hub::auth::authenticate(settings).await },
                |result| Message::Ui(UiMessage::PortalHubAuthenticated(result)),
            );
        }
        UiMessage::PortalHubAuthenticated(result) => {
            portal.ui.portal_hub_auth_loading = false;
            match result {
                Ok(summary) => {
                    portal.ui.portal_hub_auth_user =
                        Some(format!("{} @ {}", summary.username, summary.hub_url));
                    portal.ui.portal_hub_auth_error = None;
                    portal.toast_manager.push(Toast::success(
                        "Signed in to Portal Hub. Choose whether to upload local data or pull from Hub.",
                    ));
                }
                Err(error) => {
                    portal.ui.portal_hub_auth_user = None;
                    portal.ui.portal_hub_auth_error = Some(error);
                }
            }
        }
        UiMessage::PortalHubUploadLocalProfile => {
            let settings = portal.prefs.portal_hub.clone();
            portal.save_settings();
            let hosts = portal.config.hosts.clone();
            let snippets = portal.config.snippets.clone();
            let settings_config = current_settings_config(portal);
            return Task::perform(
                async move {
                    let vault = HubVaultConfig::load()?;
                    let current = crate::hub::sync::http_sync_get(&settings).await?;
                    let request = crate::hub::sync::build_sync_request(
                        &hosts,
                        &settings_config,
                        &snippets,
                        &vault,
                    )?;
                    let response =
                        crate::hub::sync::http_sync_put(&settings, current.revision, request)
                            .await?;
                    Ok(format!(
                        "Uploaded local profile to Portal Hub ({})",
                        response.revision
                    ))
                },
                |result| Message::Ui(UiMessage::PortalHubUploadLocalProfileDone(result)),
            );
        }
        UiMessage::PortalHubUploadLocalProfileDone(result) => match result {
            Ok(message) => portal.toast_manager.push(Toast::success(message)),
            Err(error) => portal
                .toast_manager
                .push(Toast::error(format!("Portal Hub upload failed: {}", error))),
        },
        UiMessage::PortalHubPullProfile => {
            let settings = portal.prefs.portal_hub.clone();
            return Task::perform(
                async move {
                    let response = crate::hub::sync::http_sync_get(&settings).await?;
                    let profile = crate::hub::sync::parse_profile(&response)?;
                    let hosts: crate::config::HostsConfig =
                        serde_json::from_value(profile.hosts)
                            .map_err(|error| format!("failed to parse synced hosts: {}", error))?;
                    let settings_config: SettingsConfig = serde_json::from_value(profile.settings)
                        .map_err(|error| format!("failed to parse synced settings: {}", error))?;
                    let snippets: crate::config::SnippetsConfig =
                        serde_json::from_value(profile.snippets).map_err(|error| {
                            format!("failed to parse synced snippets: {}", error)
                        })?;
                    let vault: HubVaultConfig = serde_json::from_value(response.vault)
                        .map_err(|error| format!("failed to parse synced vault: {}", error))?;

                    hosts.save().map_err(|error| error.to_string())?;
                    settings_config.save().map_err(|error| error.to_string())?;
                    snippets.save().map_err(|error| error.to_string())?;
                    vault.save()?;

                    Ok(
                        "Pulled Portal Hub profile. Restart Portal to reload all settings."
                            .to_string(),
                    )
                },
                |result| Message::Ui(UiMessage::PortalHubPullProfileDone(result)),
            );
        }
        UiMessage::PortalHubPullProfileDone(result) => match result {
            Ok(message) => portal.toast_manager.push(Toast::success(message)),
            Err(error) => portal
                .toast_manager
                .push(Toast::error(format!("Portal Hub pull failed: {}", error))),
        },
        _ => {}
    }

    Task::none()
}

fn current_settings_config(portal: &Portal) -> SettingsConfig {
    let mut settings = SettingsConfig::default();
    settings.terminal_font_size = portal.prefs.terminal_font_size;
    settings.terminal_scroll_speed = portal.prefs.terminal_scroll_speed;
    settings.terminal_font = portal.prefs.terminal_font;
    settings.terminal_metric_adjustments = portal.prefs.terminal_metric_adjustments;
    settings.theme = portal.prefs.theme_id;
    settings.ui_scale = portal.prefs.ui_scale_override;
    settings.vnc = portal.prefs.vnc_settings.clone();
    settings.portal_hub = portal.prefs.portal_hub.clone();
    settings.auto_reconnect = portal.prefs.auto_reconnect;
    settings.reconnect_max_attempts = portal.prefs.reconnect_max_attempts;
    settings.reconnect_base_delay_ms = portal.prefs.reconnect_base_delay_ms;
    settings.reconnect_max_delay_ms = portal.prefs.reconnect_max_delay_ms;
    settings.allow_agent_forwarding = portal.prefs.allow_agent_forwarding;
    settings.credential_timeout = portal.prefs.credential_timeout;
    settings.session_logging_enabled = portal.prefs.session_logging_enabled;
    settings.session_log_dir = portal.prefs.session_log_dir.clone();
    settings.session_log_format = portal.prefs.session_log_format;
    settings.security_audit_enabled = portal.prefs.security_audit_enabled;
    settings.security_audit_dir = portal.prefs.security_audit_dir.clone();
    settings.keybindings = portal.prefs.keybindings.clone();
    settings
}

fn clear_portal_hub_status(portal: &mut Portal) {
    portal.ui.portal_hub_status = None;
    portal.ui.portal_hub_status_error = None;
    portal.ui.portal_hub_status_loading = false;
}
