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

#[cfg(test)]
mod tests {
    use super::*;

    // === HostKeyInfo tests ===

    #[test]
    fn host_key_info_stores_host() {
        let info = HostKeyInfo {
            host: "example.com".to_string(),
            port: 22,
            fingerprint: "SHA256:abc123".to_string(),
            key_type: "ssh-ed25519".to_string(),
        };
        assert_eq!(info.host, "example.com");
    }

    #[test]
    fn host_key_info_stores_port() {
        let info = HostKeyInfo {
            host: "example.com".to_string(),
            port: 2222,
            fingerprint: "SHA256:abc123".to_string(),
            key_type: "ssh-ed25519".to_string(),
        };
        assert_eq!(info.port, 2222);
    }

    #[test]
    fn host_key_info_stores_fingerprint() {
        let info = HostKeyInfo {
            host: "example.com".to_string(),
            port: 22,
            fingerprint: "SHA256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
            key_type: "ssh-ed25519".to_string(),
        };
        assert!(info.fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn host_key_info_stores_key_type() {
        let info = HostKeyInfo {
            host: "example.com".to_string(),
            port: 22,
            fingerprint: "SHA256:abc123".to_string(),
            key_type: "ssh-rsa".to_string(),
        };
        assert_eq!(info.key_type, "ssh-rsa");
    }

    #[test]
    fn host_key_info_clone() {
        let info = HostKeyInfo {
            host: "example.com".to_string(),
            port: 22,
            fingerprint: "SHA256:abc123".to_string(),
            key_type: "ssh-ed25519".to_string(),
        };
        let cloned = info.clone();

        assert_eq!(info.host, cloned.host);
        assert_eq!(info.port, cloned.port);
        assert_eq!(info.fingerprint, cloned.fingerprint);
        assert_eq!(info.key_type, cloned.key_type);
    }

    #[test]
    fn host_key_info_debug() {
        let info = HostKeyInfo {
            host: "example.com".to_string(),
            port: 22,
            fingerprint: "SHA256:abc123".to_string(),
            key_type: "ssh-ed25519".to_string(),
        };
        let debug_str = format!("{:?}", info);

        assert!(debug_str.contains("HostKeyInfo"));
        assert!(debug_str.contains("example.com"));
        assert!(debug_str.contains("22"));
        assert!(debug_str.contains("SHA256:abc123"));
        assert!(debug_str.contains("ssh-ed25519"));
    }

    #[test]
    fn host_key_info_with_ipv4() {
        let info = HostKeyInfo {
            host: "192.168.1.1".to_string(),
            port: 22,
            fingerprint: "SHA256:abc123".to_string(),
            key_type: "ssh-ed25519".to_string(),
        };
        assert_eq!(info.host, "192.168.1.1");
    }

    #[test]
    fn host_key_info_with_ipv6() {
        let info = HostKeyInfo {
            host: "::1".to_string(),
            port: 22,
            fingerprint: "SHA256:abc123".to_string(),
            key_type: "ssh-ed25519".to_string(),
        };
        assert_eq!(info.host, "::1");
    }

    #[test]
    fn host_key_info_various_key_types() {
        let key_types = ["ssh-ed25519", "ssh-rsa", "ecdsa-sha2-nistp256", "ecdsa-sha2-nistp384"];

        for key_type in key_types {
            let info = HostKeyInfo {
                host: "example.com".to_string(),
                port: 22,
                fingerprint: "SHA256:abc123".to_string(),
                key_type: key_type.to_string(),
            };
            assert_eq!(info.key_type, key_type);
        }
    }

    // === HostKeyVerificationResponse tests ===

    #[test]
    fn response_accept() {
        let response = HostKeyVerificationResponse::Accept;
        assert_eq!(response, HostKeyVerificationResponse::Accept);
    }

    #[test]
    fn response_reject() {
        let response = HostKeyVerificationResponse::Reject;
        assert_eq!(response, HostKeyVerificationResponse::Reject);
    }

    #[test]
    fn response_equality() {
        assert_eq!(HostKeyVerificationResponse::Accept, HostKeyVerificationResponse::Accept);
        assert_eq!(HostKeyVerificationResponse::Reject, HostKeyVerificationResponse::Reject);
        assert_ne!(HostKeyVerificationResponse::Accept, HostKeyVerificationResponse::Reject);
    }

    #[test]
    fn response_clone() {
        let accept = HostKeyVerificationResponse::Accept;
        let reject = HostKeyVerificationResponse::Reject;

        assert_eq!(accept, accept.clone());
        assert_eq!(reject, reject.clone());
    }

    #[test]
    fn response_copy() {
        let accept = HostKeyVerificationResponse::Accept;
        let copied = accept; // Copy, not move
        assert_eq!(accept, copied);
    }

    #[test]
    fn response_debug() {
        let accept_debug = format!("{:?}", HostKeyVerificationResponse::Accept);
        let reject_debug = format!("{:?}", HostKeyVerificationResponse::Reject);

        assert!(accept_debug.contains("Accept"));
        assert!(reject_debug.contains("Reject"));
    }

    // === HostKeyVerificationRequest tests ===

    #[test]
    fn request_new_host_debug() {
        let (tx, _rx) = oneshot::channel();
        let request = HostKeyVerificationRequest::NewHost {
            info: HostKeyInfo {
                host: "example.com".to_string(),
                port: 22,
                fingerprint: "SHA256:abc123".to_string(),
                key_type: "ssh-ed25519".to_string(),
            },
            responder: tx,
        };

        let debug_str = format!("{:?}", request);
        assert!(debug_str.contains("NewHost"));
        assert!(debug_str.contains("example.com"));
    }

    #[test]
    fn request_changed_host_debug() {
        let (tx, _rx) = oneshot::channel();
        let request = HostKeyVerificationRequest::ChangedHost {
            info: HostKeyInfo {
                host: "example.com".to_string(),
                port: 22,
                fingerprint: "SHA256:newkey".to_string(),
                key_type: "ssh-ed25519".to_string(),
            },
            old_fingerprint: "SHA256:oldkey".to_string(),
            responder: tx,
        };

        let debug_str = format!("{:?}", request);
        assert!(debug_str.contains("ChangedHost"));
        assert!(debug_str.contains("old_fingerprint"));
        assert!(debug_str.contains("SHA256:oldkey"));
    }

    #[tokio::test]
    async fn request_new_host_responder_works() {
        let (tx, rx) = oneshot::channel();
        let _request = HostKeyVerificationRequest::NewHost {
            info: HostKeyInfo {
                host: "example.com".to_string(),
                port: 22,
                fingerprint: "SHA256:abc123".to_string(),
                key_type: "ssh-ed25519".to_string(),
            },
            responder: tx,
        };

        // Extract and use the responder
        if let HostKeyVerificationRequest::NewHost { responder, .. } = _request {
            responder.send(HostKeyVerificationResponse::Accept).unwrap();
        }

        let response = rx.await.unwrap();
        assert_eq!(response, HostKeyVerificationResponse::Accept);
    }

    #[tokio::test]
    async fn request_changed_host_responder_works() {
        let (tx, rx) = oneshot::channel();
        let _request = HostKeyVerificationRequest::ChangedHost {
            info: HostKeyInfo {
                host: "example.com".to_string(),
                port: 22,
                fingerprint: "SHA256:newkey".to_string(),
                key_type: "ssh-ed25519".to_string(),
            },
            old_fingerprint: "SHA256:oldkey".to_string(),
            responder: tx,
        };

        // Extract and use the responder
        if let HostKeyVerificationRequest::ChangedHost { responder, .. } = _request {
            responder.send(HostKeyVerificationResponse::Reject).unwrap();
        }

        let response = rx.await.unwrap();
        assert_eq!(response, HostKeyVerificationResponse::Reject);
    }

    #[tokio::test]
    async fn request_responder_dropped_causes_recv_error() {
        let (tx, rx) = oneshot::channel::<HostKeyVerificationResponse>();
        drop(tx); // Drop without sending

        let result = rx.await;
        assert!(result.is_err());
    }
}
