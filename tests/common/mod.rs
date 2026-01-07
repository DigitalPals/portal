//! Common test utilities

use std::path::PathBuf;
use tempfile::TempDir;

/// Test environment with isolated configuration directory
pub struct TestEnvironment {
    pub config_dir: TempDir,
    pub known_hosts_path: PathBuf,
}

impl TestEnvironment {
    pub fn new() -> Self {
        let config_dir = TempDir::new().expect("Failed to create temp dir");
        let known_hosts_path = config_dir.path().join("known_hosts");
        Self {
            config_dir,
            known_hosts_path,
        }
    }
}

impl Default for TestEnvironment {
    fn default() -> Self {
        Self::new()
    }
}
