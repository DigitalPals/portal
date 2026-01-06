use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;

use data_encoding::BASE64_MIME;
use hmac::{Hmac, Mac};
use russh::keys::{self, HashAlg, PublicKey};
use sha1::Sha1;

use crate::config::{paths, write_atomic};
use crate::error::SshError;

#[derive(Default)]
struct HostKeyScan {
    keys: Vec<PublicKey>,
    revoked_keys: Vec<PublicKey>,
    line_numbers: Vec<usize>,
}

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
    /// Key matches a revoked entry
    Revoked { fingerprint: String },
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
        Self::with_paths(paths::known_hosts_file(), paths::ssh_known_hosts_file())
    }

    /// Create a new manager with explicit paths (useful for tests)
    pub fn with_paths(primary_path: Option<PathBuf>, ssh_path: Option<PathBuf>) -> Self {
        Self {
            primary_path,
            ssh_path,
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

    fn scan_known_hosts_for_host(&self, host: &str, port: u16) -> HostKeyScan {
        let mut scan = HostKeyScan::default();
        self.scan_known_hosts(host, port, &mut scan);
        scan
    }

    /// Get the fingerprint of a public key
    pub fn get_fingerprint(key: &PublicKey) -> String {
        key.fingerprint(HashAlg::Sha256).to_string()
    }

    /// Check if a host key is known/valid
    pub fn check_host_key(&self, host: &str, port: u16, key: &PublicKey) -> HostKeyStatus {
        let scan = self.scan_known_hosts_for_host(host, port);
        let matches = scan.keys;
        let revoked_keys = scan.revoked_keys;
        let fingerprint = Self::get_fingerprint(key);
        if revoked_keys.iter().any(|revoked| revoked == key) {
            return HostKeyStatus::Revoked { fingerprint };
        }

        if matches.is_empty() {
            return HostKeyStatus::Unknown {
                fingerprint,
                key_type: key.algorithm().as_str().to_string(),
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
                key_type: key.algorithm().as_str().to_string(),
            };
        }
        HostKeyStatus::Unknown {
            fingerprint,
            key_type: key.algorithm().as_str().to_string(),
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
        if let Some(path) = self.primary_write_path() {
            self.remove_host_key_entries(host, port, &path)?;
        }
        self.add_host_key(host, port, key)
    }

    fn remove_host_key_entries(
        &self,
        host: &str,
        port: u16,
        path: &PathBuf,
    ) -> Result<(), SshError> {
        if !path.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(path).map_err(|e| {
            SshError::HostKeyVerification(format!(
                "Failed to read known_hosts {}: {}",
                path.display(),
                e
            ))
        })?;

        let scan = self.scan_known_hosts_path(host, port, path)?;
        if scan.line_numbers.is_empty() {
            return Ok(());
        }

        let line_numbers: HashSet<usize> = scan.line_numbers.into_iter().collect();
        let filtered: Vec<&str> = content
            .lines()
            .enumerate()
            .filter(|(idx, _)| !line_numbers.contains(&(idx + 1)))
            .map(|(_, line)| line)
            .collect();

        if filtered.len() == content.lines().count() {
            return Ok(());
        }

        let mut new_content = filtered.join("\n");
        if content.ends_with('\n') {
            new_content.push('\n');
        }

        write_atomic(path, &new_content).map_err(|e| {
            SshError::HostKeyVerification(format!(
                "Failed to update known_hosts {}: {}",
                path.display(),
                e
            ))
        })
    }

    fn scan_known_hosts(&self, host: &str, port: u16, scan: &mut HostKeyScan) {
        for path in self.known_hosts_paths() {
            match self.scan_known_hosts_path(host, port, &path) {
                Ok(result) => {
                    scan.keys.extend(result.keys);
                    scan.revoked_keys.extend(result.revoked_keys);
                }
                Err(e) => {
                    tracing::debug!("Failed to read known_hosts {}: {}", path.display(), e);
                }
            }
        }
    }

    fn scan_known_hosts_path(
        &self,
        host: &str,
        port: u16,
        path: &PathBuf,
    ) -> Result<HostKeyScan, SshError> {
        if !path.exists() {
            return Ok(HostKeyScan::default());
        }

        let content = std::fs::read_to_string(path).map_err(|e| {
            SshError::HostKeyVerification(format!(
                "Failed to read known_hosts {}: {}",
                path.display(),
                e
            ))
        })?;

        let host_port = if port == 22 {
            Cow::Borrowed(host)
        } else {
            Cow::Owned(format!("[{}]:{}", host, port))
        };

        let mut scan = HostKeyScan::default();

        for (index, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let (marker, rest) = if let Some(stripped) = trimmed.strip_prefix('@') {
                match stripped.split_once(' ') {
                    Some((marker, rest)) => (Some(marker), rest.trim_start()),
                    None => continue,
                }
            } else {
                (None, trimmed)
            };

            let mut parts = rest.split_whitespace();
            let Some(hosts_field) = parts.next() else {
                continue;
            };
            let Some(_key_type) = parts.next() else {
                continue;
            };
            let Some(key_data) = parts.next() else {
                continue;
            };

            if !self.host_matches(&host_port, host, hosts_field) {
                continue;
            }

            let key = match keys::parse_public_key_base64(key_data) {
                Ok(key) => key,
                Err(e) => {
                    tracing::debug!(
                        "Failed to parse known_hosts key in {} line {}: {}",
                        path.display(),
                        index + 1,
                        e
                    );
                    continue;
                }
            };

            match marker {
                Some("revoked") => {
                    scan.revoked_keys.push(key);
                }
                Some("cert-authority") => {
                    continue;
                }
                Some(_) => {
                    continue;
                }
                None => {
                    scan.keys.push(key);
                    scan.line_numbers.push(index + 1);
                }
            }
        }

        Ok(scan)
    }

    fn host_matches(&self, host_port: &str, host: &str, host_field: &str) -> bool {
        let mut matched = false;

        for raw_entry in host_field.split(',') {
            let entry = raw_entry.trim();
            if entry.is_empty() {
                continue;
            }

            let (negated, pattern) = entry
                .strip_prefix('!')
                .map(|p| (true, p))
                .unwrap_or((false, entry));

            let is_match = self.match_host_pattern(host_port, host, pattern);
            if negated {
                if is_match {
                    return false;
                }
                continue;
            }

            if is_match {
                matched = true;
            }
        }

        matched
    }

    fn match_host_pattern(&self, host_port: &str, host: &str, pattern: &str) -> bool {
        if pattern.starts_with("|1|") {
            return self.match_hashed_host(host_port, pattern);
        }

        if pattern.contains('*') || pattern.contains('?') {
            return glob_match(pattern, host) || glob_match(pattern, host_port);
        }

        pattern == host || pattern == host_port
    }

    fn match_hashed_host(&self, host_port: &str, pattern: &str) -> bool {
        let mut parts = pattern.split('|').skip(2);
        let Some(salt) = parts.next() else {
            return false;
        };
        let Some(hash) = parts.next() else {
            return false;
        };

        let Ok(salt) = BASE64_MIME.decode(salt.as_bytes()) else {
            return false;
        };
        let Ok(hash) = BASE64_MIME.decode(hash.as_bytes()) else {
            return false;
        };

        let Ok(mut hmac) = Hmac::<Sha1>::new_from_slice(&salt) else {
            return false;
        };
        hmac.update(host_port.as_bytes());
        hmac.verify_slice(&hash).is_ok()
    }
}

fn glob_match(pattern: &str, text: &str) -> bool {
    let (mut p_idx, mut t_idx) = (0usize, 0usize);
    let mut star_idx = None;
    let mut match_idx = 0usize;
    let p_bytes = pattern.as_bytes();
    let t_bytes = text.as_bytes();

    while t_idx < t_bytes.len() {
        if p_idx < p_bytes.len() && (p_bytes[p_idx] == b'?' || p_bytes[p_idx] == t_bytes[t_idx]) {
            p_idx += 1;
            t_idx += 1;
            continue;
        }

        if p_idx < p_bytes.len() && p_bytes[p_idx] == b'*' {
            star_idx = Some(p_idx);
            match_idx = t_idx;
            p_idx += 1;
            continue;
        }

        if let Some(star_pos) = star_idx {
            p_idx = star_pos + 1;
            match_idx += 1;
            t_idx = match_idx;
            continue;
        }

        return false;
    }

    while p_idx < p_bytes.len() && p_bytes[p_idx] == b'*' {
        p_idx += 1;
    }

    p_idx == p_bytes.len()
}

impl Default for KnownHostsManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{HostKeyStatus, KnownHostsManager};
    use russh::keys;
    use std::fs;
    use tempfile::tempdir;

    const KEY1: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const KEY2: &str = "AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF";

    #[test]
    fn known_key_matches() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("example.com ssh-ed25519 {KEY1}\n")).expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        assert!(matches!(
            manager.check_host_key("example.com", 22, &key),
            HostKeyStatus::Known
        ));
    }

    #[test]
    fn revoked_key_blocks_match() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("@revoked example.com ssh-ed25519 {KEY2}\n"))
            .expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY2).expect("parse key");

        assert!(matches!(
            manager.check_host_key("example.com", 22, &key),
            HostKeyStatus::Revoked { .. }
        ));
    }

    #[test]
    fn wildcard_and_negation_patterns() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(
            &path,
            format!("!bad.example.com,*.example.com ssh-ed25519 {KEY1}\n"),
        )
        .expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        assert!(matches!(
            manager.check_host_key("good.example.com", 22, &key),
            HostKeyStatus::Known
        ));
        assert!(matches!(
            manager.check_host_key("bad.example.com", 22, &key),
            HostKeyStatus::Unknown { .. }
        ));
    }
}
