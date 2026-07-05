use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use russh::Pty;
use russh::client::{self, Config};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};
use tokio::time::{sleep, timeout};

use crate::config::Host;
use crate::error::SshError;
use crate::security_log;

use crate::config::DetectedOs;

use secrecy::SecretString;

use super::SshEvent;
use super::auth::ResolvedAuth;
use super::auth_flow::{self, AuthContext};
use super::connection_pool::{SshConnection, SshConnectionKey};
use super::handler::ClientHandler;
use super::known_hosts::KnownHostsManager;
use super::os_detect;
use super::session::SshSession;
use super::shared_connection_pool;
use super::tunnel::{self, TunnelParams};

const SSH_TERMINAL_TYPE: &str = "xterm-256color";
const NEW_CONNECTION_TRANSPORT_ATTEMPTS: usize = 3;
const NEW_CONNECTION_TRANSPORT_RETRY_DELAY: Duration = Duration::from_millis(250);
/// Extra budget on top of the connection timeout for interactive steps
/// (host key verification dialogs and keyboard-interactive prompts); each
/// individual dialog wait is itself bounded at 60 seconds.
const INTERACTIVE_AUTH_GRACE: Duration = Duration::from_secs(120);

fn default_pty_modes() -> &'static [(Pty, u32)] {
    &[
        (Pty::VINTR, 3),
        (Pty::VQUIT, 28),
        (Pty::VERASE, 127),
        (Pty::VKILL, 21),
        (Pty::VEOF, 4),
        (Pty::VEOL, 0),
        (Pty::VEOL2, 0),
        (Pty::VSTART, 17),
        (Pty::VSTOP, 19),
        (Pty::VSUSP, 26),
        (Pty::VREPRINT, 18),
        (Pty::VWERASE, 23),
        (Pty::VLNEXT, 22),
        (Pty::VDISCARD, 15),
        (Pty::IGNPAR, 0),
        (Pty::PARMRK, 0),
        (Pty::INPCK, 0),
        (Pty::ISTRIP, 0),
        (Pty::INLCR, 0),
        (Pty::IGNCR, 0),
        (Pty::ICRNL, 1),
        (Pty::IUCLC, 0),
        (Pty::IXON, 1),
        (Pty::IXANY, 0),
        (Pty::IXOFF, 0),
        (Pty::IMAXBEL, 1),
        (Pty::IUTF8, 1),
        (Pty::ISIG, 1),
        (Pty::ICANON, 1),
        (Pty::XCASE, 0),
        (Pty::ECHO, 1),
        (Pty::ECHOE, 1),
        (Pty::ECHOK, 1),
        (Pty::ECHONL, 0),
        (Pty::NOFLSH, 0),
        (Pty::TOSTOP, 0),
        (Pty::IEXTEN, 1),
        (Pty::ECHOCTL, 1),
        (Pty::ECHOKE, 1),
        (Pty::PENDIN, 0),
        (Pty::OPOST, 1),
        (Pty::OLCUC, 0),
        (Pty::ONLCR, 1),
        (Pty::OCRNL, 0),
        (Pty::ONOCR, 0),
        (Pty::ONLRET, 0),
        (Pty::CS7, 0),
        (Pty::CS8, 1),
        (Pty::PARENB, 0),
        (Pty::PARODD, 0),
        (Pty::TTY_OP_ISPEED, 38400),
        (Pty::TTY_OP_OSPEED, 38400),
    ]
}

fn is_transient_transport_error(reason: &str) -> bool {
    let lower = reason.to_ascii_lowercase();
    [
        "connection reset",
        "reset by peer",
        "connection aborted",
        "connection refused",
        "broken pipe",
        "unexpected eof",
        "early eof",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// SSH client for establishing connections
pub struct SshClient {
    config: Arc<Config>,
    known_hosts: Arc<Mutex<KnownHostsManager>>,
}

impl SshClient {
    pub fn new(_connection_timeout: u64, keepalive_interval: u64) -> Self {
        // Treat 0 as "no keepalive" to avoid immediate timeout
        let keepalive = if keepalive_interval == 0 {
            None
        } else {
            Some(Duration::from_secs(keepalive_interval))
        };

        let config = Config {
            inactivity_timeout: Some(Duration::from_secs(3600)),
            keepalive_interval: keepalive,
            keepalive_max: 3,
            ..Default::default()
        };

        Self {
            config: Arc::new(config),
            known_hosts: Arc::new(Mutex::new(KnownHostsManager::new())),
        }
    }

    pub fn with_known_hosts(
        keepalive_interval: u64,
        known_hosts: Arc<Mutex<KnownHostsManager>>,
    ) -> Self {
        // Treat 0 as "no keepalive" to avoid immediate timeout
        let keepalive = if keepalive_interval == 0 {
            None
        } else {
            Some(Duration::from_secs(keepalive_interval))
        };

        let config = Config {
            inactivity_timeout: Some(Duration::from_secs(3600)),
            keepalive_interval: keepalive,
            keepalive_max: 3,
            ..Default::default()
        };

        Self {
            config: Arc::new(config),
            known_hosts,
        }
    }

    /// Connect to a host and establish an interactive PTY session
    /// Returns the session and optionally the detected OS
    ///
    /// `jump_chain` lists the jump (bastion) hosts to tunnel through,
    /// outermost first; pass an empty slice for a direct connection.
    #[allow(clippy::too_many_arguments)]
    pub async fn connect(
        &self,
        host: &Host,
        jump_chain: &[Host],
        terminal_size: (u16, u16),
        event_tx: mpsc::Sender<SshEvent>,
        connection_timeout: Duration,
        password: Option<SecretString>,
        passphrase: Option<SecretString>,
        detect_os_on_connect: bool,
        allow_agent_forwarding: bool,
    ) -> Result<(Arc<SshSession>, Option<DetectedOs>), SshError> {
        let addr = format!("{}:{}", host.hostname, host.port);

        // The overall budget includes a grace window for interactive dialogs
        // (host key verification, keyboard-interactive prompts) which are
        // individually bounded but may exceed the plain connection timeout.
        match timeout(
            connection_timeout + INTERACTIVE_AUTH_GRACE,
            self.establish_session(
                host,
                jump_chain,
                terminal_size,
                event_tx,
                connection_timeout,
                password,
                passphrase,
                detect_os_on_connect,
                allow_agent_forwarding,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(SshError::Timeout(addr)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn establish_session(
        &self,
        host: &Host,
        jump_chain: &[Host],
        terminal_size: (u16, u16),
        event_tx: mpsc::Sender<SshEvent>,
        connection_timeout: Duration,
        password: Option<SecretString>,
        passphrase: Option<SecretString>,
        detect_os_on_connect: bool,
        allow_agent_forwarding: bool,
    ) -> Result<(Arc<SshSession>, Option<DetectedOs>), SshError> {
        let pool = shared_connection_pool();
        let via = tunnel::chain_via_key(jump_chain);
        let key = SshConnectionKey::with_via(&host.hostname, host.port, &host.username, &via);
        let agent_forwarding_enabled = allow_agent_forwarding && host.agent_forwarding;

        // At most 1 reconnect attempt if we raced a stale pooled connection.
        for attempt in 0..2 {
            // Try to reuse an existing authenticated connection.
            let mut connection = pool.get(&key).await;
            let mut created_new_connection = false;

            if connection.is_none() {
                created_new_connection = true;

                connection = Some(
                    self.open_connection(
                        host,
                        jump_chain,
                        &event_tx,
                        connection_timeout,
                        password.clone(),
                        passphrase.clone(),
                    )
                    .await?,
                );

                if let Some(conn) = connection.as_ref() {
                    pool.put(key.clone(), conn.clone()).await;
                }
            }

            let connection = match connection {
                Some(conn) => conn,
                None => {
                    return Err(SshError::ConnectionFailed {
                        host: host.hostname.clone(),
                        port: host.port,
                        reason: "Connection pool lookup failed".to_string(),
                    });
                }
            };

            // Open channel and request PTY
            let channel = {
                let handle = connection.handle();
                let handle_guard = handle.lock().await;
                handle_guard
                    .channel_open_session()
                    .await
                    .map_err(|e| SshError::Channel(e.to_string()))
            };

            let channel = match channel {
                Ok(channel) => channel,
                Err(e) => {
                    // Connection may be stale; invalidate and retry once.
                    pool.invalidate_if_matches(&key, &connection).await;
                    if attempt == 0 && !created_new_connection {
                        continue;
                    }
                    return Err(e);
                }
            };

            if agent_forwarding_enabled {
                connection.enable_agent_forwarding();
                if let Err(e) = channel.agent_forward(false).await {
                    tracing::warn!("Agent forwarding request failed: {}", e);
                } else {
                    crate::security_log::log_agent_forwarding_enabled(
                        &host.hostname,
                        host.port,
                        &host.username,
                    );
                }
            }

            // Signal truecolor and terminal identity to the remote shell.
            // Servers only honour these if AcceptEnv includes them (sshd_config).
            // The OSC color-query response path is the reliable terminal-color
            // signal for applications when these SSH env requests are rejected.
            // Failures are non-fatal — we warn and continue.
            for (name, value) in [
                ("COLORTERM", "truecolor"),
                ("TERM_PROGRAM", "Portal"),
                ("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION")),
                ("PORTAL_TERMINAL", "1"),
                ("PORTAL_TERM_PROGRAM", "Portal"),
                ("PORTAL_TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION")),
            ] {
                if let Err(e) = channel.set_env(false, name, value).await {
                    tracing::warn!("Failed to set {name}: {e}");
                }
            }

            // Request PTY
            if let Err(e) = channel
                .request_pty(
                    false,
                    SSH_TERMINAL_TYPE,
                    terminal_size.0 as u32,
                    terminal_size.1 as u32,
                    0,
                    0,
                    default_pty_modes(),
                )
                .await
            {
                pool.invalidate_if_matches(&key, &connection).await;
                if attempt == 0 && !created_new_connection {
                    continue;
                }
                return Err(SshError::Channel(format!("PTY request failed: {}", e)));
            }

            // Request shell
            if let Err(e) = channel.request_shell(false).await {
                pool.invalidate_if_matches(&key, &connection).await;
                if attempt == 0 && !created_new_connection {
                    continue;
                }
                return Err(SshError::Channel(format!("Shell request failed: {}", e)));
            }

            // Run host OS detection only after the user-facing shell has been
            // requested. Opening exec channels first can consume login/PAM
            // output like MOTD before the visible interactive PTY exists.
            let detected_os = if detect_os_on_connect {
                let handle = connection.handle();
                let mut handle_guard = handle.lock().await;
                match os_detect::detect_os(&mut handle_guard).await {
                    Ok(os) => Some(os),
                    Err(e) => {
                        tracing::warn!("OS detection failed: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            if created_new_connection {
                security_log::log_ssh_connect(&host.hostname, host.port, &host.username);
            }
            let _ = event_tx.send(SshEvent::Connected).await;

            // Session spawns its own reader task in new()
            let session = Arc::new(SshSession::new(connection, channel, event_tx));

            return Ok((session, detected_os));
        }

        Err(SshError::ConnectionFailed {
            host: host.hostname.clone(),
            port: host.port,
            reason: "Failed to establish session".to_string(),
        })
    }

    /// Open a new authenticated SSH connection to `host`, tunneling through
    /// `jump_chain` when it is non-empty.
    async fn open_connection(
        &self,
        host: &Host,
        jump_chain: &[Host],
        event_tx: &mpsc::Sender<SshEvent>,
        connection_timeout: Duration,
        password: Option<SecretString>,
        passphrase: Option<SecretString>,
    ) -> Result<Arc<SshConnection>, SshError> {
        let addr = format!("{}:{}", host.hostname, host.port);

        let mut last_transport_error = None;
        for transport_attempt in 0..NEW_CONNECTION_TRANSPORT_ATTEMPTS {
            // Transport: direct TCP, or a direct-tcpip channel through the
            // jump chain. Jump-hop failures are not retried here — they carry
            // their own hop-specific error message.
            let (stream, tunnel_parent) = if jump_chain.is_empty() {
                match timeout(connection_timeout, TcpStream::connect(&addr)).await {
                    Ok(Ok(stream)) => (tunnel::TunnelStream::Tcp(stream), None),
                    Ok(Err(error)) => {
                        let reason = error.to_string();
                        if transport_attempt + 1 < NEW_CONNECTION_TRANSPORT_ATTEMPTS
                            && is_transient_transport_error(&reason)
                        {
                            last_transport_error = Some(reason);
                            sleep(NEW_CONNECTION_TRANSPORT_RETRY_DELAY).await;
                            continue;
                        }
                        return Err(SshError::ConnectionFailed {
                            host: host.hostname.clone(),
                            port: host.port,
                            reason,
                        });
                    }
                    Err(_) => return Err(SshError::Timeout(addr.clone())),
                }
            } else {
                let params = TunnelParams {
                    config: self.config.clone(),
                    known_hosts: self.known_hosts.clone(),
                    event_tx: event_tx.clone(),
                    connect_timeout: connection_timeout,
                };
                let tunneled =
                    tunnel::open_tunneled_stream(&params, jump_chain, &host.hostname, host.port)
                        .await?;
                (tunneled.stream, Some(tunneled.last_hop))
            };

            let agent_forwarding_enabled_flag = Arc::new(AtomicBool::new(false));
            // Create shared remote forwards registry for this connection
            let remote_forwards = Arc::new(Mutex::new(HashMap::new()));
            let handler = ClientHandler::new(
                host.hostname.clone(),
                host.port,
                self.known_hosts.clone(),
                event_tx.clone(),
                agent_forwarding_enabled_flag.clone(),
                remote_forwards.clone(),
            );

            let mut handle =
                match client::connect_stream(self.config.clone(), stream, handler).await {
                    Ok(handle) => handle,
                    Err(error) => {
                        let reason = error.to_string();
                        if transport_attempt + 1 < NEW_CONNECTION_TRANSPORT_ATTEMPTS
                            && is_transient_transport_error(&reason)
                        {
                            last_transport_error = Some(reason);
                            sleep(NEW_CONNECTION_TRANSPORT_RETRY_DELAY).await;
                            continue;
                        }
                        return Err(SshError::ConnectionFailed {
                            host: host.hostname.clone(),
                            port: host.port,
                            reason,
                        });
                    }
                };

            // Authenticate with the configured method plus automatic
            // fallback (publickey/agent -> keyboard-interactive -> password).
            let auth =
                ResolvedAuth::resolve(&host.auth, password.clone(), passphrase.clone()).await?;
            auth_flow::authenticate(
                &mut handle,
                AuthContext {
                    hostname: &host.hostname,
                    port: host.port,
                    username: &host.username,
                    event_tx,
                },
                auth,
            )
            .await?;

            return Ok(SshConnection::new_via(
                handle,
                remote_forwards,
                agent_forwarding_enabled_flag,
                Arc::from(host.hostname.clone()),
                host.port,
                tunnel_parent,
            ));
        }

        Err(SshError::ConnectionFailed {
            host: host.hostname.clone(),
            port: host.port,
            reason: last_transport_error
                .unwrap_or_else(|| "Failed to establish SSH transport".to_string()),
        })
    }
}

impl Default for SshClient {
    fn default() -> Self {
        Self::new(30, 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_transport_errors_are_retryable() {
        assert!(is_transient_transport_error("Connection reset by peer"));
        assert!(is_transient_transport_error("broken pipe"));
        assert!(is_transient_transport_error("unexpected EOF"));
        assert!(!is_transient_transport_error("Authentication failed"));
        assert!(!is_transient_transport_error(
            "Host key verification failed"
        ));
    }

    #[test]
    fn new_creates_client_with_custom_settings() {
        let client = SshClient::new(30, 120);
        // Verify config was created with proper Arc
        assert_eq!(Arc::strong_count(&client.config), 1);
        assert_eq!(Arc::strong_count(&client.known_hosts), 1);
    }

    #[test]
    fn default_creates_client_with_30s_timeout_60s_keepalive() {
        let client = SshClient::default();
        assert_eq!(Arc::strong_count(&client.config), 1);
        // Default should work without panicking
    }

    #[test]
    fn with_known_hosts_uses_provided_manager() {
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let known_hosts_clone = known_hosts.clone();

        let client = SshClient::with_known_hosts(60, known_hosts);

        // Verify the same Arc is used
        assert_eq!(Arc::strong_count(&known_hosts_clone), 2);
        assert!(Arc::ptr_eq(&client.known_hosts, &known_hosts_clone));
    }

    #[test]
    fn new_with_zero_keepalive_disables_keepalive() {
        // Zero keepalive should not panic and should disable keepalive
        let client = SshClient::new(30, 0);
        assert_eq!(Arc::strong_count(&client.config), 1);
    }

    #[test]
    fn with_known_hosts_zero_keepalive_disables_keepalive() {
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let client = SshClient::with_known_hosts(0, known_hosts);
        assert_eq!(Arc::strong_count(&client.config), 1);
    }

    #[test]
    fn new_with_large_keepalive() {
        // Large keepalive values should work
        let client = SshClient::new(300, 86400); // 5 min timeout, 24 hour keepalive
        assert_eq!(Arc::strong_count(&client.config), 1);
    }

    #[test]
    fn multiple_clients_have_separate_state() {
        let client1 = SshClient::default();
        let client2 = SshClient::default();

        // Each client should have its own config and known_hosts
        assert!(!Arc::ptr_eq(&client1.config, &client2.config));
        assert!(!Arc::ptr_eq(&client1.known_hosts, &client2.known_hosts));
    }

    #[test]
    fn clients_can_share_known_hosts_manager() {
        let shared_known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));

        let client1 = SshClient::with_known_hosts(60, shared_known_hosts.clone());
        let client2 = SshClient::with_known_hosts(60, shared_known_hosts.clone());

        // Both clients should share the same known_hosts
        assert!(Arc::ptr_eq(&client1.known_hosts, &client2.known_hosts));
        assert_eq!(Arc::strong_count(&shared_known_hosts), 3); // original + 2 clients

        // But configs should be separate
        assert!(!Arc::ptr_eq(&client1.config, &client2.config));
    }

    #[test]
    fn clients_with_different_keepalive_have_different_configs() {
        let shared_known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));

        let client1 = SshClient::with_known_hosts(30, shared_known_hosts.clone());
        let client2 = SshClient::with_known_hosts(120, shared_known_hosts.clone());

        // Configs should be separate even with shared known_hosts
        assert!(!Arc::ptr_eq(&client1.config, &client2.config));
    }

    #[test]
    fn new_with_various_timeout_values() {
        // Test various timeout values don't cause issues
        let _client1 = SshClient::new(0, 60); // zero timeout
        let _client2 = SshClient::new(1, 60); // minimal timeout
        let _client3 = SshClient::new(300, 60); // 5 minute timeout
        let _client4 = SshClient::new(3600, 60); // 1 hour timeout
    }

    #[test]
    fn config_has_expected_inactivity_timeout() {
        let client = SshClient::new(30, 60);
        // Inactivity timeout should be 1 hour (3600 seconds)
        assert_eq!(
            client.config.inactivity_timeout,
            Some(Duration::from_secs(3600))
        );
    }

    #[test]
    fn config_has_expected_keepalive_interval() {
        let client = SshClient::new(30, 60);
        assert_eq!(
            client.config.keepalive_interval,
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn config_has_expected_keepalive_max() {
        let client = SshClient::new(30, 60);
        assert_eq!(client.config.keepalive_max, 3);
    }

    #[test]
    fn zero_keepalive_sets_none_interval() {
        let client = SshClient::new(30, 0);
        assert_eq!(client.config.keepalive_interval, None);
    }

    #[test]
    fn with_known_hosts_zero_keepalive_sets_none_interval() {
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let client = SshClient::with_known_hosts(0, known_hosts);
        assert_eq!(client.config.keepalive_interval, None);
    }

    #[test]
    fn nonzero_keepalive_sets_duration() {
        let client = SshClient::new(30, 45);
        assert_eq!(
            client.config.keepalive_interval,
            Some(Duration::from_secs(45))
        );
    }
}
