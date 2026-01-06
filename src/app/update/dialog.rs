//! Dialog message handlers

use iced::Task;

use crate::app::Portal;
use crate::config::Host;
use crate::message::{DialogMessage, HostDialogField, Message};
use crate::ssh::host_key_verification::HostKeyVerificationResponse;
use crate::views::toast::Toast;
use crate::views::dialogs::host_dialog::AuthMethodChoice;
use crate::views::dialogs::host_key_dialog::HostKeyDialogState;

/// Handle dialog messages
pub fn handle_dialog(portal: &mut Portal, msg: DialogMessage) -> Task<Message> {
    match msg {
        DialogMessage::Close => {
            portal.dialogs.close();
            Task::none()
        }
        DialogMessage::Submit => {
            if let Some(dialog_state) = portal.dialogs.host() {
                if let Some(host) = dialog_state.to_host() {
                    // Preserve created_at for edits
                    let host = if let Some(existing_id) = dialog_state.editing_id {
                        if let Some(existing) = portal.hosts_config.find_host(existing_id) {
                            Host {
                                created_at: existing.created_at,
                                ..host
                            }
                        } else {
                            host
                        }
                    } else {
                        host
                    };

                    let is_edit = dialog_state.editing_id.is_some();
                    if is_edit {
                        if let Err(e) = portal.hosts_config.update_host(host.clone()) {
                            tracing::error!("Failed to update host: {}", e);
                        } else {
                            tracing::info!("Updated host: {}", host.name);
                        }
                    } else {
                        portal.hosts_config.add_host(host.clone());
                        tracing::info!("Added host: {}", host.name);
                    }

                    if let Err(e) = portal.hosts_config.save() {
                        tracing::error!("Failed to save config: {}", e);
                    }
                    portal.dialogs.close();
                }
            }
            Task::none()
        }
        DialogMessage::FieldChanged(field, value) => {
            if let Some(dialog_state) = portal.dialogs.host_mut() {
                match field {
                    HostDialogField::Name => dialog_state.name = value,
                    HostDialogField::Hostname => dialog_state.hostname = value,
                    HostDialogField::Port => dialog_state.port = value,
                    HostDialogField::Username => dialog_state.username = value,
                    HostDialogField::KeyPath => dialog_state.key_path = value,
                    HostDialogField::Tags => dialog_state.tags = value,
                    HostDialogField::Notes => dialog_state.notes = value,
                    HostDialogField::AuthMethod => {
                        dialog_state.auth_method = match value.as_str() {
                            "Agent" => AuthMethodChoice::Agent,
                            "Password" => AuthMethodChoice::Password,
                            "PublicKey" => AuthMethodChoice::PublicKey,
                            _ => dialog_state.auth_method,
                        };
                    }
                }
            }
            Task::none()
        }
        DialogMessage::HostKeyVerification(mut wrapper) => {
            if let Some(request) = wrapper.0.take() {
                portal.dialogs.open_host_key(HostKeyDialogState::from_request(*request));
                tracing::info!("Host key verification dialog opened");
            }
            Task::none()
        }
        DialogMessage::HostKeyAccept => {
            if let Some(dialog) = portal.dialogs.host_key_mut() {
                dialog.respond(HostKeyVerificationResponse::Accept);
                tracing::info!("Host key accepted for {}:{}", dialog.host, dialog.port);
            }
            portal.dialogs.close();
            Task::none()
        }
        DialogMessage::HostKeyReject => {
            if let Some(dialog) = portal.dialogs.host_key_mut() {
                dialog.respond(HostKeyVerificationResponse::Reject);
                tracing::info!("Host key rejected for {}:{}", dialog.host, dialog.port);
                portal.toast_manager.push(Toast::error(format!(
                    "Connection rejected: host key verification failed for {}",
                    dialog.host
                )));
            }
            portal.dialogs.close();
            Task::none()
        }
    }
}
