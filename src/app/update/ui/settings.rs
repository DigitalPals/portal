use iced::Task;

use crate::app::Portal;
use crate::app::services;
use crate::config::settings::{
    SettingsConfig, TERMINAL_SCROLL_SPEED_MAX, TERMINAL_SCROLL_SPEED_MIN,
};
use crate::hub::sync::{
    ConflictChoice, LocalSyncProfile, PortalHubSyncService, SyncContext, SyncRunActivity,
    SyncRunOrigin, SyncRunResult,
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
            return super::reconcile_active_terminal_size(portal);
        }
        UiMessage::FontSizeChange(size) => {
            portal.prefs.terminal_font_size = size;
            save_settings_and_queue_sync(portal);
            return super::reconcile_active_terminal_size(portal);
        }
        UiMessage::TerminalScrollSpeedChange(speed) => {
            portal.prefs.terminal_scroll_speed =
                speed.clamp(TERMINAL_SCROLL_SPEED_MIN, TERMINAL_SCROLL_SPEED_MAX);
            save_settings_and_queue_sync(portal);
        }
        UiMessage::UiScaleChange(scale) => {
            portal.prefs.ui_scale_override = Some(scale.clamp(0.8, 1.5));
            save_settings_and_queue_sync(portal);
            return super::reconcile_active_terminal_size(portal);
        }
        UiMessage::UiScaleReset => {
            portal.prefs.ui_scale_override = None;
            save_settings_and_queue_sync(portal);
            return super::reconcile_active_terminal_size(portal);
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
                match crate::security_log::init_audit_log_dir(
                    portal.prefs.security_audit_dir.clone(),
                ) {
                    Ok(Some(path)) => {
                        tracing::info!("Security audit logging enabled at {}", path.display());
                    }
                    Ok(None) => {
                        portal.toast_manager.push(Toast::error(
                            "Security audit log directory is not available",
                        ));
                        tracing::warn!(
                            "Security audit logging disabled: no audit directory available"
                        );
                    }
                    Err(error) => {
                        portal.toast_manager.push(Toast::error(format!(
                            "Security audit logging disabled: {}",
                            error
                        )));
                        tracing::warn!("Security audit logging disabled: {}", error);
                    }
                }
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
            invalidate_hub_sync(portal);
            save_settings_and_queue_sync(portal);
            return portal_hub_sync_task(portal, SyncRunOrigin::Manual, true);
        }
        UiMessage::PortalHubSettingsSyncChanged(enabled) => {
            portal.prefs.portal_hub.settings_sync_enabled = enabled;
            invalidate_hub_sync(portal);
            save_settings_and_queue_sync(portal);
            return portal_hub_sync_task(portal, SyncRunOrigin::Manual, true);
        }
        UiMessage::PortalHubSnippetsSyncChanged(enabled) => {
            portal.prefs.portal_hub.snippets_sync_enabled = enabled;
            invalidate_hub_sync(portal);
            save_settings_and_queue_sync(portal);
            return portal_hub_sync_task(portal, SyncRunOrigin::Manual, true);
        }
        UiMessage::PortalHubKeyVaultChanged(enabled) => {
            portal.prefs.portal_hub.key_vault_enabled = enabled;
            invalidate_hub_sync(portal);
            save_settings_and_queue_sync(portal);
            return portal_hub_sync_task(portal, SyncRunOrigin::Manual, true);
        }
        UiMessage::PortalHubDisableSyncRequested(service) => {
            portal.dialogs.open_portal_hub_disable_sync(service);
        }
        UiMessage::PortalHubDisableSyncKeepData(service) => {
            set_portal_hub_sync_service(&mut portal.prefs.portal_hub, service, false);
            invalidate_hub_sync(portal);
            save_settings_and_queue_sync(portal);
            portal.dialogs.close();
            portal.toast_manager.push(Toast::success(format!(
                "{} disabled. Existing Portal Hub data was kept.",
                service.label()
            )));
        }
        UiMessage::PortalHubDisableSyncDeleteData(service) => {
            set_portal_hub_sync_service(&mut portal.prefs.portal_hub, service, false);
            invalidate_hub_sync(portal);
            save_settings_and_queue_sync(portal);
            portal.dialogs.close();
            portal.ui.portal_hub_sync_loading = true;
            portal.ui.portal_hub_sync_error = None;
            let settings = portal.prefs.portal_hub.clone();
            let context = begin_hub_sync(portal);
            return Task::perform(
                async move { crate::hub::sync::clear_remote_service(&settings, service).await },
                move |result| {
                    Message::Ui(UiMessage::PortalHubDisableSyncDeleteDone(
                        context.clone(),
                        service,
                        result,
                    ))
                },
            );
        }
        UiMessage::PortalHubDisableSyncDeleteDone(context, _service, result) => {
            if !accepts_hub_sync(portal, &context) {
                return Task::none();
            }
            portal.ui.portal_hub_sync_context = None;
            portal.ui.portal_hub_sync_loading = false;
            match result {
                Ok(message) => {
                    portal.ui.portal_hub_sync_error = None;
                    portal.ui.portal_hub_sync_status = Some(message.clone());
                    portal.toast_manager.push(Toast::success(message));
                }
                Err(error) => {
                    portal.ui.portal_hub_sync_error = Some(error.clone());
                    portal.toast_manager.push(Toast::error(format!(
                        "Portal Hub data deletion failed: {}",
                        error
                    )));
                }
            }
        }
        UiMessage::PortalHubOpenOnboarding => {
            init_portal_hub_wizard(portal);
            portal.dialogs.open_portal_hub_onboarding();
            return iced::widget::operation::focus(
                crate::views::dialogs::portal_hub_dialogs::portal_hub_onboarding_field_id(
                    crate::views::dialogs::portal_hub_dialogs::PortalHubOnboardingField::WebUrl,
                ),
            );
        }
        UiMessage::PortalHubOpenManagement => {
            init_portal_hub_wizard(portal);
            let current = portal.prefs.portal_hub.effective_web_url();
            let wizard = &mut portal.ui.portal_hub_wizard;
            wizard.managed.choice = crate::hub::deployment::SetupChoice::Server;
            if let Some(record) = wizard
                .managed
                .records
                .iter()
                .find(|record| record.public_url.as_deref() == Some(current.as_str()))
                .cloned()
            {
                wizard.managed.host_id = Some(record.ssh_host_id);
                wizard.managed.hostname = record.hostname.clone();
                wizard.managed.image = record.hub_image.clone();
                wizard.managed.selected = Some(record);
            }
            portal.dialogs.open_portal_hub_onboarding();
        }
        UiMessage::PortalHubOpenDefaultsReview => {
            init_portal_hub_wizard(portal);
            portal.ui.portal_hub_wizard.route_default = true;
            portal.dialogs.open_portal_hub_onboarding();
        }
        UiMessage::PortalHubDefaultsPromptDismiss => {
            portal.ui.hub_prompt_dismissed = true;
        }
        UiMessage::PortalHubPreferVaultKeys(enabled) => {
            portal.prefs.portal_hub.prefer_vault_keys = enabled;
            save_settings_and_queue_sync(portal);
        }
        UiMessage::PortalHubWizardToggleHost(host_id) => {
            let excluded = &mut portal.ui.portal_hub_wizard.excluded_hosts;
            if !excluded.remove(&host_id) {
                excluded.insert(host_id);
            }
        }
        UiMessage::PortalHubWizardRouteDefault(enabled) => {
            portal.ui.portal_hub_wizard.route_default = enabled;
        }
        UiMessage::PortalHubWizardPreferVault(enabled) => {
            portal.ui.portal_hub_wizard.prefer_vault = enabled;
        }
        UiMessage::PortalHubWizardSyncAll(enabled) => {
            portal.ui.portal_hub_wizard.sync_all = enabled;
        }
        UiMessage::PortalHubWizardApply => {
            return apply_portal_hub_wizard(portal);
        }
        UiMessage::PortalHubWizardSkip => {
            portal
                .dialogs
                .close_onboarding(portal.ui.portal_hub_wizard.managed.operation);
            portal.ui.portal_hub_wizard.cancel();
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
        UiMessage::PortalHubPortChanged(port) => {
            if let Ok(parsed) = port.trim().parse::<u16>()
                && parsed > 0
            {
                // Deprecated legacy SSH transport setting; retained only for old configs.
                portal.prefs.portal_hub.port = parsed;
                clear_portal_hub_status(portal);
                save_settings_and_queue_sync(portal);
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
        UiMessage::PortalHubManagement(message) => {
            return super::hub_management::handle(portal, message);
        }
        UiMessage::PortalHubWebUrlChanged(url) => {
            if portal.ui.portal_hub_wizard.active {
                portal.ui.portal_hub_wizard.edit_endpoint(url);
            }
        }
        UiMessage::PortalHubCheckStatus => {
            let wizard = &mut portal.ui.portal_hub_wizard;
            if wizard.active {
                if wizard.status_loading || wizard.auth_loading {
                    return Task::none();
                }
                let settings = match wizard.checked_settings() {
                    Ok(settings) => settings,
                    Err(error) => {
                        wizard.status_error = Some(error);
                        return Task::none();
                    }
                };
                wizard.status_loading = true;
                wizard.status_error = None;
                let generation = wizard.generation;
                return Task::perform(
                    async move { crate::proxy::check_proxy_status(&settings).await },
                    move |result| {
                        Message::Ui(UiMessage::PortalHubDraftStatusLoaded(generation, result))
                    },
                );
            }
            portal.ui.portal_hub_status_loading = true;
            let request = uuid::Uuid::new_v4();
            portal.ui.portal_hub_status_request = request;
            let settings = portal.prefs.portal_hub.clone();
            let endpoint = settings.effective_web_url();
            return Task::perform(
                async move { crate::proxy::check_proxy_status(&settings).await },
                move |result| {
                    Message::Ui(UiMessage::PortalHubStatusLoaded(
                        request,
                        endpoint.clone(),
                        result,
                    ))
                },
            );
        }
        UiMessage::PortalHubDraftStatusLoaded(generation, result) => {
            let wizard = &mut portal.ui.portal_hub_wizard;
            if !wizard.accepts(generation) {
                return Task::none();
            }
            if let Ok(status) = &result {
                apply_portal_hub_discovery(&mut wizard.draft, status);
            }
            wizard.receive_status(generation, result);
        }
        UiMessage::PortalHubStatusLoaded(request, endpoint, result) => {
            if portal.ui.portal_hub_status_request != request
                || portal.prefs.portal_hub.effective_web_url() != endpoint
            {
                return Task::none();
            }
            portal.ui.portal_hub_status_loading = false;
            match result {
                Ok(status) => {
                    apply_portal_hub_discovery(&mut portal.prefs.portal_hub, &status);
                    portal.save_settings();
                    portal.ui.portal_hub_status = Some(status);
                    portal.ui.portal_hub_status_error = None;
                }
                Err(error) => {
                    portal.ui.portal_hub_status_error = Some(error);
                }
            }
        }
        UiMessage::PortalHubRunDiagnostics => {
            portal.ui.portal_hub_diagnostics_loading = true;
            portal.ui.portal_hub_diagnostics = None;
            let settings = portal.prefs.portal_hub.clone();
            return Task::perform(
                async move { crate::hub::diagnostics::run_portal_hub_diagnostics(settings).await },
                |report| Message::Ui(UiMessage::PortalHubDiagnosticsDone(report)),
            );
        }
        UiMessage::PortalHubDiagnosticsDone(report) => {
            portal.ui.portal_hub_diagnostics_loading = false;
            let summary = report.summary();
            if report.fail_count() > 0 {
                portal
                    .toast_manager
                    .push(Toast::error(format!("Portal Hub Doctor: {}", summary)));
            } else {
                portal
                    .toast_manager
                    .push(Toast::success(format!("Portal Hub Doctor: {}", summary)));
            }
            portal.ui.portal_hub_diagnostics = Some(report);
        }
        UiMessage::PortalHubAuthenticate => {
            let endpoint = if portal.ui.portal_hub_wizard.active {
                portal.ui.portal_hub_wizard.draft.effective_web_url()
            } else {
                portal.prefs.portal_hub.effective_web_url()
            };
            if portal
                .ui
                .portal_hub_logout
                .as_ref()
                .is_some_and(|(_, url)| *url == endpoint)
            {
                portal.ui.portal_hub_wizard.auth_error =
                    Some("Wait for sign out to finish, then sign in again.".into());
                return Task::none();
            }
            let wizard = &mut portal.ui.portal_hub_wizard;
            if wizard.active {
                if wizard.connection_dirty || wizard.status.is_none() || wizard.auth_loading {
                    return Task::none();
                }
                wizard.auth_loading = true;
                wizard.auth_error = None;
                let settings = wizard.draft.clone();
                let generation = wizard.generation;
                let (abort, registration) = futures::future::AbortHandle::new_pair();
                wizard.auth_abort = Some(abort);
                return Task::perform(
                    async move {
                        futures::future::Abortable::new(authenticate_hub(settings), registration)
                            .await
                    },
                    move |result| match result {
                        Ok(result) => {
                            Message::Ui(UiMessage::PortalHubDraftAuthenticated(generation, result))
                        }
                        Err(_) => Message::Noop,
                    },
                );
            }
            if portal.ui.portal_hub_auth_loading {
                return Task::none();
            }
            portal.ui.portal_hub_auth_error = None;
            portal.ui.portal_hub_auth_loading = true;
            let request = uuid::Uuid::new_v4();
            portal.ui.portal_hub_auth_request = request;
            let settings = portal.prefs.portal_hub.clone();
            let endpoint = settings.effective_web_url();
            return Task::perform(authenticate_hub(settings), move |result| {
                Message::Ui(UiMessage::PortalHubAuthenticated(
                    request,
                    endpoint.clone(),
                    result,
                ))
            });
        }
        UiMessage::PortalHubDraftAuthenticated(generation, result) => {
            let wizard = &mut portal.ui.portal_hub_wizard;
            if !wizard.accepts(generation) {
                return Task::none();
            }
            wizard.auth_loading = false;
            wizard.auth_abort = None;
            match result {
                Ok((info, auth)) => {
                    apply_portal_hub_info(&mut wizard.draft, &info);
                    wizard.receive_auth(generation, auth);
                }
                Err(error) => {
                    wizard.auth_error = Some(error);
                }
            }
        }
        UiMessage::PortalHubAuthenticated(request, endpoint, result) => {
            if portal.ui.portal_hub_auth_request != request
                || portal.prefs.portal_hub.effective_web_url() != endpoint
            {
                return Task::none();
            }
            portal.ui.portal_hub_auth_loading = false;
            match result {
                Ok((info, auth)) => {
                    if let Err(error) = auth.commit() {
                        portal.ui.portal_hub_auth_error = Some(error);
                        return Task::none();
                    }
                    apply_portal_hub_info(&mut portal.prefs.portal_hub, &info);
                    portal.prefs.portal_hub.enabled = true;
                    portal.ui.portal_hub_auth_user = Some(format!(
                        "{} @ {}",
                        auth.summary.username, auth.summary.hub_url
                    ));
                    portal.ui.portal_hub_auth_error = None;
                    save_settings_and_queue_sync(portal);
                    portal
                        .toast_manager
                        .push(Toast::success("Signed in to Portal Hub"));
                    return portal_hub_sync_task(portal, SyncRunOrigin::Login, true);
                }
                Err(error) => {
                    portal.ui.portal_hub_auth_error = Some(error);
                }
            }
        }
        UiMessage::PortalHubLogout => {
            if portal.ui.portal_hub_logout.is_some() {
                return Task::none();
            }
            let endpoint = portal.prefs.portal_hub.effective_web_url();
            portal
                .ui
                .portal_hub_wizard
                .invalidate_inherited_auth(portal.ui.portal_hub_auth_request, &endpoint);
            let request = uuid::Uuid::new_v4();
            portal.ui.portal_hub_auth_request = request;
            portal.ui.portal_hub_logout = Some((request, endpoint.clone()));
            // Stop exposing the old account immediately, including to newly opened drafts.
            portal.ui.portal_hub_auth_user = None;
            invalidate_hub_sync(portal);
            portal.ui.portal_hub_auth_loading = true;
            portal.ui.portal_hub_auth_error = None;
            let settings = portal.prefs.portal_hub.clone();
            return Task::perform(
                async move { crate::hub::auth::logout(&settings) },
                move |result| {
                    Message::Ui(UiMessage::PortalHubLoggedOut(
                        request,
                        endpoint.clone(),
                        result,
                    ))
                },
            );
        }
        UiMessage::PortalHubLoggedOut(request, endpoint, result) => {
            if portal.ui.portal_hub_logout.as_ref() != Some(&(request, endpoint.clone())) {
                return Task::none();
            }
            portal.ui.portal_hub_logout = None;
            if portal.ui.portal_hub_auth_request != request
                || portal.prefs.portal_hub.effective_web_url() != endpoint
            {
                if result.is_err() {
                    portal.toast_manager.push(Toast::error("Could not remove the previous Hub account from the keychain. Sign out of that Hub again to retry."));
                }
                return Task::none();
            }
            portal.ui.portal_hub_auth_loading = false;
            match result {
                Ok(()) => {
                    portal.prefs.portal_hub.enabled = false;
                    portal.ui.portal_hub_auth_user = None;
                    portal.ui.portal_hub_auth_error = None;
                    portal.ui.portal_hub_sync_error = None;
                    portal.ui.portal_hub_sync_status = None;
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
                    let hosts = crate::hub::sync::parse_synced_hosts(profile.hosts)?;
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
        UiMessage::PortalHubRemoteRevisions(generation, endpoint, result) => {
            if generation != portal.ui.portal_hub_auth_request
                || endpoint != portal.prefs.portal_hub.effective_web_url()
            {
                return Task::none();
            }
            match result {
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
            }
        }
        UiMessage::PortalHubSyncDone(context, origin, result) => {
            if !accepts_hub_sync(portal, &context) {
                return Task::none();
            }
            portal.ui.portal_hub_sync_context = None;
            portal.ui.portal_hub_sync_abort = None;
            portal.ui.portal_hub_sync_loading = false;
            match result {
                Ok(SyncRunResult::Synced(summary)) => {
                    let message = summary.message().to_string();
                    portal.ui.portal_hub_sync_error = None;
                    portal.ui.portal_hub_sync_status = Some(message.clone());
                    portal.ui.portal_hub_conflict_context = None;
                    portal.ui.portal_hub_conflicts.clear();
                    portal.ui.portal_hub_conflict_choices.clear();
                    reload_synced_config(portal);
                    if should_show_sync_success(origin, summary.activity) {
                        portal.toast_manager.push(Toast::success(message));
                    }
                }
                Ok(SyncRunResult::Conflicts(conflicts)) => {
                    portal.ui.portal_hub_sync_error = None;
                    portal.ui.portal_hub_sync_status = Some("Conflicts need review".to_string());
                    portal.ui.portal_hub_conflict_choices =
                        vec![ConflictChoice::Local; conflicts.len()];
                    if crate::hub::sync::validate_conflicts(&portal.prefs.portal_hub, &conflicts)
                        .is_err()
                    {
                        return Task::none();
                    }
                    portal.ui.portal_hub_conflict_context = Some(context);
                    portal.ui.portal_hub_conflicts = conflicts;
                    if !portal.ui.portal_hub_wizard.active {
                        portal.dialogs.open_portal_hub_conflicts();
                    }
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
            if portal.ui.portal_hub_conflicts.is_empty() || portal.ui.portal_hub_sync_loading {
                return Task::none();
            }
            if !portal
                .ui
                .portal_hub_conflict_context
                .as_ref()
                .is_some_and(|context| {
                    context.matches(&portal.prefs.portal_hub, portal.ui.portal_hub_auth_request)
                })
                || crate::hub::sync::validate_conflicts(
                    &portal.prefs.portal_hub,
                    &portal.ui.portal_hub_conflicts,
                )
                .is_err()
            {
                invalidate_hub_sync(portal);
                return Task::none();
            }
            if !matches!(
                portal.dialogs.active(),
                crate::app::managers::ActiveDialog::PortalHubConflicts
            ) {
                portal.dialogs.open_portal_hub_conflicts();
                return Task::none();
            }
            let context = begin_hub_sync(portal);
            let (abort, registration) = futures::future::AbortHandle::new_pair();
            portal.ui.portal_hub_sync_abort = Some(abort);
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
                futures::future::Abortable::new(
                    async move {
                        let vault = HubVaultConfig::load()?;
                        let profile = LocalSyncProfile { vault, ..profile };
                        crate::hub::sync::resolve_sync_conflicts(settings, profile, conflicts).await
                    },
                    registration,
                ),
                move |result| match result {
                    Ok(result) => Message::Ui(UiMessage::PortalHubResolveConflictsDone(
                        context.clone(),
                        result,
                    )),
                    Err(_) => Message::Noop,
                },
            );
        }
        UiMessage::PortalHubResolveConflictsDone(context, result) => {
            if !accepts_hub_sync(portal, &context) {
                return Task::none();
            }
            portal.ui.portal_hub_sync_context = None;
            portal.ui.portal_hub_sync_abort = None;
            portal.ui.portal_hub_sync_loading = false;
            match result {
                Ok(message) => {
                    portal.ui.portal_hub_sync_error = None;
                    portal.ui.portal_hub_sync_status = Some(message.clone());
                    portal.ui.portal_hub_conflict_context = None;
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

fn begin_hub_sync(portal: &mut Portal) -> SyncContext {
    let context = SyncContext::new(&portal.prefs.portal_hub, portal.ui.portal_hub_auth_request);
    portal.ui.portal_hub_sync_context = Some(context.clone());
    portal.ui.portal_hub_sync_loading = true;
    context
}

fn accepts_hub_sync(portal: &Portal, context: &SyncContext) -> bool {
    portal.ui.portal_hub_sync_context.as_ref() == Some(context)
        && context.matches(&portal.prefs.portal_hub, portal.ui.portal_hub_auth_request)
}

fn invalidate_hub_sync(portal: &mut Portal) {
    if let Some(abort) = portal.ui.portal_hub_sync_abort.take() {
        abort.abort();
    }
    portal.ui.portal_hub_sync_context = None;
    portal.ui.portal_hub_conflict_context = None;
    portal.ui.portal_hub_sync_loading = false;
    portal.ui.portal_hub_conflicts.clear();
    portal.ui.portal_hub_conflict_choices.clear();
    portal.ui.portal_hub_local_sync_pending = false;
    portal.ui.portal_hub_remote_sync_pending = false;
    portal.dialogs.close_portal_hub_conflicts();
}

pub(crate) fn portal_hub_sync_task(
    portal: &mut Portal,
    origin: SyncRunOrigin,
    force: bool,
) -> Task<Message> {
    if portal.ui.portal_hub_auth_user.is_none() || !portal.prefs.portal_hub.sync_configured() {
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
    let context = begin_hub_sync(portal);
    let settings = portal.prefs.portal_hub.clone();
    let profile = current_sync_profile(portal);
    let vault = match HubVaultConfig::load() {
        Ok(vault) => vault,
        Err(error) => {
            return Task::done(Message::Ui(UiMessage::PortalHubSyncDone(
                context,
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
                    context,
                    origin,
                    Ok(SyncRunResult::Synced(
                        crate::hub::sync::SyncRunSummary::new(SyncRunActivity::NoChanges),
                    )),
                )));
            }
            Ok(true) => {}
            Err(error) => {
                return Task::done(Message::Ui(UiMessage::PortalHubSyncDone(
                    context,
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
    let (abort, registration) = futures::future::AbortHandle::new_pair();
    portal.ui.portal_hub_sync_abort = Some(abort);
    Task::perform(
        futures::future::Abortable::new(
            crate::hub::sync::run_bidirectional_sync(settings, profile),
            registration,
        ),
        move |result| match result {
            Ok(result) => Message::Ui(UiMessage::PortalHubSyncDone(
                context.clone(),
                origin,
                result,
            )),
            Err(_) => Message::Noop,
        },
    )
}

/// Reset the onboarding Defaults state: every eligible host selected, toggles
/// seeded from current preferences.
fn init_portal_hub_wizard(portal: &mut Portal) {
    portal
        .dialogs
        .close_onboarding(portal.ui.portal_hub_wizard.managed.operation);
    portal.ui.portal_hub_wizard.cancel();
    let mut wizard = crate::app::PortalHubWizardState::begin(portal.prefs.portal_hub.clone());
    wizard.status = portal.ui.portal_hub_status.clone();
    wizard.inherit_auth(
        portal.ui.portal_hub_auth_request,
        portal.ui.portal_hub_auth_user.clone(),
    );
    wizard.managed = crate::hub::deployment::ManagedHubState::load();
    portal.ui.portal_hub_wizard = wizard;
}

async fn authenticate_hub(
    settings: crate::config::settings::PortalHubSettings,
) -> Result<(crate::hub::auth::HubInfo, crate::hub::auth::PendingHubAuth), String> {
    let info = crate::hub::auth::fetch_hub_info(&settings).await?;
    validate_portal_hub_info(&info)?;
    let auth = crate::hub::auth::authenticate(settings).await?;
    Ok((info, auth))
}

/// Publish the draft only after all writes succeed. A failed write restores the
/// previous files; background sync sees the old in-memory context throughout.
fn apply_portal_hub_wizard(portal: &mut Portal) -> Task<Message> {
    let wizard = &portal.ui.portal_hub_wizard;
    if !wizard.can_finish() {
        return Task::none();
    }
    if portal.ui.portal_hub_sync_loading {
        portal.ui.portal_hub_wizard.auth_error =
            Some("Wait for the current Hub sync to finish, then save again.".into());
        return Task::none();
    }
    if portal
        .ui
        .portal_hub_logout
        .as_ref()
        .is_some_and(|(_, url)| *url == wizard.draft.effective_web_url())
    {
        portal.ui.portal_hub_wizard.auth_error =
            Some("Wait for sign out to finish, then sign in again.".into());
        return Task::none();
    }
    if wizard.auth.is_none()
        && !wizard
            .inherited_auth
            .as_ref()
            .is_some_and(|(generation, endpoint)| {
                *generation == portal.ui.portal_hub_auth_request
                    && *endpoint == portal.prefs.portal_hub.effective_web_url()
                    && portal.ui.portal_hub_auth_user.is_some()
            })
    {
        portal.ui.portal_hub_wizard.auth_user = None;
        portal.ui.portal_hub_wizard.auth_error = Some("Sign in again before saving.".into());
        return Task::none();
    }
    let previous_settings = current_settings_config(portal);
    let previous_hosts = portal.config.hosts.clone();
    let mut settings = previous_settings.clone();
    settings.portal_hub = wizard.draft.clone();
    let hub = &mut settings.portal_hub;
    hub.enabled = true;
    hub.default_for_new_ssh_hosts = wizard.route_default;
    hub.prefer_vault_keys = wizard.prefer_vault;
    hub.hosts_sync_enabled = wizard.sync_all;
    hub.settings_sync_enabled = wizard.sync_all;
    hub.snippets_sync_enabled = wizard.sync_all;
    hub.key_vault_enabled = wizard.sync_all;
    let mut hosts = previous_hosts.clone();
    let recovery_hosts = wizard.managed.recovery_hosts(&previous_hosts.hosts);
    for host in &mut hosts.hosts {
        if !host.hub_eligible() {
            continue;
        }
        // Keep the management host and each of its bastions independent of Hub.
        let recovery = recovery_hosts.contains(&host.id);
        let routing =
            if recovery || wizard.excluded_hosts.contains(&host.id) || !wizard.route_default {
                crate::config::hosts::HubRouting::Direct
            } else {
                crate::config::hosts::HubRouting::Auto
            };
        if host.hub_routing != routing {
            host.hub_routing = routing;
            host.updated_at = chrono::Utc::now();
        }
    }
    let result = (|| -> Result<(), String> {
        hosts
            .save()
            .map_err(|_| "Could not save host routing".to_string())?;
        settings
            .save()
            .map_err(|_| "Could not save Hub settings".to_string())?;
        if let Some(auth) = &wizard.auth {
            auth.commit()?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let hosts_restored = previous_hosts.save().is_ok();
        let settings_restored = previous_settings.save().is_ok();
        let restored = hosts_restored && settings_restored;
        portal.ui.portal_hub_wizard.auth_error = Some(if restored {
            error
        } else {
            format!(
                "{error}. Previous settings could not be restored on disk; check free space and file permissions before closing Portal."
            )
        });
        return Task::none();
    }
    publish_portal_hub_wizard(portal, settings.portal_hub, hosts)
}

/// In-memory publication boundary, reached only after the draft is persisted.
fn publish_portal_hub_wizard(
    portal: &mut Portal,
    settings: crate::config::settings::PortalHubSettings,
    hosts: crate::config::HostsConfig,
) -> Task<Message> {
    let wizard = &portal.ui.portal_hub_wizard;
    portal.prefs.portal_hub = settings;
    portal.ui.portal_hub_auth_request = uuid::Uuid::new_v4();
    portal.ui.portal_hub_status_request = uuid::Uuid::new_v4();
    portal.ui.portal_hub_status_loading = false;
    portal.ui.portal_hub_auth_loading = false;
    portal.config.hosts = hosts;
    portal.ui.portal_hub_auth_user = wizard.auth_user.clone();
    portal.ui.portal_hub_auth_error = None;
    portal.ui.portal_hub_status = wizard.status.clone();
    invalidate_hub_sync(portal);
    portal.ui.portal_hub_local_sync_pending = false;
    portal.ui.portal_hub_remote_sync_pending = false;
    portal
        .dialogs
        .close_onboarding(portal.ui.portal_hub_wizard.managed.operation);
    portal.ui.portal_hub_wizard.cancel();
    portal.ui.hub_prompt_dismissed = true;
    portal
        .toast_manager
        .push(Toast::success("Portal Hub settings saved"));
    portal_hub_sync_task(portal, SyncRunOrigin::Login, true)
}

pub(crate) fn queue_portal_hub_local_sync(portal: &mut Portal) {
    if portal.ui.portal_hub_auth_user.is_some()
        && portal.prefs.portal_hub.sync_configured()
        && portal.ui.portal_hub_conflicts.is_empty()
    {
        portal.ui.portal_hub_local_sync_pending = true;
    }
}

pub(crate) fn save_settings_and_queue_sync(portal: &mut Portal) {
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
    invalidate_hub_sync(portal);
    portal.ui.portal_hub_status = None;
    portal.ui.portal_hub_status_error = None;
    portal.ui.portal_hub_status_loading = false;
    portal.ui.portal_hub_diagnostics = None;
    portal.ui.portal_hub_diagnostics_loading = false;
}

fn apply_portal_hub_discovery(
    settings: &mut crate::config::settings::PortalHubSettings,
    status: &crate::proxy::ProxyStatus,
) {
    settings.port = status.ssh_port.unwrap_or(2222);
    settings.username = status
        .ssh_username
        .clone()
        .unwrap_or_else(|| "portal-hub".to_string());
}

fn apply_portal_hub_info(
    settings: &mut crate::config::settings::PortalHubSettings,
    info: &crate::hub::auth::HubInfo,
) {
    settings.port = info.ssh_port.unwrap_or(2222);
    settings.username = info
        .ssh_username
        .clone()
        .unwrap_or_else(|| "portal-hub".to_string());
}

fn validate_portal_hub_info(info: &crate::hub::auth::HubInfo) -> Result<(), String> {
    if info.api_version != 2 {
        return Err(format!(
            "Portal Hub API version {} is unsupported; Portal supports API 2",
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

#[cfg(test)]
mod hub_onboarding_tests {
    use super::*;
    use crate::{app::PortalHubWizardState, hub::sync::SyncConflict};

    fn connected_portal() -> Portal {
        let mut portal = Portal::test_state();
        portal
            .prefs
            .portal_hub
            .apply_discovered_web_url("https://hub-a.example");
        portal.ui.portal_hub_auth_user = Some("owner @ https://hub-a.example".into());
        portal.ui.portal_hub_auth_request = uuid::Uuid::new_v4();
        portal.ui.portal_hub_status = Some(crate::proxy::ProxyStatus {
            version: "test".into(),
            api_version: 2,
            metadata_schema_version: 2,
            public_url: "https://hub-a.example".into(),
            ssh_port: Some(2222),
            ssh_username: None,
            sync_v2: true,
            sync_events: true,
            web_proxy: true,
            session_titles: true,
            key_vault: true,
            vault_enrollment: true,
        });
        portal
    }

    fn conflict() -> SyncConflict {
        SyncConflict {
            hub_url: "https://hub-a.example".into(),
            service: "hosts".into(),
            local: serde_json::json!({"private": "Hub A local payload"}),
            hub: serde_json::json!({"private": "Hub A remote payload"}),
            expected_revision: "a1".into(),
        }
    }

    fn receive_conflict(portal: &mut Portal) -> SyncContext {
        let context = begin_hub_sync(portal);
        let task = handle_settings_message(
            portal,
            UiMessage::PortalHubSyncDone(
                context.clone(),
                SyncRunOrigin::Manual,
                Ok(SyncRunResult::Conflicts(vec![conflict()])),
            ),
        );
        assert_eq!(task.units(), 0);
        assert_eq!(portal.ui.portal_hub_conflicts.len(), 1);
        context
    }

    #[test]
    fn publishing_hub_b_without_sync_discards_a_conflicts_and_rejects_late_results() {
        let mut portal = connected_portal();
        let a = receive_conflict(&mut portal);
        let mut b = portal.prefs.portal_hub.clone();
        b.apply_discovered_web_url("https://hub-b.example");
        b.hosts_sync_enabled = false;
        b.settings_sync_enabled = false;
        b.snippets_sync_enabled = false;
        b.key_vault_enabled = false;
        portal.ui.portal_hub_wizard = PortalHubWizardState::begin(b.clone());
        portal.ui.portal_hub_wizard.auth_user = Some("owner @ https://hub-b.example".into());
        assert_eq!(
            publish_portal_hub_wizard(&mut portal, b, Default::default()).units(),
            0
        );
        assert!(portal.ui.portal_hub_conflicts.is_empty());
        assert_eq!(
            handle_settings_message(&mut portal, UiMessage::PortalHubResolveConflicts).units(),
            0
        );
        let b = begin_hub_sync(&mut portal);
        for event in [
            UiMessage::PortalHubSyncDone(
                a.clone(),
                SyncRunOrigin::Manual,
                Ok(SyncRunResult::Conflicts(vec![conflict()])),
            ),
            UiMessage::PortalHubSyncDone(
                a.clone(),
                SyncRunOrigin::Manual,
                Ok(SyncRunResult::Synced(
                    crate::hub::sync::SyncRunSummary::new(SyncRunActivity::AppliedRemoteChanges),
                )),
            ),
            UiMessage::PortalHubResolveConflictsDone(a, Ok("old completion".into())),
        ] {
            assert_eq!(handle_settings_message(&mut portal, event).units(), 0);
            assert!(portal.ui.portal_hub_conflicts.is_empty());
            assert!(portal.ui.portal_hub_sync_loading);
            assert_eq!(portal.ui.portal_hub_sync_context.as_ref(), Some(&b));
            assert_eq!(
                portal.ui.portal_hub_auth_user.as_deref(),
                Some("owner @ https://hub-b.example")
            );
        }
    }

    #[test]
    fn disabled_service_cannot_dispatch_conflict_resolution() {
        let mut portal = connected_portal();
        receive_conflict(&mut portal);
        portal.prefs.portal_hub.hosts_sync_enabled = false;
        let task = handle_settings_message(&mut portal, UiMessage::PortalHubResolveConflicts);
        assert_eq!(task.units(), 0);
        assert!(portal.ui.portal_hub_conflicts.is_empty());
        assert!(!portal.ui.portal_hub_sync_loading);
    }

    #[test]
    fn older_generation_cannot_replace_new_sync_at_the_same_endpoint() {
        let mut portal = connected_portal();
        let old = begin_hub_sync(&mut portal);
        invalidate_hub_sync(&mut portal);
        let current = begin_hub_sync(&mut portal);
        let _ = handle_settings_message(
            &mut portal,
            UiMessage::PortalHubSyncDone(
                old,
                SyncRunOrigin::Manual,
                Ok(SyncRunResult::Conflicts(vec![conflict()])),
            ),
        );
        assert!(portal.ui.portal_hub_conflicts.is_empty());
        assert!(accepts_hub_sync(&portal, &current));
        assert!(portal.ui.portal_hub_sync_loading);
    }

    #[test]
    fn delayed_logout_invalidates_existing_and_newly_opened_drafts() {
        let mut portal = connected_portal();
        let _ = handle_settings_message(&mut portal, UiMessage::PortalHubOpenOnboarding);
        assert!(portal.ui.portal_hub_wizard.can_finish());
        let logout = handle_settings_message(&mut portal, UiMessage::PortalHubLogout);
        assert_eq!(logout.units(), 1); // Keep the keychain worker pending.
        assert!(!portal.ui.portal_hub_wizard.can_finish());
        assert!(portal.ui.portal_hub_auth_user.is_none());
        let _ = handle_settings_message(&mut portal, UiMessage::PortalHubOpenOnboarding);
        assert!(!portal.ui.portal_hub_wizard.can_finish());
        assert!(portal.ui.portal_hub_wizard.auth_user.is_none());
        assert_eq!(
            handle_settings_message(&mut portal, UiMessage::PortalHubWizardApply).units(),
            0
        );
        assert_eq!(
            handle_settings_message(&mut portal, UiMessage::PortalHubAuthenticate).units(),
            0
        );
    }

    #[test]
    fn completed_delayed_logout_cannot_be_undone_by_finishing_an_inherited_draft() {
        if Portal::isolate_config_test() {
            return;
        }
        let mut portal = connected_portal();
        let _ = handle_settings_message(&mut portal, UiMessage::PortalHubLogout);
        let (request, endpoint) = portal.ui.portal_hub_logout.clone().unwrap();
        let _ = handle_settings_message(&mut portal, UiMessage::PortalHubOpenOnboarding);
        assert!(portal.ui.portal_hub_wizard.status.is_some());
        let _ = handle_settings_message(
            &mut portal,
            UiMessage::PortalHubLoggedOut(request, endpoint, Ok(())),
        );
        assert!(portal.ui.portal_hub_logout.is_none());
        assert!(!portal.ui.portal_hub_wizard.can_finish());
        assert_eq!(
            handle_settings_message(&mut portal, UiMessage::PortalHubWizardApply).units(),
            0
        );
        assert!(portal.ui.portal_hub_auth_user.is_none());
        assert!(portal.ui.portal_hub_wizard.auth_user.is_none());
        assert!(!portal.prefs.portal_hub.enabled);
        assert!(!SettingsConfig::load().unwrap().portal_hub.enabled);
        assert!(portal.ui.portal_hub_wizard.active);
    }

    #[tokio::test]
    async fn disabling_a_service_aborts_active_sync_and_rejects_late_conflicts() {
        if Portal::isolate_config_test() {
            return;
        }
        let mut portal = connected_portal();
        let context = begin_hub_sync(&mut portal);
        let (abort, registration) = futures::future::AbortHandle::new_pair();
        portal.ui.portal_hub_sync_abort = Some(abort);
        let _ = handle_settings_message(
            &mut portal,
            UiMessage::PortalHubDisableSyncKeepData(PortalHubSyncService::Hosts),
        );
        assert!(
            futures::future::Abortable::new(std::future::pending::<()>(), registration)
                .await
                .is_err()
        );
        assert!(
            !SettingsConfig::load()
                .unwrap()
                .portal_hub
                .hosts_sync_enabled
        );
        let _ = handle_settings_message(
            &mut portal,
            UiMessage::PortalHubSyncDone(
                context,
                SyncRunOrigin::Manual,
                Ok(SyncRunResult::Conflicts(vec![conflict()])),
            ),
        );
        assert!(portal.ui.portal_hub_conflicts.is_empty());
        assert!(!portal.ui.portal_hub_sync_loading);
    }

    #[test]
    fn delayed_logout_keeps_new_authentication_for_a_different_origin() {
        let mut portal = connected_portal();
        let _ = handle_settings_message(&mut portal, UiMessage::PortalHubLogout);
        let (request, endpoint) = portal.ui.portal_hub_logout.clone().unwrap();
        let _ = handle_settings_message(&mut portal, UiMessage::PortalHubOpenOnboarding);
        let wizard = &mut portal.ui.portal_hub_wizard;
        wizard.edit_endpoint("https://hub-b.example".into());
        wizard.checked_settings().unwrap();
        wizard.receive_auth(
            wizard.generation,
            crate::hub::auth::PendingHubAuth::test_auth("https://hub-b.example"),
        );
        let _ = handle_settings_message(
            &mut portal,
            UiMessage::PortalHubLoggedOut(uuid::Uuid::new_v4(), endpoint.clone(), Ok(())),
        );
        assert!(portal.ui.portal_hub_logout.is_some());
        let mut b = portal.ui.portal_hub_wizard.draft.clone();
        b.hosts_sync_enabled = false;
        b.settings_sync_enabled = false;
        b.snippets_sync_enabled = false;
        b.key_vault_enabled = false;
        let expected_user = portal.ui.portal_hub_wizard.auth_user.clone();
        let _ = publish_portal_hub_wizard(&mut portal, b, Default::default());
        let _ = handle_settings_message(
            &mut portal,
            UiMessage::PortalHubLoggedOut(request, endpoint, Ok(())),
        );
        assert!(portal.ui.portal_hub_logout.is_none());
        assert_eq!(portal.ui.portal_hub_auth_user, expected_user);
        assert_eq!(
            portal.prefs.portal_hub.effective_web_url(),
            "https://hub-b.example"
        );
    }
    fn info(version: u16) -> crate::hub::auth::HubInfo {
        serde_json::from_value(serde_json::json!({
            "api_version": version, "public_url": "https://different.example",
            "capabilities": {"web_proxy":true,"sync_v2":true,"key_vault":true},
            "ssh_port":2222, "ssh_username":"portal-hub"
        }))
        .unwrap()
    }
    #[test]
    fn discovery_preserves_consent_and_the_selected_origin() {
        let mut settings = crate::config::settings::PortalHubSettings::default();
        settings.apply_discovered_web_url("https://chosen.example");
        settings.enabled = false;
        settings.hosts_sync_enabled = false;
        settings.settings_sync_enabled = true;
        settings.snippets_sync_enabled = false;
        settings.key_vault_enabled = false;
        apply_portal_hub_info(&mut settings, &info(2));
        assert!(!settings.enabled);
        assert!(!settings.hosts_sync_enabled);
        assert!(settings.settings_sync_enabled);
        assert!(!settings.snippets_sync_enabled);
        assert!(!settings.key_vault_enabled);
        assert_eq!(settings.effective_web_url(), "https://chosen.example");
    }
    #[test]
    fn negotiation_rejects_newer_unknown_api_versions() {
        assert!(validate_portal_hub_info(&info(2)).is_ok());
        for version in [0, 1, 3, u16::MAX] {
            assert!(validate_portal_hub_info(&info(version)).is_err());
        }
    }
}
