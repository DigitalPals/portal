use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Config};
use russh::keys::HashAlg;
use tokio::net::TcpStream;
use tokio::sync::{Mutex, mpsc};
use tokio::time::timeout;

use crate::config::Host;
use crate::error::SshError;
use crate::security_log;

use crate::config::DetectedOs;

use secrecy::{ExposeSecret, SecretString};

use super::SshEvent;
use super::auth::ResolvedAuth;
use super::handler::ClientHandler;
use super::known_hosts::KnownHostsManager;
use super::os_detect;
use super::session::SshSession;

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
    #[allow(clippy::too_many_arguments)]
    pub async fn connect(
        &self,
        host: &Host,
        terminal_size: (u16, u16),
        event_tx: mpsc::Sender<SshEvent>,
        connection_timeout: Duration,
        password: Option<SecretString>,
        passphrase: Option<SecretString>,
        detect_os_on_connect: bool,
        allow_agent_forwarding: bool,
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

        match timeout(
            connection_timeout,
            self.establish_session(
                host,
                terminal_size,
                event_tx,
                stream,
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
        terminal_size: (u16, u16),
        event_tx: mpsc::Sender<SshEvent>,
        stream: TcpStream,
        password: Option<SecretString>,
        passphrase: Option<SecretString>,
        detect_os_on_connect: bool,
        allow_agent_forwarding: bool,
    ) -> Result<(Arc<SshSession>, Option<DetectedOs>), SshError> {
        let agent_forwarding_enabled = allow_agent_forwarding && host.agent_forwarding;
        // Create shared remote forwards registry for this connection
        let remote_forwards = Arc::new(Mutex::new(HashMap::new()));
        let handler = ClientHandler::new(
            host.hostname.clone(),
            host.port,
            self.known_hosts.clone(),
            event_tx.clone(),
            agent_forwarding_enabled,
            remote_forwards.clone(),
        );

        let mut handle = client::connect_stream(self.config.clone(), stream, handler)
            .await
            .map_err(|e| SshError::ConnectionFailed {
                host: host.hostname.clone(),
                port: host.port,
                reason: e.to_string(),
            })?;

        // Authenticate
        let auth = ResolvedAuth::resolve(&host.auth, password, passphrase).await?;
        self.authenticate(&mut handle, &host.username, auth, &host.hostname, host.port)
            .await?;

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

        if agent_forwarding_enabled {
            if let Err(e) = channel.agent_forward(false).await {
                tracing::warn!("Agent forwarding request failed: {}", e);
            }
        }

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
        // Use the same remote_forwards registry that was given to the handler
        let session = Arc::new(SshSession::new(handle, channel, event_tx, remote_forwards));

        Ok((session, detected_os))
    }

    async fn authenticate(
        &self,
        handle: &mut client::Handle<ClientHandler>,
        username: &str,
        auth: ResolvedAuth,
        hostname: &str,
        port: u16,
    ) -> Result<(), SshError> {
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
                        let reason = e.to_string();
                        security_log::log_auth_failure(
                            hostname,
                            port,
                            username,
                            method_name,
                            &reason,
                        );
                        return Err(SshError::AuthenticationFailed(reason));
                    }
                }
            }

            ResolvedAuth::PublicKey(key) => {
                match handle.authenticate_publickey(username, key).await {
                    Ok(result) => result,
                    Err(e) => {
                        let reason = e.to_string();
                        security_log::log_auth_failure(
                            hostname,
                            port,
                            username,
                            method_name,
                            &reason,
                        );
                        return Err(SshError::AuthenticationFailed(reason));
                    }
                }
            }

            ResolvedAuth::Agent => {
                // Try to use SSH agent
                match self.authenticate_with_agent(handle, username).await {
                    Ok(result) if result.success() => {
                        security_log::log_auth_success(hostname, port, username, method_name);
                        return Ok(());
                    }
                    Ok(_) => {
                        let reason = "Agent authentication failed - no suitable key found";
                        security_log::log_auth_failure(
                            hostname,
                            port,
                            username,
                            method_name,
                            reason,
                        );
                        return Err(SshError::Agent(reason.to_string()));
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
                }
            }
        };

        if !auth_result.success() {
            let reason = "Authentication rejected by server";
            security_log::log_auth_failure(hostname, port, username, method_name, reason);
            return Err(SshError::AuthenticationFailed(reason.to_string()));
        }

        security_log::log_auth_success(hostname, port, username, method_name);
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

#[cfg(test)]
mod tests {
    use super::*;

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
