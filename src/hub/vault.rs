use std::path::Path;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, OsRng, rand_core::RngCore};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use chrono::{DateTime, Utc};
use data_encoding::BASE64;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::{self, paths};

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HubVaultConfig {
    #[serde(default)]
    pub keys: Vec<VaultKey>,
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
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
        serde_json::from_str(&content)
            .map_err(|error| format!("failed to parse {}: {}", path.display(), error))
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
}

pub fn import_private_key_file(
    path: &Path,
    name: Option<String>,
    passphrase: &SecretString,
) -> Result<VaultKey, String> {
    let private_key = std::fs::read_to_string(path)
        .map_err(|error| format!("failed to read key {}: {}", path.display(), error))?;
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

pub fn encrypt_private_key(
    name: String,
    private_key: &[u8],
    passphrase: &SecretString,
) -> Result<VaultKey, String> {
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);

    let key = derive_key(passphrase.expose_secret().as_bytes(), &salt, default_kdf())?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key)
        .map_err(|_| "failed to initialize vault cipher".to_string())?;
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), private_key)
        .map_err(|_| "failed to encrypt private key".to_string())?;
    let now = Utc::now();

    Ok(VaultKey {
        id: Uuid::new_v4(),
        name,
        public_key: None,
        fingerprint: None,
        algorithm: None,
        created_at: now,
        updated_at: now,
        encryption: VaultEncryption {
            kdf: default_kdf(),
            salt_base64: BASE64.encode(&salt),
            cipher: CIPHER.to_string(),
            nonce_base64: BASE64.encode(&nonce),
            ciphertext_base64: BASE64.encode(&ciphertext),
        },
    })
}

pub fn decrypt_private_key(
    key: &VaultKey,
    passphrase: &SecretString,
) -> Result<SecretString, String> {
    if key.encryption.cipher != CIPHER {
        return Err(format!(
            "unsupported vault cipher: {}",
            key.encryption.cipher
        ));
    }

    let salt = BASE64
        .decode(key.encryption.salt_base64.as_bytes())
        .map_err(|error| format!("invalid vault salt: {}", error))?;
    let nonce = BASE64
        .decode(key.encryption.nonce_base64.as_bytes())
        .map_err(|error| format!("invalid vault nonce: {}", error))?;
    let ciphertext = BASE64
        .decode(key.encryption.ciphertext_base64.as_bytes())
        .map_err(|error| format!("invalid vault ciphertext: {}", error))?;
    if nonce.len() != NONCE_LEN {
        return Err("invalid vault nonce length".to_string());
    }

    let derived = derive_key(
        passphrase.expose_secret().as_bytes(),
        &salt,
        &key.encryption.kdf,
    )?;
    let cipher = XChaCha20Poly1305::new_from_slice(&derived)
        .map_err(|_| "failed to initialize vault cipher".to_string())?;
    let plaintext = cipher
        .decrypt(XNonce::from_slice(&nonce), ciphertext.as_ref())
        .map_err(|_| "failed to decrypt vault key; check the vault passphrase".to_string())?;
    let text = String::from_utf8(plaintext)
        .map_err(|_| "decrypted vault key is not valid UTF-8".to_string())?;
    Ok(SecretString::from(text))
}

pub fn load_decrypted_private_key(id: Uuid) -> Result<SecretString, String> {
    let vault = HubVaultConfig::load()?;
    let key = vault
        .find_key(id)
        .ok_or_else(|| format!("vault key {} was not found", id))?;
    let passphrase =
        load_stored_vault_passphrase()?.ok_or_else(|| "Portal Hub vault is locked".to_string())?;
    decrypt_private_key(key, &passphrase)
}

pub fn store_vault_passphrase(passphrase: &SecretString) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .map_err(|error| format!("failed to open OS keychain: {}", error))?;
    entry
        .set_password(passphrase.expose_secret())
        .map_err(|error| format!("failed to store vault passphrase in OS keychain: {}", error))
}

pub fn load_stored_vault_passphrase() -> Result<Option<SecretString>, String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, KEYCHAIN_USER)
        .map_err(|error| format!("failed to open OS keychain: {}", error))?;
    match entry.get_password() {
        Ok(passphrase) => Ok(Some(SecretString::from(passphrase))),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "failed to read vault passphrase from OS keychain: {}",
            error
        )),
    }
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
    fn vault_decrypt_rejects_wrong_passphrase() {
        let key = encrypt_private_key(
            "default".to_string(),
            b"-----BEGIN OPENSSH PRIVATE KEY-----\nexample\n-----END OPENSSH PRIVATE KEY-----\n",
            &SecretString::from("right"),
        )
        .unwrap();

        assert!(decrypt_private_key(&key, &SecretString::from("wrong")).is_err());
    }
}
