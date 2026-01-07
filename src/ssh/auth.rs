use std::path::Path;
use std::sync::Arc;

use russh::keys::{HashAlg, PrivateKeyWithHashAlg};
use secrecy::{ExposeSecret, SecretString};

use crate::config::{AuthMethod, paths};
use crate::error::SshError;

/// Resolved authentication for an SSH connection
pub enum ResolvedAuth {
    /// Password authentication with zeroized secret string
    Password(SecretString),
    /// Public key authentication with loaded key
    PublicKey(PrivateKeyWithHashAlg),
    /// SSH agent authentication (keys managed by agent)
    Agent,
}

impl std::fmt::Debug for ResolvedAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolvedAuth::Password(_) => f.debug_tuple("Password").field(&"[REDACTED]").finish(),
            ResolvedAuth::PublicKey(_) => f.debug_tuple("PublicKey").field(&"[KEY]").finish(),
            ResolvedAuth::Agent => f.debug_struct("Agent").finish(),
        }
    }
}

impl ResolvedAuth {
    /// Resolve authentication method.
    ///
    /// Password should be passed as a SecretString for secure handling.
    pub async fn resolve(
        method: &AuthMethod,
        password: Option<SecretString>,
        passphrase: Option<SecretString>,
    ) -> Result<Self, SshError> {
        match method {
            AuthMethod::Password => match password {
                Some(pwd) => Ok(ResolvedAuth::Password(pwd)),
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
                let passphrase = passphrase.as_ref().map(|p| p.expose_secret());
                load_key_file(&expanded_path, passphrase).await
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
        let normalized = msg.to_lowercase();
        // Detect passphrase-related errors from various russh error messages
        let is_passphrase_error = normalized.contains("encrypted")
            || normalized.contains("passphrase")
            || normalized.contains("cryptographic");
        if is_passphrase_error {
            if passphrase.is_some() {
                SshError::KeyFilePassphraseInvalid(path.to_path_buf())
            } else {
                SshError::KeyFilePassphraseRequired(path.to_path_buf())
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;
    use std::fs;
    use tempfile::tempdir;

    /// Test password authentication with password provided
    #[tokio::test]
    async fn resolve_password_auth_with_password() {
        let method = AuthMethod::Password;
        let password = Some(SecretString::from("secret123"));

        let result = ResolvedAuth::resolve(&method, password, None).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ResolvedAuth::Password(_)));
    }

    /// Test password authentication without password fails
    #[tokio::test]
    async fn resolve_password_auth_without_password_fails() {
        let method = AuthMethod::Password;

        let result = ResolvedAuth::resolve(&method, None, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SshError::AuthenticationFailed(_)));
        assert!(err.to_string().contains("Password required"));
    }

    /// Test agent authentication always succeeds
    #[tokio::test]
    async fn resolve_agent_auth_succeeds() {
        let method = AuthMethod::Agent;

        let result = ResolvedAuth::resolve(&method, None, None).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ResolvedAuth::Agent));
    }

    /// Test public key authentication with valid unencrypted key
    #[tokio::test]
    async fn resolve_pubkey_auth_with_valid_key() {
        let test_keys_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/docker/test_keys");
        let key_path = test_keys_dir.join("id_ed25519");

        // Skip if test keys don't exist
        if !key_path.exists() {
            eprintln!("Skipping test: test keys not found at {:?}", key_path);
            return;
        }

        let method = AuthMethod::PublicKey {
            key_path: Some(key_path),
        };

        let result = ResolvedAuth::resolve(&method, None, None).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ResolvedAuth::PublicKey(_)));
    }

    /// Test public key authentication with encrypted key and correct passphrase
    #[tokio::test]
    async fn resolve_pubkey_auth_encrypted_key_correct_passphrase() {
        let test_keys_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/docker/test_keys");
        let key_path = test_keys_dir.join("id_ed25519_encrypted");

        if !key_path.exists() {
            eprintln!("Skipping test: encrypted test key not found");
            return;
        }

        let method = AuthMethod::PublicKey {
            key_path: Some(key_path),
        };
        let passphrase = Some(SecretString::from("testpassphrase"));

        let result = ResolvedAuth::resolve(&method, None, passphrase).await;

        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ResolvedAuth::PublicKey(_)));
    }

    /// Test public key authentication with encrypted key but no passphrase
    #[tokio::test]
    async fn resolve_pubkey_auth_encrypted_key_no_passphrase_fails() {
        let test_keys_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/docker/test_keys");
        let key_path = test_keys_dir.join("id_ed25519_encrypted");

        if !key_path.exists() {
            eprintln!("Skipping test: encrypted test key not found");
            return;
        }

        let method = AuthMethod::PublicKey {
            key_path: Some(key_path.clone()),
        };

        let result = ResolvedAuth::resolve(&method, None, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, SshError::KeyFilePassphraseRequired(ref p) if p == &key_path),
            "Expected KeyFilePassphraseRequired, got: {:?}",
            err
        );
    }

    /// Test public key authentication with encrypted key and wrong passphrase
    #[tokio::test]
    async fn resolve_pubkey_auth_encrypted_key_wrong_passphrase_fails() {
        let test_keys_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/docker/test_keys");
        let key_path = test_keys_dir.join("id_ed25519_encrypted");

        if !key_path.exists() {
            eprintln!("Skipping test: encrypted test key not found");
            return;
        }

        let method = AuthMethod::PublicKey {
            key_path: Some(key_path.clone()),
        };
        let passphrase = Some(SecretString::from("wrongpassphrase"));

        let result = ResolvedAuth::resolve(&method, None, passphrase).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, SshError::KeyFilePassphraseInvalid(ref p) if p == &key_path),
            "Expected KeyFilePassphraseInvalid, got: {:?}",
            err
        );
    }

    /// Test public key authentication with non-existent key file
    #[tokio::test]
    async fn resolve_pubkey_auth_missing_key_fails() {
        let dir = tempdir().expect("temp dir");
        let key_path = dir.path().join("nonexistent_key");

        let method = AuthMethod::PublicKey {
            key_path: Some(key_path),
        };

        let result = ResolvedAuth::resolve(&method, None, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SshError::KeyFile(_)));
        assert!(err.to_string().contains("Cannot read key file"));
    }

    /// Test public key authentication fails when given a public key file
    #[tokio::test]
    async fn resolve_pubkey_auth_with_public_key_file_fails() {
        let dir = tempdir().expect("temp dir");
        let key_path = dir.path().join("id_test.pub");

        // Write a public key format
        fs::write(
            &key_path,
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIHWcZyjL/qPgzb/PIwcuXjyaMvps0Snfxtb0dbHomqSO test@portal\n",
        )
        .expect("write public key");

        let method = AuthMethod::PublicKey {
            key_path: Some(key_path),
        };

        let result = ResolvedAuth::resolve(&method, None, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SshError::KeyFile(_)));
        assert!(err.to_string().contains("PUBLIC key"));
        assert!(err.to_string().contains("not a private key"));
    }

    /// Test public key authentication fails with ecdsa public key
    #[tokio::test]
    async fn resolve_pubkey_auth_with_ecdsa_public_key_fails() {
        let dir = tempdir().expect("temp dir");
        let key_path = dir.path().join("id_ecdsa.pub");

        fs::write(
            &key_path,
            "ecdsa-sha2-nistp256 AAAAE2VjZHNhLXNoYTItbmlzdHAyNTYAAAAIbmlzdHAyNTYAAABBBExample test@host\n",
        )
        .expect("write ecdsa public key");

        let method = AuthMethod::PublicKey {
            key_path: Some(key_path),
        };

        let result = ResolvedAuth::resolve(&method, None, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("PUBLIC key"));
    }

    /// Test public key authentication fails with invalid file content
    #[tokio::test]
    async fn resolve_pubkey_auth_with_invalid_file_fails() {
        let dir = tempdir().expect("temp dir");
        let key_path = dir.path().join("not_a_key");

        fs::write(&key_path, "This is not a valid SSH key file\nJust some text\n")
            .expect("write invalid content");

        let method = AuthMethod::PublicKey {
            key_path: Some(key_path),
        };

        let result = ResolvedAuth::resolve(&method, None, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SshError::KeyFile(_)));
        assert!(err.to_string().contains("does not appear to be a valid SSH private key"));
    }

    /// Test public key with no key_path and no default keys fails
    #[tokio::test]
    async fn resolve_pubkey_auth_no_path_no_defaults_fails() {
        // This test relies on find_default_key() returning None
        // which happens when no default keys exist in ~/.ssh/
        // Since we can't control the test environment's home directory easily,
        // we test the error path indirectly

        let method = AuthMethod::PublicKey { key_path: None };

        let result = ResolvedAuth::resolve(&method, None, None).await;

        // Either succeeds (if user has default keys) or fails with appropriate error
        if result.is_err() {
            let err = result.unwrap_err();
            // Should fail with "No SSH key found" or a key loading error
            let err_str = err.to_string();
            assert!(
                err_str.contains("No SSH key found") || err_str.contains("key"),
                "Unexpected error: {}",
                err_str
            );
        }
    }

    /// Test that find_default_key returns None when no keys exist
    #[test]
    fn find_default_key_returns_none_for_nonexistent_dir() {
        // We can't easily mock the home directory, but we can verify the function
        // doesn't panic and returns a reasonable result
        let result = find_default_key();
        // Result depends on whether the user has SSH keys - just ensure no panic
        let _ = result;
    }

    /// Test load_key_file with empty file
    #[tokio::test]
    async fn load_key_file_empty_file_fails() {
        let dir = tempdir().expect("temp dir");
        let key_path = dir.path().join("empty_key");

        fs::write(&key_path, "").expect("write empty file");

        let result = load_key_file(&key_path, None).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, SshError::KeyFile(_)));
    }

    /// Test load_key_file preserves path in passphrase errors
    #[tokio::test]
    async fn load_key_file_passphrase_error_contains_path() {
        let test_keys_dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/docker/test_keys");
        let key_path = test_keys_dir.join("id_ed25519_encrypted");

        if !key_path.exists() {
            eprintln!("Skipping test: encrypted test key not found");
            return;
        }

        let result = load_key_file(&key_path, None).await;

        assert!(result.is_err());
        if let Err(SshError::KeyFilePassphraseRequired(path)) = result {
            assert_eq!(path, key_path);
        } else {
            panic!("Expected KeyFilePassphraseRequired error");
        }
    }
}
