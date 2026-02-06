//! Passphrase cache for encrypted SSH keys.
//!
//! Caches passphrases for encrypted private keys to avoid prompting
//! users repeatedly within a configurable timeout period.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use secrecy::{ExposeSecret, SecretString};

/// Cached passphrase entry with expiration
struct CacheEntry {
    passphrase: SecretString,
    expires_at: Instant,
}

/// Thread-safe passphrase cache with automatic expiration.
///
/// Passphrases are stored in memory and automatically cleared
/// after the configured timeout period.
pub struct PassphraseCache {
    entries: Mutex<HashMap<PathBuf, CacheEntry>>,
    timeout_seconds: AtomicU64,
}

impl PassphraseCache {
    /// Create a new passphrase cache with the given timeout.
    ///
    /// A timeout of 0 disables caching entirely.
    pub fn new(timeout_seconds: u64) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            timeout_seconds: AtomicU64::new(timeout_seconds),
        }
    }

    /// Store a passphrase for a key path.
    ///
    /// The passphrase will expire after the configured timeout.
    /// If timeout is 0, the passphrase is not stored.
    pub fn store(&self, key_path: PathBuf, passphrase: SecretString) {
        let timeout_seconds = self.timeout_seconds.load(Ordering::Relaxed);
        if timeout_seconds == 0 {
            return;
        }

        let entry = CacheEntry {
            passphrase,
            expires_at: Instant::now() + Duration::from_secs(timeout_seconds),
        };

        if let Ok(mut entries) = self.entries.lock() {
            entries.insert(key_path, entry);
        }
    }

    /// Retrieve a cached passphrase for a key path.
    ///
    /// Returns None if the passphrase is not cached or has expired.
    /// Expired entries are automatically removed.
    pub fn get(&self, key_path: &PathBuf) -> Option<SecretString> {
        let mut entries = self.entries.lock().ok()?;

        if let Some(entry) = entries.get(key_path) {
            if Instant::now() < entry.expires_at {
                // Clone the passphrase for return
                return Some(SecretString::new(
                    entry.passphrase.expose_secret().to_string().into(),
                ));
            } else {
                // Entry expired, remove it
                entries.remove(key_path);
            }
        }

        None
    }

    /// Clear all cached passphrases.
    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }

    /// Clear the cached passphrase for a specific key.
    pub fn remove(&self, key_path: &PathBuf) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.remove(key_path);
        }
    }

    /// Remove all expired entries.
    pub fn cleanup_expired(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            let now = Instant::now();
            entries.retain(|_, entry| entry.expires_at > now);
        }
    }

    /// Update the cache timeout.
    ///
    /// This only affects new entries; existing entries keep their
    /// original expiration time.
    pub fn set_timeout(&self, timeout_seconds: u64) {
        self.timeout_seconds
            .store(timeout_seconds, Ordering::Relaxed);
    }

    /// Get the current timeout in seconds.
    pub fn timeout_seconds(&self) -> u64 {
        self.timeout_seconds.load(Ordering::Relaxed)
    }
}

impl Default for PassphraseCache {
    fn default() -> Self {
        // Default 5 minute timeout
        Self::new(300)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_and_retrieve() {
        let cache = PassphraseCache::new(60);
        let key_path = PathBuf::from("/home/user/.ssh/id_ed25519");
        let passphrase = SecretString::new("my_secret_passphrase".to_string().into());

        cache.store(key_path.clone(), passphrase);

        let retrieved = cache.get(&key_path);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().expose_secret(), "my_secret_passphrase");
    }

    #[test]
    fn test_zero_timeout_disables_caching() {
        let cache = PassphraseCache::new(0);
        let key_path = PathBuf::from("/home/user/.ssh/id_ed25519");
        let passphrase = SecretString::new("my_secret_passphrase".to_string().into());

        cache.store(key_path.clone(), passphrase);

        let retrieved = cache.get(&key_path);
        assert!(retrieved.is_none());
    }

    #[test]
    fn test_clear_all() {
        let cache = PassphraseCache::new(60);
        let key1 = PathBuf::from("/home/user/.ssh/id_ed25519");
        let key2 = PathBuf::from("/home/user/.ssh/id_rsa");

        cache.store(key1.clone(), SecretString::new("pass1".to_string().into()));
        cache.store(key2.clone(), SecretString::new("pass2".to_string().into()));

        cache.clear();

        assert!(cache.get(&key1).is_none());
        assert!(cache.get(&key2).is_none());
    }

    #[test]
    fn test_remove_specific_key() {
        let cache = PassphraseCache::new(60);
        let key1 = PathBuf::from("/home/user/.ssh/id_ed25519");
        let key2 = PathBuf::from("/home/user/.ssh/id_rsa");

        cache.store(key1.clone(), SecretString::new("pass1".to_string().into()));
        cache.store(key2.clone(), SecretString::new("pass2".to_string().into()));

        cache.remove(&key1);

        assert!(cache.get(&key1).is_none());
        assert!(cache.get(&key2).is_some());
    }

    #[test]
    fn test_default_timeout() {
        let cache = PassphraseCache::default();
        assert_eq!(cache.timeout_seconds(), 300); // 5 minutes
    }
}
