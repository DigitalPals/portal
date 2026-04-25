//! SSH public key installation on remote servers
//!
//! This module provides functionality to install the local user's SSH public key
//! on a remote server by appending it to ~/.ssh/authorized_keys.

use directories::BaseDirs;

use crate::error::SshError;

use super::SshSession;

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

    let pub_key = std::fs::read_to_string(&pub_key_path)
        .map_err(|e| SshError::KeyInstall(format!("Failed to read public key: {}", e)))?;
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
    use super::is_supported_public_key_line;

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
}
