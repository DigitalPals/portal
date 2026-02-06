//! Dialog message handlers

use crate::app::Portal;
use crate::app::services::connection;
use crate::config::{AuthMethod, Host};
use crate::message::{DialogMessage, HostDialogField, Message, QuickConnectField};
use crate::security_log;
use crate::ssh::host_key_verification::HostKeyVerificationResponse;
use crate::views::dialogs::host_dialog::AuthMethodChoice;
use crate::views::dialogs::host_key_dialog::HostKeyDialogState;
use crate::views::dialogs::passphrase_dialog::PassphraseDialogState;
use crate::views::toast::Toast;
use iced::Task;
use uuid::Uuid;

/// Handle dialog messages
pub fn handle_dialog(portal: &mut Portal, msg: DialogMessage) -> Task<Message> {
    match msg {
        DialogMessage::Close => {
            portal.dialogs.close();
            Task::none()
        }
        DialogMessage::Submit => {
            if let Some(dialog_state) = portal.dialogs.host_mut() {
                // to_host() runs validation and returns None if validation fails
                // Validation errors are stored in dialog_state.validation_errors
                let editing_id = dialog_state.editing_id;
                if let Some(host) = dialog_state.to_host() {
                    // Preserve created_at for edits
                    let host = if let Some(existing_id) = editing_id {
                        if let Some(existing) = portal.config.hosts.find_host(existing_id) {
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

                    let is_edit = editing_id.is_some();
                    if is_edit {
                        if let Err(e) = portal.config.hosts.update_host(host.clone()) {
                            tracing::error!("Failed to update host: {}", e);
                        } else {
                            tracing::info!("Updated host");
                        }
                    } else {
                        portal.config.hosts.add_host(host.clone());
                        tracing::info!("Added host");
                    }

                    if let Err(e) = portal.config.hosts.save() {
                        tracing::error!("Failed to save config: {}", e);
                    }
                    portal.dialogs.close();
                }
                // If to_host() returned None, validation failed and errors are shown in the UI
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
                    HostDialogField::Protocol => {
                        use crate::views::dialogs::host_dialog::ProtocolChoice;
                        dialog_state.protocol = match value.as_str() {
                            "Ssh" => ProtocolChoice::Ssh,
                            "Vnc" => {
                                // Auto-set port to 5900 for VNC
                                if dialog_state.port == "22" {
                                    dialog_state.port = "5900".to_string();
                                }
                                ProtocolChoice::Vnc
                            }
                            _ => dialog_state.protocol,
                        };
                    }
                }
            }
            Task::none()
        }
        DialogMessage::ImportFromSshConfig => {
            match portal.config.hosts.import_from_ssh_config() {
                Ok(count) => {
                    if count > 0 {
                        if let Err(e) = portal.config.hosts.save() {
                            tracing::error!("Failed to save hosts after SSH import: {}", e);
                            portal
                                .toast_manager
                                .push(Toast::error("Failed to save imported hosts"));
                            return Task::none();
                        }
                    }
                    let message = format!("Imported {} host(s) from SSH config", count);
                    if count > 0 {
                        portal.toast_manager.push(Toast::success(message));
                    } else {
                        portal.toast_manager.push(Toast::warning(message));
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to import hosts from SSH config: {}", e);
                    portal
                        .toast_manager
                        .push(Toast::error("Failed to import SSH config"));
                }
            }
            Task::none()
        }
        DialogMessage::HostKeyVerification(mut wrapper) => {
            if let Some(request) = wrapper.0.take() {
                portal
                    .dialogs
                    .open_host_key(HostKeyDialogState::from_request(*request));
                tracing::info!("Host key verification dialog opened");
            }
            Task::none()
        }
        DialogMessage::HostKeyAccept => {
            if let Some(dialog) = portal.dialogs.host_key_mut() {
                let was_changed = dialog.is_changed_host;
                dialog.respond(HostKeyVerificationResponse::Accept);
                tracing::info!("Host key accepted");
                // Log security event - warn if host key was changed (potential MITM)
                security_log::log_host_key_accepted(
                    &dialog.host,
                    dialog.port,
                    &dialog.fingerprint,
                    was_changed,
                );
            }
            portal.dialogs.close();
            Task::none()
        }
        DialogMessage::HostKeyReject => {
            if let Some(dialog) = portal.dialogs.host_key_mut() {
                dialog.respond(HostKeyVerificationResponse::Reject);
                tracing::info!("Host key rejected");
                // Log security event
                security_log::log_host_key_rejected(
                    &dialog.host,
                    dialog.port,
                    "User rejected host key",
                );
                portal.toast_manager.push(Toast::error(format!(
                    "Connection rejected: host key verification failed for {}",
                    dialog.host
                )));
            }
            portal.dialogs.close();
            Task::none()
        }
        DialogMessage::PasswordUsernameChanged(username) => {
            if let Some(dialog) = portal.dialogs.password_mut() {
                dialog.username = username;
            }
            Task::none()
        }
        DialogMessage::PasswordChanged(password) => {
            if let Some(dialog) = portal.dialogs.password_mut() {
                dialog.password = password;
                // Clear any previous error when user starts typing
                dialog.error = None;
            }
            Task::none()
        }
        DialogMessage::PasswordSubmit => {
            if let Some(dialog) = portal.dialogs.password_mut() {
                let password = std::mem::take(&mut dialog.password);
                let host_id = dialog.host_id;
                let connection_kind = dialog.connection_kind;
                let sftp_context = dialog.sftp_context;
                let dialog_username = dialog.username.clone();
                dialog.error = None;

                // Find the host and start connection with password
                if let Some(host) = portal.config.hosts.find_host(host_id) {
                    let host = std::sync::Arc::new(host.clone());

                    portal.dialogs.close();

                    use crate::views::dialogs::password_dialog::PasswordConnectionKind;
                    let protocol_label = match &connection_kind {
                        PasswordConnectionKind::Ssh => "SSH",
                        PasswordConnectionKind::Sftp => "SFTP",
                        PasswordConnectionKind::Vnc => "VNC",
                    };
                    portal
                        .dialogs
                        .open_connecting(host.name.clone(), protocol_label);

                    match connection_kind {
                        PasswordConnectionKind::Ssh => {
                            let session_id = uuid::Uuid::new_v4();
                            let should_detect_os =
                                connection::should_detect_os(host.detected_os.as_ref());
                            return connection::ssh_connect_tasks_with_password(
                                host,
                                session_id,
                                host_id,
                                should_detect_os,
                                password,
                            );
                        }
                        PasswordConnectionKind::Sftp => {
                            if let Some(ctx) = sftp_context {
                                let sftp_session_id = uuid::Uuid::new_v4();
                                return connection::sftp_connect_tasks_with_password(
                                    host,
                                    ctx.tab_id,
                                    ctx.pane_id,
                                    sftp_session_id,
                                    host_id,
                                    password,
                                );
                            }
                        }
                        PasswordConnectionKind::Vnc => {
                            // Use username from dialog (user may have edited it)
                            if host.username != dialog_username {
                                let mut host_with_username = (*host).clone();
                                host_with_username.username = dialog_username;
                                let host = std::sync::Arc::new(host_with_username);
                                return portal.connect_vnc_host_with_password(&host, password);
                            }
                            return portal.connect_vnc_host_with_password(&host, password);
                        }
                    }
                }
            }
            Task::none()
        }
        DialogMessage::PasswordCancel => {
            if let Some(dialog) = portal.dialogs.password_mut() {
                // Clear password for security
                dialog.clear_password();
            }
            portal.dialogs.close();
            Task::none()
        }
        DialogMessage::PassphraseRequired(request) => {
            portal
                .dialogs
                .open_passphrase(PassphraseDialogState::from_request(request));
            Task::none()
        }
        DialogMessage::PassphraseChanged(passphrase) => {
            if let Some(dialog) = portal.dialogs.passphrase_mut() {
                dialog.passphrase = passphrase;
                // Clear any previous error when user starts typing
                dialog.error = None;
            }
            Task::none()
        }
        DialogMessage::PassphraseSubmit => {
            if let Some(dialog) = portal.dialogs.passphrase_mut() {
                let passphrase = std::mem::take(&mut dialog.passphrase);
                let host_id = dialog.host_id;
                let is_ssh = dialog.is_ssh;
                let session_id = dialog.session_id;
                let should_detect_os = dialog.should_detect_os;
                let sftp_context = dialog.sftp_context;
                dialog.error = None;

                // Find the host and start connection with passphrase
                if let Some(host) = portal.config.hosts.find_host(host_id) {
                    let host = std::sync::Arc::new(host.clone());
                    portal.dialogs.close();

                    if is_ssh {
                        if let Some(session_id) = session_id {
                            return connection::ssh_connect_tasks_with_passphrase(
                                host,
                                session_id,
                                host_id,
                                should_detect_os,
                                passphrase,
                            );
                        }
                    } else if let Some(ctx) = sftp_context {
                        return connection::sftp_connect_tasks_with_passphrase(
                            host,
                            ctx.tab_id,
                            ctx.pane_id,
                            ctx.sftp_session_id,
                            host_id,
                            passphrase,
                        );
                    }
                }
            }
            Task::none()
        }
        DialogMessage::PassphraseCancel => {
            if let Some(dialog) = portal.dialogs.passphrase_mut() {
                // Clear passphrase for security
                dialog.clear_passphrase();
            }
            portal.dialogs.close();
            Task::none()
        }
        DialogMessage::QuickConnectFieldChanged(field, value) => {
            if let Some(dialog_state) = portal.dialogs.quick_connect_mut() {
                match field {
                    QuickConnectField::Hostname => dialog_state.hostname = value,
                    QuickConnectField::Port => dialog_state.port = value,
                    QuickConnectField::Username => dialog_state.username = value,
                    QuickConnectField::AuthMethod => {
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
        DialogMessage::QuickConnectSubmit => {
            if let Some(dialog_state) = portal.dialogs.quick_connect_mut() {
                // Run validation
                if !dialog_state.validate() {
                    return Task::none();
                }

                let hostname = dialog_state.hostname.trim().to_string();
                let port = dialog_state.port_u16();
                let username = dialog_state.effective_username();
                let auth_method = dialog_state.auth_method;

                let auth = match auth_method {
                    AuthMethodChoice::Agent => AuthMethod::Agent,
                    AuthMethodChoice::Password => AuthMethod::Password,
                    AuthMethodChoice::PublicKey => AuthMethod::PublicKey { key_path: None },
                };

                let now = chrono::Utc::now();
                let temp_host = Host {
                    id: Uuid::new_v4(),
                    name: format!("{}@{}", username, hostname),
                    hostname,
                    port,
                    username,
                    protocol: crate::config::Protocol::Ssh,
                    vnc_port: None,
                    auth,
                    group_id: None,
                    notes: None,
                    tags: vec![],
                    created_at: now,
                    updated_at: now,
                    detected_os: None,
                    last_connected: None,
                };

                portal.dialogs.close();
                tracing::info!("Quick connect requested");
                return portal.connect_to_host(&temp_host);
            }
            Task::none()
        }
    }
}
