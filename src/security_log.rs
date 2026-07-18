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

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use chrono::Local;
use tracing::{info, warn};

use crate::fs_utils::{ensure_private_dir_no_follow, open_append_regular_file};

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

/// Initialize security audit logging in a directory after validating it is private.
pub fn init_audit_log_dir(dir: Option<PathBuf>) -> std::io::Result<Option<PathBuf>> {
    match dir {
        Some(dir) => match prepare_audit_log_path(&dir) {
            Ok(path) => {
                init_audit_log(Some(path.clone()));
                Ok(Some(path))
            }
            Err(error) => {
                init_audit_log(None);
                Err(error)
            }
        },
        None => {
            init_audit_log(None);
            Ok(None)
        }
    }
}

fn prepare_audit_log_path(dir: &Path) -> std::io::Result<PathBuf> {
    ensure_private_audit_dir(dir)?;
    Ok(dir.join("audit.log"))
}

fn ensure_private_audit_dir(dir: &Path) -> std::io::Result<()> {
    ensure_private_dir_no_follow(dir)
}

/// Write an entry to the audit log file, if configured.
fn write_audit_entry(entry: &str) {
    let path = AUDIT_LOG_PATH
        .get()
        .and_then(|lock| lock.read().ok().map(|guard| guard.clone()))
        .flatten();

    if let Some(path) = path
        && let Ok(mut file) = open_append_regular_file(&path)
    {
        let timestamp = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let _ = writeln!(file, "[{}] {}", timestamp, sanitize_audit_entry(entry));
    }
}

fn sanitize_audit_entry(entry: &str) -> String {
    entry
        .chars()
        .flat_map(|ch| match ch {
            '\n' => "\\n".chars().collect::<Vec<_>>(),
            '\r' => "\\r".chars().collect::<Vec<_>>(),
            '\t' => "\\t".chars().collect::<Vec<_>>(),
            ch if ch.is_control() => format!("\\u{{{:x}}}", ch as u32).chars().collect(),
            ch => vec![ch],
        })
        .collect()
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

/// Log when a host's private key is sent to Portal Hub to start a proxied
/// session (key-file and vault-key auth; agent auth sends no key).
pub fn log_hub_private_key_sent(host: &str, port: u16, username: &str) {
    warn!(
        target: "security",
        event = "hub_private_key_sent",
        host = %host,
        port = port,
        username = %username,
        "Private key sent to Portal Hub to start a proxied session - key exposed to hub"
    );
    write_audit_entry(&format!(
        "HUB_PRIVATE_KEY_SENT host={}:{} user={} WARNING: key exposed to hub",
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

#[cfg(test)]
mod tests {
    use super::{init_audit_log, prepare_audit_log_path, sanitize_audit_entry, write_audit_entry};

    #[test]
    fn audit_entry_sanitizes_control_characters() {
        let sanitized = sanitize_audit_entry("host=good\nFORGED\ruser\tname\u{1f}");

        assert_eq!(sanitized, r"host=good\nFORGED\ruser\tname\u{1f}");
        assert!(!sanitized.contains('\n'));
        assert!(!sanitized.contains('\r'));
        assert!(!sanitized.contains('\t'));
    }

    #[test]
    fn prepare_audit_log_path_creates_directory() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("security");

        let path = prepare_audit_log_path(&dir).expect("audit directory should be created");

        assert_eq!(path, dir.join("audit.log"));
        assert!(dir.is_dir());
    }

    #[test]
    fn prepare_audit_log_path_rejects_file_directory() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("security");
        std::fs::write(&dir, "not a directory").unwrap();

        let error =
            prepare_audit_log_path(&dir).expect_err("file path should not be accepted as dir");

        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
    }

    #[cfg(unix)]
    #[test]
    fn prepare_audit_log_path_makes_directory_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("security");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        prepare_audit_log_path(&dir).expect("audit directory should be accepted");

        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn prepare_audit_log_path_rejects_symlink_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("security");
        std::fs::create_dir(&target).unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = prepare_audit_log_path(&link).expect_err("symlink dir should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(
            std::fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn audit_entry_does_not_append_through_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.log");
        let link = temp.path().join("audit.log");
        std::fs::write(&target, "original\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        init_audit_log(Some(link));
        write_audit_entry("AUTH_ATTEMPT host=example.com");
        init_audit_log(None);

        assert_eq!(std::fs::read_to_string(target).unwrap(), "original\n");
    }
}
