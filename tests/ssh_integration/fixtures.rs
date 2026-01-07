//! Docker-based SSH test fixtures

use std::path::PathBuf;
use std::process::Command;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tempfile::TempDir;
use tokio::net::TcpStream;
use tokio::time::{sleep, timeout};

use portal::config::{AuthMethod, Host};
use portal::ssh::known_hosts::KnownHostsManager;

// Ensure Docker containers are started only once per test run
static DOCKER_INIT: Once = Once::new();
static DOCKER_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// Configuration for the test SSH server
#[derive(Debug, Clone)]
pub struct TestSshServer {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub private_key_path: PathBuf,
    pub encrypted_key_path: PathBuf,
    pub key_passphrase: String,
}

impl Default for TestSshServer {
    fn default() -> Self {
        let test_keys_dir =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/docker/test_keys");
        Self {
            host: "127.0.0.1".to_string(),
            port: 2222,
            username: "testuser".to_string(),
            password: "testpass123".to_string(),
            private_key_path: test_keys_dir.join("id_ed25519"),
            encrypted_key_path: test_keys_dir.join("id_ed25519_encrypted"),
            key_passphrase: "testpassphrase".to_string(),
        }
    }
}

/// Start Docker containers for SSH testing
pub fn ensure_docker_started() {
    DOCKER_INIT.call_once(|| {
        let docker_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/docker");

        // Check if Docker is available
        let docker_check = Command::new("docker").arg("--version").output();

        if docker_check.is_err() {
            eprintln!("WARNING: Docker not available, SSH integration tests will be skipped");
            return;
        }

        // Check if docker-compose or docker compose is available
        let compose_cmd = if Command::new("docker-compose")
            .arg("--version")
            .output()
            .is_ok()
        {
            "docker-compose"
        } else if Command::new("docker")
            .args(["compose", "version"])
            .output()
            .is_ok()
        {
            "docker"
        } else {
            eprintln!("WARNING: docker-compose not available");
            return;
        };

        // Build and start containers
        let status = if compose_cmd == "docker" {
            Command::new("docker")
                .current_dir(&docker_dir)
                .args(["compose", "up", "-d", "--build", "--wait"])
                .status()
        } else {
            Command::new(compose_cmd)
                .current_dir(&docker_dir)
                .args(["up", "-d", "--build", "--wait"])
                .status()
        };

        match status {
            Ok(s) if s.success() => {
                DOCKER_AVAILABLE.store(true, Ordering::SeqCst);
                eprintln!("SSH test containers started successfully");
            }
            Ok(s) => {
                eprintln!(
                    "Failed to start SSH test containers: exit code {:?}",
                    s.code()
                );
            }
            Err(e) => {
                eprintln!("Failed to start SSH test containers: {}", e);
            }
        }
    });
}

/// Check if Docker containers are running
pub fn is_docker_available() -> bool {
    ensure_docker_started();
    DOCKER_AVAILABLE.load(Ordering::SeqCst)
}

/// Wait for SSH server to be ready
pub async fn wait_for_ssh_ready(host: &str, port: u16) -> Result<(), String> {
    let addr = format!("{}:{}", host, port);
    let max_attempts = 30;

    for attempt in 1..=max_attempts {
        match timeout(Duration::from_secs(2), TcpStream::connect(&addr)).await {
            Ok(Ok(_)) => return Ok(()),
            _ => {
                if attempt == max_attempts {
                    return Err(format!(
                        "SSH server not ready after {} attempts",
                        max_attempts
                    ));
                }
                sleep(Duration::from_millis(200)).await;
            }
        }
    }

    Err("SSH server not ready".to_string())
}

/// Test environment with isolated known_hosts and Docker fixtures
pub struct SshTestEnvironment {
    pub server: TestSshServer,
    pub config_dir: TempDir,
    pub known_hosts_path: PathBuf,
}

impl SshTestEnvironment {
    pub async fn new() -> Result<Self, String> {
        if !is_docker_available() {
            return Err("Docker not available".to_string());
        }

        let server = TestSshServer::default();
        wait_for_ssh_ready(&server.host, server.port).await?;

        let config_dir = TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;
        let known_hosts_path = config_dir.path().join("known_hosts");

        Ok(Self {
            server,
            config_dir,
            known_hosts_path,
        })
    }

    /// Create a Host configuration for testing with the specified auth method
    pub fn create_test_host(&self, auth: AuthMethod) -> Host {
        Host {
            id: uuid::Uuid::new_v4(),
            name: "Test Host".to_string(),
            hostname: self.server.host.clone(),
            port: self.server.port,
            username: self.server.username.clone(),
            auth,
            group_id: None,
            notes: None,
            tags: vec![],
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
            detected_os: None,
            last_connected: None,
        }
    }

    /// Create a KnownHostsManager with isolated known_hosts file
    pub fn create_known_hosts_manager(&self) -> KnownHostsManager {
        KnownHostsManager::with_paths(Some(self.known_hosts_path.clone()), None)
    }
}

/// Macro to skip tests when Docker is not available
#[macro_export]
macro_rules! skip_if_no_docker {
    () => {
        if !super::fixtures::is_docker_available() {
            eprintln!("Skipping test: Docker not available");
            return;
        }
    };
}
