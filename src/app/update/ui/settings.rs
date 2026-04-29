use iced::Task;

use crate::app::Portal;
use crate::app::services;
use crate::config::settings::{
    SettingsConfig, TERMINAL_SCROLL_SPEED_MAX, TERMINAL_SCROLL_SPEED_MIN,
};
use crate::hub::sync::{
    ConflictChoice, LocalSyncProfile, PortalHubSyncService, SyncRunActivity, SyncRunOrigin,
    SyncRunResult,
};
use crate::hub::vault::HubVaultConfig;
use crate::message::{Message, UiMessage, VaultMessage};
use crate::views::toast::Toast;

pub(super) fn handle_settings_message(portal: &mut Portal, msg: UiMessage) -> Task<Message> {
    match msg {
        UiMessage::ThemeChange(theme_id) => {
            portal.prefs.theme_id = theme_id;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::FontChange(font) => {
            tracing::info!("Font changed");
            portal.prefs.terminal_font = font;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::FontSizeChange(size) => {
            portal.prefs.terminal_font_size = size;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::TerminalScrollSpeedChange(speed) => {
            portal.prefs.terminal_scroll_speed =
                speed.clamp(TERMINAL_SCROLL_SPEED_MIN, TERMINAL_SCROLL_SPEED_MAX);
            save_settings_and_queue_sync(portal);
        }
        UiMessage::UiScaleChange(scale) => {
            portal.prefs.ui_scale_override = Some(scale.clamp(0.8, 1.5));
            save_settings_and_queue_sync(portal);
        }
        UiMessage::UiScaleReset => {
            portal.prefs.ui_scale_override = None;
            save_settings_and_queue_sync(portal);
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
            save_settings_and_queue_sync(portal);
        }
        UiMessage::AllowAgentForwarding(enabled) => {
            portal.prefs.allow_agent_forwarding = enabled;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::AutoReconnectEnabled(enabled) => {
            portal.prefs.auto_reconnect = enabled;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::ReconnectMaxAttemptsChanged(attempts) => {
            portal.prefs.reconnect_max_attempts = attempts.clamp(1, 20);
            save_settings_and_queue_sync(portal);
        }
        UiMessage::ReconnectBaseDelayChanged(delay_ms) => {
            let base_delay = delay_ms.clamp(500, 10_000);
            portal.prefs.reconnect_base_delay_ms = base_delay;
            if portal.prefs.reconnect_max_delay_ms < base_delay {
                portal.prefs.reconnect_max_delay_ms = base_delay;
            }
            save_settings_and_queue_sync(portal);
        }
        UiMessage::ReconnectMaxDelayChanged(delay_ms) => {
            portal.prefs.reconnect_max_delay_ms =
                delay_ms.clamp(portal.prefs.reconnect_base_delay_ms.max(500), 120_000);
            save_settings_and_queue_sync(portal);
        }
        UiMessage::CredentialTimeoutChange(timeout_seconds) => {
            let clamped = timeout_seconds.min(3600);
            portal.prefs.credential_timeout = clamped;
            save_settings_and_queue_sync(portal);
            services::connection::init_passphrase_cache(clamped);
        }
        UiMessage::SecurityAuditLoggingEnabled(enabled) => {
            portal.prefs.security_audit_enabled = enabled;

            if enabled {
                if portal.prefs.security_audit_dir.is_none() {
                    portal.prefs.security_audit_dir = crate::config::paths::config_dir()
                        .map(|dir| dir.join("logs").join("security"));
                }

                save_settings_and_queue_sync(portal);
                let audit_path = portal.prefs.security_audit_dir.as_ref().map(|dir| {
                    let _ = std::fs::create_dir_all(dir);
                    dir.join("audit.log")
                });
                crate::security_log::init_audit_log(audit_path);
                tracing::info!("Security audit logging enabled");
            } else {
                save_settings_and_queue_sync(portal);
                crate::security_log::init_audit_log(None);
                tracing::info!("Security audit logging disabled");
            }
        }
        UiMessage::VncQualityPresetChanged(preset) => {
            portal.prefs.vnc_settings.quality_preset = preset;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::VncScalingModeChanged(mode) => {
            portal.prefs.vnc_settings.scaling_mode = mode;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::VncEncodingPreferenceChanged(encoding) => {
            portal.prefs.vnc_settings.encoding = encoding;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::VncColorDepthChanged(depth) => {
            if matches!(depth, 16 | 32) {
                portal.prefs.vnc_settings.color_depth = depth;
                save_settings_and_queue_sync(portal);
            }
        }
        UiMessage::VncRefreshFpsChanged(fps) => {
            portal.prefs.vnc_settings.refresh_fps = fps.clamp(1, 20);
            save_settings_and_queue_sync(portal);
        }
        UiMessage::VncPointerIntervalChanged(interval_ms) => {
            portal.prefs.vnc_settings.pointer_interval_ms = interval_ms.min(1000);
            save_settings_and_queue_sync(portal);
        }
        UiMessage::VncRemoteResizeChanged(enabled) => {
            portal.prefs.vnc_settings.remote_resize = enabled;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::VncClipboardSharingChanged(enabled) => {
            portal.prefs.vnc_settings.clipboard_sharing = enabled;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::VncViewOnlyChanged(enabled) => {
            portal.prefs.vnc_settings.view_only = enabled;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::VncShowCursorDotChanged(enabled) => {
            portal.prefs.vnc_settings.show_cursor_dot = enabled;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::VncShowStatsOverlayChanged(enabled) => {
            portal.prefs.vnc_settings.show_stats_overlay = enabled;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::PortalHubEnabled(enabled) => {
            portal.prefs.portal_hub.enabled = enabled;
            clear_portal_hub_status(portal);
            save_settings_and_queue_sync(portal);
        }
        UiMessage::PortalHubHostsSyncChanged(enabled) => {
            portal.prefs.portal_hub.hosts_sync_enabled = enabled;
            save_settings_and_queue_sync(portal);
            return portal_hub_sync_task(portal, SyncRunOrigin::Manual, true);
        }
        UiMessage::PortalHubSettingsSyncChanged(enabled) => {
            portal.prefs.portal_hub.settings_sync_enabled = enabled;
            save_settings_and_queue_sync(portal);
            return portal_hub_sync_task(portal, SyncRunOrigin::Manual, true);
        }
        UiMessage::PortalHubSnippetsSyncChanged(enabled) => {
            portal.prefs.portal_hub.snippets_sync_enabled = enabled;
            save_settings_and_queue_sync(portal);
            return portal_hub_sync_task(portal, SyncRunOrigin::Manual, true);
        }
        UiMessage::PortalHubKeyVaultChanged(enabled) => {
            portal.prefs.portal_hub.key_vault_enabled = enabled;
            save_settings_and_queue_sync(portal);
            return portal_hub_sync_task(portal, SyncRunOrigin::Manual, true);
        }
        UiMessage::PortalHubDisableSyncRequested(service) => {
            portal.dialogs.open_portal_hub_disable_sync(service);
        }
        UiMessage::PortalHubDisableSyncKeepData(service) => {
            set_portal_hub_sync_service(&mut portal.prefs.portal_hub, service, false);
            save_settings_and_queue_sync(portal);
            portal.dialogs.close();
            portal.toast_manager.push(Toast::success(format!(
                "{} disabled. Existing Portal Hub data was kept.",
                service.label()
            )));
        }
        UiMessage::PortalHubDisableSyncDeleteData(service) => {
            set_portal_hub_sync_service(&mut portal.prefs.portal_hub, service, false);
            save_settings_and_queue_sync(portal);
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
                    save_settings_and_queue_sync(portal);
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
        UiMessage::PortalHubOpenGithub => {
            if let Err(error) = open::that("https://github.com/DigitalPals/portal-hub") {
                portal.toast_manager.push(Toast::error(format!(
                    "Failed to open Portal Hub project: {}",
                    error
                )));
            }
        }
        UiMessage::PortalHubDefaultForNewHosts(enabled) => {
            portal.prefs.portal_hub.default_for_new_ssh_hosts = enabled;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::PortalHubHostChanged(host) => {
            portal.prefs.portal_hub.apply_host_input(host);
            clear_portal_hub_status(portal);
            save_settings_and_queue_sync(portal);
        }
        UiMessage::PortalHubWebPortChanged(port) => {
            if let Ok(parsed) = port.trim().parse::<u16>() {
                if parsed > 0 {
                    portal.prefs.portal_hub.web_port = parsed;
                    portal.prefs.portal_hub.web_url = portal.prefs.portal_hub.derived_web_url();
                    portal.ui.portal_hub_auth_user = None;
                    portal.ui.portal_hub_auth_error = None;
                    save_settings_and_queue_sync(portal);
                }
            }
        }
        UiMessage::PortalHubPortChanged(port) => {
            if let Ok(parsed) = port.trim().parse::<u16>() {
                if parsed > 0 {
                    // Deprecated legacy SSH transport setting; retained only for old configs.
                    portal.prefs.portal_hub.port = parsed;
                    clear_portal_hub_status(portal);
                    save_settings_and_queue_sync(portal);
                }
            }
        }
        UiMessage::PortalHubUsernameChanged(username) => {
            // Deprecated legacy SSH transport setting; retained only for old configs.
            portal.prefs.portal_hub.username = username;
            clear_portal_hub_status(portal);
            save_settings_and_queue_sync(portal);
        }
        UiMessage::PortalHubIdentityFileChanged(path) => {
            let trimmed = path.trim();
            portal.prefs.portal_hub.identity_file = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.into())
            };
            clear_portal_hub_status(portal);
            save_settings_and_queue_sync(portal);
        }
        UiMessage::PortalHubWebUrlChanged(url) => {
            portal.prefs.portal_hub.apply_web_url_input(url);
            portal.ui.portal_hub_auth_user = None;
            portal.ui.portal_hub_auth_error = None;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::PortalHubCheckStatus => {
            if !portal.prefs.portal_hub.web_configured() {
                portal.ui.portal_hub_status = None;
                portal.ui.portal_hub_status_error =
                    Some("Enter a Portal Hub URL or host first".to_string());
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
                    apply_portal_hub_discovery(&mut portal.prefs.portal_hub, &status);
                    save_settings_and_queue_sync(portal);
                    portal.ui.portal_hub_status = Some(status);
                    portal.ui.portal_hub_status_error = None;
                    portal
                        .toast_manager
                        .push(Toast::success("Portal Hub settings detected"));
                }
                Err(error) => {
                    portal.ui.portal_hub_status = None;
                    portal.ui.portal_hub_status_error = Some(error);
                }
            }
        }
        UiMessage::PortalHubAuthenticate => {
            if portal.prefs.portal_hub.web_url.trim().is_empty() {
                portal.prefs.portal_hub.web_url = portal.prefs.portal_hub.derived_web_url();
            }
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
                    validate_portal_hub_info(&info)?;
                    let summary = crate::hub::auth::authenticate(settings).await?;
                    Ok((info, summary))
                },
                |result| Message::Ui(UiMessage::PortalHubAuthenticated(result)),
            );
        }
        UiMessage::PortalHubAuthenticated(result) => {
            portal.ui.portal_hub_auth_loading = false;
            match result {
                Ok((info, summary)) => {
                    apply_portal_hub_info(&mut portal.prefs.portal_hub, &info);
                    save_settings_and_queue_sync(portal);
                    portal.ui.portal_hub_auth_user =
                        Some(format!("{} @ {}", summary.username, summary.hub_url));
                    portal.ui.portal_hub_auth_error = None;
                    portal
                        .toast_manager
                        .push(Toast::success("Signed in to Portal Hub. Sync is enabled."));
                    return portal_hub_sync_task(portal, SyncRunOrigin::Login, true);
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
                    portal.ui.portal_hub_local_sync_pending = false;
                    portal.ui.portal_hub_remote_sync_pending = false;
                    portal.ui.portal_hub_conflicts.clear();
                    portal.ui.portal_hub_conflict_choices.clear();
                    clear_portal_hub_status(portal);
                    save_settings_and_queue_sync(portal);
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
            save_settings_and_queue_sync(portal);
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
            return portal_hub_sync_task(portal, SyncRunOrigin::Manual, true);
        }
        UiMessage::PortalHubLocalSyncDue => {
            portal.ui.portal_hub_local_sync_pending = false;
            return portal_hub_sync_task(portal, SyncRunOrigin::Background, false);
        }
        UiMessage::PortalHubRemoteRevisions(result) => match result {
            Ok(event) => {
                match crate::hub::sync::remote_revisions_require_sync(
                    &portal.prefs.portal_hub,
                    &event.services,
                ) {
                    Ok(true) => {
                        return portal_hub_sync_task(portal, SyncRunOrigin::RemoteEvent, true);
                    }
                    Ok(false) => {
                        portal.ui.portal_hub_sync_status =
                            Some("Portal Hub is up to date".to_string());
                    }
                    Err(error) => {
                        portal.ui.portal_hub_sync_error = Some(error.clone());
                        tracing::warn!("Portal Hub sync event handling failed: {}", error);
                    }
                }
            }
            Err(error) => {
                tracing::warn!("Portal Hub sync event stream disconnected: {}", error);
            }
        },
        UiMessage::PortalHubSyncDone(origin, result) => {
            portal.ui.portal_hub_sync_loading = false;
            match result {
                Ok(SyncRunResult::Synced(summary)) => {
                    let message = summary.message().to_string();
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
                    if should_show_sync_success(origin, summary.activity) {
                        portal.toast_manager.push(Toast::success(message));
                    }
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
            if portal.ui.portal_hub_remote_sync_pending && portal.ui.portal_hub_conflicts.is_empty()
            {
                portal.ui.portal_hub_remote_sync_pending = false;
                return portal_hub_sync_task(portal, SyncRunOrigin::RemoteEvent, true);
            }
            if portal.ui.portal_hub_sync_error.is_none()
                && portal.ui.portal_hub_conflicts.is_empty()
                && matches!(
                    origin,
                    SyncRunOrigin::Login | SyncRunOrigin::Startup | SyncRunOrigin::RemoteEvent
                )
            {
                return Task::done(Message::Vault(VaultMessage::EnrollmentRefresh));
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

pub(crate) fn portal_hub_sync_task(
    portal: &mut Portal,
    origin: SyncRunOrigin,
    force: bool,
) -> Task<Message> {
    if !portal.prefs.portal_hub.sync_configured() {
        return Task::none();
    }
    if portal.ui.portal_hub_sync_loading {
        if force {
            portal.ui.portal_hub_remote_sync_pending = true;
        } else {
            portal.ui.portal_hub_local_sync_pending = true;
        }
        return Task::none();
    }
    let settings = portal.prefs.portal_hub.clone();
    let profile = current_sync_profile(portal);
    let vault = match HubVaultConfig::load() {
        Ok(vault) => vault,
        Err(error) => {
            return Task::done(Message::Ui(UiMessage::PortalHubSyncDone(
                origin,
                Err(error),
            )));
        }
    };
    let profile = LocalSyncProfile { vault, ..profile };

    if !force {
        match crate::hub::sync::local_sync_changes_pending(&settings, &profile) {
            Ok(false) => {
                return Task::done(Message::Ui(UiMessage::PortalHubSyncDone(
                    origin,
                    Ok(SyncRunResult::Synced(
                        crate::hub::sync::SyncRunSummary::new(SyncRunActivity::NoChanges),
                    )),
                )));
            }
            Ok(true) => {}
            Err(error) => {
                return Task::done(Message::Ui(UiMessage::PortalHubSyncDone(
                    origin,
                    Err(error),
                )));
            }
        }
    }

    portal.ui.portal_hub_sync_loading = true;
    portal.ui.portal_hub_sync_error = None;
    portal.ui.portal_hub_local_sync_pending = false;
    portal.ui.portal_hub_remote_sync_pending = false;
    Task::perform(
        async move { crate::hub::sync::run_bidirectional_sync(settings, profile).await },
        move |result| Message::Ui(UiMessage::PortalHubSyncDone(origin, result)),
    )
}

pub(crate) fn queue_portal_hub_local_sync(portal: &mut Portal) {
    if portal.ui.portal_hub_auth_user.is_some()
        && portal.prefs.portal_hub.sync_configured()
        && portal.ui.portal_hub_conflicts.is_empty()
    {
        portal.ui.portal_hub_local_sync_pending = true;
    }
}

fn save_settings_and_queue_sync(portal: &mut Portal) {
    portal.save_settings();
    queue_portal_hub_local_sync(portal);
}

fn should_show_sync_success(origin: SyncRunOrigin, activity: SyncRunActivity) -> bool {
    if activity == SyncRunActivity::Disabled {
        return false;
    }
    matches!(origin, SyncRunOrigin::Manual | SyncRunOrigin::Login)
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
    if let Ok(vault) = HubVaultConfig::load() {
        portal.config.vault = vault;
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

fn apply_portal_hub_discovery(
    settings: &mut crate::config::settings::PortalHubSettings,
    status: &crate::proxy::ProxyStatus,
) {
    settings.enabled = status.web_proxy;
    settings.hosts_sync_enabled = status.sync_v2;
    settings.settings_sync_enabled = status.sync_v2;
    settings.snippets_sync_enabled = status.sync_v2;
    settings.key_vault_enabled = status.key_vault;
    settings.port = status.ssh_port.unwrap_or(2222);
    settings.username = status
        .ssh_username
        .clone()
        .unwrap_or_else(|| "portal-hub".to_string());
    if !status.public_url.trim().is_empty() {
        settings.apply_discovered_web_url(&status.public_url);
    }
}

fn apply_portal_hub_info(
    settings: &mut crate::config::settings::PortalHubSettings,
    info: &crate::hub::auth::HubInfo,
) {
    settings.enabled = info.capabilities.web_proxy;
    settings.hosts_sync_enabled = info.capabilities.sync_v2;
    settings.settings_sync_enabled = info.capabilities.sync_v2;
    settings.snippets_sync_enabled = info.capabilities.sync_v2;
    settings.key_vault_enabled = info.capabilities.key_vault;
    settings.port = info.ssh_port.unwrap_or(2222);
    settings.username = info
        .ssh_username
        .clone()
        .unwrap_or_else(|| "portal-hub".to_string());
    if !info.public_url.trim().is_empty() {
        settings.apply_discovered_web_url(&info.public_url);
    }
}

fn validate_portal_hub_info(info: &crate::hub::auth::HubInfo) -> Result<(), String> {
    if info.api_version < 2 {
        return Err(format!(
            "Portal Hub API version {} is too old; Portal requires 2",
            info.api_version
        ));
    }
    if !info.capabilities.web_proxy {
        return Err("Portal Hub does not advertise persistent session proxy support".to_string());
    }
    if !info.capabilities.sync_v2 {
        return Err("Portal Hub does not advertise sync v2 support".to_string());
    }
    Ok(())
}
