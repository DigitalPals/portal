use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use russh::client::{Handler, Session};
use russh::keys::PublicKey;
use russh::{Channel, ChannelId};
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::error::SshError;

use super::SshEvent;
use super::host_key_verification::{
    HostKeyInfo, HostKeyVerificationRequest, HostKeyVerificationResponse,
};
use super::known_hosts::{HostKeyStatus, KnownHostsManager};

/// SSH client handler implementation
pub struct ClientHandler {
    host: Arc<str>,
    port: u16,
    known_hosts: Arc<Mutex<KnownHostsManager>>,
    /// Channel to send events to UI (including verification requests)
    event_tx: mpsc::Sender<SshEvent>,
    agent_forwarding_enabled: bool,
}

impl ClientHandler {
    pub fn new(
        host: String,
        port: u16,
        known_hosts: Arc<Mutex<KnownHostsManager>>,
        event_tx: mpsc::Sender<SshEvent>,
        agent_forwarding_enabled: bool,
    ) -> Self {
        Self {
            host: Arc::from(host),
            port,
            known_hosts,
            event_tx,
            agent_forwarding_enabled,
        }
    }
}

impl Handler for ClientHandler {
    type Error = SshError;

    fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let host = Arc::clone(&self.host);
        let port = self.port;
        let known_hosts = Arc::clone(&self.known_hosts);
        let key = Arc::new(server_public_key.clone());
        let event_tx = self.event_tx.clone();

        async move {
            let status = tokio::task::spawn_blocking({
                let known_hosts = Arc::clone(&known_hosts);
                let host = Arc::clone(&host);
                let key = Arc::clone(&key);
                move || {
                    let manager = known_hosts.blocking_lock();
                    manager.check_host_key(host.as_ref(), port, key.as_ref())
                }
            })
            .await
            .map_err(|e| SshError::HostKeyVerification(format!("Host key check failed: {}", e)))?;

            match status {
                HostKeyStatus::Known => {
                    tracing::debug!("Host key verified");
                    Ok(true)
                }
                HostKeyStatus::Revoked { .. } => {
                    tracing::warn!("Host key revoked");
                    Err(SshError::HostKeyVerification(
                        "Host key has been revoked".to_string(),
                    ))
                }
                HostKeyStatus::Unknown {
                    fingerprint,
                    key_type,
                } => {
                    tracing::debug!("New host key received");

                    // Create oneshot channel for response
                    let (tx, rx) = oneshot::channel();

                    let request = HostKeyVerificationRequest::NewHost {
                        info: HostKeyInfo {
                            host: host.to_string(),
                            port,
                            fingerprint,
                            key_type,
                        },
                        responder: tx,
                    };

                    // Send request to UI via event channel
                    event_tx
                        .send(SshEvent::HostKeyVerification(Box::new(request)))
                        .await
                        .map_err(|_| {
                            SshError::HostKeyVerification(
                                "Failed to request host key verification".to_string(),
                            )
                        })?;

                    // Wait for user response (with timeout)
                    match tokio::time::timeout(Duration::from_secs(60), rx).await {
                        Ok(Ok(HostKeyVerificationResponse::Accept)) => {
                            tracing::debug!("User accepted host key");
                            // Save key to known_hosts (fail closed if we cannot persist)
                            let store_result = tokio::task::spawn_blocking({
                                let known_hosts = Arc::clone(&known_hosts);
                                let host = Arc::clone(&host);
                                let key = Arc::clone(&key);
                                move || {
                                    let mut manager = known_hosts.blocking_lock();
                                    manager.add_host_key(host.as_ref(), port, key.as_ref())
                                }
                            })
                            .await
                            .map_err(|e| {
                                SshError::HostKeyVerification(format!(
                                    "Host key store task failed: {}",
                                    e
                                ))
                            })?;

                            match store_result {
                                Ok(()) => Ok(true),
                                Err(e) => Err(SshError::HostKeyVerification(format!(
                                    "Failed to store host key: {}",
                                    e
                                ))),
                            }
                        }
                        Ok(Ok(HostKeyVerificationResponse::Reject)) | Ok(Err(_)) => {
                            tracing::debug!("User rejected host key");
                            Err(SshError::HostKeyVerification(
                                "Host key rejected by user".to_string(),
                            ))
                        }
                        Err(_) => {
                            tracing::warn!("Host key verification timed out");
                            Err(SshError::HostKeyVerification(
                                "Host key verification timed out".to_string(),
                            ))
                        }
                    }
                }
                HostKeyStatus::Changed {
                    old_fingerprint,
                    new_fingerprint,
                    key_type,
                } => {
                    tracing::warn!("Host key changed");

                    // Create oneshot channel for response
                    let (tx, rx) = oneshot::channel();

                    let request = HostKeyVerificationRequest::ChangedHost {
                        info: HostKeyInfo {
                            host: host.to_string(),
                            port,
                            fingerprint: new_fingerprint,
                            key_type,
                        },
                        old_fingerprint,
                        responder: tx,
                    };

                    // Send request to UI via event channel
                    event_tx
                        .send(SshEvent::HostKeyVerification(Box::new(request)))
                        .await
                        .map_err(|_| {
                            SshError::HostKeyVerification(
                                "Failed to request host key verification".to_string(),
                            )
                        })?;

                    // Wait for user response (with timeout)
                    match tokio::time::timeout(Duration::from_secs(60), rx).await {
                        Ok(Ok(HostKeyVerificationResponse::Accept)) => {
                            tracing::debug!("User accepted changed host key");
                            // Update key in known_hosts (fail closed if we cannot persist)
                            let update_result = tokio::task::spawn_blocking({
                                let known_hosts = Arc::clone(&known_hosts);
                                let host = Arc::clone(&host);
                                let key = Arc::clone(&key);
                                move || {
                                    let mut manager = known_hosts.blocking_lock();
                                    manager.update_host_key(host.as_ref(), port, key.as_ref())
                                }
                            })
                            .await
                            .map_err(|e| {
                                SshError::HostKeyVerification(format!(
                                    "Host key update task failed: {}",
                                    e
                                ))
                            })?;

                            match update_result {
                                Ok(()) => Ok(true),
                                Err(e) => Err(SshError::HostKeyVerification(format!(
                                    "Failed to update host key: {}",
                                    e
                                ))),
                            }
                        }
                        Ok(Ok(HostKeyVerificationResponse::Reject)) | Ok(Err(_)) => {
                            tracing::debug!("User rejected changed host key");
                            Err(SshError::HostKeyVerification(
                                "Host key change rejected by user".to_string(),
                            ))
                        }
                        Err(_) => {
                            tracing::warn!("Host key verification timed out");
                            Err(SshError::HostKeyVerification(
                                "Host key verification timed out".to_string(),
                            ))
                        }
                    }
                }
            }
        }
    }

    async fn channel_eof(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn channel_close(
        &mut self,
        _channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn server_channel_open_agent_forward(
        &mut self,
        channel: Channel<russh::client::Msg>,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if !self.agent_forwarding_enabled {
            tracing::warn!("Rejected agent forwarding request (disabled)");
            if let Err(e) = channel.close().await {
                tracing::debug!("Failed to close agent forwarding channel: {}", e);
            }
            return Ok(());
        }

        if let Err(e) = super::session::spawn_agent_forwarding(channel).await {
            tracing::warn!("Agent forwarding failed: {}", e);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_handler() -> (ClientHandler, mpsc::Receiver<SshEvent>) {
        let (tx, rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let handler = ClientHandler::new("example.com".to_string(), 22, known_hosts, tx, false);
        (handler, rx)
    }

    // === ClientHandler::new tests ===

    #[test]
    fn new_creates_handler_with_host() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let handler = ClientHandler::new(
            "myserver.example.com".to_string(),
            22,
            known_hosts,
            tx,
            false,
        );
        assert_eq!(handler.host.as_ref(), "myserver.example.com");
    }

    #[test]
    fn new_creates_handler_with_port() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let handler = ClientHandler::new("example.com".to_string(), 2222, known_hosts, tx, false);
        assert_eq!(handler.port, 2222);
    }

    #[test]
    fn new_creates_handler_with_default_port() {
        let (handler, _rx) = create_test_handler();
        assert_eq!(handler.port, 22);
    }

    #[test]
    fn new_creates_handler_with_shared_known_hosts() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let known_hosts_clone = known_hosts.clone();

        let handler = ClientHandler::new("example.com".to_string(), 22, known_hosts, tx, false);

        // Verify the same Arc is used
        assert!(Arc::ptr_eq(&handler.known_hosts, &known_hosts_clone));
    }

    #[test]
    fn new_with_ipv4_host() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let handler = ClientHandler::new("192.168.1.100".to_string(), 22, known_hosts, tx, false);
        assert_eq!(handler.host.as_ref(), "192.168.1.100");
    }

    #[test]
    fn new_with_ipv6_host() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let handler = ClientHandler::new("::1".to_string(), 22, known_hosts, tx, false);
        assert_eq!(handler.host.as_ref(), "::1");
    }

    #[test]
    fn new_with_localhost() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let handler = ClientHandler::new("localhost".to_string(), 22, known_hosts, tx, false);
        assert_eq!(handler.host.as_ref(), "localhost");
    }

    #[test]
    fn new_with_high_port() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let handler = ClientHandler::new("example.com".to_string(), 65535, known_hosts, tx, false);
        assert_eq!(handler.port, 65535);
    }

    #[test]
    fn new_with_port_one() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let handler = ClientHandler::new("example.com".to_string(), 1, known_hosts, tx, false);
        assert_eq!(handler.port, 1);
    }

    #[test]
    fn multiple_handlers_can_share_known_hosts() {
        let (tx1, _rx1) = mpsc::channel(16);
        let (tx2, _rx2) = mpsc::channel(16);
        let shared_known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));

        let handler1 = ClientHandler::new(
            "server1.example.com".to_string(),
            22,
            shared_known_hosts.clone(),
            tx1,
            false,
        );
        let handler2 = ClientHandler::new(
            "server2.example.com".to_string(),
            22,
            shared_known_hosts.clone(),
            tx2,
            false,
        );

        // Both handlers should share the same known_hosts
        assert!(Arc::ptr_eq(&handler1.known_hosts, &handler2.known_hosts));
        assert_eq!(Arc::strong_count(&shared_known_hosts), 3);
    }

    #[test]
    fn handlers_have_separate_event_channels() {
        let (tx1, _rx1) = mpsc::channel(16);
        let (tx2, _rx2) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));

        let _handler1 = ClientHandler::new(
            "server1.example.com".to_string(),
            22,
            known_hosts.clone(),
            tx1,
            false,
        );
        let _handler2 = ClientHandler::new(
            "server2.example.com".to_string(),
            22,
            known_hosts.clone(),
            tx2,
            false,
        );

        // Handlers created successfully with separate channels
        // (can't directly compare channels, but creation succeeds)
    }

    #[test]
    fn new_with_empty_host() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let handler = ClientHandler::new(String::new(), 22, known_hosts, tx, false);
        assert!(handler.host.is_empty());
    }

    #[test]
    fn new_with_unicode_host() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        // IDN domain (internationalized domain name)
        let handler = ClientHandler::new("例え.jp".to_string(), 22, known_hosts, tx, false);
        assert_eq!(handler.host.as_ref(), "例え.jp");
    }

    #[test]
    fn new_preserves_host_case() {
        let (tx, _rx) = mpsc::channel(16);
        let known_hosts = Arc::new(Mutex::new(KnownHostsManager::new()));
        let handler = ClientHandler::new(
            "MyServer.Example.COM".to_string(),
            22,
            known_hosts,
            tx,
            false,
        );
        // Host should preserve original case
        assert_eq!(handler.host.as_ref(), "MyServer.Example.COM");
    }
}
