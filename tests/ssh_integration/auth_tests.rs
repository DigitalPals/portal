//! Authentication failure tests

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::{Mutex, mpsc};

use portal::config::AuthMethod;
use portal::error::SshError;
use portal::ssh::host_key_verification::{HostKeyVerificationRequest, HostKeyVerificationResponse};
use portal::ssh::{SshClient, SshEvent};

use super::fixtures::SshTestEnvironment;

/// Helper to spawn a task that auto-accepts host keys
fn spawn_auto_accept_handler(
    mut event_rx: mpsc::Receiver<SshEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let SshEvent::HostKeyVerification(req) = event {
                match *req {
                    HostKeyVerificationRequest::NewHost { responder, .. }
                    | HostKeyVerificationRequest::ChangedHost { responder, .. } => {
                        let _ = responder.send(HostKeyVerificationResponse::Accept);
                    }
                }
            }
        }
    })
}

/// Test wrong password authentication fails
#[tokio::test]
async fn test_wrong_password() {
    skip_if_no_docker!();

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let _handler = spawn_auto_accept_handler(event_rx);

    let wrong_password = SecretString::from("wrongpassword".to_string());
    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            Some(wrong_password),
            None,
            false,
        )
        .await;

    assert!(result.is_err(), "Should fail with wrong password");
    assert!(
        matches!(result.unwrap_err(), SshError::AuthenticationFailed(_)),
        "Should be AuthenticationFailed error"
    );
}

/// Test non-existent user fails authentication
#[tokio::test]
async fn test_nonexistent_user() {
    skip_if_no_docker!();

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let mut host = env.create_test_host(AuthMethod::Password);
    host.username = "nonexistent_user_12345".to_string();

    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let _handler = spawn_auto_accept_handler(event_rx);

    let password = SecretString::from(env.server.password.clone());
    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            Some(password),
            None,
            false,
        )
        .await;

    assert!(result.is_err(), "Should fail with non-existent user");
    assert!(
        matches!(result.unwrap_err(), SshError::AuthenticationFailed(_)),
        "Should be AuthenticationFailed error"
    );
}

/// Test invalid key file path fails
#[tokio::test]
async fn test_invalid_key_path() {
    skip_if_no_docker!();

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::PublicKey {
        key_path: Some(PathBuf::from("/nonexistent/path/to/key")),
    });

    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let _handler = spawn_auto_accept_handler(event_rx);

    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            None,
            None,
            false,
        )
        .await;

    assert!(result.is_err(), "Should fail with invalid key path");
    assert!(
        matches!(result.unwrap_err(), SshError::KeyFile(_)),
        "Should be KeyFile error"
    );
}

/// Test wrong passphrase for encrypted key fails
#[tokio::test]
async fn test_wrong_passphrase() {
    skip_if_no_docker!();

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::PublicKey {
        key_path: Some(env.server.encrypted_key_path.clone()),
    });

    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let _handler = spawn_auto_accept_handler(event_rx);

    let wrong_passphrase = SecretString::from("wrongpassphrase".to_string());
    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            None,
            Some(wrong_passphrase),
            false,
        )
        .await;

    assert!(result.is_err(), "Should fail with wrong passphrase");
    match result.unwrap_err() {
        SshError::KeyFilePassphraseInvalid(path) => {
            assert_eq!(path, env.server.encrypted_key_path);
        }
        err => panic!("Expected KeyFilePassphraseInvalid, got: {:?}", err),
    }
}

/// Test connection refused on wrong port
#[tokio::test]
async fn test_connection_refused() {
    skip_if_no_docker!();

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let mut host = env.create_test_host(AuthMethod::Password);
    host.port = 29999; // Wrong port

    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let _handler = spawn_auto_accept_handler(event_rx);

    let password = SecretString::from(env.server.password.clone());
    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(5),
            Some(password),
            None,
            false,
        )
        .await;

    assert!(result.is_err(), "Should fail with wrong port");
    assert!(
        matches!(result.unwrap_err(), SshError::ConnectionFailed { .. }),
        "Should be ConnectionFailed error"
    );
}

/// Test connection timeout to non-routable address
#[tokio::test]
async fn test_connection_timeout() {
    // Use a non-routable IP to trigger timeout
    let host = portal::config::Host {
        id: uuid::Uuid::new_v4(),
        name: "Timeout Test".to_string(),
        hostname: "10.255.255.1".to_string(), // Non-routable
        port: 22,
        username: "testuser".to_string(),
        auth: AuthMethod::Password,
        group_id: None,
        notes: None,
        tags: vec![],
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
        detected_os: None,
        last_connected: None,
    };

    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(
        portal::ssh::known_hosts::KnownHostsManager::with_paths(None, None),
    ));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let _handler = spawn_auto_accept_handler(event_rx);

    let password = SecretString::from("anypassword".to_string());
    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(2), // Short timeout
            Some(password),
            None,
            false,
        )
        .await;

    assert!(result.is_err(), "Should timeout");
    assert!(
        matches!(result.unwrap_err(), SshError::Timeout(_)),
        "Should be Timeout error"
    );
}
