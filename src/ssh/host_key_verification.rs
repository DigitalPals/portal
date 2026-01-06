//! Host key verification types for SSH connections.
//!
//! This module provides the channel-based verification flow that allows
//! the async SSH handler to pause and await user confirmation before
//! accepting unknown or changed host keys.

use tokio::sync::oneshot;

/// Information about a host key requiring verification
#[derive(Debug, Clone)]
pub struct HostKeyInfo {
    pub host: String,
    pub port: u16,
    pub fingerprint: String,
    pub key_type: String,
}

/// Request for host key verification sent to the UI
pub enum HostKeyVerificationRequest {
    /// New unknown host - user should verify fingerprint
    NewHost {
        info: HostKeyInfo,
        responder: oneshot::Sender<HostKeyVerificationResponse>,
    },
    /// Host key changed - potential MITM attack warning
    ChangedHost {
        info: HostKeyInfo,
        old_fingerprint: String,
        responder: oneshot::Sender<HostKeyVerificationResponse>,
    },
}

impl std::fmt::Debug for HostKeyVerificationRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostKeyVerificationRequest::NewHost { info, .. } => {
                f.debug_struct("NewHost").field("info", info).finish()
            }
            HostKeyVerificationRequest::ChangedHost {
                info,
                old_fingerprint,
                ..
            } => f
                .debug_struct("ChangedHost")
                .field("info", info)
                .field("old_fingerprint", old_fingerprint)
                .finish(),
        }
    }
}

/// User's response to host key verification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostKeyVerificationResponse {
    /// Accept the key and save to known_hosts
    Accept,
    /// Reject the key and abort connection
    Reject,
}
