use crate::{
    app::Portal,
    hub::deployment::{
        self, DeploymentAction as Action, DeploymentRecord, DeploymentRequest, ManagedHubState,
        ManagementMessage as M, SetupChoice,
    },
    message::{Message, UiMessage},
    ssh::SshEvent,
};
use iced::Task;
use uuid::Uuid;

fn message(value: M) -> Message {
    Message::Ui(UiMessage::PortalHubManagement(value))
}

pub(super) fn handle(portal: &mut Portal, value: M) -> Task<Message> {
    if let M::CleanupFailed {
        host, directory, ..
    } = value
    {
        let server = portal
            .config
            .hosts
            .hosts
            .iter()
            .find(|h| h.id == host)
            .map(|h| deployment::bounded_text(&format!("{} ({})", h.name, h.hostname), 256))
            .unwrap_or_else(|| format!("SSH host {host}"));
        let warning = format!(
            "Hub upload cleanup failed on {server}: /tmp/portal-hub-{directory}. Remove this directory through SSH, then refresh Hub status before retrying."
        );
        if !portal.ui.portal_hub_cleanup_warnings.contains(&warning) {
            portal.ui.portal_hub_cleanup_warnings.push(warning.clone());
            portal
                .toast_manager
                .push(crate::views::toast::Toast::warning(warning).persistent());
        }
        return Task::none();
    }
    if !portal.ui.portal_hub_wizard.active {
        return Task::none();
    }
    match value {
        M::CleanupFailed { .. } => unreachable!(),
        M::Choose(choice) => {
            let wizard = &mut portal.ui.portal_hub_wizard;
            portal.dialogs.finish_management(wizard.managed.operation);
            wizard.managed.cancel();
            wizard.managed.choice = choice;
            wizard.edit_endpoint(if choice == SetupChoice::Server {
                String::new()
            } else {
                wizard.hub_url_input.clone()
            });
        }
        M::SelectHost(id) => {
            let state = &mut portal.ui.portal_hub_wizard.managed;
            if state.busy || state.selected.is_some() {
                return Task::none();
            }
            state.host_id = Some(id);
            state.preflight = None;
            state.response = None;
            state.error = None;
        }
        M::Resume(record) => {
            let wizard = &mut portal.ui.portal_hub_wizard;
            if wizard.managed.busy {
                return Task::none();
            }
            wizard.edit_endpoint(String::new());
            let state = &mut wizard.managed;
            state.host_id = Some(record.ssh_host_id);
            state.hostname = record.hostname.clone();
            state.image = record.hub_image.clone();
            state.selected = Some(record);
            state.response = None;
            state.preflight = None;
            state.error = None;
            state.install_docker = false;
            state.allow_interruptions = false;
            state.uninstall_confirmed = false;
            state.archive = None;
            state.progress = "Recovery record loaded. Check the server to continue.".into();
        }
        M::NewDeployment => {
            if portal.ui.portal_hub_wizard.managed.busy {
                return Task::none();
            }
            portal.ui.portal_hub_wizard.edit_endpoint(String::new());
            portal.ui.portal_hub_wizard.managed = ManagedHubState {
                choice: SetupChoice::Server,
                ..ManagedHubState::load()
            };
        }
        M::Cancel => {
            let state = &mut portal.ui.portal_hub_wizard.managed;
            portal.dialogs.finish_management(state.operation);
            state.cancel();
            state.progress = "Stopped waiting. Changes already made on the server are retained. Refresh status before retrying.".into();
        }
        M::Hostname(value) => {
            if !portal.ui.portal_hub_wizard.managed.busy {
                portal.ui.portal_hub_wizard.managed.hostname = value;
            }
        }
        M::Image(value) => {
            if !portal.ui.portal_hub_wizard.managed.busy {
                portal.ui.portal_hub_wizard.managed.image = value;
            }
        }
        M::InstallDocker(value) => portal.ui.portal_hub_wizard.managed.install_docker = value,
        M::AllowInterruptions(value) => {
            portal.ui.portal_hub_wizard.managed.allow_interruptions = value
        }
        M::ConfirmUninstall(value) => {
            portal.ui.portal_hub_wizard.managed.uninstall_confirmed = value
        }
        M::OwnerName(value) => portal.ui.portal_hub_wizard.managed.owner_name = value,
        M::OwnerPassword(value) => portal.ui.portal_hub_wizard.managed.owner_password = value,
        M::ClearArchive => {
            if !portal.ui.portal_hub_wizard.managed.busy {
                portal.ui.portal_hub_wizard.managed.archive = None;
            }
        }
        M::ChooseArchive => {
            if portal.ui.portal_hub_wizard.managed.busy {
                return Task::none();
            }
            let generation = portal.ui.portal_hub_wizard.generation;
            return Task::perform(
                async {
                    rfd::AsyncFileDialog::new()
                        .set_title("Choose a Docker image archive")
                        .add_filter("Docker image tar", &["tar"])
                        .pick_file()
                        .await
                        .map(|file| file.path().to_path_buf())
                },
                move |path| message(M::ArchiveChosen(generation, path)),
            );
        }
        M::ArchiveChosen(generation, path) => {
            if portal.ui.portal_hub_wizard.accepts(generation) {
                portal.ui.portal_hub_wizard.managed.archive = path;
            }
        }
        M::OpenEnrollment => {
            let state = &mut portal.ui.portal_hub_wizard.managed;
            if let Some(url) = state
                .response
                .as_ref()
                .and_then(|r| r.string("auth_url").or_else(|| r.string("approval_url")))
            {
                match deployment::validate_enrollment_url(url) {
                    Ok(url) => {
                        if open::that(url.as_str()).is_err() {
                            state.error = Some("Could not open the browser. Check your default browser, then retry.".into());
                        }
                    }
                    Err(error) => state.error = Some(error),
                }
            }
        }
        M::UseEndpoint => {
            let wizard = &mut portal.ui.portal_hub_wizard;
            let response = wizard.managed.response.as_ref();
            if let Some(response) =
                response.filter(|r| r.ok && r.phase == "ready" && r.bool("owner_exists"))
                && let Some(url) = response.string("public_url")
                && deployment::validate_public_url(url).is_ok()
            {
                wizard.edit_endpoint(url.to_string());
                return Task::done(Message::Ui(UiMessage::PortalHubCheckStatus));
            }
            wizard.managed.error = Some(
                "Refresh status and complete owner creation and HTTPS setup before connecting."
                    .into(),
            );
        }
        M::Progress(id, text) => {
            if portal.ui.portal_hub_wizard.managed.accepts(id) {
                portal.ui.portal_hub_wizard.managed.progress = text;
            }
        }
        M::HostKey(id, mut wrapper) => {
            if portal.ui.portal_hub_wizard.managed.accepts(id)
                && let Some(request) = wrapper.0.take()
            {
                portal.dialogs.open_management_host_key(
                    id,
                    crate::views::dialogs::host_key_dialog::HostKeyDialogState::from_request(
                        *request,
                    ),
                );
            }
        }
        M::AuthPrompt(id, mut wrapper) => {
            if portal.ui.portal_hub_wizard.managed.accepts(id)
                && let Some(request) = wrapper.0.take()
            {
                portal.dialogs.open_management_auth(
                    id,
                    crate::views::dialogs::auth_prompt_dialog::AuthPromptDialogState::from_request(
                        *request,
                    ),
                );
            }
        }
        M::Completed(id, action, result) => {
            let state = &mut portal.ui.portal_hub_wizard.managed;
            if !state.accepts(id) {
                return Task::none();
            }
            state.busy = false;
            state.cancel_tx = None;
            state.owner_password = Default::default();
            state.allow_interruptions = false;
            state.uninstall_confirmed = false;
            match result {
                Ok(response) => {
                    state.progress = response.message.clone();
                    state.error = (!response.ok).then(|| match response.error_code.as_deref() {
                        Some(code) => format!("{} ({code})", response.message),
                        None => response.message.clone(),
                    });
                    if action == Action::Inspect && response.ok {
                        state.preflight = Some(response.clone());
                    }
                    if !response.ok || matches!(action, Action::Logs | Action::Doctor) {
                        state.diagnostics = Some(deployment::bounded_text(
                            &serde_json::to_string_pretty(&response.data).unwrap_or_default(),
                            12_000,
                        ));
                    } else if response.ok && action != Action::Backup {
                        state.response = Some(response.clone());
                    }
                    if response.ok {
                        if let Some(path) = response
                            .string("backup_path")
                            .or_else(|| response.string("retained_path"))
                        {
                            state.progress.push_str(&format!(
                                "\nStored on the server: {}",
                                deployment::bounded_text(path, 1024)
                            ));
                        }
                        if action == Action::Uninstall {
                            state.preflight = None;
                        }
                        if action == Action::Install
                            && let Some(preflight) = state.preflight.as_mut()
                        {
                            preflight.data["installed"] = true.into();
                        }
                        if let Some(record) = state.selected.as_mut() {
                            if let Some(url) = response.string("public_url") {
                                record.public_url = Some(url.into());
                            }
                            if matches!(action, Action::Install | Action::Update) {
                                record.hub_image = state.image.clone();
                            }
                            if let Err(error) = deployment::save_record(record) {
                                state.error = Some(error);
                            }
                            if let Some(existing) = state
                                .records
                                .iter_mut()
                                .find(|r| r.deployment_id == record.deployment_id)
                            {
                                *existing = record.clone();
                            }
                        }
                    }
                }
                Err(error) => {
                    state.error = Some(error);
                    state.progress = "The operation did not complete. Review the error and refresh status before retrying.".into();
                }
            }
            portal.dialogs.finish_management(id);
        }
        M::Run(action) => return start(portal, action),
    }
    Task::none()
}

fn start(portal: &mut Portal, action: Action) -> Task<Message> {
    let state = &mut portal.ui.portal_hub_wizard.managed;
    if state.busy {
        return Task::none();
    }
    let prepare = (|| -> Result<_, String> {
        let host_id = state.host_id.ok_or("Choose an SSH host first")?;
        let host = portal
            .config
            .hosts
            .hosts
            .iter()
            .find(|h| h.id == host_id && h.protocol == crate::config::Protocol::Ssh)
            .cloned()
            .ok_or(
                "The management SSH host is missing. Restore its host definition before resuming.",
            )?;
        let chain = crate::ssh::tunnel::resolve_jump_chain(&portal.config.hosts.hosts, &host)
            .map_err(|e| e.to_string())?;
        // Vault-backed keys require the Hub being repaired. Only independent
        // agent/file auth is suitable for a durable recovery path.
        for hop in chain.iter().chain(std::iter::once(&host)) {
            if matches!(
                &hop.auth,
                crate::config::AuthMethod::PublicKey {
                    vault_key_id: Some(_),
                    ..
                }
            ) {
                return Err(format!(
                    "{} uses a Hub Vault key. Choose an independent local SSH key, agent, or password for the recovery connection.",
                    hop.name
                ));
            }
        }
        if action == Action::Install {
            let preflight = state
                .preflight
                .as_ref()
                .ok_or("Check the server before installing")?;
            if (!preflight.bool("docker_available") || !preflight.bool("compose_available"))
                && !state.install_docker
            {
                return Err(
                    "Docker is missing. Review and approve Docker installation before continuing."
                        .into(),
                );
            }
            if !preflight.bool("tun_available") {
                return Err("The server needs /dev/net/tun for Tailscale. Enable TUN on the server, then check it again.".into());
            }
        }
        if action == Action::Uninstall && !state.uninstall_confirmed {
            return Err(
                "Confirm that you want to uninstall this Hub. Its data will be retained.".into(),
            );
        }
        if action.interrupts()
            && state
                .response
                .as_ref()
                .and_then(|r| r.sessions())
                .unwrap_or(0)
                > 0
            && !state.allow_interruptions
        {
            return Err(
                "This Hub has active sessions. Approve interrupting them before continuing.".into(),
            );
        }
        if state.selected.is_none() && action != Action::Inspect {
            return Err("Check the server first to create its recovery record".into());
        }
        if matches!(action, Action::Install | Action::Update) {
            deployment::validate_image(&state.image)?;
        }
        if state.selected.is_none() || action == Action::Install {
            deployment::validate_hostname(&state.hostname)?;
        }
        let record = state.selected.clone().unwrap_or_else(|| DeploymentRecord {
            deployment_id: Uuid::new_v4(),
            ssh_host_id: host_id,
            hostname: state.hostname.clone(),
            hub_image: state.image.clone(),
            public_url: None,
        });
        deployment::save_record(&record)?;
        if !state
            .records
            .iter()
            .any(|r| r.deployment_id == record.deployment_id)
        {
            state.records.push(record.clone());
        }
        state.selected = Some(record.clone());
        let request = DeploymentRequest {
            action,
            deployment_id: record.deployment_id,
            hostname: state.hostname.clone(),
            hub_image: state.image.clone(),
            image_archive: None,
            image_archive_sha256: None,
            install_docker: state.install_docker,
            allow_session_interruptions: state.allow_interruptions,
            username: state.owner_name.clone(),
            password: std::mem::take(&mut state.owner_password),
        };
        request.stdin_json()?;
        Ok((host, chain, request))
    })();
    let (host, chain, request) = match prepare {
        Ok(value) => value,
        Err(error) => {
            state.error = Some(error);
            return Task::none();
        }
    };
    let archive = if matches!(action, Action::Install | Action::Update) {
        state.archive.clone()
    } else {
        None
    };
    let id = Uuid::new_v4();
    let host_id = host.id;
    state.operation = id;
    state.busy = true;
    state.error = None;
    state.last_action = Some(action);
    state.progress = "Connecting directly over SSH…".into();
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    state.cancel_tx = Some(cancel_tx);
    let (events_tx, events_rx) = tokio::sync::mpsc::channel(16);
    let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(16);
    let events = Task::run(
        futures::stream::unfold(events_rx, |mut rx| async {
            rx.recv().await.map(|v| (v, rx))
        }),
        move |event| match event {
            SshEvent::HostKeyVerification(request) => message(M::HostKey(
                id,
                crate::message::VerificationRequestWrapper(Some(request)),
            )),
            SshEvent::AuthPrompt(request) => message(M::AuthPrompt(
                id,
                crate::message::AuthPromptRequestWrapper(Some(request)),
            )),
            _ => Message::Noop,
        },
    );
    let progress = Task::run(
        futures::stream::unfold(progress_rx, |mut rx| async {
            rx.recv().await.map(|v| (v, rx))
        }),
        move |value| match value {
            deployment::ManagementProgress::Status(text) => message(M::Progress(id, text)),
            deployment::ManagementProgress::CleanupFailed(directory) => message(M::CleanupFailed {
                operation: id,
                host: host_id,
                directory,
            }),
        },
    );
    let work = Task::perform(
        deployment::run_remote(
            host,
            chain,
            request,
            archive,
            events_tx,
            cancel_rx,
            progress_tx,
        ),
        move |result| message(M::Completed(id, action, result)),
    );
    Task::batch([events, progress, work])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        app::managers::ActiveDialog,
        message::{AuthPromptRequestWrapper, DialogMessage},
        ssh::auth_prompt::{AuthPromptRequest, AuthPromptResponse},
    };
    use tokio::sync::oneshot;

    fn wizard() -> (Portal, Uuid) {
        let mut portal = Portal::test_state();
        portal.ui.portal_hub_wizard = crate::app::PortalHubWizardState::begin(Default::default());
        portal.dialogs.open_portal_hub_onboarding();
        let id = Uuid::new_v4();
        portal.ui.portal_hub_wizard.managed.operation = id;
        portal.ui.portal_hub_wizard.managed.busy = true;
        (portal, id)
    }

    fn prompt(
        host: &str,
    ) -> (
        AuthPromptRequestWrapper,
        oneshot::Receiver<AuthPromptResponse>,
    ) {
        let (responder, receiver) = oneshot::channel();
        (
            AuthPromptRequestWrapper(Some(Box::new(AuthPromptRequest {
                host: host.into(),
                port: 22,
                username: "user".into(),
                name: "Password".into(),
                instructions: String::new(),
                prompts: Vec::new(),
                responder,
            }))),
            receiver,
        )
    }

    #[test]
    fn management_completion_does_not_steal_terminal_reconnect_prompt() {
        let (mut portal, id) = wizard();
        let (terminal, mut terminal_rx) = prompt("terminal");
        let _ = crate::app::update::dialog::handle_dialog(
            &mut portal,
            DialogMessage::AuthPrompt(terminal),
        );
        let (management, mut management_rx) = prompt("management");
        let _ = handle(&mut portal, M::AuthPrompt(id, management));
        assert!(
            matches!(portal.dialogs.active(), ActiveDialog::AuthPrompt(p) if p.host == "terminal")
        );
        assert!(matches!(
            management_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let _ = handle(
            &mut portal,
            M::Completed(id, Action::Inspect, Err("stopped".into())),
        );
        assert!(
            matches!(portal.dialogs.active(), ActiveDialog::AuthPrompt(p) if p.host == "terminal")
        );
        assert!(matches!(
            terminal_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            management_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        let _ =
            crate::app::update::dialog::handle_dialog(&mut portal, DialogMessage::AuthPromptSubmit);
        assert!(matches!(
            terminal_rx.try_recv(),
            Ok(AuthPromptResponse::Submit(_))
        ));
        assert!(matches!(
            portal.dialogs.active(),
            ActiveDialog::PortalHubOnboarding
        ));
    }

    #[test]
    fn terminal_reconnect_waits_for_management_prompt_without_losing_responders() {
        let (mut portal, id) = wizard();
        let (management, mut management_rx) = prompt("management");
        let _ = handle(&mut portal, M::AuthPrompt(id, management));
        let (terminal, mut terminal_rx) = prompt("terminal");
        let _ = crate::app::update::dialog::handle_dialog(
            &mut portal,
            DialogMessage::AuthPrompt(terminal),
        );
        assert!(
            matches!(portal.dialogs.active(), ActiveDialog::AuthPrompt(p) if p.host == "management")
        );
        assert!(matches!(
            terminal_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Empty)
        ));
        let _ =
            crate::app::update::dialog::handle_dialog(&mut portal, DialogMessage::AuthPromptSubmit);
        assert!(matches!(
            management_rx.try_recv(),
            Ok(AuthPromptResponse::Submit(_))
        ));
        assert!(
            matches!(portal.dialogs.active(), ActiveDialog::AuthPrompt(p) if p.host == "terminal")
        );
        let _ = handle(&mut portal, M::Cancel);
        let _ = handle(
            &mut portal,
            M::Completed(id, Action::Inspect, Err("late cancellation".into())),
        );
        assert!(
            matches!(portal.dialogs.active(), ActiveDialog::AuthPrompt(p) if p.host == "terminal")
        );
        let _ =
            crate::app::update::dialog::handle_dialog(&mut portal, DialogMessage::AuthPromptCancel);
        assert!(matches!(
            terminal_rx.try_recv(),
            Ok(AuthPromptResponse::Cancel)
        ));
        assert!(matches!(
            portal.dialogs.active(),
            ActiveDialog::PortalHubOnboarding
        ));
    }

    #[test]
    fn host_key_prompt_survives_management_cancel_and_onboarding_close() {
        use crate::ssh::host_key_verification::{
            HostKeyInfo, HostKeyVerificationRequest, HostKeyVerificationResponse,
        };
        let (mut portal, id) = wizard();
        let (management, mut management_rx) = prompt("management");
        let _ = handle(&mut portal, M::AuthPrompt(id, management));
        let (responder, mut terminal_rx) = oneshot::channel();
        let request = HostKeyVerificationRequest::NewHost {
            info: HostKeyInfo {
                host: "terminal".into(),
                port: 22,
                key_type: "ssh-ed25519".into(),
                fingerprint: "test".into(),
            },
            responder,
        };
        let _ = crate::app::update::dialog::handle_dialog(
            &mut portal,
            DialogMessage::HostKeyVerification(crate::message::VerificationRequestWrapper(Some(
                Box::new(request),
            ))),
        );
        portal.dialogs.close_onboarding(id);
        portal.ui.portal_hub_wizard.cancel();
        assert!(matches!(
            management_rx.try_recv(),
            Err(oneshot::error::TryRecvError::Closed)
        ));
        assert!(
            matches!(portal.dialogs.active(), ActiveDialog::HostKey(p) if p.host == "terminal")
        );
        let _ = handle(
            &mut portal,
            M::Completed(id, Action::Inspect, Err("late".into())),
        );
        portal
            .dialogs
            .host_key_mut()
            .unwrap()
            .respond(HostKeyVerificationResponse::Accept);
        portal.dialogs.close();
        assert!(matches!(
            terminal_rx.try_recv(),
            Ok(HostKeyVerificationResponse::Accept)
        ));
        assert!(matches!(portal.dialogs.active(), ActiveDialog::None));
    }

    #[test]
    fn cleanup_warning_survives_cancel_close_choice_change_and_new_wizard() {
        let (mut portal, id) = wizard();
        let host = Uuid::new_v4();
        let directory = Uuid::new_v4();
        let _ = handle(&mut portal, M::Cancel);
        let _ = crate::app::update::dialog::handle_dialog(&mut portal, DialogMessage::Close);
        assert!(!portal.ui.portal_hub_wizard.active);
        let _ = handle(
            &mut portal,
            M::CleanupFailed {
                operation: id,
                host,
                directory,
            },
        );
        let _ = handle(
            &mut portal,
            M::Completed(
                id,
                Action::Inspect,
                Err("ignored obsolete completion".into()),
            ),
        );
        assert_eq!(portal.ui.portal_hub_cleanup_warnings.len(), 1);
        assert!(
            portal.ui.portal_hub_cleanup_warnings[0]
                .contains(&format!("/tmp/portal-hub-{directory}"))
        );
        assert!(portal.ui.portal_hub_cleanup_warnings[0].contains(&host.to_string()));
        assert!(
            portal
                .toast_manager
                .toasts()
                .last()
                .unwrap()
                .duration
                .is_none()
        );
        let _ = super::super::settings::handle_settings_message(
            &mut portal,
            UiMessage::PortalHubOpenOnboarding,
        );
        let _ = handle(&mut portal, M::Choose(SetupChoice::Server));
        let _ = handle(&mut portal, M::NewDeployment);
        assert_eq!(portal.ui.portal_hub_cleanup_warnings.len(), 1);
        // Duplicate deliveries do not displace another warning/toast.
        let _ = handle(
            &mut portal,
            M::CleanupFailed {
                operation: id,
                host,
                directory,
            },
        );
        assert_eq!(portal.ui.portal_hub_cleanup_warnings.len(), 1);
        assert!(portal.ui.portal_hub_wizard.managed.error.is_none());
    }

    #[test]
    fn invalid_hostname_does_not_freeze_a_recovery_record() {
        if Portal::isolate_config_test() {
            return;
        }
        let (mut portal, _) = wizard();
        let host: crate::config::Host = serde_json::from_value(serde_json::json!({
            "id":Uuid::new_v4(),"name":"recovery","hostname":"server.example","port":22,
            "username":"user","auth":{"type":"agent"},
            "created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"
        }))
        .unwrap();
        portal.ui.portal_hub_wizard.managed.busy = false;
        portal.ui.portal_hub_wizard.managed.host_id = Some(host.id);
        portal.config.hosts.hosts.push(host);
        let _ = handle(&mut portal, M::Hostname("My Hub".into()));
        assert_eq!(handle(&mut portal, M::Run(Action::Inspect)).units(), 0);
        let state = &portal.ui.portal_hub_wizard.managed;
        assert!(
            state
                .error
                .as_deref()
                .unwrap()
                .contains("lowercase server name")
        );
        assert!(state.selected.is_none());
        assert!(state.records.is_empty());
        assert!(!state.busy);
        let _ = handle(&mut portal, M::Hostname("my-hub".into()));
        let state = &portal.ui.portal_hub_wizard.managed;
        assert!(deployment::validate_hostname(&state.hostname).is_ok());
        assert!(state.selected.is_none()); // The editable field remains visible.
        let task = handle(&mut portal, M::Run(Action::Inspect));
        assert!(task.units() > 0);
        let state = &portal.ui.portal_hub_wizard.managed;
        assert_eq!(state.selected.as_ref().unwrap().hostname, "my-hub");
        assert_eq!(state.records.len(), 1);
        let records = ManagedHubState::load().records;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].hostname, "my-hub");
    }

    #[test]
    fn failed_https_action_keeps_redacted_diagnostic_payload() {
        let (mut portal, id) = wizard();
        let secret = secrecy::SecretString::from("owner-secret\"value");
        let response = deployment::DeploymentResponse::parse(&serde_json::json!({
            "protocol_version":1,"ok":false,"phase":"awaiting_https","message":"Enable HTTPS",
            "error_code":"serve_failed", "data":{"next_step":"approve tailnet HTTPS", "logs":"owner-secret\"value"}
        }).to_string(), &secret).unwrap();
        let _ = handle(
            &mut portal,
            M::Completed(id, Action::ConfigureHttps, Ok(response)),
        );
        let state = &portal.ui.portal_hub_wizard.managed;
        assert_eq!(state.error.as_deref(), Some("Enable HTTPS (serve_failed)"));
        let diagnostics = state.diagnostics.as_ref().unwrap();
        assert!(diagnostics.contains("approve tailnet HTTPS"));
        assert!(diagnostics.contains("[redacted]"));
        assert!(!diagnostics.contains("owner-secret"));
    }
}
