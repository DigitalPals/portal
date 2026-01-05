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

    // Try ed25519 first, then rsa
    let pub_key_path = if ssh_dir.join("id_ed25519.pub").exists() {
        ssh_dir.join("id_ed25519.pub")
    } else if ssh_dir.join("id_rsa.pub").exists() {
        ssh_dir.join("id_rsa.pub")
    } else {
        return Err(SshError::KeyInstall(
            "No SSH public key found (~/.ssh/id_ed25519.pub or ~/.ssh/id_rsa.pub)".into(),
        ));
    };

    let pub_key = std::fs::read_to_string(&pub_key_path)
        .map_err(|e| SshError::KeyInstall(format!("Failed to read public key: {}", e)))?;
    let pub_key = pub_key.trim();

    // 2. Validate key format
    if !pub_key.starts_with("ssh-") {
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
