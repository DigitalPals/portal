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

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

use chrono::Local;
use tracing::{info, warn};

/// Path to the security audit log file, if enabled
static AUDIT_LOG_PATH: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

/// Initialize the security audit log file path.
///
/// Call this at startup and whenever the user changes the setting.
pub fn init_audit_log(path: Option<PathBuf>) {
    let lock = AUDIT_LOG_PATH.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = lock.write() {
        *guard = path;
    }
}

/// Write an entry to the audit log file, if configured.
fn write_audit_entry(entry: &str) {
    let path = AUDIT_LOG_PATH
        .get()
        .and_then(|lock| lock.read().ok().map(|guard| guard.clone()))
        .flatten();

    if let Some(path) = path {
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
            let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
            let _ = writeln!(file, "[{}] {}", timestamp, entry);
        }
    }
}

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
    write_audit_entry(&format!(
        "AUTH_ATTEMPT host={}:{} user={} method={}",
        host, port, username, method
    ));
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
    write_audit_entry(&format!(
        "AUTH_SUCCESS host={}:{} user={} method={}",
        host, port, username, method
    ));
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
    write_audit_entry(&format!(
        "AUTH_FAILURE host={}:{} user={} method={} reason={}",
        host, port, username, method, reason
    ));
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
    write_audit_entry(&format!(
        "SFTP_CONNECT host={}:{} user={}",
        host, port, username
    ));
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
        write_audit_entry(&format!(
            "HOST_KEY_CHANGED_ACCEPTED host={}:{} fingerprint={} WARNING: potential MITM",
            host, port, fingerprint
        ));
    } else {
        info!(
            target: "security",
            event = "host_key_accepted",
            host = %host,
            port = port,
            fingerprint = %fingerprint,
            "User accepted new host key"
        );
        write_audit_entry(&format!(
            "HOST_KEY_ACCEPTED host={}:{} fingerprint={}",
            host, port, fingerprint
        ));
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
    write_audit_entry(&format!(
        "HOST_KEY_REJECTED host={}:{} reason={}",
        host, port, reason
    ));
}

/// Log when SSH agent forwarding is enabled for a connection.
pub fn log_agent_forwarding_enabled(host: &str, port: u16, username: &str) {
    warn!(
        target: "security",
        event = "agent_forwarding_enabled",
        host = %host,
        port = port,
        username = %username,
        "SSH agent forwarding enabled - keys may be exposed to remote host"
    );
    write_audit_entry(&format!(
        "AGENT_FORWARDING_ENABLED host={}:{} user={} WARNING: keys exposed",
        host, port, username
    ));
}

/// Log when a cached passphrase is used.
pub fn log_passphrase_cache_hit(key_path: &str) {
    info!(
        target: "security",
        event = "passphrase_cache_hit",
        key_path = %key_path,
        "Using cached passphrase for key"
    );
    write_audit_entry(&format!("PASSPHRASE_CACHE_HIT key={}", key_path));
}

/// Log an SSH session connection.
pub fn log_ssh_connect(host: &str, port: u16, username: &str) {
    info!(
        target: "security",
        event = "ssh_connect",
        host = %host,
        port = port,
        username = %username,
        "SSH session connected"
    );
    write_audit_entry(&format!(
        "SSH_CONNECT host={}:{} user={}",
        host, port, username
    ));
}

/// Log an SSH session disconnection.
pub fn log_ssh_disconnect(host: &str, port: u16, clean: bool) {
    info!(
        target: "security",
        event = "ssh_disconnect",
        host = %host,
        port = port,
        clean = clean,
        "SSH session disconnected"
    );
    write_audit_entry(&format!(
        "SSH_DISCONNECT host={}:{} clean={}",
        host, port, clean
    ));
}

/// Log a VNC connection.
pub fn log_vnc_connect(host: &str, port: u16) {
    info!(
        target: "security",
        event = "vnc_connect",
        host = %host,
        port = port,
        "VNC session connected"
    );
    write_audit_entry(&format!("VNC_CONNECT host={}:{}", host, port));
}

/// Log a VNC disconnection.
pub fn log_vnc_disconnect(host: &str, port: u16) {
    info!(
        target: "security",
        event = "vnc_disconnect",
        host = %host,
        port = port,
        "VNC session disconnected"
    );
    write_audit_entry(&format!("VNC_DISCONNECT host={}:{}", host, port));
}
