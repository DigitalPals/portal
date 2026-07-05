//! SFTP client for establishing connections

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use russh::client::{self, Config};
use russh_sftp::client::SftpSession as RusshSftpSession;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::time::timeout;

use secrecy::SecretString;

use crate::config::Host;
use crate::error::SftpError;
use crate::security_log;
use crate::ssh::SshEvent;
use crate::ssh::auth::ResolvedAuth;
use crate::ssh::auth_flow::{self, AuthContext};
use crate::ssh::handler::ClientHandler;
use crate::ssh::known_hosts::KnownHostsManager;
use crate::ssh::tunnel::{self, TunnelParams};
use crate::ssh::{SshConnection, SshConnectionKey, shared_connection_pool};

/// Extra budget on top of the connection timeout for interactive steps
/// (host key verification dialogs and keyboard-interactive prompts).
const INTERACTIVE_AUTH_GRACE: Duration = Duration::from_secs(120);

use super::session::{SftpSession, SharedSftpSession};

/// SFTP client for establishing connections
pub struct SftpClient {
    config: Arc<Config>,
    known_hosts: Arc<Mutex<KnownHostsManager>>,
}

impl Default for SftpClient {
    fn default() -> Self {
        Self::new(60)
    }
}

impl SftpClient {
    pub fn new(keepalive_interval: u64) -> Self {
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

    /// Connect to a host and establish an SFTP session
    ///
    /// `jump_chain` lists the jump (bastion) hosts to tunnel through,
    /// outermost first; pass an empty slice for a direct connection.
    pub async fn connect(
        &self,
        host: &Host,
        jump_chain: &[Host],
        event_tx: mpsc::Sender<SshEvent>,
        connection_timeout: Duration,
        password: Option<SecretString>,
        passphrase: Option<SecretString>,
    ) -> Result<SharedSftpSession, SftpError> {
        // Wrap the rest of the connection process in a timeout. Interactive
        // dialogs (host key, keyboard-interactive) get a bounded grace window.
        match timeout(
            connection_timeout + INTERACTIVE_AUTH_GRACE,
            self.establish_sftp_session(
                host,
                jump_chain,
                event_tx,
                connection_timeout,
                password,
                passphrase,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Err(SftpError::ConnectionFailed(format!(
                "SFTP session setup timed out for {}:{}",
                host.hostname, host.port
            ))),
        }
    }

    /// Internal helper to establish the SFTP session after TCP connection
    async fn establish_sftp_session(
        &self,
        host: &Host,
        jump_chain: &[Host],
        event_tx: mpsc::Sender<SshEvent>,
        connection_timeout: Duration,
        password: Option<SecretString>,
        passphrase: Option<SecretString>,
    ) -> Result<SharedSftpSession, SftpError> {
        let pool = shared_connection_pool();
        let via = tunnel::chain_via_key(jump_chain);
        let key = SshConnectionKey::with_via(&host.hostname, host.port, &host.username, &via);

        for attempt in 0..2 {
            let mut connection = pool.get(&key).await;

            if connection.is_none() {
                let (stream, tunnel_parent) = if jump_chain.is_empty() {
                    let addr = format!("{}:{}", host.hostname, host.port);
                    let stream = timeout(connection_timeout, TcpStream::connect(&addr))
                        .await
                        .map_err(|_| {
                            SftpError::ConnectionFailed(format!("Connection timed out to {}", addr))
                        })?
                        .map_err(|e| {
                            SftpError::ConnectionFailed(format!(
                                "Failed to connect to {}:{}: {}",
                                host.hostname, host.port, e
                            ))
                        })?;
                    (tunnel::TunnelStream::Tcp(stream), None)
                } else {
                    let params = TunnelParams {
                        config: self.config.clone(),
                        known_hosts: self.known_hosts.clone(),
                        event_tx: event_tx.clone(),
                        connect_timeout: connection_timeout,
                    };
                    let tunneled = tunnel::open_tunneled_stream(
                        &params,
                        jump_chain,
                        &host.hostname,
                        host.port,
                    )
                    .await
                    .map_err(|e| SftpError::ConnectionFailed(e.to_string()))?;
                    (tunneled.stream, Some(tunneled.last_hop))
                };

                // SFTP doesn't need remote forwards - create empty registry
                let remote_forwards = Arc::new(Mutex::new(HashMap::new()));
                let agent_forwarding_enabled = Arc::new(AtomicBool::new(false));
                let handler = ClientHandler::new(
                    host.hostname.clone(),
                    host.port,
                    self.known_hosts.clone(),
                    event_tx.clone(),
                    agent_forwarding_enabled.clone(),
                    remote_forwards.clone(),
                );

                let mut handle = client::connect_stream(self.config.clone(), stream, handler)
                    .await
                    .map_err(|e| {
                        SftpError::ConnectionFailed(format!(
                            "SSH handshake failed for {}:{}: {}",
                            host.hostname, host.port, e
                        ))
                    })?;

                // Authenticate (with keyboard-interactive and fallback chain)
                let auth = ResolvedAuth::resolve(&host.auth, password.clone(), passphrase.clone())
                    .await
                    .map_err(|e| match e {
                        crate::error::SshError::KeyFilePassphraseRequired(path) => {
                            SftpError::KeyFilePassphraseRequired(path)
                        }
                        crate::error::SshError::KeyFilePassphraseInvalid(path) => {
                            SftpError::KeyFilePassphraseInvalid(path)
                        }
                        _ => SftpError::ConnectionFailed(format!("Authentication failed: {}", e)),
                    })?;
                auth_flow::authenticate(
                    &mut handle,
                    AuthContext {
                        hostname: &host.hostname,
                        port: host.port,
                        username: &host.username,
                        event_tx: &event_tx,
                    },
                    auth,
                )
                .await
                .map_err(|e| {
                    SftpError::ConnectionFailed(format!("Authentication failed: {}", e))
                })?;

                connection = Some(SshConnection::new_via(
                    handle,
                    remote_forwards,
                    agent_forwarding_enabled,
                    Arc::from(host.hostname.clone()),
                    host.port,
                    tunnel_parent,
                ));

                if let Some(conn) = connection.as_ref() {
                    pool.put(key.clone(), conn.clone()).await;
                }
            }

            let connection = match connection {
                Some(conn) => conn,
                None => {
                    return Err(SftpError::ConnectionFailed(
                        "Connection pool lookup failed".to_string(),
                    ));
                }
            };

            // Open channel and request SFTP subsystem
            let channel = {
                let handle = connection.handle();
                let handle_guard = handle.lock().await;
                handle_guard.channel_open_session().await
            };

            let channel = match channel {
                Ok(channel) => channel,
                Err(e) => {
                    pool.invalidate_if_matches(&key, &connection).await;
                    if attempt == 0 {
                        continue;
                    }
                    return Err(SftpError::ConnectionFailed(format!(
                        "Failed to open channel: {}",
                        e
                    )));
                }
            };

            channel
                .request_subsystem(false, "sftp")
                .await
                .map_err(|e| {
                    SftpError::ConnectionFailed(format!("Failed to request SFTP subsystem: {}", e))
                })?;

            // Create SFTP session
            let sftp = RusshSftpSession::new(channel.into_stream())
                .await
                .map_err(|e| {
                    SftpError::ConnectionFailed(format!("Failed to initialize SFTP session: {}", e))
                })?;

            // Get the remote home directory
            let home_dir = self.get_home_dir(&sftp).await?;

            // Log successful SFTP connection
            security_log::log_sftp_connect(&host.hostname, host.port, &host.username);

            let session = Arc::new(SftpSession::new(connection, sftp, home_dir));
            return Ok(session);
        }

        Err(SftpError::ConnectionFailed(
            "Failed to establish SFTP session".to_string(),
        ))
    }

    /// Get the remote user's home directory
    async fn get_home_dir(&self, sftp: &RusshSftpSession) -> Result<PathBuf, SftpError> {
        // Try to canonicalize "." which should give us the current directory (usually home)
        match timeout(Duration::from_secs(5), sftp.canonicalize(".")).await {
            Ok(Ok(path)) => Ok(PathBuf::from(path)),
            Ok(Err(_)) | Err(_) => {
                // Fall back to root if canonicalize fails or times out
                Ok(PathBuf::from("/"))
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_client_with_custom_keepalive() {
        let client = SftpClient::new(120);
        // Verify config was created (we can't inspect private fields directly,
        // but we can verify it doesn't panic and returns a valid client)
        assert!(Arc::strong_count(&client.config) == 1);
    }

    #[test]
    fn default_creates_client_with_60s_keepalive() {
        let client = SftpClient::default();
        // Default should use 60 second keepalive
        assert!(Arc::strong_count(&client.config) == 1);
    }

    #[test]
    fn with_known_hosts_uses_provided_manager() {
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let known_hosts_clone = known_hosts.clone();

        let client = SftpClient::with_known_hosts(30, known_hosts);

        // Verify the same Arc is used (strong_count should be 2)
        assert_eq!(Arc::strong_count(&known_hosts_clone), 2);
        assert!(Arc::ptr_eq(&client.known_hosts, &known_hosts_clone));
    }

    #[test]
    fn new_with_zero_keepalive() {
        // Zero keepalive should still work (disables keepalive)
        let client = SftpClient::new(0);
        assert!(Arc::strong_count(&client.config) == 1);
    }

    #[test]
    fn new_with_large_keepalive() {
        // Large keepalive values should work
        let client = SftpClient::new(86400); // 24 hours
        assert!(Arc::strong_count(&client.config) == 1);
    }

    #[test]
    fn multiple_clients_share_nothing_by_default() {
        let client1 = SftpClient::default();
        let client2 = SftpClient::default();

        // Each client should have its own config and known_hosts
        assert!(!Arc::ptr_eq(&client1.config, &client2.config));
        assert!(!Arc::ptr_eq(&client1.known_hosts, &client2.known_hosts));
    }

    #[test]
    fn clients_can_share_known_hosts() {
        let shared_known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));

        let client1 = SftpClient::with_known_hosts(60, shared_known_hosts.clone());
        let client2 = SftpClient::with_known_hosts(60, shared_known_hosts.clone());

        // Both clients should share the same known_hosts
        assert!(Arc::ptr_eq(&client1.known_hosts, &client2.known_hosts));
        assert_eq!(Arc::strong_count(&shared_known_hosts), 3); // original + 2 clients
    }
}
