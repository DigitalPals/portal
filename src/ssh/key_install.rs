//! SSH public key installation on remote servers
//!
//! This module provides functionality to install the local user's SSH public key
//! on a remote server by appending it to ~/.ssh/authorized_keys.

use directories::BaseDirs;
use std::path::Path;

use crate::error::SshError;
use crate::fs_utils;

use super::SshSession;

const PUBLIC_KEY_FILE_MAX_BYTES: u64 = 64 * 1024;

/// Install the local SSH public key on the remote server.
///
/// This function:
/// 1. Reads the local public key (~/.ssh/id_ed25519.pub or ~/.ssh/id_rsa.pub)
/// 2. Checks if the key already exists in the remote ~/.ssh/authorized_keys
/// 3. If not present, appends the key with proper permissions
///
/// Returns:
/// - `Ok(true)` if the key was newly installed
/// - `Ok(false)` if the key was already present
/// - `Err(...)` if installation failed
pub async fn install_ssh_key(session: &SshSession) -> Result<bool, SshError> {
    // 1. Find and read local public key
    let home_dir = BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .ok_or_else(|| SshError::KeyInstall("Cannot determine home directory".into()))?;

    let ssh_dir = home_dir.join(".ssh");

    let pub_key_path = [
        "id_ed25519.pub",
        "id_ecdsa.pub",
        "id_rsa.pub",
        "id_ed25519_sk.pub",
        "id_ecdsa_sk.pub",
    ]
    .into_iter()
    .map(|name| ssh_dir.join(name))
    .find(|path| path.exists())
    .ok_or_else(|| {
        SshError::KeyInstall(
            "No SSH public key found (~/.ssh/id_ed25519.pub, id_ecdsa.pub, id_rsa.pub, or security-key variants)".into(),
        )
    })?;

    let pub_key = read_public_key_file(&pub_key_path)?;
    let pub_key = pub_key.trim();

    // 2. Validate key format
    if !is_supported_public_key_line(pub_key) {
        return Err(SshError::KeyInstall("Invalid public key format".into()));
    }

    // 3. Escape key for shell command (handle single quotes)
    let escaped_key = pub_key.replace('\'', "'\\''");

    // 4. Check if key already exists on remote
    let check_cmd = format!(
        "test -f ~/.ssh/authorized_keys && grep -qF '{}' ~/.ssh/authorized_keys && echo FOUND || echo NOTFOUND",
        escaped_key
    );

    let output = session.execute_command(&check_cmd).await?;
    if output.trim() == "FOUND" {
        tracing::info!("SSH key already installed on remote server");
        return Ok(false);
    }

    // 5. Install the key with proper directory and file permissions
    let install_cmd = format!(
        "mkdir -p ~/.ssh && chmod 700 ~/.ssh && echo '{}' >> ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys",
        escaped_key
    );

    session.execute_command(&install_cmd).await?;

    tracing::info!("SSH key installed on remote server");
    Ok(true)
}

fn read_public_key_file(path: &Path) -> Result<String, SshError> {
    fs_utils::read_regular_file_follow_symlink_to_string_limited(
        path,
        PUBLIC_KEY_FILE_MAX_BYTES,
        "Public key",
    )
    .map_err(|error| SshError::KeyInstall(format!("Failed to read public key: {error}")))
}

fn is_supported_public_key_line(line: &str) -> bool {
    let algorithm = line.split_whitespace().next().unwrap_or_default();
    matches!(
        algorithm,
        "ssh-ed25519"
            | "ssh-rsa"
            | "ecdsa-sha2-nistp256"
            | "ecdsa-sha2-nistp384"
            | "ecdsa-sha2-nistp521"
            | "sk-ssh-ed25519@openssh.com"
            | "sk-ecdsa-sha2-nistp256@openssh.com"
    )
}

#[cfg(test)]
mod tests {
    use super::{PUBLIC_KEY_FILE_MAX_BYTES, is_supported_public_key_line, read_public_key_file};

    #[test]
    fn recognizes_common_public_key_algorithms() {
        assert!(is_supported_public_key_line("ssh-ed25519 AAAA comment"));
        assert!(is_supported_public_key_line("ssh-rsa AAAA comment"));
        assert!(is_supported_public_key_line(
            "ecdsa-sha2-nistp256 AAAA comment"
        ));
        assert!(is_supported_public_key_line(
            "sk-ssh-ed25519@openssh.com AAAA comment"
        ));
        assert!(is_supported_public_key_line(
            "sk-ecdsa-sha2-nistp256@openssh.com AAAA comment"
        ));
    }

    #[test]
    fn rejects_non_public_key_lines() {
        assert!(!is_supported_public_key_line(
            "-----BEGIN OPENSSH PRIVATE KEY-----"
        ));
        assert!(!is_supported_public_key_line(""));
    }

    #[test]
    fn read_public_key_file_reads_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519.pub");
        std::fs::write(&path, "ssh-ed25519 AAAA comment\n").unwrap();

        let content = read_public_key_file(&path).unwrap();

        assert_eq!(content, "ssh-ed25519 AAAA comment\n");
    }

    #[test]
    fn read_public_key_file_rejects_oversized_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519.pub");
        let data = vec![b'a'; PUBLIC_KEY_FILE_MAX_BYTES as usize + 1];
        std::fs::write(&path, data).unwrap();

        let error = read_public_key_file(&path).expect_err("oversized key should be rejected");

        assert!(error.to_string().contains("too large"));
    }

    #[test]
    fn read_public_key_file_rejects_directory() {
        let dir = tempfile::tempdir().unwrap();

        let error = read_public_key_file(dir.path()).expect_err("directory should be rejected");

        assert!(error.to_string().contains("not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn read_public_key_file_allows_symlinked_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real_id_ed25519.pub");
        let link = dir.path().join("id_ed25519.pub");
        std::fs::write(&target, "ssh-ed25519 AAAA comment\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let content = read_public_key_file(&link).unwrap();

        assert_eq!(content, "ssh-ed25519 AAAA comment\n");
    }

    #[cfg(unix)]
    #[test]
    fn read_public_key_file_rejects_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519.pub");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let error = read_public_key_file(&path).expect_err("socket should be rejected");

        assert!(error.to_string().contains("not a regular file"));
    }
}
