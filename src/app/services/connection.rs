use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::stream;
use iced::Task;
use secrecy::SecretString;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use crate::config::{DetectedOs, Host};
use crate::message::{
    DialogMessage, Message, SessionId, SessionMessage, SftpMessage, VerificationRequestWrapper,
};
use crate::sftp::SftpClient;
use crate::ssh::known_hosts::KnownHostsManager;
use crate::ssh::{SshClient, SshEvent};
use crate::views::sftp::PaneId;

const SSH_EVENT_CHANNEL_CAPACITY: usize = 1024;
const SSH_KEEPALIVE_INTERVAL_SECS: u64 = 60;

static KNOWN_HOSTS_MANAGER: OnceLock<Arc<Mutex<KnownHostsManager>>> = OnceLock::new();

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

pub fn ssh_connect_tasks(
    host: Arc<Host>,
    session_id: SessionId,
    host_id: Uuid,
    should_detect_os: bool,
) -> Task<Message> {
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(SSH_EVENT_CHANNEL_CAPACITY);

    let event_listener = Task::run(
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
    );

    let known_hosts = shared_known_hosts_manager();
    let ssh_client = SshClient::with_known_hosts(SSH_KEEPALIVE_INTERVAL_SECS, known_hosts);
    let host_for_task = Arc::clone(&host);
    let connect_task = Task::perform(
        async move {
            let result = ssh_client
                .connect(
                    &host_for_task,
                    (80, 24),
                    event_tx,
                    Duration::from_secs(30),
                    None,
                    should_detect_os,
                )
                .await;

            (session_id, host_id, host_for_task.name.clone(), result)
        },
        |(session_id, host_id, host_name, result)| match result {
            Ok((ssh_session, detected_os)) => Message::Session(SessionMessage::Connected {
                session_id,
                host_name,
                ssh_session,
                host_id,
                detected_os,
            }),
            Err(e) => Message::Session(SessionMessage::Error(format!("Connection failed: {}", e))),
        },
    );

    Task::batch([event_listener, connect_task])
}

/// SSH connection tasks with password authentication
pub fn ssh_connect_tasks_with_password(
    host: Arc<Host>,
    session_id: SessionId,
    host_id: Uuid,
    should_detect_os: bool,
    password: SecretString,
) -> Task<Message> {
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(SSH_EVENT_CHANNEL_CAPACITY);

    let event_listener = Task::run(
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
    );

    let known_hosts = shared_known_hosts_manager();
    let ssh_client = SshClient::with_known_hosts(SSH_KEEPALIVE_INTERVAL_SECS, known_hosts);
    let host_for_task = Arc::clone(&host);
    let connect_task = Task::perform(
        async move {
            let result = ssh_client
                .connect(
                    &host_for_task,
                    (80, 24),
                    event_tx,
                    Duration::from_secs(30),
                    Some(password),
                    should_detect_os,
                )
                .await;

            (session_id, host_id, host_for_task.name.clone(), result)
        },
        |(session_id, host_id, host_name, result)| match result {
            Ok((ssh_session, detected_os)) => Message::Session(SessionMessage::Connected {
                session_id,
                host_name,
                ssh_session,
                host_id,
                detected_os,
            }),
            Err(e) => Message::Session(SessionMessage::Error(format!("Connection failed: {}", e))),
        },
    );

    Task::batch([event_listener, connect_task])
}

pub fn sftp_connect_tasks(
    host: Arc<Host>,
    tab_id: SessionId,
    pane_id: PaneId,
    sftp_session_id: SessionId,
    host_id: Uuid,
) -> Task<Message> {
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(SSH_EVENT_CHANNEL_CAPACITY);
    let known_hosts = shared_known_hosts_manager();
    let sftp_client = SftpClient::with_known_hosts(SSH_KEEPALIVE_INTERVAL_SECS, known_hosts);

    let event_listener = Task::run(
        stream::unfold(event_rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        }),
        move |event| match event {
            SshEvent::HostKeyVerification(request) => Message::Dialog(
                DialogMessage::HostKeyVerification(VerificationRequestWrapper(Some(request))),
            ),
            _ => Message::Noop,
        },
    );

    let host_for_task = Arc::clone(&host);
    let connect_task = Task::perform(
        async move {
            let result = sftp_client
                .connect(&host_for_task, event_tx, Duration::from_secs(30), None)
                .await;

            (
                tab_id,
                pane_id,
                sftp_session_id,
                host_for_task.name.clone(),
                result,
            )
        },
        move |(tab_id, pane_id, sftp_session_id, host_name, result)| match result {
            Ok(sftp_session) => Message::Sftp(SftpMessage::Connected {
                tab_id,
                pane_id,
                sftp_session_id,
                host_id,
                host_name,
                sftp_session,
            }),
            Err(e) => Message::Session(SessionMessage::Error(format!(
                "SFTP connection failed: {}",
                e
            ))),
        },
    );

    Task::batch([event_listener, connect_task])
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
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(SSH_EVENT_CHANNEL_CAPACITY);
    let known_hosts = shared_known_hosts_manager();
    let sftp_client = SftpClient::with_known_hosts(SSH_KEEPALIVE_INTERVAL_SECS, known_hosts);

    let event_listener = Task::run(
        stream::unfold(event_rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        }),
        move |event| match event {
            SshEvent::HostKeyVerification(request) => Message::Dialog(
                DialogMessage::HostKeyVerification(VerificationRequestWrapper(Some(request))),
            ),
            _ => Message::Noop,
        },
    );

    let host_for_task = Arc::clone(&host);
    let connect_task = Task::perform(
        async move {
            let result = sftp_client
                .connect(&host_for_task, event_tx, Duration::from_secs(30), Some(password))
                .await;

            (
                tab_id,
                pane_id,
                sftp_session_id,
                host_for_task.name.clone(),
                result,
            )
        },
        move |(tab_id, pane_id, sftp_session_id, host_name, result)| match result {
            Ok(sftp_session) => Message::Sftp(SftpMessage::Connected {
                tab_id,
                pane_id,
                sftp_session_id,
                host_id,
                host_name,
                sftp_session,
            }),
            Err(e) => Message::Session(SessionMessage::Error(format!(
                "SFTP connection failed: {}",
                e
            ))),
        },
    );

    Task::batch([event_listener, connect_task])
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
