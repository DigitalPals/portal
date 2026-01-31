use std::collections::HashSet;
use std::path::PathBuf;

use russh::keys::{self, HashAlg, PublicKey};

use crate::config::{paths, write_atomic};
use crate::error::SshError;

mod matchers;
mod scan;

#[cfg(test)]
pub(crate) use matchers::glob_match;

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
        self.select_write_path()
    }

    fn select_write_path(&self) -> Option<PathBuf> {
        if let Some(path) = &self.primary_path {
            if Self::ensure_parent_dir(path).is_ok() {
                return Some(path.clone());
            }
        }
        if let Some(path) = &self.ssh_path {
            if Self::ensure_parent_dir(path).is_ok() {
                return Some(path.clone());
            }
        }
        None
    }

    fn ensure_parent_dir(path: &std::path::Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    fn scan_known_hosts_for_host(&self, host: &str, port: u16) -> scan::HostKeyScan {
        let mut scan = scan::HostKeyScan::default();
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
        self.remove_host_key_entries_all(host, port)?;
        self.add_host_key(host, port, key)
    }

    fn remove_host_key_entries_all(&self, host: &str, port: u16) -> Result<(), SshError> {
        for path in self.known_hosts_paths() {
            self.remove_host_key_entries(host, port, &path)?;
        }
        Ok(())
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

    fn scan_known_hosts(&self, host: &str, port: u16, scan: &mut scan::HostKeyScan) {
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
    ) -> Result<scan::HostKeyScan, SshError> {
        scan::scan_known_hosts_path(host, port, path)
    }
}

impl Default for KnownHostsManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{HostKeyStatus, KnownHostsManager, glob_match, matchers};
    use russh::keys;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::tempdir;

    const KEY1: &str = "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const KEY2: &str = "AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF";

    // === Existing tests ===

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

    #[test]
    fn changed_key_detected_for_same_algorithm() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("example.com ssh-ed25519 {KEY1}\n")).expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY2).expect("parse key");

        assert!(matches!(
            manager.check_host_key("example.com", 22, &key),
            HostKeyStatus::Changed { .. }
        ));
    }

    // === HostKeyStatus tests ===

    #[test]
    fn host_key_status_known_debug() {
        let status = HostKeyStatus::Known;
        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("Known"));
    }

    #[test]
    fn host_key_status_unknown_debug() {
        let status = HostKeyStatus::Unknown {
            fingerprint: "SHA256:abc123".to_string(),
            key_type: "ssh-ed25519".to_string(),
        };
        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("Unknown"));
        assert!(debug_str.contains("SHA256:abc123"));
        assert!(debug_str.contains("ssh-ed25519"));
    }

    #[test]
    fn host_key_status_changed_debug() {
        let status = HostKeyStatus::Changed {
            old_fingerprint: "SHA256:old".to_string(),
            new_fingerprint: "SHA256:new".to_string(),
            key_type: "ssh-ed25519".to_string(),
        };
        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("Changed"));
        assert!(debug_str.contains("old_fingerprint"));
        assert!(debug_str.contains("new_fingerprint"));
    }

    #[test]
    fn host_key_status_revoked_debug() {
        let status = HostKeyStatus::Revoked {
            fingerprint: "SHA256:revoked".to_string(),
        };
        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("Revoked"));
        assert!(debug_str.contains("SHA256:revoked"));
    }

    #[test]
    fn host_key_status_clone() {
        let status = HostKeyStatus::Unknown {
            fingerprint: "SHA256:test".to_string(),
            key_type: "ssh-rsa".to_string(),
        };
        let cloned = status.clone();
        if let HostKeyStatus::Unknown {
            fingerprint,
            key_type,
        } = cloned
        {
            assert_eq!(fingerprint, "SHA256:test");
            assert_eq!(key_type, "ssh-rsa");
        } else {
            panic!("Clone should preserve variant");
        }
    }

    // === KnownHostsManager constructor tests ===

    #[test]
    fn with_paths_both_none() {
        let manager = KnownHostsManager::with_paths(None, None);
        assert!(manager.primary_path.is_none());
        assert!(manager.ssh_path.is_none());
    }

    #[test]
    fn with_paths_primary_only() {
        let path = PathBuf::from("/tmp/test_known_hosts");
        let manager = KnownHostsManager::with_paths(Some(path.clone()), None);
        assert_eq!(manager.primary_path, Some(path));
        assert!(manager.ssh_path.is_none());
    }

    #[test]
    fn with_paths_ssh_only() {
        let path = PathBuf::from("/home/user/.ssh/known_hosts");
        let manager = KnownHostsManager::with_paths(None, Some(path.clone()));
        assert!(manager.primary_path.is_none());
        assert_eq!(manager.ssh_path, Some(path));
    }

    #[test]
    fn with_paths_both_set() {
        let primary = PathBuf::from("/config/known_hosts");
        let ssh = PathBuf::from("/home/user/.ssh/known_hosts");
        let manager = KnownHostsManager::with_paths(Some(primary.clone()), Some(ssh.clone()));
        assert_eq!(manager.primary_path, Some(primary));
        assert_eq!(manager.ssh_path, Some(ssh));
    }

    // === known_hosts_paths tests ===

    #[test]
    fn known_hosts_paths_empty_when_none() {
        let manager = KnownHostsManager::with_paths(None, None);
        let paths = manager.known_hosts_paths();
        assert!(paths.is_empty());
    }

    #[test]
    fn known_hosts_paths_primary_only() {
        let primary = PathBuf::from("/config/known_hosts");
        let manager = KnownHostsManager::with_paths(Some(primary.clone()), None);
        let paths = manager.known_hosts_paths();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], primary);
    }

    #[test]
    fn known_hosts_paths_both_different() {
        let primary = PathBuf::from("/config/known_hosts");
        let ssh = PathBuf::from("/home/user/.ssh/known_hosts");
        let manager = KnownHostsManager::with_paths(Some(primary.clone()), Some(ssh.clone()));
        let paths = manager.known_hosts_paths();
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0], primary);
        assert_eq!(paths[1], ssh);
    }

    #[test]
    fn known_hosts_paths_deduplicates_same_path() {
        let path = PathBuf::from("/same/path/known_hosts");
        let manager = KnownHostsManager::with_paths(Some(path.clone()), Some(path.clone()));
        let paths = manager.known_hosts_paths();
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], path);
    }

    // === get_fingerprint tests ===

    #[test]
    fn get_fingerprint_returns_sha256() {
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");
        let fingerprint = KnownHostsManager::get_fingerprint(&key);
        assert!(fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn get_fingerprint_different_keys_different_fingerprints() {
        let key1 = keys::parse_public_key_base64(KEY1).expect("parse key1");
        let key2 = keys::parse_public_key_base64(KEY2).expect("parse key2");
        let fp1 = KnownHostsManager::get_fingerprint(&key1);
        let fp2 = KnownHostsManager::get_fingerprint(&key2);
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn get_fingerprint_same_key_same_fingerprint() {
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");
        let fp1 = KnownHostsManager::get_fingerprint(&key);
        let fp2 = KnownHostsManager::get_fingerprint(&key);
        assert_eq!(fp1, fp2);
    }

    // === check_host_key tests ===

    #[test]
    fn check_host_key_unknown_when_no_file() {
        let manager = KnownHostsManager::with_paths(None, None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let status = manager.check_host_key("example.com", 22, &key);
        assert!(matches!(status, HostKeyStatus::Unknown { .. }));
    }

    #[test]
    fn check_host_key_unknown_when_file_empty() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, "").expect("write empty file");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let status = manager.check_host_key("example.com", 22, &key);
        assert!(matches!(status, HostKeyStatus::Unknown { .. }));
    }

    #[test]
    fn check_host_key_unknown_when_host_not_found() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("other.com ssh-ed25519 {KEY1}\n")).expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let status = manager.check_host_key("example.com", 22, &key);
        assert!(matches!(status, HostKeyStatus::Unknown { .. }));
    }

    #[test]
    fn check_host_key_non_standard_port_format() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("[example.com]:2222 ssh-ed25519 {KEY1}\n"))
            .expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let status = manager.check_host_key("example.com", 2222, &key);
        assert!(matches!(status, HostKeyStatus::Known));
    }

    #[test]
    fn check_host_key_standard_port_no_brackets() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("example.com ssh-ed25519 {KEY1}\n")).expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        // Port 22 should match without brackets
        let status = manager.check_host_key("example.com", 22, &key);
        assert!(matches!(status, HostKeyStatus::Known));
    }

    #[test]
    fn check_host_key_returns_unknown_with_key_info() {
        let manager = KnownHostsManager::with_paths(None, None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let status = manager.check_host_key("newhost.com", 22, &key);
        if let HostKeyStatus::Unknown {
            fingerprint,
            key_type,
        } = status
        {
            assert!(fingerprint.starts_with("SHA256:"));
            assert_eq!(key_type, "ssh-ed25519");
        } else {
            panic!("Expected Unknown status");
        }
    }

    #[test]
    fn check_host_key_changed_returns_both_fingerprints() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("example.com ssh-ed25519 {KEY1}\n")).expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let new_key = keys::parse_public_key_base64(KEY2).expect("parse key");

        let status = manager.check_host_key("example.com", 22, &new_key);
        if let HostKeyStatus::Changed {
            old_fingerprint,
            new_fingerprint,
            key_type,
        } = status
        {
            assert!(old_fingerprint.starts_with("SHA256:"));
            assert!(new_fingerprint.starts_with("SHA256:"));
            assert_ne!(old_fingerprint, new_fingerprint);
            assert_eq!(key_type, "ssh-ed25519");
        } else {
            panic!("Expected Changed status");
        }
    }

    #[test]
    fn check_host_key_revoked_returns_fingerprint() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("@revoked example.com ssh-ed25519 {KEY1}\n"))
            .expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let status = manager.check_host_key("example.com", 22, &key);
        if let HostKeyStatus::Revoked { fingerprint } = status {
            assert!(fingerprint.starts_with("SHA256:"));
        } else {
            panic!("Expected Revoked status");
        }
    }

    // === File parsing tests ===

    #[test]
    fn scan_ignores_comment_lines() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(
            &path,
            format!("# This is a comment\nexample.com ssh-ed25519 {KEY1}\n# Another comment\n"),
        )
        .expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let status = manager.check_host_key("example.com", 22, &key);
        assert!(matches!(status, HostKeyStatus::Known));
    }

    #[test]
    fn scan_ignores_empty_lines() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("\n\nexample.com ssh-ed25519 {KEY1}\n\n"))
            .expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let status = manager.check_host_key("example.com", 22, &key);
        assert!(matches!(status, HostKeyStatus::Known));
    }

    #[test]
    fn scan_ignores_cert_authority_marker() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(
            &path,
            format!("@cert-authority *.example.com ssh-ed25519 {KEY1}\n"),
        )
        .expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        // cert-authority entries are skipped, so should be Unknown
        let status = manager.check_host_key("test.example.com", 22, &key);
        assert!(matches!(status, HostKeyStatus::Unknown { .. }));
    }

    #[test]
    fn scan_handles_multiple_hosts_per_line() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(
            &path,
            format!("host1.com,host2.com,host3.com ssh-ed25519 {KEY1}\n"),
        )
        .expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        assert!(matches!(
            manager.check_host_key("host1.com", 22, &key),
            HostKeyStatus::Known
        ));
        assert!(matches!(
            manager.check_host_key("host2.com", 22, &key),
            HostKeyStatus::Known
        ));
        assert!(matches!(
            manager.check_host_key("host3.com", 22, &key),
            HostKeyStatus::Known
        ));
    }

    #[test]
    fn scan_reads_from_multiple_files() {
        let dir = tempdir().expect("temp dir");
        let primary = dir.path().join("primary_known_hosts");
        let secondary = dir.path().join("ssh_known_hosts");

        fs::write(&primary, format!("host1.com ssh-ed25519 {KEY1}\n")).expect("write primary");
        fs::write(&secondary, format!("host2.com ssh-ed25519 {KEY1}\n")).expect("write secondary");

        let manager = KnownHostsManager::with_paths(Some(primary), Some(secondary));
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        // Both hosts should be found
        assert!(matches!(
            manager.check_host_key("host1.com", 22, &key),
            HostKeyStatus::Known
        ));
        assert!(matches!(
            manager.check_host_key("host2.com", 22, &key),
            HostKeyStatus::Known
        ));
    }

    // === glob_match tests ===

    #[test]
    fn glob_match_exact() {
        assert!(glob_match("example.com", "example.com"));
        assert!(!glob_match("example.com", "other.com"));
    }

    #[test]
    fn glob_match_star_wildcard() {
        assert!(glob_match("*.example.com", "www.example.com"));
        assert!(glob_match("*.example.com", "mail.example.com"));
        assert!(!glob_match("*.example.com", "example.com"));
        assert!(!glob_match("*.example.com", "other.com"));
    }

    #[test]
    fn glob_match_question_wildcard() {
        assert!(glob_match("host?.com", "host1.com"));
        assert!(glob_match("host?.com", "hosta.com"));
        assert!(!glob_match("host?.com", "host.com"));
        assert!(!glob_match("host?.com", "host12.com"));
    }

    #[test]
    fn glob_match_star_at_end() {
        assert!(glob_match("example*", "example.com"));
        assert!(glob_match("example*", "example"));
        assert!(glob_match("example*", "example123"));
    }

    #[test]
    fn glob_match_star_at_start() {
        assert!(glob_match("*.com", "example.com"));
        assert!(glob_match("*.com", "test.com"));
        assert!(!glob_match("*.com", "example.org"));
    }

    #[test]
    fn glob_match_multiple_stars() {
        assert!(glob_match("*.*", "example.com"));
        assert!(glob_match("*.*.*", "www.example.com"));
    }

    #[test]
    fn glob_match_empty_pattern() {
        assert!(glob_match("", ""));
        assert!(!glob_match("", "text"));
    }

    #[test]
    fn glob_match_only_star() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
        assert!(glob_match("*", "a.b.c.d"));
    }

    #[test]
    fn glob_match_complex_pattern() {
        assert!(glob_match("host-*-prod.*.com", "host-web-prod.example.com"));
        assert!(glob_match("???.example.com", "www.example.com"));
        assert!(!glob_match("???.example.com", "mail.example.com"));
    }

    // === host_matches tests ===

    #[test]
    fn host_matches_exact() {
        assert!(matchers::host_matches(
            "example.com",
            "example.com",
            "example.com"
        ));
        assert!(!matchers::host_matches(
            "other.com",
            "other.com",
            "example.com"
        ));
    }

    #[test]
    fn host_matches_with_port_format() {
        assert!(matchers::host_matches(
            "[example.com]:2222",
            "example.com",
            "[example.com]:2222"
        ));
    }

    #[test]
    fn host_matches_comma_separated() {
        assert!(matchers::host_matches(
            "host2.com",
            "host2.com",
            "host1.com,host2.com,host3.com"
        ));
    }

    #[test]
    fn host_matches_negation() {
        // bad.example.com is negated, so should not match
        assert!(!matchers::host_matches(
            "bad.example.com",
            "bad.example.com",
            "!bad.example.com,*.example.com"
        ));
        // good.example.com matches the wildcard
        assert!(matchers::host_matches(
            "good.example.com",
            "good.example.com",
            "!bad.example.com,*.example.com"
        ));
    }

    #[test]
    fn host_matches_empty_entries_ignored() {
        assert!(matchers::host_matches(
            "example.com",
            "example.com",
            ",example.com,"
        ));
    }

    // === add_host_key / update_host_key tests ===

    #[test]
    fn add_host_key_creates_entry() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");

        let mut manager = KnownHostsManager::with_paths(Some(path.clone()), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        manager
            .add_host_key("newhost.com", 22, &key)
            .expect("add key");

        // Verify the file was created and contains the host
        let content = fs::read_to_string(&path).expect("read file");
        assert!(content.contains("newhost.com"));
    }

    #[test]
    fn add_host_key_fails_with_no_path() {
        let mut manager = KnownHostsManager::with_paths(None, None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let result = manager.add_host_key("newhost.com", 22, &key);
        assert!(result.is_err());
    }

    #[test]
    fn update_host_key_replaces_existing() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("example.com ssh-ed25519 {KEY1}\n")).expect("write initial");

        let mut manager = KnownHostsManager::with_paths(Some(path.clone()), None);
        let new_key = keys::parse_public_key_base64(KEY2).expect("parse new key");

        manager
            .update_host_key("example.com", 22, &new_key)
            .expect("update key");

        // Verify the key changed
        let status = manager.check_host_key("example.com", 22, &new_key);
        assert!(matches!(status, HostKeyStatus::Known));

        // Old key should now be unknown (removed)
        let old_key = keys::parse_public_key_base64(KEY1).expect("parse old key");
        let status = manager.check_host_key("example.com", 22, &old_key);
        assert!(matches!(status, HostKeyStatus::Changed { .. }));
    }

    // === Default trait test ===

    #[test]
    fn default_creates_manager() {
        // Just verify Default trait works (actual paths depend on environment)
        let _manager = KnownHostsManager::default();
    }

    // === IP address handling ===

    #[test]
    fn check_host_key_ipv4() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("192.168.1.1 ssh-ed25519 {KEY1}\n")).expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let status = manager.check_host_key("192.168.1.1", 22, &key);
        assert!(matches!(status, HostKeyStatus::Known));
    }

    #[test]
    fn check_host_key_ipv6() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("known_hosts");
        fs::write(&path, format!("::1 ssh-ed25519 {KEY1}\n")).expect("write known_hosts");

        let manager = KnownHostsManager::with_paths(Some(path), None);
        let key = keys::parse_public_key_base64(KEY1).expect("parse key");

        let status = manager.check_host_key("::1", 22, &key);
        assert!(matches!(status, HostKeyStatus::Known));
    }

    // === Hashed hostname tests ===

    #[test]
    fn match_hashed_host_invalid_format() {
        // Invalid format (not enough parts)
        assert!(!matchers::match_hashed_host("example.com", "|1|"));
        assert!(!matchers::match_hashed_host("example.com", "|1|salt"));
    }

    #[test]
    fn match_hashed_host_invalid_base64() {
        // Invalid base64 in salt or hash
        assert!(!matchers::match_hashed_host(
            "example.com",
            "|1|!!!invalid!!!|hash"
        ));
    }
}
