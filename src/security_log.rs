//! Security event logging for audit trails.
//!
//! Provides structured logging functions for security-relevant events such as
//! authentication attempts, connection establishment, and host key verification.
//!
//! All security events are logged with `target: "security"` to allow filtering
//! in production environments.
//!
//! # Example
//!
//! Filter security events only:
//! ```bash
//! RUST_LOG=portal::security_log=info cargo run
//! ```

use tracing::{info, warn};

/// Log an SSH authentication attempt.
///
/// Called before attempting to authenticate with a remote host.
pub fn log_auth_attempt(host: &str, port: u16, username: &str, method: &str) {
    info!(
        target: "security",
        event = "auth_attempt",
        host = %host,
        port = port,
        username = %username,
        method = %method,
        "SSH authentication attempt"
    );
}

/// Log a successful SSH authentication.
pub fn log_auth_success(host: &str, port: u16, username: &str, method: &str) {
    info!(
        target: "security",
        event = "auth_success",
        host = %host,
        port = port,
        username = %username,
        method = %method,
        "SSH authentication succeeded"
    );
}

/// Log a failed SSH authentication attempt.
pub fn log_auth_failure(host: &str, port: u16, username: &str, method: &str, reason: &str) {
    warn!(
        target: "security",
        event = "auth_failure",
        host = %host,
        port = port,
        username = %username,
        method = %method,
        reason = %reason,
        "SSH authentication failed"
    );
}

/// Log an SFTP connection establishment.
pub fn log_sftp_connect(host: &str, port: u16, username: &str) {
    info!(
        target: "security",
        event = "sftp_connect",
        host = %host,
        port = port,
        username = %username,
        "SFTP connection established"
    );
}

/// Log when a user accepts a new or changed host key.
pub fn log_host_key_accepted(host: &str, port: u16, fingerprint: &str, was_changed: bool) {
    if was_changed {
        warn!(
            target: "security",
            event = "host_key_change_accepted",
            host = %host,
            port = port,
            fingerprint = %fingerprint,
            "User accepted CHANGED host key - potential security risk"
        );
    } else {
        info!(
            target: "security",
            event = "host_key_accepted",
            host = %host,
            port = port,
            fingerprint = %fingerprint,
            "User accepted new host key"
        );
    }
}

/// Log when a user rejects a host key.
pub fn log_host_key_rejected(host: &str, port: u16, reason: &str) {
    info!(
        target: "security",
        event = "host_key_rejected",
        host = %host,
        port = port,
        reason = %reason,
        "User rejected host key"
    );
}
