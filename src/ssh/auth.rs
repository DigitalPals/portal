use std::path::Path;
use std::sync::Arc;

use russh::keys::{HashAlg, PrivateKeyWithHashAlg};

use crate::config::{paths, AuthMethod};
use crate::error::SshError;

/// Resolved authentication for an SSH connection
pub enum ResolvedAuth {
    /// Password authentication
    Password(String),
    /// Public key authentication with loaded key
    PublicKey(PrivateKeyWithHashAlg),
    /// SSH agent authentication (keys managed by agent)
    Agent,
}

impl ResolvedAuth {
    /// Resolve authentication method
    pub async fn resolve(method: &AuthMethod, password: Option<&str>) -> Result<Self, SshError> {
        match method {
            AuthMethod::Password => match password {
                Some(pwd) => Ok(ResolvedAuth::Password(pwd.to_string())),
                None => Err(SshError::AuthenticationFailed(
                    "Password required".to_string(),
                )),
            },
            AuthMethod::PublicKey { key_path } => {
                let path = key_path
                    .clone()
                    .or_else(find_default_key)
                    .ok_or_else(|| SshError::KeyFile("No SSH key found".to_string()))?;

                let expanded_path = paths::expand_tilde(&path.to_string_lossy());
                load_key_file(&expanded_path, None).await
            }
            AuthMethod::Agent => Ok(ResolvedAuth::Agent),
        }
    }
}

/// Find the first available default SSH key
fn find_default_key() -> Option<std::path::PathBuf> {
    paths::default_identity_files()
        .into_iter()
        .find(|path| path.exists())
}

/// Load an SSH private key from file
async fn load_key_file(path: &Path, passphrase: Option<&str>) -> Result<ResolvedAuth, SshError> {
    // Check if the file exists and is readable
    let content = std::fs::read_to_string(path).map_err(|e| {
        SshError::KeyFile(format!("Cannot read key file {}: {}", path.display(), e))
    })?;

    // Check if this is actually a public key (common mistake)
    let first_line = content.lines().next().unwrap_or("");
    if first_line.starts_with("ssh-") || first_line.starts_with("ecdsa-") {
        return Err(SshError::KeyFile(format!(
            "File {} contains a PUBLIC key, not a private key. \
             Private keys start with '-----BEGIN' and are usually named without .pub extension",
            path.display()
        )));
    }

    // Check if it looks like a private key at all
    if !first_line.starts_with("-----BEGIN") {
        return Err(SshError::KeyFile(format!(
            "File {} does not appear to be a valid SSH private key. \
             Private keys should start with '-----BEGIN OPENSSH PRIVATE KEY-----' or similar",
            path.display()
        )));
    }

    let key = russh::keys::load_secret_key(path, passphrase).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("encrypted") || msg.contains("passphrase") {
            SshError::KeyFile(format!(
                "Key is encrypted and passphrase is required: {}",
                path.display()
            ))
        } else {
            SshError::KeyFile(format!("Failed to load key {}: {}", path.display(), e))
        }
    })?;

    // Only use SHA-512 hash algorithm for RSA keys
    // ED25519 and other keys use their native signing algorithms
    let hash_alg = if key.algorithm().is_rsa() {
        Some(HashAlg::Sha512)
    } else {
        None
    };
    let key_with_hash = PrivateKeyWithHashAlg::new(Arc::new(key), hash_alg);

    Ok(ResolvedAuth::PublicKey(key_with_hash))
}
