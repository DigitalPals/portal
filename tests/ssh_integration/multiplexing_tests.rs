//! SSH connection multiplexing tests

use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::{Mutex, mpsc};

use portal::config::AuthMethod;
use portal::sftp::SftpClient;
use portal::ssh::{SshClient, SshEvent};

use super::connection_tests::spawn_host_key_handler;
use super::fixtures::SshTestEnvironment;

#[tokio::test]
async fn test_reuse_connection_ignores_bad_password_for_second_session() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    // Session 1 (creates the underlying SSH connection).
    let (event_tx1, event_rx1) = mpsc::channel::<SshEvent>(64);
    let (_handler1, _accept_count1, _connected1) = spawn_host_key_handler(event_rx1);
    let password_ok = SecretString::from(env.server.password.clone());
    let (session1, _detected_os1) = client
        .connect(
            &host,
            (80, 24),
            event_tx1,
            Duration::from_secs(10),
            Some(password_ok),
            None,
            false,
            false,
        )
        .await
        .expect("first SSH connect should succeed");

    // Session 2 (should reuse the existing connection and succeed even with a bad password).
    let (event_tx2, event_rx2) = mpsc::channel::<SshEvent>(64);
    let (_handler2, _accept_count2, _connected2) = spawn_host_key_handler(event_rx2);
    let password_bad = SecretString::from("wrong-password".to_string());
    let result = client
        .connect(
            &host,
            (80, 24),
            event_tx2,
            Duration::from_secs(10),
            Some(password_bad),
            None,
            false,
            false,
        )
        .await;

    assert!(
        result.is_ok(),
        "second SSH connect should reuse pooled connection (and not re-auth): {:?}",
        result.err()
    );

    // Keep both sessions alive briefly to ensure channels are established.
    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(session1);
    drop(result.unwrap().0);
}

#[tokio::test]
async fn test_ssh_and_sftp_share_connection_pool() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));

    // Establish the pooled SSH connection first.
    let ssh_client = SshClient::with_known_hosts(60, known_hosts.clone());
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(64);
    let (_handler, _accept_count, _connected) = spawn_host_key_handler(event_rx);
    let password_ok = SecretString::from(env.server.password.clone());
    let (ssh_session, _detected_os) = ssh_client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            Some(password_ok),
            None,
            false,
            false,
        )
        .await
        .expect("SSH connect should succeed");

    // Now connect SFTP with a bad password; it should still succeed by reusing the pooled SSH
    // connection (no re-auth).
    let sftp_client = SftpClient::with_known_hosts(60, known_hosts);
    let (sftp_event_tx, _sftp_event_rx) = mpsc::channel::<SshEvent>(16);
    let password_bad = SecretString::from("wrong-password".to_string());
    let result = sftp_client
        .connect(
            &host,
            sftp_event_tx,
            Duration::from_secs(10),
            Some(password_bad),
            None,
        )
        .await;

    assert!(
        result.is_ok(),
        "SFTP connect should reuse pooled SSH connection: {:?}",
        result.err()
    );

    tokio::time::sleep(Duration::from_millis(100)).await;
    drop(ssh_session);
    drop(result.unwrap());
}
