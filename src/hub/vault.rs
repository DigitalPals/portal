use std::path::{Path, PathBuf};

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, OsRng, rand_core::RngCore};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use chrono::{DateTime, Utc};
use data_encoding::BASE64;
use russh::keys::{HashAlg, PublicKeyBase64};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::{self, paths};
use crate::fs_utils;

const KEYCHAIN_SERVICE: &str = "com.digitalpals.portal";
const KEYCHAIN_USER: &str = "portal-hub-vault";
const CIPHER: &str = "XChaCha20Poly1305";
const KDF: &str = "Argon2id";
const KDF_MEMORY_KIB: u32 = 64 * 1024;
const KDF_ITERATIONS: u32 = 3;
const KDF_PARALLELISM: u32 = 1;
const KEY_LEN: usize = 32;
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const DEVICE_SECRET_LEN: usize = 32;
const PRIVATE_KEY_FILE_MAX_BYTES: u64 = 1024 * 1024;
const HUB_VAULT_FILE_MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HubVaultConfig {
    #[serde(default)]
    pub keys: Vec<VaultKey>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<VaultSecret>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultKey {
    pub id: Uuid,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub algorithm: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub encryption: VaultEncryption,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultSecret {
    pub id: Uuid,
    pub name: String,
    pub kind: VaultSecretKind,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub encryption: VaultEncryption,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VaultSecretKind {
    VncPassword,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultEncryption {
    pub kdf: VaultKdf,
    pub salt_base64: String,
    pub cipher: String,
    pub nonce_base64: String,
    pub ciphertext_base64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultKdf {
    pub algorithm: String,
    pub memory_kib: u32,
    pub iterations: u32,
    pub parallelism: u32,
}

impl HubVaultConfig {
    pub fn load() -> Result<Self, String> {
        let path =
            paths::hub_vault_file().ok_or_else(|| "could not determine vault path".to_string())?;
        let content = match fs_utils::read_regular_file_to_string_limited_io(
            &path,
            HUB_VAULT_FILE_MAX_BYTES,
            "Hub vault",
        ) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) if error.kind() == std::io::ErrorKind::FileTooLarge => {
                return Err(format!("failed to read {}: {}", path.display(), error));
            }
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                config::recover_corrupt_config(&path, "Hub vault", &error.to_string()).map_err(
                    |backup_error| {
                        format!(
                            "failed to recover corrupt vault {}: {}",
                            path.display(),
                            backup_error
                        )
                    },
                )?;
                return Ok(Self::default());
            }
            Err(error) => {
                return Err(format!("failed to read {}: {}", path.display(), error));
            }
        };
        match serde_json::from_str(&content) {
            Ok(config) => Ok(config),
            Err(error) => {
                config::recover_corrupt_config(&path, "Hub vault", &error.to_string()).map_err(
                    |backup_error| {
                        format!(
                            "failed to recover corrupt vault {}: {}",
                            path.display(),
                            backup_error
                        )
                    },
                )?;
                Ok(Self::default())
            }
        }
    }

    pub fn save(&self) -> Result<(), String> {
        paths::ensure_config_dir()
            .map_err(|error| format!("failed to create config directory: {}", error))?;
        let path =
            paths::hub_vault_file().ok_or_else(|| "could not determine vault path".to_string())?;
        let content = serde_json::to_string_pretty(self)
            .map_err(|error| format!("failed to serialize vault: {}", error))?;
        config::write_atomic(&path, &content)
            .map_err(|error| format!("failed to write {}: {}", path.display(), error))
    }

    pub fn find_key(&self, id: Uuid) -> Option<&VaultKey> {
        self.keys.iter().find(|key| key.id == id)
    }

    pub fn find_key_mut(&mut self, id: Uuid) -> Option<&mut VaultKey> {
        self.keys.iter_mut().find(|key| key.id == id)
    }

    pub fn find_secret(&self, id: Uuid) -> Option<&VaultSecret> {
        self.secrets.iter().find(|secret| secret.id == id)
    }

    pub fn find_secret_mut(&mut self, id: Uuid) -> Option<&mut VaultSecret> {
        self.secrets.iter_mut().find(|secret| secret.id == id)
    }
}

#[allow(dead_code)]
pub fn import_private_key_file(
    path: &Path,
    name: Option<String>,
    passphrase: &SecretString,
) -> Result<VaultKey, String> {
    let private_key = read_private_key_file(path)?;
    encrypt_private_key(
        name.unwrap_or_else(|| {
            path.file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("SSH key")
                .to_string()
        }),
        private_key.as_bytes(),
        passphrase,
    )
}

pub fn read_private_key_file(path: &Path) -> Result<String, String> {
    fs_utils::read_regular_file_follow_symlink_to_string_limited(
        path,
        PRIVATE_KEY_FILE_MAX_BYTES,
        "private key",
    )
    .map_err(|error| format!("failed to read key {}: {}", path.display(), error))
}

pub fn encrypt_private_key(
    name: String,
    private_key: &[u8],
    passphrase: &SecretString,
) -> Result<VaultKey, String> {
    let encryption = encrypt_bytes(private_key, passphrase, "private key")?;
    let now = Utc::now();

    let metadata = private_key_metadata(private_key, None).ok().flatten();

    Ok(VaultKey {
        id: Uuid::new_v4(),
        name,
        public_key: metadata.as_ref().map(|value| value.public_key.clone()),
        fingerprint: metadata.as_ref().map(|value| value.fingerprint.clone()),
        algorithm: metadata.map(|value| value.algorithm),
        created_at: now,
        updated_at: now,
        encryption,
    })
}

pub fn encrypt_secret(
    name: String,
    kind: VaultSecretKind,
    secret: &SecretString,
    passphrase: &SecretString,
) -> Result<VaultSecret, String> {
    let now = Utc::now();
    Ok(VaultSecret {
        id: Uuid::new_v4(),
        name,
        kind,
        created_at: now,
        updated_at: now,
        encryption: encrypt_bytes(secret.expose_secret().as_bytes(), passphrase, "secret")?,
    })
}

pub fn upsert_vnc_password(
    vault: &mut HubVaultConfig,
    existing_id: Option<Uuid>,
    name: String,
    password: &SecretString,
    passphrase: &SecretString,
) -> Result<Uuid, String> {
    let replacement = encrypt_secret(name, VaultSecretKind::VncPassword, password, passphrase)?;
    let id = replacement.id;

    if let Some(existing_id) = existing_id
        && let Some(secret) = vault.find_secret_mut(existing_id)
    {
        *secret = VaultSecret {
            id: existing_id,
            created_at: secret.created_at,
            ..replacement
        };
        return Ok(existing_id);
    }

    vault.secrets.push(replacement);
    Ok(id)
}

fn encrypt_bytes(
    plaintext: &[u8],
    passphrase: &SecretString,
    label: &str,
) -> Result<VaultEncryption, String> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);

    let kdf = default_kdf();
    let key = derive_key(passphrase.expose_secret().as_bytes(), &salt, &kdf)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|_| "failed to initialize vault cipher".to_string())?;
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .map_err(|_| format!("failed to encrypt vault {}", label))?;

    Ok(VaultEncryption {
        kdf,
        salt_base64: BASE64.encode(&salt),
        cipher: CIPHER.to_string(),
        nonce_base64: BASE64.encode(&nonce),
        ciphertext_base64: BASE64.encode(&ciphertext),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultKeyMetadata {
    pub public_key: String,
    pub fingerprint: String,
    pub algorithm: String,
}

pub fn private_key_metadata(
    private_key: &[u8],
    key_passphrase: Option<&str>,
) -> Result<Option<VaultKeyMetadata>, String> {
    let content = std::str::from_utf8(private_key)
        .map_err(|_| "private key is not valid UTF-8".to_string())?;
    let first_line = content.lines().next().unwrap_or("");
    if !first_line.starts_with("-----BEGIN") {
        return Ok(None);
    }

    let key = russh::keys::decode_secret_key(content, key_passphrase)
        .map_err(|error| format!("failed to read private key metadata: {}", error))?;
    let public = key.public_key();
    let public_key = format!("{} {}", public.algorithm(), public.public_key_base64());
    let fingerprint = public.fingerprint(HashAlg::Sha256).to_string();
    Ok(Some(VaultKeyMetadata {
        public_key,
        fingerprint,
        algorithm: key.algorithm().to_string(),
    }))
}

pub fn decrypt_private_key(
    key: &VaultKey,
    passphrase: &SecretString,
) -> Result<SecretString, String> {
    decrypt_encryption(&key.encryption, passphrase, "key")
}

pub fn decrypt_secret(
    secret: &VaultSecret,
    passphrase: &SecretString,
) -> Result<SecretString, String> {
    decrypt_encryption(&secret.encryption, passphrase, "secret")
}

fn decrypt_encryption(
    encryption: &VaultEncryption,
    passphrase: &SecretString,
    label: &str,
) -> Result<SecretString, String> {
    if encryption.cipher != CIPHER {
        return Err(format!("unsupported vault cipher: {}", encryption.cipher));
    }

    let salt = BASE64
        .decode(encryption.salt_base64.as_bytes())
        .map_err(|error| format!("invalid vault salt: {}", error))?;
    let nonce = BASE64
        .decode(encryption.nonce_base64.as_bytes())
        .map_err(|error| format!("invalid vault nonce: {}", error))?;
    let ciphertext = BASE64
        .decode(encryption.ciphertext_base64.as_bytes())
        .map_err(|error| format!("invalid vault ciphertext: {}", error))?;
    if nonce.len() != NONCE_LEN {
        return Err("invalid vault nonce length".to_string());
    }

    let derived = derive_key(
        passphrase.expose_secret().as_bytes(),
        &salt,
        &encryption.kdf,
    )?;
    let cipher = XChaCha20Poly1305::new_from_slice(&derived)
        .map_err(|_| "failed to initialize vault cipher".to_string())?;
    let plaintext = cipher
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| {
            "failed to decrypt vault key; check the OS keychain vault secret".to_string()
        })?;
    let text = String::from_utf8(plaintext)
        .map_err(|_| format!("decrypted vault {} is not valid UTF-8", label))?;
    Ok(SecretString::from(text))
}

#[allow(dead_code)]
pub fn load_decrypted_private_key(id: Uuid) -> Result<SecretString, String> {
    let vault = HubVaultConfig::load()?;
    let key = vault
        .find_key(id)
        .ok_or_else(|| format!("vault key {} was not found", id))?;
    let vault_secret =
        load_stored_vault_secret()?.ok_or_else(|| "Portal Hub vault is locked".to_string())?;
    decrypt_private_key(key, &vault_secret)
}

pub fn load_decrypted_private_key_or_local_file(
    id: Uuid,
    key_path: Option<&Path>,
) -> Result<SecretString, String> {
    let vault = HubVaultConfig::load()?;
    let key = vault
        .find_key(id)
        .ok_or_else(|| format!("vault key {} was not found", id))?;
    match load_stored_vault_secret()? {
        Some(vault_secret) => decrypt_private_key(key, &vault_secret),
        None => {
            if let Some(private_key) = load_matching_local_private_key(key, key_path)? {
                return Ok(SecretString::from(private_key));
            }
            Err("Portal Hub vault is locked".to_string())
        }
    }
}

pub fn load_decrypted_secret(id: Uuid) -> Result<SecretString, String> {
    let vault = HubVaultConfig::load()?;
    let secret = vault
        .find_secret(id)
        .ok_or_else(|| format!("vault secret {} was not found", id))?;
    let vault_secret =
        load_stored_vault_secret()?.ok_or_else(|| "Portal Hub vault is locked".to_string())?;
    decrypt_secret(secret, &vault_secret)
}

pub fn load_or_create_vault_secret(vault: &HubVaultConfig) -> Result<SecretString, String> {
    if let Some(vault_secret) = load_stored_vault_secret()? {
        return Ok(vault_secret);
    }

    if !vault.keys.is_empty() || !vault.secrets.is_empty() {
        return Err(
            "Portal vault is locked because no vault secret was found in the OS keychain"
                .to_string(),
        );
    }

    let mut secret = [0u8; DEVICE_SECRET_LEN];
    OsRng.fill_bytes(&mut secret);
    let vault_secret = SecretString::from(BASE64.encode(&secret));
    store_vault_secret(&vault_secret)?;
    Ok(vault_secret)
}

pub fn store_vault_secret(vault_secret: &SecretString) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .map_err(|error| format!("failed to open OS keychain: {}", error))?;
    entry
        .set_password(vault_secret.expose_secret())
        .map_err(|error| format!("failed to store vault secret in OS keychain: {}", error))
}

pub fn load_stored_vault_secret() -> Result<Option<SecretString>, String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .map_err(|error| format!("failed to open OS keychain: {}", error))?;
    match entry.get_password() {
        Ok(vault_secret) => Ok(Some(SecretString::from(vault_secret))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "failed to read vault secret from OS keychain: {}",
            error
        )),
    }
}

fn load_matching_local_private_key(
    vault_key: &VaultKey,
    key_path: Option<&Path>,
) -> Result<Option<String>, String> {
    let mut candidates = Vec::new();
    if let Some(path) = key_path {
        candidates.push(paths::expand_tilde(&path.to_string_lossy()));
    }
    for path in paths::default_identity_files() {
        if !candidates
            .iter()
            .any(|candidate: &PathBuf| candidate == &path)
        {
            candidates.push(path);
        }
    }

    for path in candidates.into_iter().filter(|path| path.exists()) {
        let private_key = match read_private_key_file(&path) {
            Ok(private_key) => private_key,
            Err(error) => {
                tracing::debug!(
                    "Skipping local vault key fallback {}: {}",
                    path.display(),
                    error
                );
                continue;
            }
        };
        let metadata = match private_key_metadata(private_key.as_bytes(), None) {
            Ok(Some(metadata)) => metadata,
            Ok(None) => continue,
            Err(error) => {
                tracing::debug!(
                    "Skipping local vault key fallback {}: {}",
                    path.display(),
                    error
                );
                continue;
            }
        };
        if vault_key_matches_metadata(vault_key, &metadata) {
            tracing::info!(
                "Using matching local SSH key {} because Portal Hub vault is locked",
                path.display()
            );
            return Ok(Some(private_key));
        }
    }

    Ok(None)
}

fn vault_key_matches_metadata(vault_key: &VaultKey, metadata: &VaultKeyMetadata) -> bool {
    vault_key
        .fingerprint
        .as_deref()
        .is_some_and(|fingerprint| fingerprint == metadata.fingerprint)
        || vault_key
            .public_key
            .as_deref()
            .is_some_and(|public_key| public_key == metadata.public_key)
}

fn default_kdf() -> VaultKdf {
    VaultKdf {
        algorithm: KDF.to_string(),
        memory_kib: KDF_MEMORY_KIB,
        iterations: KDF_ITERATIONS,
        parallelism: KDF_PARALLELISM,
    }
}

fn derive_key(password: &[u8], salt: &[u8], kdf: &VaultKdf) -> Result<[u8; KEY_LEN], String> {
    if kdf.algorithm != KDF {
        return Err(format!("unsupported vault KDF: {}", kdf.algorithm));
    }
    let params = Params::new(
        kdf.memory_kib,
        kdf.iterations,
        kdf.parallelism,
        Some(KEY_LEN),
    )
    .map_err(|error| format!("invalid vault KDF params: {}", error))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut output = [0u8; KEY_LEN];
    argon2
        .hash_password_into(password, salt, &mut output)
        .map_err(|error| format!("failed to derive vault key: {}", error))?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vault_key_encrypts_and_decrypts_private_key_bytes() {
        let passphrase = SecretString::from("correct horse battery staple");
        let key = encrypt_private_key(
            "default".to_string(),
            b"-----BEGIN OPENSSH PRIVATE KEY-----\nexample\n-----END OPENSSH PRIVATE KEY-----\n",
            &passphrase,
        )
        .unwrap();

        assert_ne!(
            key.encryption.ciphertext_base64,
            "-----BEGIN OPENSSH PRIVATE KEY-----"
        );

        let decrypted = decrypt_private_key(&key, &passphrase).unwrap();
        assert!(
            decrypted
                .expose_secret()
                .starts_with("-----BEGIN OPENSSH PRIVATE KEY-----")
        );
    }

    #[test]
    fn vault_decrypt_rejects_wrong_secret() {
        let key = encrypt_private_key(
            "default".to_string(),
            b"-----BEGIN OPENSSH PRIVATE KEY-----\nexample\n-----END OPENSSH PRIVATE KEY-----\n",
            &SecretString::from("right"),
        )
        .unwrap();

        assert!(decrypt_private_key(&key, &SecretString::from("wrong")).is_err());
    }

    #[test]
    fn vault_secret_encrypts_and_decrypts_vnc_password() {
        let passphrase = SecretString::from("correct horse battery staple");
        let password = SecretString::from("screen-sharing-password");
        let secret = encrypt_secret(
            "VNC password for Lab Mac".to_string(),
            VaultSecretKind::VncPassword,
            &password,
            &passphrase,
        )
        .unwrap();

        assert_ne!(
            secret.encryption.ciphertext_base64,
            "screen-sharing-password"
        );
        assert_eq!(secret.kind, VaultSecretKind::VncPassword);

        let decrypted = decrypt_secret(&secret, &passphrase).unwrap();
        assert_eq!(decrypted.expose_secret(), password.expose_secret());
    }

    #[test]
    fn read_private_key_file_reads_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519");
        std::fs::write(&path, "-----BEGIN OPENSSH PRIVATE KEY-----\nexample\n").unwrap();

        let content = read_private_key_file(&path).unwrap();

        assert!(content.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
    }

    #[test]
    fn read_private_key_file_rejects_oversized_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519");
        let data = vec![b'a'; PRIVATE_KEY_FILE_MAX_BYTES as usize + 1];
        std::fs::write(&path, data).unwrap();

        let error = read_private_key_file(&path).expect_err("oversized key should be rejected");

        assert!(error.contains("too large"));
    }

    #[test]
    fn read_private_key_file_rejects_directory() {
        let dir = tempfile::tempdir().unwrap();

        let error = read_private_key_file(dir.path()).expect_err("directory should be rejected");

        assert!(error.contains("not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn read_private_key_file_allows_symlinked_key_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real_id_ed25519");
        let link = dir.path().join("id_ed25519");
        std::fs::write(&target, "-----BEGIN OPENSSH PRIVATE KEY-----\nexample\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let content = read_private_key_file(&link).unwrap();

        assert!(content.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
    }

    #[cfg(unix)]
    #[test]
    fn read_private_key_file_rejects_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("id_ed25519.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let error = read_private_key_file(&path).expect_err("socket should be rejected");

        assert!(error.contains("not a regular file"));
    }

    #[test]
    fn import_metadata_extracts_public_key_details_when_key_is_plaintext() {
        let key_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/docker/test_keys/id_ed25519");
        if !key_path.exists() {
            eprintln!("Skipping test: test key not found at {:?}", key_path);
            return;
        }

        let private_key = std::fs::read(&key_path).unwrap();
        let metadata = private_key_metadata(&private_key, None)
            .unwrap()
            .expect("metadata");

        assert!(metadata.public_key.starts_with("ssh-ed25519 "));
        assert!(metadata.fingerprint.starts_with("SHA256:"));
        assert!(metadata.algorithm.contains("ssh-ed25519"));
    }

    #[test]
    fn local_private_key_fallback_requires_matching_metadata() {
        let key_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/docker/test_keys/id_ed25519");
        if !key_path.exists() {
            eprintln!("Skipping test: test key not found at {:?}", key_path);
            return;
        }

        let private_key = std::fs::read(&key_path).unwrap();
        let mut key = encrypt_private_key(
            "default".to_string(),
            &private_key,
            &SecretString::from("vault secret"),
        )
        .unwrap();

        assert!(
            load_matching_local_private_key(&key, Some(&key_path))
                .unwrap()
                .is_some()
        );

        key.fingerprint = Some("SHA256:not-the-same-key".to_string());
        key.public_key = Some("ssh-ed25519 not-the-same-key".to_string());
        assert!(
            load_matching_local_private_key(&key, Some(&key_path))
                .unwrap()
                .is_none()
        );
    }
}
