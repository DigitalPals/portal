use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::stream;
use iced::Task;
use secrecy::SecretString;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use crate::config::settings::PortalHubSettings;
use crate::config::{DetectedOs, Host, PortForwardKind, Protocol};
use crate::message::{
    DialogMessage, Message, PassphraseRequest, PassphraseSftpContext, SessionId, SessionMessage,
    SftpMessage, VerificationRequestWrapper,
};
use crate::proxy::{ListedProxySession, ProxyEvent, ProxySession, ProxySessionTarget};
use crate::sftp::SftpClient;
use crate::ssh::known_hosts::KnownHostsManager;
use crate::ssh::passphrase_cache::PassphraseCache;
use crate::ssh::{SshClient, SshEvent};
use crate::views::sftp::PaneId;

const SSH_EVENT_CHANNEL_CAPACITY: usize = 1024;
const SSH_DATA_COALESCE_LIMIT: usize = 256 * 1024;
const SSH_KEEPALIVE_INTERVAL_SECS: u64 = 60;
const DEFAULT_CREDENTIAL_TIMEOUT: u64 = 300; // 5 minutes

static KNOWN_HOSTS_MANAGER: OnceLock<Arc<Mutex<KnownHostsManager>>> = OnceLock::new();
static PASSPHRASE_CACHE: OnceLock<Arc<PassphraseCache>> = OnceLock::new();

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

/// Get the shared passphrase cache instance
pub fn shared_passphrase_cache() -> Arc<PassphraseCache> {
    PASSPHRASE_CACHE
        .get_or_init(|| Arc::new(PassphraseCache::new(DEFAULT_CREDENTIAL_TIMEOUT)))
        .clone()
}

/// Initialize the passphrase cache with a custom timeout
pub fn init_passphrase_cache(timeout_seconds: u64) {
    if let Some(cache) = PASSPHRASE_CACHE.get() {
        // Apply updates at runtime; this only affects new entries.
        cache.set_timeout(timeout_seconds);
        return;
    }

    let _ = PASSPHRASE_CACHE.get_or_init(|| Arc::new(PassphraseCache::new(timeout_seconds)));
}

pub fn should_detect_os(detected_os: Option<&DetectedOs>) -> bool {
    match detected_os {
        None => true,
        Some(DetectedOs::Linux) => true,
        Some(_) => false,
    }
}

pub fn should_use_portal_hub(settings: &PortalHubSettings, host: &Host) -> bool {
    settings.is_configured() && host.portal_hub_enabled && host.protocol == Protocol::Ssh
}

fn ssh_event_listener(session_id: SessionId, event_rx: mpsc::Receiver<SshEvent>) -> Task<Message> {
    Task::run(
        stream::unfold(SshEventStreamState::new(event_rx), |mut state| async move {
            next_coalesced_ssh_event(&mut state)
                .await
                .map(|event| (event, state))
        }),
        move |event| match event {
            SshEvent::Data(data) => Message::Session(SessionMessage::Data(session_id, data)),
            SshEvent::Disconnected { clean } => {
                Message::Session(SessionMessage::Disconnected { session_id, clean })
            }
            SshEvent::HostKeyVerification(request) => Message::Dialog(
                DialogMessage::HostKeyVerification(VerificationRequestWrapper(Some(request))),
            ),
            SshEvent::Connected => Message::Noop,
        },
    )
}

struct SshEventStreamState {
    rx: mpsc::Receiver<SshEvent>,
    pending: Option<SshEvent>,
}

impl SshEventStreamState {
    fn new(rx: mpsc::Receiver<SshEvent>) -> Self {
        Self { rx, pending: None }
    }
}

async fn next_coalesced_ssh_event(state: &mut SshEventStreamState) -> Option<SshEvent> {
    let event = match state.pending.take() {
        Some(event) => event,
        None => state.rx.recv().await?,
    };

    let SshEvent::Data(mut data) = event else {
        return Some(event);
    };

    while data.len() < SSH_DATA_COALESCE_LIMIT {
        match state.rx.try_recv() {
            Ok(SshEvent::Data(next)) if data.len() + next.len() <= SSH_DATA_COALESCE_LIMIT => {
                data.extend_from_slice(&next);
            }
            Ok(event) => {
                state.pending = Some(event);
                break;
            }
            Err(mpsc::error::TryRecvError::Empty) => break,
            Err(mpsc::error::TryRecvError::Disconnected) => break,
        }
    }

    Some(SshEvent::Data(data))
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

fn proxy_event_listener(
    session_id: SessionId,
    event_rx: mpsc::Receiver<ProxyEvent>,
) -> Task<Message> {
    Task::run(
        stream::unfold(event_rx, |mut rx| async move {
            rx.recv().await.map(|event| (event, rx))
        }),
        move |event| match event {
            ProxyEvent::Data(data) => Message::Session(SessionMessage::Data(session_id, data)),
            ProxyEvent::Disconnected { clean } => {
                Message::Session(SessionMessage::Disconnected { session_id, clean })
            }
        },
    )
}

pub fn proxy_connect_tasks(
    settings: PortalHubSettings,
    host: Arc<Host>,
    session_id: SessionId,
    host_id: Uuid,
    terminal_size: (u16, u16),
) -> Task<Message> {
    let (event_tx, event_rx) = mpsc::channel::<ProxyEvent>(SSH_EVENT_CHANNEL_CAPACITY);
    let event_listener = proxy_event_listener(session_id, event_rx);
    let host_for_task = Arc::clone(&host);

    let connect_task = Task::perform(
        async move {
            let result = ProxySession::spawn(
                &settings,
                &host_for_task,
                session_id,
                terminal_size.0,
                terminal_size.1,
                event_tx,
            )
            .map(Arc::new)
            .map_err(|error| error.to_string());
            (session_id, host_id, host_for_task, result)
        },
        |(session_id, host_id, host, result)| match result {
            Ok(proxy_session) => Message::Session(SessionMessage::ProxyConnected {
                session_id,
                proxy_session,
                host_name: host.name.clone(),
                host_id: Some(host_id),
                session_started_at: None,
            }),
            Err(error) => Message::Session(SessionMessage::ConnectFailed {
                session_id,
                error: format!("Portal Hub connection failed: {}", error),
            }),
        },
    );

    Task::batch([event_listener, connect_task])
}

pub fn proxy_resume_tasks(
    settings: PortalHubSettings,
    listed_session: ListedProxySession,
    host_id: Option<Uuid>,
    display_name: String,
    terminal_size: (u16, u16),
) -> Task<Message> {
    let session_id = listed_session.session_id;
    let session_started_at = listed_session.created_at;
    let target = ProxySessionTarget {
        session_id,
        target_host: listed_session.target_host,
        target_port: listed_session.target_port,
        target_user: listed_session.target_user,
    };
    let (event_tx, event_rx) = mpsc::channel::<ProxyEvent>(SSH_EVENT_CHANNEL_CAPACITY);
    let event_listener = proxy_event_listener(session_id, event_rx);

    let connect_task = Task::perform(
        async move {
            let result = ProxySession::spawn_target(
                &settings,
                &target,
                terminal_size.0,
                terminal_size.1,
                event_tx,
            )
            .map(Arc::new)
            .map_err(|error| error.to_string());
            (session_id, display_name, host_id, result)
        },
        move |(session_id, host_name, host_id, result)| match result {
            Ok(proxy_session) => Message::Session(SessionMessage::ProxyConnected {
                session_id,
                proxy_session,
                host_name,
                host_id,
                session_started_at: Some(session_started_at),
            }),
            Err(error) => Message::Session(SessionMessage::ConnectFailed {
                session_id,
                error: format!("Portal Hub connection failed: {}", error),
            }),
        },
    );

    Task::batch([event_listener, connect_task])
}

fn ssh_connect_tasks_with_auth(
    host: Arc<Host>,
    session_id: SessionId,
    host_id: Uuid,
    terminal_size: (u16, u16),
    should_detect_os: bool,
    allow_agent_forwarding: bool,
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
                    terminal_size,
                    event_tx,
                    Duration::from_secs(30),
                    password,
                    passphrase,
                    should_detect_os,
                    allow_agent_forwarding,
                )
                .await;
            let result = match result {
                Ok((session, detected_os)) => {
                    for forward in host_for_task
                        .port_forwards
                        .iter()
                        .filter(|forward| forward.enabled)
                    {
                        let creation_result = match forward.kind {
                            PortForwardKind::Local => {
                                session.create_local_forward(forward.clone()).await
                            }
                            PortForwardKind::Remote => {
                                session.create_remote_forward(forward.clone()).await
                            }
                            PortForwardKind::Dynamic => {
                                session.create_dynamic_forward(forward.clone()).await
                            }
                        };

                        if let Err(e) = creation_result {
                            tracing::warn!(
                                "Failed to create port forward {} on {}: {}",
                                forward.id,
                                host_for_task.name,
                                e
                            );
                        }
                    }
                    Ok((session, detected_os))
                }
                Err(e) => Err(e),
            };

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
    terminal_size: (u16, u16),
    should_detect_os: bool,
    allow_agent_forwarding: bool,
) -> Task<Message> {
    ssh_connect_tasks_with_auth(
        host,
        session_id,
        host_id,
        terminal_size,
        should_detect_os,
        allow_agent_forwarding,
        SshAuth::None,
    )
}

/// SSH connection tasks with password authentication
pub fn ssh_connect_tasks_with_password(
    host: Arc<Host>,
    session_id: SessionId,
    host_id: Uuid,
    terminal_size: (u16, u16),
    should_detect_os: bool,
    allow_agent_forwarding: bool,
    password: SecretString,
) -> Task<Message> {
    ssh_connect_tasks_with_auth(
        host,
        session_id,
        host_id,
        terminal_size,
        should_detect_os,
        allow_agent_forwarding,
        SshAuth::Password(password),
    )
}

/// SSH connection tasks with key passphrase authentication
pub fn ssh_connect_tasks_with_passphrase(
    host: Arc<Host>,
    session_id: SessionId,
    host_id: Uuid,
    terminal_size: (u16, u16),
    should_detect_os: bool,
    allow_agent_forwarding: bool,
    passphrase: SecretString,
) -> Task<Message> {
    ssh_connect_tasks_with_auth(
        host,
        session_id,
        host_id,
        terminal_size,
        should_detect_os,
        allow_agent_forwarding,
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
        _ => Message::Session(SessionMessage::ConnectFailed {
            session_id,
            error: format!("Connection failed: {}", error),
        }),
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

    #[tokio::test]
    async fn coalesces_consecutive_ssh_data_events() {
        let (tx, rx) = mpsc::channel(8);
        tx.send(SshEvent::Data(b"a".to_vec())).await.unwrap();
        tx.send(SshEvent::Data(b"b".to_vec())).await.unwrap();
        drop(tx);

        let mut state = SshEventStreamState::new(rx);
        match next_coalesced_ssh_event(&mut state).await {
            Some(SshEvent::Data(data)) => assert_eq!(data, b"ab"),
            other => panic!("unexpected event: {:?}", other),
        }
        assert!(next_coalesced_ssh_event(&mut state).await.is_none());
    }

    #[tokio::test]
    async fn coalescing_preserves_non_data_event_order() {
        let (tx, rx) = mpsc::channel(8);
        tx.send(SshEvent::Data(b"a".to_vec())).await.unwrap();
        tx.send(SshEvent::Disconnected { clean: false })
            .await
            .unwrap();
        tx.send(SshEvent::Data(b"b".to_vec())).await.unwrap();
        drop(tx);

        let mut state = SshEventStreamState::new(rx);
        match next_coalesced_ssh_event(&mut state).await {
            Some(SshEvent::Data(data)) => assert_eq!(data, b"a"),
            other => panic!("unexpected first event: {:?}", other),
        }
        match next_coalesced_ssh_event(&mut state).await {
            Some(SshEvent::Disconnected { clean }) => assert!(!clean),
            other => panic!("unexpected second event: {:?}", other),
        }
        match next_coalesced_ssh_event(&mut state).await {
            Some(SshEvent::Data(data)) => assert_eq!(data, b"b"),
            other => panic!("unexpected third event: {:?}", other),
        }
    }

    #[tokio::test]
    async fn coalescing_stops_at_limit() {
        let (tx, rx) = mpsc::channel(8);
        tx.send(SshEvent::Data(vec![b'a'; SSH_DATA_COALESCE_LIMIT - 1]))
            .await
            .unwrap();
        tx.send(SshEvent::Data(vec![b'b'; 2])).await.unwrap();
        drop(tx);

        let mut state = SshEventStreamState::new(rx);
        match next_coalesced_ssh_event(&mut state).await {
            Some(SshEvent::Data(data)) => {
                assert_eq!(data.len(), SSH_DATA_COALESCE_LIMIT - 1);
                assert!(data.iter().all(|byte| *byte == b'a'));
            }
            other => panic!("unexpected first event: {:?}", other),
        }
        match next_coalesced_ssh_event(&mut state).await {
            Some(SshEvent::Data(data)) => assert_eq!(data, vec![b'b'; 2]),
            other => panic!("unexpected second event: {:?}", other),
        }
    }

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
            auth: AuthMethod::PublicKey {
                key_path: None,
                vault_key_id: None,
            },
            protocol: crate::config::Protocol::Ssh,
            vnc_port: None,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            portal_hub_enabled: false,
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
            auth: AuthMethod::PublicKey {
                key_path: None,
                vault_key_id: None,
            },
            protocol: crate::config::Protocol::Ssh,
            vnc_port: None,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            portal_hub_enabled: false,
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

    fn proxy_test_host(auth: AuthMethod) -> Host {
        let now = Utc::now();
        Host {
            id: Uuid::new_v4(),
            name: "Proxy Test".to_string(),
            hostname: "example.com".to_string(),
            port: 22,
            username: "john".to_string(),
            auth,
            protocol: crate::config::Protocol::Ssh,
            vnc_port: None,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            portal_hub_enabled: true,
            group_id: None,
            notes: None,
            tags: vec![],
            created_at: now,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        }
    }

    fn configured_proxy_settings() -> PortalHubSettings {
        PortalHubSettings {
            enabled: true,
            hosts_sync_enabled: true,
            settings_sync_enabled: true,
            snippets_sync_enabled: true,
            key_vault_enabled: true,
            default_for_new_ssh_hosts: false,
            host: "proxy.example.com".to_string(),
            web_port: 8080,
            port: 22,
            username: "portal-hub".to_string(),
            identity_file: None,
            web_url: "http://portal-hub.localhost:8080".to_string(),
        }
    }

    #[test]
    fn portal_hub_routing_requires_global_and_host_enablement() {
        let settings = configured_proxy_settings();
        let mut host = proxy_test_host(AuthMethod::PublicKey {
            key_path: None,
            vault_key_id: Some(Uuid::new_v4()),
        });

        assert!(should_use_portal_hub(&settings, &host));

        host.portal_hub_enabled = false;
        assert!(!should_use_portal_hub(&settings, &host));

        host.portal_hub_enabled = true;
        let mut disabled = settings.clone();
        disabled.enabled = false;
        assert!(!should_use_portal_hub(&disabled, &host));
    }

    #[test]
    fn portal_hub_routing_supports_any_ssh_auth_method() {
        let settings = configured_proxy_settings();

        assert!(should_use_portal_hub(
            &settings,
            &proxy_test_host(AuthMethod::PublicKey {
                key_path: None,
                vault_key_id: Some(Uuid::new_v4()),
            })
        ));
        assert!(should_use_portal_hub(
            &settings,
            &proxy_test_host(AuthMethod::Agent)
        ));
        assert!(should_use_portal_hub(
            &settings,
            &proxy_test_host(AuthMethod::PublicKey {
                key_path: Some("/tmp/key".into()),
                vault_key_id: None,
            })
        ));
        assert!(should_use_portal_hub(
            &settings,
            &proxy_test_host(AuthMethod::Password)
        ));
    }
}
