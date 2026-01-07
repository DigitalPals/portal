//! SFTP client for establishing connections

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Config};
use russh::keys::HashAlg;
use russh_sftp::client::SftpSession as RusshSftpSession;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::mpsc;
use tokio::time::timeout;

use secrecy::{ExposeSecret, SecretString};

use crate::config::Host;
use crate::error::SftpError;
use crate::security_log;
use crate::ssh::SshEvent;
use crate::ssh::auth::ResolvedAuth;
use crate::ssh::handler::ClientHandler;
use crate::ssh::known_hosts::KnownHostsManager;

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
        let config = Config {
            inactivity_timeout: Some(Duration::from_secs(3600)),
            keepalive_interval: Some(Duration::from_secs(keepalive_interval)),
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
        let config = Config {
            inactivity_timeout: Some(Duration::from_secs(3600)),
            keepalive_interval: Some(Duration::from_secs(keepalive_interval)),
            keepalive_max: 3,
            ..Default::default()
        };

        Self {
            config: Arc::new(config),
            known_hosts,
        }
    }

    /// Connect to a host and establish an SFTP session
    pub async fn connect(
        &self,
        host: &Host,
        event_tx: mpsc::Sender<SshEvent>,
        connection_timeout: Duration,
        password: Option<SecretString>,
    ) -> Result<SharedSftpSession, SftpError> {
        let addr = format!("{}:{}", host.hostname, host.port);

        // Connect with timeout
        let stream = timeout(connection_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| SftpError::ConnectionFailed(format!("Connection timed out to {}", addr)))?
            .map_err(|e| {
                SftpError::ConnectionFailed(format!(
                    "Failed to connect to {}:{}: {}",
                    host.hostname, host.port, e
                ))
            })?;

        // Wrap the rest of the connection process in a timeout
        match timeout(
            connection_timeout,
            self.establish_sftp_session(host, event_tx, stream, password),
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
        event_tx: mpsc::Sender<SshEvent>,
        stream: TcpStream,
        password: Option<SecretString>,
    ) -> Result<SharedSftpSession, SftpError> {
        let handler = ClientHandler::new(
            host.hostname.clone(),
            host.port,
            self.known_hosts.clone(),
            event_tx,
        );

        let mut handle = client::connect_stream(self.config.clone(), stream, handler)
            .await
            .map_err(|e| {
                SftpError::ConnectionFailed(format!(
                    "SSH handshake failed for {}:{}: {}",
                    host.hostname, host.port, e
                ))
            })?;

        // Authenticate
        let auth = ResolvedAuth::resolve(&host.auth, password)
            .await
            .map_err(|e| SftpError::ConnectionFailed(format!("Authentication failed: {}", e)))?;
        self.authenticate(&mut handle, &host.username, auth, &host.hostname, host.port)
            .await?;

        // Open channel and request SFTP subsystem
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| SftpError::ConnectionFailed(format!("Failed to open channel: {}", e)))?;

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

        let session = Arc::new(SftpSession::new(sftp, home_dir));
        Ok(session)
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

    async fn authenticate(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
        auth: ResolvedAuth,
        hostname: &str,
        port: u16,
    ) -> Result<(), SftpError> {
        // Determine auth method name for logging
        let method_name = match &auth {
            ResolvedAuth::Password(_) => "password",
            ResolvedAuth::PublicKey(_) => "publickey",
            ResolvedAuth::Agent => "agent",
        };

        // Log authentication attempt
        security_log::log_auth_attempt(hostname, port, username, method_name);

        let auth_result = match auth {
            ResolvedAuth::Password(password) => {
                // Use expose_secret() only at the point of authentication
                match handle
                    .authenticate_password(username, password.expose_secret())
                    .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        let reason = format!("Password auth failed: {}", e);
                        security_log::log_auth_failure(
                            hostname,
                            port,
                            username,
                            method_name,
                            &reason,
                        );
                        return Err(SftpError::ConnectionFailed(reason));
                    }
                }
            }

            ResolvedAuth::PublicKey(key) => {
                match handle.authenticate_publickey(username, key).await {
                    Ok(result) => result,
                    Err(e) => {
                        let reason = format!("Public key auth failed: {}", e);
                        security_log::log_auth_failure(
                            hostname,
                            port,
                            username,
                            method_name,
                            &reason,
                        );
                        return Err(SftpError::ConnectionFailed(reason));
                    }
                }
            }

            ResolvedAuth::Agent => match self.authenticate_with_agent(handle, username).await {
                Ok(result) if result.success() => {
                    security_log::log_auth_success(hostname, port, username, method_name);
                    return Ok(());
                }
                Ok(_) => {
                    let reason = "Agent authentication failed - no suitable key found";
                    security_log::log_auth_failure(hostname, port, username, method_name, reason);
                    return Err(SftpError::ConnectionFailed(reason.to_string()));
                }
                Err(e) => {
                    security_log::log_auth_failure(
                        hostname,
                        port,
                        username,
                        method_name,
                        &e.to_string(),
                    );
                    return Err(e);
                }
            },
        };

        if !auth_result.success() {
            let reason = "Authentication rejected by server";
            security_log::log_auth_failure(hostname, port, username, method_name, reason);
            return Err(SftpError::ConnectionFailed(reason.to_string()));
        }

        security_log::log_auth_success(hostname, port, username, method_name);
        Ok(())
    }

    async fn authenticate_with_agent(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
    ) -> Result<russh::client::AuthResult, SftpError> {
        let agent_path = std::env::var("SSH_AUTH_SOCK").map_err(|_| {
            SftpError::ConnectionFailed("SSH_AUTH_SOCK not set - is ssh-agent running?".to_string())
        })?;

        let stream = tokio::net::UnixStream::connect(&agent_path)
            .await
            .map_err(|e| {
                SftpError::ConnectionFailed(format!("Failed to connect to SSH agent: {}", e))
            })?;

        let mut agent = russh::keys::agent::client::AgentClient::connect(stream);

        let identities = agent.request_identities().await.map_err(|e| {
            SftpError::ConnectionFailed(format!("Failed to get identities from agent: {}", e))
        })?;

        if identities.is_empty() {
            return Err(SftpError::ConnectionFailed(
                "No identities found in SSH agent".to_string(),
            ));
        }

        for identity in identities {
            let hash_alg = if identity.algorithm().is_rsa() {
                Some(HashAlg::Sha512)
            } else {
                None
            };

            match handle
                .authenticate_publickey_with(username, identity, hash_alg, &mut agent)
                .await
            {
                Ok(result) if result.success() => return Ok(result),
                Ok(_) => continue,
                Err(e) => {
                    tracing::debug!("Agent key failed: {}", e);
                    continue;
                }
            }
        }

        Err(SftpError::ConnectionFailed(
            "No agent key accepted by server".to_string(),
        ))
    }
}
