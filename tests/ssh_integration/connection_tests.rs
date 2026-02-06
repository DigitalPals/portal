//! SSH connection tests

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::{Mutex, mpsc};

use portal::config::AuthMethod;
use portal::error::SshError;
use portal::ssh::host_key_verification::{HostKeyVerificationRequest, HostKeyVerificationResponse};
use portal::ssh::{SshClient, SshEvent};

use super::fixtures::SshTestEnvironment;

/// Helper to spawn a task that auto-accepts host keys and tracks verification requests
fn spawn_host_key_handler(
    mut event_rx: mpsc::Receiver<SshEvent>,
) -> (
    tokio::task::JoinHandle<()>,
    Arc<AtomicUsize>,
    Arc<AtomicBool>,
) {
    let accept_count = Arc::new(AtomicUsize::new(0));
    let connected = Arc::new(AtomicBool::new(false));
    let accept_count_clone = accept_count.clone();
    let connected_clone = connected.clone();

    let handle = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                SshEvent::HostKeyVerification(req) => {
                    accept_count_clone.fetch_add(1, Ordering::SeqCst);
                    match *req {
                        HostKeyVerificationRequest::NewHost { responder, .. } => {
                            let _ = responder.send(HostKeyVerificationResponse::Accept);
                        }
                        HostKeyVerificationRequest::ChangedHost { responder, .. } => {
                            let _ = responder.send(HostKeyVerificationResponse::Accept);
                        }
                    }
                }
                SshEvent::Connected => {
                    connected_clone.store(true, Ordering::SeqCst);
                }
                SshEvent::Disconnected => {
                    break;
                }
                SshEvent::Data(_) => {}
            }
        }
    });

    (handle, accept_count, connected)
}

/// Test successful connection with password authentication
#[tokio::test]
async fn test_password_auth_success() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let (_handler, _accept_count, connected) = spawn_host_key_handler(event_rx);

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
            false, // No agent forwarding in tests
        )
        .await;

    assert!(
        result.is_ok(),
        "Password auth should succeed: {:?}",
        result.err()
    );

    // Give the handler time to process the Connected event
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        connected.load(Ordering::SeqCst),
        "Should have received Connected event"
    );
}

/// Test successful connection with public key authentication
#[tokio::test]
async fn test_pubkey_auth_success() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::PublicKey {
        key_path: Some(env.server.private_key_path.clone()),
    });
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let (_handler, _accept_count, connected) = spawn_host_key_handler(event_rx);

    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            None,
            None,
            false,
            false, // No agent forwarding in tests
        )
        .await;

    assert!(
        result.is_ok(),
        "Public key auth should succeed: {:?}",
        result.err()
    );

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        connected.load(Ordering::SeqCst),
        "Should have received Connected event"
    );
}

/// Test connection with encrypted key and correct passphrase
#[tokio::test]
async fn test_encrypted_key_with_passphrase() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::PublicKey {
        key_path: Some(env.server.encrypted_key_path.clone()),
    });
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let (_handler, _, connected) = spawn_host_key_handler(event_rx);

    let passphrase = SecretString::from(env.server.key_passphrase.clone());
    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            None,
            Some(passphrase),
            false,
            false, // No agent forwarding in tests
        )
        .await;

    assert!(
        result.is_ok(),
        "Encrypted key auth should succeed with passphrase: {:?}",
        result.err()
    );

    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        connected.load(Ordering::SeqCst),
        "Should have received Connected event"
    );
}

/// Test encrypted key without passphrase returns appropriate error
#[tokio::test]
async fn test_encrypted_key_without_passphrase() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::PublicKey {
        key_path: Some(env.server.encrypted_key_path.clone()),
    });
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let (_handler, _, _) = spawn_host_key_handler(event_rx);

    // No passphrase provided for encrypted key
    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            None,
            None,
            false,
            false, // No agent forwarding in tests
        )
        .await;

    assert!(result.is_err(), "Should fail without passphrase");
    match result.unwrap_err() {
        SshError::KeyFilePassphraseRequired(path) => {
            assert_eq!(path, env.server.encrypted_key_path);
        }
        err => panic!("Expected KeyFilePassphraseRequired, got: {:?}", err),
    }
}

/// Test that session returns OS detection when requested
#[tokio::test]
async fn test_os_detection_on_connect() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let (_handler, _, _) = spawn_host_key_handler(event_rx);

    let password = SecretString::from(env.server.password.clone());
    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            Some(password),
            None,
            true,  // Enable OS detection
            false, // No agent forwarding in tests
        )
        .await;

    assert!(
        result.is_ok(),
        "Connection should succeed: {:?}",
        result.err()
    );

    let (_session, detected_os) = result.unwrap();
    // Alpine container should be detected
    assert!(
        detected_os.is_some(),
        "OS should be detected for Alpine container"
    );
}
