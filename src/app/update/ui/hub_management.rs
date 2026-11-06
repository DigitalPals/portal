use crate::{
    app::Portal,
    hub::deployment::{
        self, DeploymentAction as Action, DeploymentRecord, DeploymentRequest, ManagedHubState,
        ManagementMessage as M, OwnerSetupLink, OwnerSetupMode, SetupChoice,
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
            wizard.auto_finish = None;
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
            state.response = None;
            state.error = None;
        }
        M::Resume(record) => {
            let wizard = &mut portal.ui.portal_hub_wizard;
            if wizard.managed.busy {
                return Task::none();
            }
            wizard.edit_endpoint(String::new());
            wizard.auto_finish = None;
            let state = &mut wizard.managed;
            state.host_id = Some(record.ssh_host_id);
            state.hostname = record.hostname.clone();
            state.image = record.hub_image.clone();
            state.selected = Some(record);
            state.response = None;
            state.error = None;
            state.allow_interruptions = false;
            state.uninstall_confirmed = false;
            state.reset_confirmed = false;
            state.archive = None;
            state.automated = false;
            state.browser_url = None;
            state.diagnostics = None;
            state.progress.clear();
            // Read-only: shows where setup stopped, or the maintenance options.
            // A missing SSH host is already explained next to the server.
            let host = state.host_id;
            if portal.config.hosts.hosts.iter().any(|h| Some(h.id) == host) {
                return start(portal, Action::Status, OwnerSetupMode::Setup);
            }
        }
        M::NewDeployment => {
            if portal.ui.portal_hub_wizard.managed.busy {
                return Task::none();
            }
            portal.ui.portal_hub_wizard.edit_endpoint(String::new());
            portal.ui.portal_hub_wizard.auto_finish = None;
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
        M::AllowInterruptions(value) => {
            portal.ui.portal_hub_wizard.managed.allow_interruptions = value
        }
        M::ConfirmUninstall(value) => {
            portal.ui.portal_hub_wizard.managed.uninstall_confirmed = value
        }
        M::ConfirmReset(value) => portal.ui.portal_hub_wizard.managed.reset_confirmed = value,
        M::OwnerName(value) => portal.ui.portal_hub_wizard.managed.owner_name = value,
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
        M::ConnectDevice => {
            let wizard = &mut portal.ui.portal_hub_wizard;
            if wizard.managed.busy {
                return Task::none();
            }
            if let Some(url) = wizard.managed.ready_url() {
                return connect_device(wizard, url, None);
            }
            wizard.managed.error =
                Some("Resume setup to finish this Hub before connecting.".into());
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
            let wizard = &mut portal.ui.portal_hub_wizard;
            let state = &mut wizard.managed;
            if !state.accepts(id) {
                return Task::none();
            }
            state.busy = false;
            state.cancel_tx = None;
            state.allow_interruptions = false;
            state.uninstall_confirmed = false;
            state.reset_confirmed = false;
            let mut handoff = None;
            match result {
                Ok(mut response) => {
                    if let Some(link) = response.owner_setup.take()
                        && let Some(url) = response
                            .string("public_url")
                            .filter(|url| deployment::validate_public_url(url).is_ok())
                    {
                        handoff = Some((url.to_string(), link));
                    }
                    state.progress = response.message.clone();
                    state.error = (!response.ok).then(|| match response.error_code.as_deref() {
                        Some(code) => format!("{} ({code})", response.message),
                        None => response.message.clone(),
                    });
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
                        if let Some(record) = state.selected.as_mut() {
                            if let Some(url) = response.string("public_url") {
                                record.public_url = Some(url.into());
                            }
                            if action == Action::Update {
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
            if let Some((url, link)) = handoff {
                return connect_device(wizard, url, Some(link));
            }
        }
        M::Run(action) => return start(portal, action, OwnerSetupMode::Setup),
        M::SetUpPasskey(mode) => return start(portal, Action::OwnerSetup, mode),
        M::UpdateToPortalVersion => {
            let state = &mut portal.ui.portal_hub_wizard.managed;
            if state.busy {
                return Task::none();
            }
            state.image = deployment::DEFAULT_IMAGE.into();
            return start(portal, Action::Update, OwnerSetupMode::Setup);
        }
        M::Deploy => return start_automated(portal),
        M::ToggleAdvanced => {
            let state = &mut portal.ui.portal_hub_wizard.managed;
            state.show_advanced = !state.show_advanced;
        }
        M::OpenBrowser(id, url) => {
            let state = &mut portal.ui.portal_hub_wizard.managed;
            if state.accepts(id) {
                open_tailscale_page(state, url);
            }
        }
        M::ReopenBrowser => {
            let state = &mut portal.ui.portal_hub_wizard.managed;
            if let Some(url) = state.browser_url.clone() {
                open_tailscale_page(state, url);
            }
        }
        M::AutoCompleted(id, result) => return auto_completed(portal, id, result),
    }
    Task::none()
}

fn open_tailscale_page(state: &mut ManagedHubState, url: String) {
    match deployment::validate_enrollment_url(&url) {
        Ok(valid) => {
            state.browser_url = Some(url);
            if open::that(valid.as_str()).is_err() {
                state.error = Some(
                    "Could not open the browser. Use Open Tailscale page to try again.".into(),
                );
            }
        }
        Err(error) => state.error = Some(error),
    }
}

fn auto_completed(
    portal: &mut Portal,
    id: Uuid,
    result: Result<deployment::DeploymentResponse, String>,
) -> Task<Message> {
    let wizard = &mut portal.ui.portal_hub_wizard;
    let state = &mut wizard.managed;
    if !state.accepts(id) {
        return Task::none();
    }
    state.busy = false;
    state.cancel_tx = None;
    portal.dialogs.finish_management(id);
    let mut response = match result {
        Ok(response) if response.ok => response,
        Ok(response) => {
            state.error = Some(match response.error_code.as_deref() {
                Some(code) => format!("{} ({code})", response.message),
                None => response.message.clone(),
            });
            state.diagnostics = Some(deployment::bounded_text(
                &serde_json::to_string_pretty(&response.data).unwrap_or_default(),
                12_000,
            ));
            state.progress = "Setup stopped. Fix the problem above, then resume setup.".into();
            return Task::none();
        }
        Err(error) => {
            state.error = Some(error);
            state.progress = "Setup stopped. Changes already made on the server are kept; resume setup to continue.".into();
            return Task::none();
        }
    };
    let Some(url) = response
        .string("public_url")
        .filter(|url| deployment::validate_public_url(url).is_ok())
        .map(str::to_string)
    else {
        state.error = Some("The Hub did not report a valid private HTTPS address.".into());
        return Task::none();
    };
    if let Some(record) = state.selected.as_mut() {
        record.public_url = Some(url.clone());
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
    let setup = response.owner_setup.take();
    state.response = Some(response);
    connect_device(wizard, url, setup)
}

/// The Hub answers on its private HTTPS name: connect this device. With a
/// setup link the browser creates the owner passkey, otherwise it signs in
/// with an existing one; either way Portal then saves and syncs.
fn connect_device(
    wizard: &mut crate::hub::onboarding::PortalHubWizardState,
    url: String,
    setup: Option<OwnerSetupLink>,
) -> Task<Message> {
    wizard.managed.browser_url = None;
    wizard.managed.progress = "Your Hub is ready. Connecting this device…".into();
    wizard.edit_endpoint(url);
    wizard.auto_finish = Some(crate::hub::onboarding::AutoFinish { setup });
    Task::done(Message::Ui(UiMessage::PortalHubCheckStatus))
}

/// The owner account name: the Advanced field, then the name Portal used
/// before, then the local login name.
fn owner_name(state: &ManagedHubState, record: &DeploymentRecord) -> String {
    let typed = state.owner_name.trim();
    if !typed.is_empty() {
        typed.to_string()
    } else {
        record
            .owner_username
            .clone()
            .unwrap_or_else(deployment::default_owner_username)
    }
}

/// One action on a saved deployment: status, diagnostics, maintenance or an
/// owner passkey link. Setup itself always runs through [`start_automated`].
fn start(portal: &mut Portal, action: Action, owner_mode: OwnerSetupMode) -> Task<Message> {
    let state = &mut portal.ui.portal_hub_wizard.managed;
    if state.busy {
        return Task::none();
    }
    let prepare = (|| -> Result<_, String> {
        let host_id = state.host_id.ok_or("Choose an SSH host first")?;
        let (host, chain) = management_target(&portal.config.hosts.hosts, host_id)?;
        let mut record = state
            .selected
            .clone()
            .ok_or("Deploy Hub on this server first")?;
        if action == Action::Uninstall && !state.uninstall_confirmed {
            return Err(
                "Confirm that you want to uninstall this Hub. Its data will be retained.".into(),
            );
        }
        if action == Action::OwnerSetup
            && owner_mode == OwnerSetupMode::Reset
            && !state.reset_confirmed
        {
            return Err("Confirm the reset first: it removes every existing passkey and signs out every device.".into());
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
        if action == Action::Update {
            deployment::validate_image(&state.image)?;
        }
        let username = owner_name(state, &record);
        if action == Action::OwnerSetup && owner_mode == OwnerSetupMode::Setup {
            deployment::validate_owner_username(&username)?;
            remember_owner_name(state, &mut record, &username)?;
        }
        let request = DeploymentRequest {
            action,
            deployment_id: record.deployment_id,
            hostname: record.hostname.clone(),
            hub_image: state.image.clone(),
            image_archive: None,
            image_archive_sha256: None,
            allow_session_interruptions: state.allow_interruptions,
            username,
            owner_mode,
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
    let archive = if action == Action::Update {
        state.archive.clone()
    } else {
        None
    };
    let id = Uuid::new_v4();
    let host_id = host.id;
    state.operation = id;
    state.busy = true;
    state.automated = false;
    state.error = None;
    state.last_action = Some(action);
    state.progress = "Connecting directly over SSH…".into();
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    state.cancel_tx = Some(cancel_tx);
    let (events_tx, events_rx) = tokio::sync::mpsc::channel(16);
    let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(16);
    let [events, progress] = operation_tasks(id, host_id, events_rx, progress_rx);
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

/// Run the whole setup on one SSH session: check, install (Docker too),
/// Tailscale enrollment and HTTPS, ending with a passkey setup link for the
/// browser. The click is the consent.
fn start_automated(portal: &mut Portal) -> Task<Message> {
    let state = &mut portal.ui.portal_hub_wizard.managed;
    if state.busy {
        return Task::none();
    }
    let prepare = (|| -> Result<_, String> {
        let host_id = state.host_id.ok_or("Choose the server to install Hub on")?;
        let (host, chain) = management_target(&portal.config.hosts.hosts, host_id)?;
        deployment::validate_image(&state.image)?;
        if state.selected.is_none() {
            deployment::validate_hostname(&state.hostname)?;
        }
        let mut record = select_record(state, host_id)?;
        let owner_username = owner_name(state, &record);
        deployment::validate_owner_username(&owner_username)?;
        remember_owner_name(state, &mut record, &owner_username)?;
        let plan = deployment::AutomatedPlan {
            deployment_id: record.deployment_id,
            hostname: record.hostname.clone(),
            hub_image: state.image.clone(),
            archive: state.archive.clone(),
            owner_username,
        };
        Ok((host, chain, plan))
    })();
    let (host, chain, plan) = match prepare {
        Ok(value) => value,
        Err(error) => {
            state.error = Some(error);
            return Task::none();
        }
    };
    let id = Uuid::new_v4();
    let host_id = host.id;
    state.operation = id;
    state.busy = true;
    state.automated = true;
    state.error = None;
    state.diagnostics = None;
    state.response = None;
    state.browser_url = None;
    state.last_action = None;
    state.progress = "Connecting to the server…".into();
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    state.cancel_tx = Some(cancel_tx);
    let (events_tx, events_rx) = tokio::sync::mpsc::channel(16);
    let (progress_tx, progress_rx) = tokio::sync::mpsc::channel(16);
    let [events, progress] = operation_tasks(id, host_id, events_rx, progress_rx);
    let work = Task::perform(
        deployment::run_automated(host, chain, plan, events_tx, cancel_rx, progress_tx),
        move |result| message(M::AutoCompleted(id, result)),
    );
    Task::batch([events, progress, work])
}

fn remember_owner_name(
    state: &mut ManagedHubState,
    record: &mut DeploymentRecord,
    username: &str,
) -> Result<(), String> {
    if record.owner_username.as_deref() == Some(username) {
        return Ok(());
    }
    record.owner_username = Some(username.to_string());
    deployment::save_record(record)?;
    state.selected = Some(record.clone());
    if let Some(existing) = state
        .records
        .iter_mut()
        .find(|r| r.deployment_id == record.deployment_id)
    {
        *existing = record.clone();
    }
    Ok(())
}

/// The management host and its jump chain, refusing Hub Vault keys: the
/// recovery path must not depend on the Hub it repairs.
fn management_target(
    hosts: &[crate::config::Host],
    host_id: Uuid,
) -> Result<(crate::config::Host, Vec<crate::config::Host>), String> {
    let host = hosts
        .iter()
        .find(|h| h.id == host_id && h.protocol == crate::config::Protocol::Ssh)
        .cloned()
        .ok_or(
            "The management SSH host is missing. Restore its host definition before resuming.",
        )?;
    let chain = crate::ssh::tunnel::resolve_jump_chain(hosts, &host).map_err(|e| e.to_string())?;
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
    Ok((host, chain))
}

/// The selected deployment, or a new durable recovery record for this host.
fn select_record(state: &mut ManagedHubState, host_id: Uuid) -> Result<DeploymentRecord, String> {
    let record = state.selected.clone().unwrap_or_else(|| DeploymentRecord {
        deployment_id: Uuid::new_v4(),
        ssh_host_id: host_id,
        hostname: state.hostname.clone(),
        hub_image: state.image.clone(),
        public_url: None,
        owner_username: None,
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
    Ok(record)
}

fn operation_tasks(
    id: Uuid,
    host_id: Uuid,
    events_rx: tokio::sync::mpsc::Receiver<SshEvent>,
    progress_rx: tokio::sync::mpsc::Receiver<deployment::ManagementProgress>,
) -> [Task<Message>; 2] {
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
            deployment::ManagementProgress::OpenBrowser(url) => message(M::OpenBrowser(id, url)),
            deployment::ManagementProgress::CleanupFailed(directory) => message(M::CleanupFailed {
                operation: id,
                host: host_id,
                directory,
            }),
        },
    );
    [events, progress]
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
        assert_eq!(handle(&mut portal, M::Deploy).units(), 0);
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
        let task = handle(&mut portal, M::Deploy);
        assert!(task.units() > 0);
        let state = &portal.ui.portal_hub_wizard.managed;
        assert_eq!(state.selected.as_ref().unwrap().hostname, "my-hub");
        assert_eq!(state.records.len(), 1);
        let records = ManagedHubState::load().records;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].hostname, "my-hub");
    }

    fn ready_response(url: &str) -> deployment::DeploymentResponse {
        deployment::DeploymentResponse::parse(
            &serde_json::json!({"protocol_version":1,"ok":true,"phase":"ready","message":"Hub is ready",
                "data":{"owner_exists":true,"owner_ready":true,"public_url":url}})
            .to_string(),
        )
        .unwrap()
    }

    const TOKEN: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ";

    fn setup_response(url: &str) -> deployment::DeploymentResponse {
        deployment::DeploymentResponse::parse(
            &serde_json::json!({"protocol_version":1,"ok":true,"phase":"awaiting_owner","message":"Create the owner passkey",
                "data":{"owner_exists":false,"owner_ready":false,"public_url":url,
                    "owner_setup":{"token":TOKEN,"username":"owner","mode":"setup"}}})
            .to_string(),
        )
        .unwrap()
    }

    fn proxy_status(url: &str) -> crate::proxy::ProxyStatus {
        crate::proxy::ProxyStatus {
            version: env!("CARGO_PKG_VERSION").into(),
            api_version: 2,
            metadata_schema_version: 2,
            public_url: url.into(),
            ssh_port: None,
            ssh_username: None,
            sync_v2: true,
            sync_events: true,
            web_proxy: true,
            session_titles: true,
            key_vault: true,
            vault_enrollment: true,
        }
    }

    #[test]
    fn deploy_requires_a_server_and_starts_nothing_without_one() {
        let (mut portal, _) = wizard();
        portal.ui.portal_hub_wizard.managed.busy = false;
        assert_eq!(handle(&mut portal, M::Deploy).units(), 0);
        let state = &portal.ui.portal_hub_wizard.managed;
        assert!(
            state
                .error
                .as_deref()
                .unwrap()
                .contains("Choose the server")
        );
        assert!(!state.busy);
        assert!(state.selected.is_none());
    }

    #[test]
    fn untrusted_browser_pages_are_never_opened() {
        let (mut portal, id) = wizard();
        let _ = handle(
            &mut portal,
            M::OpenBrowser(id, "https://evil.example/login".into()),
        );
        let state = &portal.ui.portal_hub_wizard.managed;
        assert!(state.browser_url.is_none());
        assert!(state.error.is_some());
        // Late events from a cancelled run are ignored entirely.
        let _ = handle(&mut portal, M::Cancel);
        portal.ui.portal_hub_wizard.managed.error = None;
        let _ = handle(
            &mut portal,
            M::OpenBrowser(id, "https://login.tailscale.com/a/1".into()),
        );
        assert!(portal.ui.portal_hub_wizard.managed.browser_url.is_none());
    }

    #[test]
    fn ready_deployment_connects_and_signs_in_without_clicks() {
        let url = "https://portal-hub.example-tail.ts.net";
        let (mut portal, id) = wizard();
        let task = handle(&mut portal, M::AutoCompleted(id, Ok(ready_response(url))));
        assert!(task.units() > 0);
        let wizard = &portal.ui.portal_hub_wizard;
        assert!(!wizard.managed.busy);
        assert_eq!(wizard.hub_url_input, url);
        assert!(wizard.auto_finish.is_some());
        let generation = wizard.generation;
        let _ = super::super::settings::handle_settings_message(
            &mut portal,
            UiMessage::PortalHubCheckStatus,
        );
        let next = super::super::settings::handle_settings_message(
            &mut portal,
            UiMessage::PortalHubDraftStatusLoaded(generation, Ok(proxy_status(url))),
        );
        assert!(next.units() > 0, "sign-in starts automatically");
        // An owner with a passkey signs in; there is no setup link to use.
        let _ = super::super::settings::handle_settings_message(
            &mut portal,
            UiMessage::PortalHubAuthenticate,
        );
        assert!(portal.ui.portal_hub_wizard.auth_loading);
        assert!(
            portal
                .ui
                .portal_hub_wizard
                .managed
                .progress
                .contains("Sign in with your passkey")
        );
    }

    #[test]
    fn setup_link_is_used_once_for_the_owner_passkey() {
        let url = "https://portal-hub.example-tail.ts.net";
        let (mut portal, id) = wizard();
        let task = handle(&mut portal, M::AutoCompleted(id, Ok(setup_response(url))));
        assert!(task.units() > 0);
        let wizard = &portal.ui.portal_hub_wizard;
        assert_eq!(wizard.hub_url_input, url);
        let link = wizard.auto_finish.as_ref().unwrap().setup.as_ref().unwrap();
        assert_eq!(link.username, "owner");
        // The token is kept only for the browser hand-off, never in diagnostics.
        let response = format!("{:?}", wizard.managed.response);
        assert!(!response.contains(TOKEN));
        assert!(
            wizard
                .managed
                .response
                .as_ref()
                .unwrap()
                .owner_setup
                .is_none()
        );
        let generation = wizard.generation;
        let _ = super::super::settings::handle_settings_message(
            &mut portal,
            UiMessage::PortalHubCheckStatus,
        );
        let _ = super::super::settings::handle_settings_message(
            &mut portal,
            UiMessage::PortalHubDraftStatusLoaded(generation, Ok(proxy_status(url))),
        );
        let _ = super::super::settings::handle_settings_message(
            &mut portal,
            UiMessage::PortalHubAuthenticate,
        );
        let wizard = &portal.ui.portal_hub_wizard;
        assert!(wizard.auth_loading);
        assert!(wizard.auto_finish.as_ref().unwrap().setup.is_none());
        assert!(wizard.managed.progress.contains("Create your passkey"));
        let _ = super::super::settings::handle_settings_message(
            &mut portal,
            UiMessage::PortalHubDraftAuthenticated(generation, Err("cancelled".into())),
        );
        let wizard = &portal.ui.portal_hub_wizard;
        assert_eq!(wizard.auth_error.as_deref(), Some("cancelled"));
        assert!(wizard.managed.progress.is_empty());
    }

    #[test]
    fn manual_passkey_actions_hand_off_to_the_browser_and_reset_needs_confirmation() {
        let url = "https://portal-hub.example-tail.ts.net";
        let (mut portal, id) = wizard();
        let task = handle(
            &mut portal,
            M::Completed(id, Action::OwnerSetup, Ok(setup_response(url))),
        );
        assert!(task.units() > 0);
        let wizard = &portal.ui.portal_hub_wizard;
        assert!(wizard.auto_finish.as_ref().unwrap().setup.is_some());
        assert!(!format!("{:?}", wizard.managed.response).contains(TOKEN));
        portal.ui.portal_hub_wizard.managed.busy = false;
        portal.ui.portal_hub_wizard.managed.host_id = Some(Uuid::new_v4());
        assert_eq!(
            handle(&mut portal, M::SetUpPasskey(OwnerSetupMode::Reset)).units(),
            0
        );
        let error = portal.ui.portal_hub_wizard.managed.error.clone().unwrap();
        assert!(
            error.contains("management SSH host is missing") || error.contains("Confirm the reset"),
            "{error}"
        );
    }

    #[test]
    fn unreachable_new_hub_explains_the_tailscale_requirement() {
        let url = "https://portal-hub.example-tail.ts.net";
        let (mut portal, id) = wizard();
        let _ = handle(&mut portal, M::AutoCompleted(id, Ok(ready_response(url))));
        let generation = portal.ui.portal_hub_wizard.generation;
        let task = super::super::settings::handle_settings_message(
            &mut portal,
            UiMessage::PortalHubDraftStatusLoaded(generation, Err("timed out".into())),
        );
        assert_eq!(task.units(), 0);
        let error = portal.ui.portal_hub_wizard.managed.error.clone().unwrap();
        assert!(error.contains("same Tailscale network"), "{error}");
        assert!(error.contains(url));
    }

    #[test]
    fn failed_automated_run_keeps_diagnostics_and_offers_resume() {
        let (mut portal, id) = wizard();
        let failed = deployment::DeploymentResponse::parse(
            &serde_json::json!({"protocol_version":1,"ok":false,"phase":"error","message":"Host prerequisites are missing",
                "error_code":"prerequisites_failed","data":{"blockers":["linux_pidfd_required"]}})
            .to_string(),
        )
        .unwrap();
        portal.ui.portal_hub_wizard.managed.automated = true;
        let _ = handle(&mut portal, M::AutoCompleted(id, Ok(failed)));
        let state = &portal.ui.portal_hub_wizard.managed;
        assert!(!state.busy);
        assert!(
            state
                .error
                .as_deref()
                .unwrap()
                .contains("prerequisites_failed")
        );
        assert!(
            state
                .diagnostics
                .as_deref()
                .unwrap()
                .contains("linux_pidfd_required")
        );
        assert!(portal.ui.portal_hub_wizard.auto_finish.is_none());
    }

    #[test]
    fn update_offer_selects_the_hub_image_of_this_portal_version() {
        let (mut portal, _) = wizard();
        portal.ui.portal_hub_wizard.managed.busy = false;
        portal.ui.portal_hub_wizard.managed.image = "ghcr.io/digitalpals/portal-hub:0.5.0".into();
        let _ = handle(&mut portal, M::UpdateToPortalVersion);
        let state = &portal.ui.portal_hub_wizard.managed;
        assert_eq!(state.image, deployment::DEFAULT_IMAGE);
        assert!(state.image.ends_with(env!("CARGO_PKG_VERSION")));
        // No server is selected, so nothing starts.
        assert!(!state.busy);
        assert!(state.error.is_some());
    }

    #[test]
    fn failed_action_keeps_its_diagnostic_payload() {
        let (mut portal, id) = wizard();
        let response = deployment::DeploymentResponse::parse(&serde_json::json!({
            "protocol_version":1,"ok":false,"phase":"error","message":"Update failed; the previous image was restored",
            "error_code":"update_rolled_back", "data":{"cause":"health_failed", "logs":"token=[redacted]"}
        }).to_string()).unwrap();
        let _ = handle(&mut portal, M::Completed(id, Action::Update, Ok(response)));
        let state = &portal.ui.portal_hub_wizard.managed;
        assert_eq!(
            state.error.as_deref(),
            Some("Update failed; the previous image was restored (update_rolled_back)")
        );
        let diagnostics = state.diagnostics.as_ref().unwrap();
        assert!(diagnostics.contains("health_failed"));
        assert!(diagnostics.contains("[redacted]"));
    }

    fn saved_server(portal: &mut Portal) -> DeploymentRecord {
        let host: crate::config::Host = serde_json::from_value(serde_json::json!({
            "id":Uuid::new_v4(),"name":"recovery","hostname":"server.example","port":22,
            "username":"user","auth":{"type":"agent"},
            "created_at":"2026-01-01T00:00:00Z","updated_at":"2026-01-01T00:00:00Z"
        }))
        .unwrap();
        let record = DeploymentRecord {
            deployment_id: Uuid::new_v4(),
            ssh_host_id: host.id,
            hostname: "portal-hub".into(),
            hub_image: deployment::DEFAULT_IMAGE.into(),
            public_url: None,
            owner_username: Some("owner".into()),
        };
        portal.config.hosts.hosts.push(host);
        record
    }

    #[test]
    fn selecting_a_saved_server_checks_its_status() {
        let (mut portal, _) = wizard();
        portal.ui.portal_hub_wizard.managed.busy = false;
        let record = saved_server(&mut portal);
        let task = handle(&mut portal, M::Resume(record.clone()));
        assert!(task.units() > 0);
        let state = &portal.ui.portal_hub_wizard.managed;
        assert!(state.busy);
        assert_eq!(state.last_action, Some(Action::Status));
        assert_eq!(state.selected.as_ref(), Some(&record));
    }

    #[test]
    fn a_ready_hub_connects_this_device_without_ssh() {
        let url = "https://portal-hub.example-tail.ts.net";
        let (mut portal, _) = wizard();
        portal.ui.portal_hub_wizard.managed.busy = false;
        assert_eq!(handle(&mut portal, M::ConnectDevice).units(), 0);
        assert!(portal.ui.portal_hub_wizard.managed.error.is_some());
        portal.ui.portal_hub_wizard.managed.response = Some(ready_response(url));
        let task = handle(&mut portal, M::ConnectDevice);
        assert!(task.units() > 0);
        let wizard = &portal.ui.portal_hub_wizard;
        assert_eq!(wizard.hub_url_input, url);
        assert!(wizard.auto_finish.as_ref().unwrap().setup.is_none());
    }
}
