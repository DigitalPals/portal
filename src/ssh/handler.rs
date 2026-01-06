use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use russh::ChannelId;
use russh::client::{Handler, Session};
use russh::keys::PublicKey;
use tokio::sync::{Mutex, mpsc, oneshot};

use crate::error::SshError;

use super::SshEvent;
use super::host_key_verification::{
    HostKeyInfo, HostKeyVerificationRequest, HostKeyVerificationResponse,
};
use super::known_hosts::{HostKeyStatus, KnownHostsManager};

/// SSH client handler implementation
pub struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: Arc<Mutex<KnownHostsManager>>,
    /// Channel to send events to UI (including verification requests)
    event_tx: mpsc::Sender<SshEvent>,
}

impl ClientHandler {
    pub fn new(
        host: String,
        port: u16,
        known_hosts: Arc<Mutex<KnownHostsManager>>,
        event_tx: mpsc::Sender<SshEvent>,
    ) -> Self {
        Self {
            host,
            port,
            known_hosts,
            event_tx,
        }
    }
}

impl Handler for ClientHandler {
    type Error = SshError;

    fn check_server_key(
        &mut self,
        server_public_key: &PublicKey,
    ) -> impl Future<Output = Result<bool, Self::Error>> + Send {
        let host = self.host.clone();
        let port = self.port;
        let known_hosts = self.known_hosts.clone();
        let key = server_public_key.clone();
        let event_tx = self.event_tx.clone();

        async move {
            let status = tokio::task::spawn_blocking({
                let known_hosts = known_hosts.clone();
                let host = host.clone();
                let key = key.clone();
                move || {
                    let manager = known_hosts.blocking_lock();
                    manager.check_host_key(&host, port, &key)
                }
            })
            .await
            .map_err(|e| SshError::HostKeyVerification(format!("Host key check failed: {}", e)))?;

            match status {
                HostKeyStatus::Known => {
                    tracing::debug!("Host key verified for {}:{}", host, port);
                    Ok(true)
                }
                HostKeyStatus::Revoked { fingerprint, .. } => {
                    tracing::warn!("HOST KEY REVOKED for {}:{} - {}", host, port, fingerprint);
                    Err(SshError::HostKeyVerification(
                        "Host key has been revoked".to_string(),
                    ))
                }
                HostKeyStatus::Unknown {
                    fingerprint,
                    key_type,
                } => {
                    tracing::debug!(
                        "New host key for {}:{} - {} ({})",
                        host,
                        port,
                        fingerprint,
                        key_type
                    );

                    // Create oneshot channel for response
                    let (tx, rx) = oneshot::channel();

                    let request = HostKeyVerificationRequest::NewHost {
                        info: HostKeyInfo {
                            host: host.clone(),
                            port,
                            fingerprint: fingerprint.clone(),
                            key_type: key_type.clone(),
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
                            tracing::debug!("User accepted host key for {}:{}", host, port);
                            // Save key to known_hosts
                            match tokio::task::spawn_blocking({
                                let known_hosts = known_hosts.clone();
                                let host = host.clone();
                                let key = key.clone();
                                move || {
                                    let mut manager = known_hosts.blocking_lock();
                                    manager.add_host_key(&host, port, &key)
                                }
                            })
                            .await
                            {
                                Ok(Ok(())) => {}
                                Ok(Err(e)) => {
                                    tracing::warn!("Failed to store host key: {}", e);
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to store host key: {}", e);
                                }
                            }
                            Ok(true)
                        }
                        Ok(Ok(HostKeyVerificationResponse::Reject)) | Ok(Err(_)) => {
                            tracing::debug!("User rejected host key for {}:{}", host, port);
                            Err(SshError::HostKeyVerification(
                                "Host key rejected by user".to_string(),
                            ))
                        }
                        Err(_) => {
                            tracing::warn!("Host key verification timed out for {}:{}", host, port);
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
                    tracing::warn!(
                        "HOST KEY CHANGED for {}:{} - old: {}, new: {} ({})",
                        host,
                        port,
                        old_fingerprint,
                        new_fingerprint,
                        key_type
                    );

                    // Create oneshot channel for response
                    let (tx, rx) = oneshot::channel();

                    let request = HostKeyVerificationRequest::ChangedHost {
                        info: HostKeyInfo {
                            host: host.clone(),
                            port,
                            fingerprint: new_fingerprint.clone(),
                            key_type: key_type.clone(),
                        },
                        old_fingerprint: old_fingerprint.clone(),
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
                            tracing::debug!("User accepted changed host key for {}:{}", host, port);
                            // Update key in known_hosts
                            match tokio::task::spawn_blocking({
                                let known_hosts = known_hosts.clone();
                                let host = host.clone();
                                let key = key.clone();
                                move || {
                                    let mut manager = known_hosts.blocking_lock();
                                    manager.update_host_key(&host, port, &key)
                                }
                            })
                            .await
                            {
                                Ok(Ok(())) => {}
                                Ok(Err(e)) => {
                                    tracing::warn!("Failed to update host key: {}", e);
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to update host key: {}", e);
                                }
                            }
                            Ok(true)
                        }
                        Ok(Ok(HostKeyVerificationResponse::Reject)) | Ok(Err(_)) => {
                            tracing::debug!("User rejected changed host key for {}:{}", host, port);
                            Err(SshError::HostKeyVerification(
                                "Host key change rejected by user".to_string(),
                            ))
                        }
                        Err(_) => {
                            tracing::warn!("Host key verification timed out for {}:{}", host, port);
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
}
