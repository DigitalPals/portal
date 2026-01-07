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

/// Log an SFTP disconnection.
pub fn log_sftp_disconnect(host: &str, port: u16) {
    info!(
        target: "security",
        event = "sftp_disconnect",
        host = %host,
        port = port,
        "SFTP connection closed"
    );
}

/// Log host key verification result.
pub fn log_host_key_verified(host: &str, port: u16, fingerprint: &str, is_new: bool) {
    info!(
        target: "security",
        event = "host_key_verified",
        host = %host,
        port = port,
        fingerprint = %fingerprint,
        is_new = is_new,
        "Host key verified"
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

/// Log a connection timeout.
pub fn log_connection_timeout(host: &str, port: u16) {
    warn!(
        target: "security",
        event = "connection_timeout",
        host = %host,
        port = port,
        "Connection attempt timed out"
    );
}

/// Log a connection error.
pub fn log_connection_error(host: &str, port: u16, error: &str) {
    warn!(
        target: "security",
        event = "connection_error",
        host = %host,
        port = port,
        error = %error,
        "Connection error"
    );
}
