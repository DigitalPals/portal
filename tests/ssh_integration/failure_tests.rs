//! SSH failure-injection tests.

use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::{Mutex, mpsc};

use portal::config::AuthMethod;
use portal::ssh::{SshClient, SshEvent};

use super::connection_tests::spawn_host_key_handler;
use super::fixtures::{SshTestEnvironment, wait_for_ssh_ready};

fn restart_test_ssh_container() -> Result<(), String> {
    let status = Command::new("docker")
        .args(["restart", "portal-ssh-test"])
        .status()
        .map_err(|error| format!("failed to restart SSH test container: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "failed to restart SSH test container: exit code {:?}",
            status.code()
        ))
    }
}

#[tokio::test]
async fn test_stale_pooled_connection_is_invalidated_after_server_restart() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let (event_tx1, event_rx1) = mpsc::channel::<SshEvent>(64);
    let (_handler1, _accept_count1, _connected1) = spawn_host_key_handler(event_rx1);
    let password = SecretString::from(env.server.password.clone());
    let (session1, _detected_os1) = client
        .connect(
            &host,
            (80, 24),
            event_tx1,
            Duration::from_secs(10),
            Some(password),
            None,
            false,
            false,
        )
        .await
        .expect("initial SSH connect should succeed");
    drop(session1);

    restart_test_ssh_container().expect("SSH test container restart should succeed");
    wait_for_ssh_ready(&env.server.host, env.server.port)
        .await
        .expect("SSH test container should become ready after restart");

    let (event_tx2, event_rx2) = mpsc::channel::<SshEvent>(64);
    let (_handler2, _accept_count2, _connected2) = spawn_host_key_handler(event_rx2);
    let password = SecretString::from(env.server.password.clone());
    let (session2, _detected_os2) = client
        .connect(
            &host,
            (80, 24),
            event_tx2,
            Duration::from_secs(10),
            Some(password),
            None,
            false,
            false,
        )
        .await
        .expect("SSH connect should recover after stale pooled transport is invalidated");

    let output = session2
        .execute_command("printf recovered")
        .await
        .expect("recovered SSH session should execute commands");
    assert_eq!(output, "recovered");
}
