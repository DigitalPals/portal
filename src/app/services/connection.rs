use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::stream;
use iced::Task;
use secrecy::SecretString;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use crate::config::{DetectedOs, Host};
use crate::message::{
    DialogMessage, Message, PassphraseRequest, PassphraseSftpContext, SessionId, SessionMessage,
    SftpMessage, VerificationRequestWrapper,
};
use crate::sftp::SftpClient;
use crate::ssh::known_hosts::KnownHostsManager;
use crate::ssh::{SshClient, SshEvent};
use crate::views::sftp::PaneId;

const SSH_EVENT_CHANNEL_CAPACITY: usize = 1024;
const SSH_KEEPALIVE_INTERVAL_SECS: u64 = 60;

static KNOWN_HOSTS_MANAGER: OnceLock<Arc<Mutex<KnownHostsManager>>> = OnceLock::new();

enum SshAuth {
    None,
    Password(SecretString),
    Passphrase(SecretString),
}

impl SshAuth {
    fn split(self) -> (Option<SecretString>, Option<SecretString>) {
        match self {
            SshAuth::None => (None, None),
            SshAuth::Password(password) => (Some(password), None),
            SshAuth::Passphrase(passphrase) => (None, Some(passphrase)),
        }
    }
}

enum SftpAuth {
    None,
    Password(SecretString),
    Passphrase(SecretString),
}

impl SftpAuth {
    fn split(self) -> (Option<SecretString>, Option<SecretString>) {
        match self {
            SftpAuth::None => (None, None),
            SftpAuth::Password(password) => (Some(password), None),
            SftpAuth::Passphrase(passphrase) => (None, Some(passphrase)),
        }
    }
}

pub fn shared_known_hosts_manager() -> Arc<Mutex<KnownHostsManager>> {
    KNOWN_HOSTS_MANAGER
        .get_or_init(|| Arc::new(Mutex::new(KnownHostsManager::new())))
        .clone()
}

pub fn should_detect_os(detected_os: Option<&DetectedOs>) -> bool {
    match detected_os {
        None => true,
        Some(DetectedOs::Linux) => true,
        Some(_) => false,
    }
}

fn ssh_event_listener(session_id: SessionId, event_rx: mpsc::Receiver<SshEvent>) -> Task<Message> {
    Task::run(
        stream::unfold(event_rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        }),
        move |event| match event {
            SshEvent::Data(data) => Message::Session(SessionMessage::Data(session_id, data)),
            SshEvent::Disconnected => Message::Session(SessionMessage::Disconnected(session_id)),
            SshEvent::HostKeyVerification(request) => Message::Dialog(
                DialogMessage::HostKeyVerification(VerificationRequestWrapper(Some(request))),
            ),
            SshEvent::Connected => Message::Noop,
        },
    )
}

fn sftp_event_listener(event_rx: mpsc::Receiver<SshEvent>) -> Task<Message> {
    Task::run(
        stream::unfold(event_rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        }),
        move |event| match event {
            SshEvent::HostKeyVerification(request) => Message::Dialog(
                DialogMessage::HostKeyVerification(VerificationRequestWrapper(Some(request))),
            ),
            _ => Message::Noop,
        },
    )
}

fn ssh_connect_tasks_with_auth(
    host: Arc<Host>,
    session_id: SessionId,
    host_id: Uuid,
    should_detect_os: bool,
    auth: SshAuth,
) -> Task<Message> {
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(SSH_EVENT_CHANNEL_CAPACITY);
    let event_listener = ssh_event_listener(session_id, event_rx);

    let known_hosts = shared_known_hosts_manager();
    let ssh_client = SshClient::with_known_hosts(SSH_KEEPALIVE_INTERVAL_SECS, known_hosts);
    let host_for_task = Arc::clone(&host);
    let (password, passphrase) = auth.split();
    let connect_task = Task::perform(
        async move {
            let result = ssh_client
                .connect(
                    &host_for_task,
                    (80, 24),
                    event_tx,
                    Duration::from_secs(30),
                    password,
                    passphrase,
                    should_detect_os,
                )
                .await;

            (session_id, host_id, host_for_task, result, should_detect_os)
        },
        |(session_id, host_id, host, result, should_detect_os)| match result {
            Ok((ssh_session, detected_os)) => Message::Session(SessionMessage::Connected {
                session_id,
                host_name: host.name.clone(),
                ssh_session,
                host_id,
                detected_os,
            }),
            Err(e) => map_ssh_connect_error(session_id, host_id, &host, should_detect_os, e),
        },
    );

    Task::batch([event_listener, connect_task])
}

pub fn ssh_connect_tasks(
    host: Arc<Host>,
    session_id: SessionId,
    host_id: Uuid,
    should_detect_os: bool,
) -> Task<Message> {
    ssh_connect_tasks_with_auth(host, session_id, host_id, should_detect_os, SshAuth::None)
}

/// SSH connection tasks with password authentication
pub fn ssh_connect_tasks_with_password(
    host: Arc<Host>,
    session_id: SessionId,
    host_id: Uuid,
    should_detect_os: bool,
    password: SecretString,
) -> Task<Message> {
    ssh_connect_tasks_with_auth(
        host,
        session_id,
        host_id,
        should_detect_os,
        SshAuth::Password(password),
    )
}

/// SSH connection tasks with key passphrase authentication
pub fn ssh_connect_tasks_with_passphrase(
    host: Arc<Host>,
    session_id: SessionId,
    host_id: Uuid,
    should_detect_os: bool,
    passphrase: SecretString,
) -> Task<Message> {
    ssh_connect_tasks_with_auth(
        host,
        session_id,
        host_id,
        should_detect_os,
        SshAuth::Passphrase(passphrase),
    )
}

pub fn sftp_connect_tasks(
    host: Arc<Host>,
    tab_id: SessionId,
    pane_id: PaneId,
    sftp_session_id: SessionId,
    host_id: Uuid,
) -> Task<Message> {
    sftp_connect_tasks_with_auth(
        host,
        tab_id,
        pane_id,
        sftp_session_id,
        host_id,
        SftpAuth::None,
    )
}

/// SFTP connection tasks with password authentication
pub fn sftp_connect_tasks_with_password(
    host: Arc<Host>,
    tab_id: SessionId,
    pane_id: PaneId,
    sftp_session_id: SessionId,
    host_id: Uuid,
    password: SecretString,
) -> Task<Message> {
    sftp_connect_tasks_with_auth(
        host,
        tab_id,
        pane_id,
        sftp_session_id,
        host_id,
        SftpAuth::Password(password),
    )
}

/// SFTP connection tasks with key passphrase authentication
pub fn sftp_connect_tasks_with_passphrase(
    host: Arc<Host>,
    tab_id: SessionId,
    pane_id: PaneId,
    sftp_session_id: SessionId,
    host_id: Uuid,
    passphrase: SecretString,
) -> Task<Message> {
    sftp_connect_tasks_with_auth(
        host,
        tab_id,
        pane_id,
        sftp_session_id,
        host_id,
        SftpAuth::Passphrase(passphrase),
    )
}

fn sftp_connect_tasks_with_auth(
    host: Arc<Host>,
    tab_id: SessionId,
    pane_id: PaneId,
    sftp_session_id: SessionId,
    host_id: Uuid,
    auth: SftpAuth,
) -> Task<Message> {
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(SSH_EVENT_CHANNEL_CAPACITY);
    let known_hosts = shared_known_hosts_manager();
    let sftp_client = SftpClient::with_known_hosts(SSH_KEEPALIVE_INTERVAL_SECS, known_hosts);
    let event_listener = sftp_event_listener(event_rx);

    let host_for_task = Arc::clone(&host);
    let (password, passphrase) = auth.split();
    let connect_task = Task::perform(
        async move {
            let result = sftp_client
                .connect(
                    &host_for_task,
                    event_tx,
                    Duration::from_secs(30),
                    password,
                    passphrase,
                )
                .await;

            (tab_id, pane_id, sftp_session_id, host_for_task, result)
        },
        move |(tab_id, pane_id, sftp_session_id, host, result)| match result {
            Ok(sftp_session) => Message::Sftp(SftpMessage::Connected {
                tab_id,
                pane_id,
                sftp_session_id,
                host_id,
                host_name: host.name.clone(),
                sftp_session,
            }),
            Err(e) => map_sftp_connect_error(tab_id, pane_id, sftp_session_id, host_id, &host, e),
        },
    );

    Task::batch([event_listener, connect_task])
}

fn map_ssh_connect_error(
    session_id: SessionId,
    host_id: Uuid,
    host: &Host,
    should_detect_os: bool,
    error: crate::error::SshError,
) -> Message {
    match error {
        crate::error::SshError::KeyFilePassphraseRequired(path) => {
            Message::Dialog(DialogMessage::PassphraseRequired(PassphraseRequest {
                host_id,
                host_name: host.name.clone(),
                hostname: host.hostname.clone(),
                port: host.port,
                username: host.username.clone(),
                key_path: path,
                is_ssh: true,
                session_id: Some(session_id),
                should_detect_os,
                sftp_context: None,
                error: None,
            }))
        }
        crate::error::SshError::KeyFilePassphraseInvalid(path) => {
            Message::Dialog(DialogMessage::PassphraseRequired(PassphraseRequest {
                host_id,
                host_name: host.name.clone(),
                hostname: host.hostname.clone(),
                port: host.port,
                username: host.username.clone(),
                key_path: path,
                is_ssh: true,
                session_id: Some(session_id),
                should_detect_os,
                sftp_context: None,
                error: Some("Incorrect passphrase".to_string()),
            }))
        }
        _ => Message::Session(SessionMessage::Error(format!(
            "Connection failed: {}",
            error
        ))),
    }
}

fn map_sftp_connect_error(
    tab_id: SessionId,
    pane_id: PaneId,
    sftp_session_id: SessionId,
    host_id: Uuid,
    host: &Host,
    error: crate::error::SftpError,
) -> Message {
    match error {
        crate::error::SftpError::KeyFilePassphraseRequired(path) => {
            Message::Dialog(DialogMessage::PassphraseRequired(PassphraseRequest {
                host_id,
                host_name: host.name.clone(),
                hostname: host.hostname.clone(),
                port: host.port,
                username: host.username.clone(),
                key_path: path,
                is_ssh: false,
                session_id: None,
                should_detect_os: false,
                sftp_context: Some(PassphraseSftpContext {
                    tab_id,
                    pane_id,
                    sftp_session_id,
                }),
                error: None,
            }))
        }
        crate::error::SftpError::KeyFilePassphraseInvalid(path) => {
            Message::Dialog(DialogMessage::PassphraseRequired(PassphraseRequest {
                host_id,
                host_name: host.name.clone(),
                hostname: host.hostname.clone(),
                port: host.port,
                username: host.username.clone(),
                key_path: path,
                is_ssh: false,
                session_id: None,
                should_detect_os: false,
                sftp_context: Some(PassphraseSftpContext {
                    tab_id,
                    pane_id,
                    sftp_session_id,
                }),
                error: Some("Incorrect passphrase".to_string()),
            }))
        }
        _ => Message::Session(SessionMessage::Error(format!(
            "SFTP connection failed: {}",
            error
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AuthMethod;
    use crate::error::{SftpError, SshError};
    use chrono::Utc;
    use std::path::PathBuf;

    #[test]
    fn should_detect_os_when_missing() {
        assert!(should_detect_os(None));
    }

    #[test]
    fn should_detect_os_when_generic_linux() {
        assert!(should_detect_os(Some(&DetectedOs::Linux)));
    }

    #[test]
    fn should_detect_os_when_specific_os() {
        assert!(!should_detect_os(Some(&DetectedOs::MacOS)));
    }

    #[test]
    fn ssh_connect_error_requests_passphrase() {
        let now = Utc::now();
        let host = Host {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            hostname: "example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            auth: AuthMethod::PublicKey { key_path: None },
            protocol: crate::config::Protocol::Ssh,
            vnc_port: None,
            group_id: None,
            notes: None,
            tags: vec![],
            created_at: now,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        };
        let session_id = Uuid::new_v4();
        let host_id = host.id;
        let error = SshError::KeyFilePassphraseRequired(PathBuf::from("/tmp/id_ed25519"));

        let message = map_ssh_connect_error(session_id, host_id, &host, true, error);
        match message {
            Message::Dialog(DialogMessage::PassphraseRequired(request)) => {
                assert!(request.is_ssh);
                assert_eq!(request.session_id, Some(session_id));
                assert_eq!(request.key_path, PathBuf::from("/tmp/id_ed25519"));
                assert!(request.error.is_none());
            }
            other => panic!("unexpected message: {:?}", other),
        }
    }

    #[test]
    fn sftp_connect_error_requests_passphrase() {
        let now = Utc::now();
        let host = Host {
            id: Uuid::new_v4(),
            name: "Test".to_string(),
            hostname: "example.com".to_string(),
            port: 22,
            username: "user".to_string(),
            auth: AuthMethod::PublicKey { key_path: None },
            protocol: crate::config::Protocol::Ssh,
            vnc_port: None,
            group_id: None,
            notes: None,
            tags: vec![],
            created_at: now,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        };
        let tab_id = Uuid::new_v4();
        let pane_id = PaneId::Left;
        let sftp_session_id = Uuid::new_v4();
        let host_id = host.id;
        let error = SftpError::KeyFilePassphraseRequired(PathBuf::from("/tmp/id_ed25519"));

        let message =
            map_sftp_connect_error(tab_id, pane_id, sftp_session_id, host_id, &host, error);
        match message {
            Message::Dialog(DialogMessage::PassphraseRequired(request)) => {
                assert!(!request.is_ssh);
                assert!(request.session_id.is_none());
                assert_eq!(request.key_path, PathBuf::from("/tmp/id_ed25519"));
                assert!(request.sftp_context.is_some());
            }
            other => panic!("unexpected message: {:?}", other),
        }
    }
}
