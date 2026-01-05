use std::path::PathBuf;

use russh::keys::{self, HashAlg, PublicKey};

use crate::config::paths;
use crate::error::SshError;

/// Result of checking a host key
#[derive(Debug, Clone)]
pub enum HostKeyStatus {
    /// Key matches stored key
    Known,
    /// First connection - key not in known_hosts
    Unknown {
        fingerprint: String,
        key_type: String,
    },
    /// Key CHANGED from stored value (potential MITM!)
    Changed {
        old_fingerprint: String,
        new_fingerprint: String,
        key_type: String,
    },
}

/// Manager for known_hosts file operations
pub struct KnownHostsManager {
    /// Primary known_hosts file (Portal config dir)
    primary_path: Option<PathBuf>,
    /// Optional OpenSSH known_hosts file (~/.ssh/known_hosts)
    ssh_path: Option<PathBuf>,
}

impl KnownHostsManager {
    /// Create a new manager and load known hosts from file
    pub fn new() -> Self {
        Self {
            primary_path: paths::known_hosts_file(),
            ssh_path: paths::ssh_known_hosts_file(),
        }
    }

    fn known_hosts_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Some(path) = &self.primary_path {
            paths.push(path.clone());
        }
        if let Some(path) = &self.ssh_path {
            let should_add = match &self.primary_path {
                Some(primary) => primary != path,
                None => true,
            };
            if should_add {
                paths.push(path.clone());
            }
        }
        paths
    }

    fn primary_write_path(&self) -> Option<PathBuf> {
        self.primary_path.clone().or(self.ssh_path.clone())
    }

    fn matching_keys(&self, host: &str, port: u16) -> Vec<PublicKey> {
        let mut keys = Vec::new();
        for path in self.known_hosts_paths() {
            match keys::known_hosts::known_host_keys_path(host, port, &path) {
                Ok(entries) => {
                    keys.extend(entries.into_iter().map(|(_, key)| key));
                }
                Err(e) => {
                    tracing::debug!("Failed to read known_hosts {}: {}", path.display(), e);
                }
            }
        }
        keys
    }

    /// Get the fingerprint of a public key
    pub fn get_fingerprint(key: &PublicKey) -> String {
        key.fingerprint(HashAlg::Sha256).to_string()
    }

    /// Check if a host key is known/valid
    pub fn check_host_key(&self, host: &str, port: u16, key: &PublicKey) -> HostKeyStatus {
        let matches = self.matching_keys(host, port);
        let fingerprint = Self::get_fingerprint(key);
        let key_type = key.algorithm().as_str().to_string();

        if matches.is_empty() {
            return HostKeyStatus::Unknown {
                fingerprint,
                key_type,
            };
        }

        if matches.iter().any(|known_key| known_key == key) {
            return HostKeyStatus::Known;
        }

        if let Some(old_key) = matches
            .iter()
            .find(|known_key| known_key.algorithm() == key.algorithm())
        {
            let old_fingerprint = Self::get_fingerprint(old_key);
            return HostKeyStatus::Changed {
                old_fingerprint,
                new_fingerprint: fingerprint,
                key_type,
            };
        }
        HostKeyStatus::Unknown {
            fingerprint,
            key_type,
        }
    }

    /// Add a host key to known_hosts
    pub fn add_host_key(&mut self, host: &str, port: u16, key: &PublicKey) -> Result<(), SshError> {
        let path = self.primary_write_path().ok_or_else(|| {
            SshError::HostKeyVerification("No known_hosts path configured".to_string())
        })?;

        keys::known_hosts::learn_known_hosts_path(host, port, key, &path).map_err(|e| {
            SshError::HostKeyVerification(format!(
                "Failed to write known_hosts {}: {}",
                path.display(),
                e
            ))
        })
    }

    /// Update a host key (after user confirms key change)
    pub fn update_host_key(
        &mut self,
        host: &str,
        port: u16,
        key: &PublicKey,
    ) -> Result<(), SshError> {
        self.add_host_key(host, port, key)
    }
}

impl Default for KnownHostsManager {
    fn default() -> Self {
        Self::new()
    }
}
