use iced::Task;

use crate::app::Portal;
use crate::app::services;
use crate::config::settings::{
    SettingsConfig, TERMINAL_SCROLL_SPEED_MAX, TERMINAL_SCROLL_SPEED_MIN,
};
use crate::hub::sync::{ConflictChoice, LocalSyncProfile, PortalHubSyncService, SyncRunResult};
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
        UiMessage::PortalHubHostsSyncChanged(enabled) => {
            portal.prefs.portal_hub.hosts_sync_enabled = enabled;
            portal.save_settings();
            return portal_hub_sync_task(portal);
        }
        UiMessage::PortalHubSettingsSyncChanged(enabled) => {
            portal.prefs.portal_hub.settings_sync_enabled = enabled;
            portal.save_settings();
            return portal_hub_sync_task(portal);
        }
        UiMessage::PortalHubSnippetsSyncChanged(enabled) => {
            portal.prefs.portal_hub.snippets_sync_enabled = enabled;
            portal.save_settings();
            return portal_hub_sync_task(portal);
        }
        UiMessage::PortalHubKeyVaultChanged(enabled) => {
            portal.prefs.portal_hub.key_vault_enabled = enabled;
            portal.save_settings();
            return portal_hub_sync_task(portal);
        }
        UiMessage::PortalHubDisableSyncRequested(service) => {
            portal.dialogs.open_portal_hub_disable_sync(service);
        }
        UiMessage::PortalHubDisableSyncKeepData(service) => {
            set_portal_hub_sync_service(&mut portal.prefs.portal_hub, service, false);
            portal.save_settings();
            portal.dialogs.close();
            portal.toast_manager.push(Toast::success(format!(
                "{} disabled. Existing Portal Hub data was kept.",
                service.label()
            )));
        }
        UiMessage::PortalHubDisableSyncDeleteData(service) => {
            set_portal_hub_sync_service(&mut portal.prefs.portal_hub, service, false);
            portal.save_settings();
            portal.dialogs.close();
            portal.ui.portal_hub_sync_loading = true;
            portal.ui.portal_hub_sync_error = None;
            let settings = portal.prefs.portal_hub.clone();
            return Task::perform(
                async move { crate::hub::sync::clear_remote_service(&settings, service).await },
                move |result| {
                    Message::Ui(UiMessage::PortalHubDisableSyncDeleteDone(service, result))
                },
            );
        }
        UiMessage::PortalHubDisableSyncDeleteDone(service, result) => {
            portal.ui.portal_hub_sync_loading = false;
            match result {
                Ok(message) => {
                    portal.ui.portal_hub_sync_error = None;
                    portal.ui.portal_hub_sync_status = Some(message.clone());
                    portal.toast_manager.push(Toast::success(message));
                }
                Err(error) => {
                    set_portal_hub_sync_service(&mut portal.prefs.portal_hub, service, true);
                    portal.save_settings();
                    portal.ui.portal_hub_sync_error = Some(error.clone());
                    portal.toast_manager.push(Toast::error(format!(
                        "Portal Hub data deletion failed: {}",
                        error
                    )));
                }
            }
        }
        UiMessage::PortalHubOpenOnboarding => {
            portal.dialogs.open_portal_hub_onboarding();
        }
        UiMessage::PortalHubDefaultForNewHosts(enabled) => {
            portal.prefs.portal_hub.default_for_new_ssh_hosts = enabled;
            portal.save_settings();
        }
        UiMessage::PortalHubHostChanged(host) => {
            portal.prefs.portal_hub.host = host;
            portal.prefs.portal_hub.web_url = portal.prefs.portal_hub.derived_web_url();
            clear_portal_hub_status(portal);
            portal.save_settings();
        }
        UiMessage::PortalHubWebPortChanged(port) => {
            if let Ok(parsed) = port.trim().parse::<u16>() {
                if parsed > 0 {
                    portal.prefs.portal_hub.web_port = parsed;
                    portal.prefs.portal_hub.web_url = portal.prefs.portal_hub.derived_web_url();
                    portal.ui.portal_hub_auth_user = None;
                    portal.ui.portal_hub_auth_error = None;
                    portal.save_settings();
                }
            }
        }
        UiMessage::PortalHubPortChanged(port) => {
            if let Ok(parsed) = port.trim().parse::<u16>() {
                if parsed > 0 {
                    // Deprecated legacy SSH transport setting; retained only for old configs.
                    portal.prefs.portal_hub.port = parsed;
                    clear_portal_hub_status(portal);
                    portal.save_settings();
                }
            }
        }
        UiMessage::PortalHubUsernameChanged(username) => {
            // Deprecated legacy SSH transport setting; retained only for old configs.
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
            portal.prefs.portal_hub.web_url = portal.prefs.portal_hub.derived_web_url();
            if portal.prefs.portal_hub.web_url.trim().is_empty() {
                portal.ui.portal_hub_auth_user = None;
                portal.ui.portal_hub_auth_error =
                    Some("Portal Hub host and web port are not configured".to_string());
                portal.ui.portal_hub_auth_loading = false;
                return Task::none();
            }

            portal.ui.portal_hub_auth_user = None;
            portal.ui.portal_hub_auth_error = None;
            portal.ui.portal_hub_auth_loading = true;
            let settings = portal.prefs.portal_hub.clone();
            return Task::perform(
                async move {
                    let info = crate::hub::auth::fetch_hub_info(&settings).await?;
                    let summary = crate::hub::auth::authenticate(settings).await?;
                    Ok((info, summary))
                },
                |result| Message::Ui(UiMessage::PortalHubAuthenticated(result)),
            );
        }
        UiMessage::PortalHubAuthenticated(result) => {
            portal.ui.portal_hub_auth_loading = false;
            match result {
                Ok((_info, summary)) => {
                    portal.prefs.portal_hub.enabled = true;
                    portal.prefs.portal_hub.hosts_sync_enabled = true;
                    portal.prefs.portal_hub.settings_sync_enabled = true;
                    portal.prefs.portal_hub.snippets_sync_enabled = true;
                    portal.prefs.portal_hub.key_vault_enabled = true;
                    portal.save_settings();
                    portal.ui.portal_hub_auth_user =
                        Some(format!("{} @ {}", summary.username, summary.hub_url));
                    portal.ui.portal_hub_auth_error = None;
                    portal
                        .toast_manager
                        .push(Toast::success("Signed in to Portal Hub. Sync is enabled."));
                    return portal_hub_sync_task(portal);
                }
                Err(error) => {
                    portal.ui.portal_hub_auth_user = None;
                    portal.ui.portal_hub_auth_error = Some(error);
                }
            }
        }
        UiMessage::PortalHubLogout => {
            portal.ui.portal_hub_auth_loading = true;
            portal.ui.portal_hub_auth_error = None;
            let settings = portal.prefs.portal_hub.clone();
            return Task::perform(
                async move { crate::hub::auth::logout(&settings) },
                |result| Message::Ui(UiMessage::PortalHubLoggedOut(result)),
            );
        }
        UiMessage::PortalHubLoggedOut(result) => {
            portal.ui.portal_hub_auth_loading = false;
            match result {
                Ok(()) => {
                    portal.prefs.portal_hub.enabled = false;
                    portal.ui.portal_hub_auth_user = None;
                    portal.ui.portal_hub_auth_error = None;
                    portal.ui.portal_hub_sync_loading = false;
                    portal.ui.portal_hub_sync_error = None;
                    portal.ui.portal_hub_sync_status = None;
                    portal.ui.portal_hub_conflicts.clear();
                    portal.ui.portal_hub_conflict_choices.clear();
                    clear_portal_hub_status(portal);
                    portal.save_settings();
                    portal
                        .toast_manager
                        .push(Toast::success("Signed out of Portal Hub"));
                }
                Err(error) => {
                    portal.ui.portal_hub_auth_error = Some(error.clone());
                    portal
                        .toast_manager
                        .push(Toast::error(format!("Portal Hub logout failed: {}", error)));
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
        UiMessage::PortalHubSyncNow => {
            return portal_hub_sync_task(portal);
        }
        UiMessage::PortalHubSyncDone(result) => {
            portal.ui.portal_hub_sync_loading = false;
            match result {
                Ok(SyncRunResult::Synced(message)) => {
                    portal.ui.portal_hub_sync_error = None;
                    portal.ui.portal_hub_sync_status = Some(message.clone());
                    portal.ui.portal_hub_conflicts.clear();
                    portal.ui.portal_hub_conflict_choices.clear();
                    reload_synced_config(portal);
                    if matches!(
                        portal.dialogs.active(),
                        crate::app::managers::ActiveDialog::PortalHubOnboarding
                    ) {
                        portal.dialogs.close();
                    }
                    portal.toast_manager.push(Toast::success(message));
                }
                Ok(SyncRunResult::Conflicts(conflicts)) => {
                    portal.ui.portal_hub_sync_error = None;
                    portal.ui.portal_hub_sync_status = Some("Conflicts need review".to_string());
                    portal.ui.portal_hub_conflict_choices =
                        vec![ConflictChoice::Local; conflicts.len()];
                    portal.ui.portal_hub_conflicts = conflicts;
                    portal.dialogs.open_portal_hub_conflicts();
                }
                Err(error) => {
                    portal.ui.portal_hub_sync_error = Some(error.clone());
                    portal
                        .toast_manager
                        .push(Toast::error(format!("Portal Hub sync failed: {}", error)));
                }
            }
        }
        UiMessage::PortalHubConflictChoiceChanged(index, choice) => {
            if let Some(slot) = portal.ui.portal_hub_conflict_choices.get_mut(index) {
                *slot = choice;
            }
        }
        UiMessage::PortalHubResolveConflicts => {
            if portal.ui.portal_hub_conflicts.is_empty() {
                return Task::none();
            }
            if !matches!(
                portal.dialogs.active(),
                crate::app::managers::ActiveDialog::PortalHubConflicts
            ) {
                portal.dialogs.open_portal_hub_conflicts();
                return Task::none();
            }
            portal.ui.portal_hub_sync_loading = true;
            let settings = portal.prefs.portal_hub.clone();
            let profile = current_sync_profile(portal);
            let conflicts = portal
                .ui
                .portal_hub_conflicts
                .clone()
                .into_iter()
                .zip(portal.ui.portal_hub_conflict_choices.clone())
                .collect::<Vec<_>>();
            return Task::perform(
                async move {
                    let vault = HubVaultConfig::load()?;
                    let profile = LocalSyncProfile { vault, ..profile };
                    crate::hub::sync::resolve_sync_conflicts(settings, profile, conflicts).await
                },
                |result| Message::Ui(UiMessage::PortalHubResolveConflictsDone(result)),
            );
        }
        UiMessage::PortalHubResolveConflictsDone(result) => {
            portal.ui.portal_hub_sync_loading = false;
            match result {
                Ok(message) => {
                    portal.ui.portal_hub_sync_error = None;
                    portal.ui.portal_hub_sync_status = Some(message.clone());
                    portal.ui.portal_hub_conflicts.clear();
                    portal.ui.portal_hub_conflict_choices.clear();
                    reload_synced_config(portal);
                    if matches!(
                        portal.dialogs.active(),
                        crate::app::managers::ActiveDialog::PortalHubConflicts
                    ) {
                        portal.dialogs.close();
                    }
                    portal.toast_manager.push(Toast::success(message));
                }
                Err(error) => {
                    portal.ui.portal_hub_sync_error = Some(error.clone());
                    portal.toast_manager.push(Toast::error(format!(
                        "Portal Hub conflict resolution failed: {}",
                        error
                    )));
                }
            }
        }
        _ => {}
    }

    Task::none()
}

pub(crate) fn portal_hub_sync_task(portal: &mut Portal) -> Task<Message> {
    if !portal.prefs.portal_hub.sync_configured() {
        return Task::none();
    }
    portal.ui.portal_hub_sync_loading = true;
    portal.ui.portal_hub_sync_error = None;
    let settings = portal.prefs.portal_hub.clone();
    let profile = current_sync_profile(portal);
    Task::perform(
        async move {
            let vault = HubVaultConfig::load()?;
            let profile = LocalSyncProfile { vault, ..profile };
            crate::hub::sync::run_bidirectional_sync(settings, profile).await
        },
        |result| Message::Ui(UiMessage::PortalHubSyncDone(result)),
    )
}

fn current_sync_profile(portal: &Portal) -> LocalSyncProfile {
    LocalSyncProfile {
        hosts: portal.config.hosts.clone(),
        settings: current_settings_config(portal),
        snippets: portal.config.snippets.clone(),
        vault: HubVaultConfig::default(),
    }
}

fn set_portal_hub_sync_service(
    settings: &mut crate::config::settings::PortalHubSettings,
    service: PortalHubSyncService,
    enabled: bool,
) {
    match service {
        PortalHubSyncService::Hosts => settings.hosts_sync_enabled = enabled,
        PortalHubSyncService::Settings => settings.settings_sync_enabled = enabled,
        PortalHubSyncService::Snippets => settings.snippets_sync_enabled = enabled,
        PortalHubSyncService::Vault => settings.key_vault_enabled = enabled,
    }
}

fn reload_synced_config(portal: &mut Portal) {
    if let Ok(hosts) = crate::config::HostsConfig::load() {
        portal.config.hosts = hosts;
    }
    if let Ok(snippets) = crate::config::SnippetsConfig::load() {
        portal.config.snippets = snippets;
    }
    if let Ok(settings) = SettingsConfig::load() {
        apply_settings_config(portal, settings);
    }
}

fn apply_settings_config(portal: &mut Portal, settings: SettingsConfig) {
    portal.prefs.theme_id = settings.theme;
    portal.prefs.ui_scale_override = settings.ui_scale;
    portal.prefs.terminal_font_size = settings.terminal_font_size;
    portal.prefs.terminal_scroll_speed = settings.terminal_scroll_speed;
    portal.prefs.terminal_font = settings.terminal_font;
    portal.prefs.terminal_metric_adjustments = settings.terminal_metric_adjustments;
    portal.prefs.sftp_column_widths = settings.sftp_column_widths;
    portal.prefs.vnc_settings = settings.vnc;
    portal.prefs.portal_hub = settings.portal_hub;
    portal.prefs.auto_reconnect = settings.auto_reconnect;
    portal.prefs.reconnect_max_attempts = settings.reconnect_max_attempts;
    portal.prefs.reconnect_base_delay_ms = settings.reconnect_base_delay_ms;
    portal.prefs.reconnect_max_delay_ms = settings.reconnect_max_delay_ms;
    portal.prefs.allow_agent_forwarding = settings.allow_agent_forwarding;
    portal.prefs.credential_timeout = settings.credential_timeout;
    portal.prefs.session_logging_enabled = settings.session_logging_enabled;
    portal.prefs.session_log_dir = settings.session_log_dir;
    portal.prefs.session_log_format = settings.session_log_format;
    portal.prefs.security_audit_enabled = settings.security_audit_enabled;
    portal.prefs.security_audit_dir = settings.security_audit_dir;
    portal.prefs.keybindings = settings.keybindings;
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
