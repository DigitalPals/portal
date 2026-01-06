use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Config};
use russh::keys::HashAlg;
use tokio::net::TcpStream;
use tokio::sync::{mpsc, Mutex};
use tokio::time::timeout;

use crate::config::Host;
use crate::error::SshError;

use crate::config::DetectedOs;

use super::auth::ResolvedAuth;
use super::handler::ClientHandler;
use super::known_hosts::KnownHostsManager;
use super::os_detect;
use super::session::SshSession;
use super::SshEvent;

/// SSH client for establishing connections
pub struct SshClient {
    config: Arc<Config>,
    known_hosts: Arc<Mutex<KnownHostsManager>>,
}

impl SshClient {
    pub fn new(_connection_timeout: u64, keepalive_interval: u64) -> Self {
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

    /// Connect to a host and establish an interactive PTY session
    /// Returns the session and optionally the detected OS
    pub async fn connect(
        &self,
        host: &Host,
        terminal_size: (u16, u16),
        event_tx: mpsc::Sender<SshEvent>,
        connection_timeout: Duration,
        password: Option<&str>,
        detect_os_on_connect: bool,
    ) -> Result<(Arc<SshSession>, Option<DetectedOs>), SshError> {
        let addr = format!("{}:{}", host.hostname, host.port);

        // Connect with timeout
        let stream = timeout(connection_timeout, TcpStream::connect(&addr))
            .await
            .map_err(|_| SshError::Timeout(addr.clone()))?
            .map_err(|e| SshError::ConnectionFailed {
                host: host.hostname.clone(),
                port: host.port,
                reason: e.to_string(),
            })?;

        let handler = ClientHandler::new(
            host.hostname.clone(),
            host.port,
            self.known_hosts.clone(),
            event_tx.clone(),
        );

        let mut handle = client::connect_stream(self.config.clone(), stream, handler)
            .await
            .map_err(|e| SshError::ConnectionFailed {
                host: host.hostname.clone(),
                port: host.port,
                reason: e.to_string(),
            })?;

        // Authenticate
        let auth = ResolvedAuth::resolve(&host.auth, password).await?;
        self.authenticate(&mut handle, &host.username, auth).await?;

        // Detect OS if requested (before opening the shell channel)
        let detected_os = if detect_os_on_connect {
            match os_detect::detect_os(&mut handle).await {
                Ok(os) => Some(os),
                Err(e) => {
                    tracing::warn!("OS detection failed: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Open channel and request PTY
        let channel = handle
            .channel_open_session()
            .await
            .map_err(|e| SshError::Channel(e.to_string()))?;

        // Request PTY
        channel
            .request_pty(
                false,
                "xterm-256color",
                terminal_size.0 as u32,
                terminal_size.1 as u32,
                0,
                0,
                &[],
            )
            .await
            .map_err(|e| SshError::Channel(format!("PTY request failed: {}", e)))?;

        // Request shell
        channel
            .request_shell(false)
            .await
            .map_err(|e| SshError::Channel(format!("Shell request failed: {}", e)))?;

        let _ = event_tx.send(SshEvent::Connected).await;

        // Session spawns its own reader task in new()
        let session = Arc::new(SshSession::new(handle, channel, event_tx));

        Ok((session, detected_os))
    }

    async fn authenticate(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
        auth: ResolvedAuth,
    ) -> Result<(), SshError> {
        let auth_result = match auth {
            ResolvedAuth::Password(password) => handle
                .authenticate_password(username, &password)
                .await
                .map_err(|e| SshError::AuthenticationFailed(e.to_string()))?,

            ResolvedAuth::PublicKey(key) => handle
                .authenticate_publickey(username, key)
                .await
                .map_err(|e| SshError::AuthenticationFailed(e.to_string()))?,

            ResolvedAuth::Agent => {
                // Try to use SSH agent
                match self.authenticate_with_agent(handle, username).await {
                    Ok(result) if result.success() => return Ok(()),
                    Ok(_) => {
                        return Err(SshError::Agent(
                            "Agent authentication failed - no suitable key found".to_string(),
                        ));
                    }
                    Err(e) => return Err(e),
                }
            }
        };

        if !auth_result.success() {
            return Err(SshError::AuthenticationFailed(
                "Authentication rejected by server".to_string(),
            ));
        }

        Ok(())
    }

    async fn authenticate_with_agent(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
    ) -> Result<russh::client::AuthResult, SshError> {
        // Try to connect to SSH agent
        let agent_path = std::env::var("SSH_AUTH_SOCK").map_err(|_| {
            SshError::Agent("SSH_AUTH_SOCK not set - is ssh-agent running?".to_string())
        })?;

        let stream = tokio::net::UnixStream::connect(&agent_path)
            .await
            .map_err(|e| SshError::Agent(format!("Failed to connect to SSH agent: {}", e)))?;

        let mut agent = russh::keys::agent::client::AgentClient::connect(stream);

        // Get identities from agent
        let identities = agent
            .request_identities()
            .await
            .map_err(|e| SshError::Agent(format!("Failed to get identities: {}", e)))?;

        if identities.is_empty() {
            return Err(SshError::Agent(
                "No identities found in SSH agent".to_string(),
            ));
        }

        // Try each identity with SHA-512 for RSA keys
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

        Err(SshError::Agent(
            "No agent key accepted by server".to_string(),
        ))
    }
}

impl Default for SshClient {
    fn default() -> Self {
        Self::new(30, 60)
    }
}
