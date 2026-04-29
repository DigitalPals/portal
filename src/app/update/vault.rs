use iced::Task;
use iced::widget::text_editor;

use crate::app::{Portal, VaultModal};
use crate::config::paths;
use crate::hub::vault::{HubVaultConfig, encrypt_private_key, load_or_create_vault_secret};
use crate::message::{Message, VaultMessage};
use crate::views::toast::{Toast, ToastAction};

pub fn handle_vault(portal: &mut Portal, msg: VaultMessage) -> Task<Message> {
    match msg {
        VaultMessage::SearchChanged(query) => {
            portal.vault_ui.search_query = query;
        }
        VaultMessage::AddKeyOpen => {
            reset_add_key_state(portal);
            portal.vault_ui.modal = VaultModal::AddKey;
        }
        VaultMessage::AddKeyCancel => {
            reset_add_key_state(portal);
            portal.vault_ui.modal = VaultModal::None;
        }
        VaultMessage::AddKeyNameChanged(name) => {
            portal.vault_ui.key_name = name;
        }
        VaultMessage::AddKeyPrivateKeyChanged(action) => {
            portal.vault_ui.private_key.perform(action);
            portal.vault_ui.add_key_error = None;
        }
        VaultMessage::AddKeyImportFileRequested => {
            return Task::perform(
                async move {
                    let Some(path) = rfd::FileDialog::new()
                        .set_title("Import SSH private key")
                        .pick_file()
                    else {
                        return Ok(None);
                    };
                    let content = std::fs::read_to_string(&path)
                        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
                    Ok(Some((path, content)))
                },
                |result| match result {
                    Ok(Some((path, content))) => {
                        Message::Vault(VaultMessage::AddKeyFileLoaded(Ok((path, content))))
                    }
                    Ok(None) => Message::Noop,
                    Err(error) => Message::Vault(VaultMessage::AddKeyFileLoaded(Err(error))),
                },
            );
        }
        VaultMessage::AddKeyImportDefaultRequested => {
            return Task::perform(
                async move {
                    let path = paths::default_identity_files()
                        .into_iter()
                        .find(|path| path.exists())
                        .ok_or_else(|| "No default SSH private key found in ~/.ssh".to_string())?;
                    let content = std::fs::read_to_string(&path)
                        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
                    Ok((path, content))
                },
                |result| Message::Vault(VaultMessage::AddKeyFileLoaded(result)),
            );
        }
        VaultMessage::AddKeyFileLoaded(result) => match result {
            Ok((path, content)) => {
                portal.vault_ui.imported_path = Some(path.clone());
                if portal.vault_ui.key_name.trim().is_empty() {
                    portal.vault_ui.key_name = path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("SSH key")
                        .to_string();
                }
                portal.vault_ui.private_key = text_editor::Content::with_text(&content);
                portal.vault_ui.add_key_error = None;
                portal.vault_ui.operation_error = None;
            }
            Err(error) => {
                portal.vault_ui.add_key_error = Some(error);
            }
        },
        VaultMessage::AddKeySubmit => {
            let name = portal.vault_ui.key_name.trim().to_string();
            let private_key = portal.vault_ui.private_key.text();
            let existing_vault = portal.config.vault.clone();

            if name.is_empty() {
                portal.vault_ui.add_key_error = Some("Name is required".to_string());
                return Task::none();
            }
            if private_key.trim().is_empty() {
                portal.vault_ui.add_key_error = Some("Private key is required".to_string());
                return Task::none();
            }

            match load_or_create_vault_secret(&existing_vault) {
                Ok(vault_secret) => {
                    portal.vault_ui.operation_error = None;
                    portal.vault_ui.add_key_error = None;
                    return Task::perform(
                        async move { encrypt_private_key(name, private_key.as_bytes(), &vault_secret) },
                        |result| Message::Vault(VaultMessage::AddKeyDone(result)),
                    );
                }
                Err(error) => portal.vault_ui.add_key_error = Some(error),
            }
        }
        VaultMessage::AddKeyDone(result) => match result {
            Ok(key) => {
                portal.config.vault.keys.push(key);
                if let Err(error) = save_vault(portal) {
                    portal.vault_ui.operation_error = Some(error.clone());
                    portal
                        .toast_manager
                        .push(Toast::error(format!("Vault save failed: {}", error)));
                } else {
                    reset_add_key_state(portal);
                    portal.vault_ui.modal = VaultModal::None;
                    portal.toast_manager.push(Toast::success("Added vault key"));
                    super::ui::settings::queue_portal_hub_local_sync(portal);
                }
            }
            Err(error) => {
                portal.vault_ui.add_key_error = Some(error.clone());
                portal
                    .toast_manager
                    .push(Toast::error(format!("Vault key add failed: {}", error)));
            }
        },
        VaultMessage::EditOpen(id) => {
            if let Some(key) = portal.config.vault.find_key(id) {
                portal.vault_ui.modal = VaultModal::EditKey;
                portal.vault_ui.edit_key_id = Some(id);
                portal.vault_ui.edit_name = key.name.clone();
                portal.vault_ui.edit_delete_requested = false;
                portal.vault_ui.operation_error = None;
            }
        }
        VaultMessage::EditNameChanged(value) => {
            portal.vault_ui.edit_name = value;
        }
        VaultMessage::EditSave => {
            if let Some(id) = portal.vault_ui.edit_key_id {
                let new_name = portal.vault_ui.edit_name.trim().to_string();
                if new_name.is_empty() {
                    portal.vault_ui.operation_error = Some("Name is required".to_string());
                    return Task::none();
                }
                if let Some(key) = portal.config.vault.find_key_mut(id) {
                    key.name = new_name;
                    key.updated_at = chrono::Utc::now();
                }
                match save_vault(portal) {
                    Ok(()) => {
                        reset_edit_key_state(portal);
                        portal.vault_ui.modal = VaultModal::None;
                        portal.toast_manager.push(Toast::success("Saved vault key"));
                        super::ui::settings::queue_portal_hub_local_sync(portal);
                    }
                    Err(error) => portal.vault_ui.operation_error = Some(error),
                }
            }
        }
        VaultMessage::EditDeleteRequested => {
            portal.vault_ui.edit_delete_requested = true;
        }
        VaultMessage::EditDeleteConfirm => {
            if let Some(id) = portal.vault_ui.edit_key_id {
                portal.config.vault.keys.retain(|key| key.id != id);
                match save_vault(portal) {
                    Ok(()) => {
                        reset_edit_key_state(portal);
                        portal.vault_ui.modal = VaultModal::None;
                        portal
                            .toast_manager
                            .push(Toast::success("Deleted vault key"));
                        super::ui::settings::queue_portal_hub_local_sync(portal);
                    }
                    Err(error) => portal.vault_ui.operation_error = Some(error),
                }
            }
        }
        VaultMessage::EditCancel => {
            reset_edit_key_state(portal);
            portal.vault_ui.modal = VaultModal::None;
        }
        VaultMessage::CopyPublicKey(id) => {
            if let Some(value) = portal
                .config
                .vault
                .find_key(id)
                .and_then(|key| key.public_key.clone())
            {
                portal
                    .toast_manager
                    .push(Toast::success("Copied public key"));
                return iced::clipboard::write(value);
            }
        }
        VaultMessage::CopyFingerprint(id) => {
            if let Some(value) = portal
                .config
                .vault
                .find_key(id)
                .and_then(|key| key.fingerprint.clone())
            {
                portal
                    .toast_manager
                    .push(Toast::success("Copied fingerprint"));
                return iced::clipboard::write(value);
            }
        }
        VaultMessage::EnrollmentRefresh => {
            portal.vault_ui.enrollment_loading = true;
            portal.vault_ui.enrollment_status = None;
            portal.vault_ui.operation_error = None;
            let settings = portal.prefs.portal_hub.clone();
            return Task::perform(
                async move { crate::hub::vault_enrollment::list_pending(&settings).await },
                |result| Message::Vault(VaultMessage::EnrollmentRefreshDone(result)),
            );
        }
        VaultMessage::EnrollmentRefreshDone(result) => {
            portal.vault_ui.enrollment_loading = false;
            match result {
                Ok(requests) => {
                    let count = requests.len();
                    portal.vault_ui.enrollment_requests = requests;
                    portal.vault_ui.enrollment_status = Some(if count == 0 {
                        "No pending Android vault access requests".to_string()
                    } else {
                        format!("{} pending Android vault access request(s)", count)
                    });
                    if count > 0 {
                        portal
                            .toast_manager
                            .dismiss_action(ToastAction::OpenVaultApprovals);
                        portal.toast_manager.push_or_refresh(Toast::warning(format!(
                            "{} Android vault access request(s) need approval. Click to review.",
                            count
                        ))
                        .persistent()
                        .action(ToastAction::OpenVaultApprovals));
                    } else {
                        portal
                            .toast_manager
                            .dismiss_action(ToastAction::OpenVaultApprovals);
                    }
                }
                Err(error) => {
                    portal.vault_ui.operation_error = Some(error);
                }
            }
        }
        VaultMessage::EnrollmentApprove(id) => {
            let Some(enrollment) = portal
                .vault_ui
                .enrollment_requests
                .iter()
                .find(|request| request.id == id)
                .cloned()
            else {
                portal.vault_ui.operation_error =
                    Some("Vault enrollment request was not found".to_string());
                return Task::none();
            };
            portal.vault_ui.enrollment_loading = true;
            portal.vault_ui.enrollment_status = None;
            portal.vault_ui.operation_error = None;
            let settings = portal.prefs.portal_hub.clone();
            return Task::perform(
                async move { crate::hub::vault_enrollment::approve(settings, enrollment).await },
                |result| Message::Vault(VaultMessage::EnrollmentApproveDone(result)),
            );
        }
        VaultMessage::EnrollmentApproveDone(result) => {
            portal.vault_ui.enrollment_loading = false;
            match result {
                Ok(enrollment) => {
                    portal
                        .vault_ui
                        .enrollment_requests
                        .retain(|request| request.id != enrollment.id);
                    portal.vault_ui.enrollment_status = Some(format!(
                        "Approved vault access for {}",
                        enrollment.device_name
                    ));
                    if portal.vault_ui.enrollment_requests.is_empty() {
                        portal
                            .toast_manager
                            .dismiss_action(ToastAction::OpenVaultApprovals);
                    }
                    portal.toast_manager.push(Toast::success(format!(
                        "Approved vault access for {}",
                        enrollment.device_name
                    )));
                }
                Err(error) => {
                    portal.vault_ui.operation_error = Some(error.clone());
                    portal
                        .toast_manager
                        .push(Toast::error(format!("Vault enrollment failed: {}", error)));
                }
            }
        }
    }

    Task::none()
}

fn reset_add_key_state(portal: &mut Portal) {
    portal.vault_ui.key_name.clear();
    portal.vault_ui.private_key = text_editor::Content::new();
    portal.vault_ui.imported_path = None;
    portal.vault_ui.add_key_error = None;
}

fn reset_edit_key_state(portal: &mut Portal) {
    portal.vault_ui.edit_key_id = None;
    portal.vault_ui.edit_name.clear();
    portal.vault_ui.edit_delete_requested = false;
}

fn save_vault(portal: &mut Portal) -> Result<(), String> {
    portal.config.vault.save()?;
    portal.config.vault = HubVaultConfig::load()?;
    Ok(())
}
