//! Host key verification tests

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::{Mutex, mpsc};

use portal::config::AuthMethod;
use portal::error::SshError;
use portal::ssh::host_key_verification::{HostKeyVerificationRequest, HostKeyVerificationResponse};
use portal::ssh::{SshClient, SshEvent};

use super::fixtures::SshTestEnvironment;

/// Test that first connection to unknown host triggers verification request
#[tokio::test]
async fn test_unknown_host_prompts_verification() {
    skip_if_no_docker!();

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    // Ensure known_hosts is empty
    assert!(
        !env.known_hosts_path.exists(),
        "known_hosts should not exist initially"
    );

    let host = env.create_test_host(AuthMethod::Password);
    let (event_tx, mut event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let verification_requested = Arc::new(AtomicBool::new(false));
    let verification_requested_clone = verification_requested.clone();

    // Handler that tracks verification requests and accepts them
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let SshEvent::HostKeyVerification(req) = event {
                verification_requested_clone.store(true, Ordering::SeqCst);
                match *req {
                    HostKeyVerificationRequest::NewHost { responder, info } => {
                        // Verify we got the expected host info
                        assert!(
                            !info.fingerprint.is_empty(),
                            "Fingerprint should not be empty"
                        );
                        assert!(!info.key_type.is_empty(), "Key type should not be empty");
                        let _ = responder.send(HostKeyVerificationResponse::Accept);
                    }
                    HostKeyVerificationRequest::ChangedHost { responder, .. } => {
                        let _ = responder.send(HostKeyVerificationResponse::Accept);
                    }
                }
            }
        }
    });

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

    assert!(
        result.is_ok(),
        "Connection should succeed: {:?}",
        result.err()
    );
    assert!(
        verification_requested.load(Ordering::SeqCst),
        "Host key verification should be requested for unknown host"
    );

    // Verify key was saved to known_hosts
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        env.known_hosts_path.exists(),
        "known_hosts should be created after accepting key"
    );
}

/// Test that subsequent connections to known host don't prompt
#[tokio::test]
async fn test_known_host_no_prompt() {
    skip_if_no_docker!();

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let password = SecretString::from(env.server.password.clone());

    // First connection - accept the host key
    {
        let (event_tx, mut event_rx) = mpsc::channel::<SshEvent>(32);
        let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
        let client = SshClient::with_known_hosts(60, known_hosts);

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
        });

        let result = client
            .connect(
                &host,
                (80, 24),
                event_tx,
                Duration::from_secs(10),
                Some(password.clone()),
                None,
                false,
            )
            .await;

        assert!(result.is_ok(), "First connection should succeed");
    }

    // Give time for known_hosts to be written
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Second connection - should NOT prompt
    {
        let (event_tx, mut event_rx) = mpsc::channel::<SshEvent>(32);
        let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
        let client = SshClient::with_known_hosts(60, known_hosts);

        let verification_requested = Arc::new(AtomicBool::new(false));
        let verification_requested_clone = verification_requested.clone();

        tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                if let SshEvent::HostKeyVerification(_) = event {
                    verification_requested_clone.store(true, Ordering::SeqCst);
                }
            }
        });

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

        assert!(
            result.is_ok(),
            "Second connection should succeed: {:?}",
            result.err()
        );

        // Give handler time to process
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(
            !verification_requested.load(Ordering::SeqCst),
            "Should NOT prompt for known host key"
        );
    }
}

/// Test that rejecting host key aborts connection
#[tokio::test]
async fn test_host_key_rejection_aborts() {
    skip_if_no_docker!();

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let (event_tx, mut event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    // Handler that rejects host keys
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let SshEvent::HostKeyVerification(req) = event {
                match *req {
                    HostKeyVerificationRequest::NewHost { responder, .. }
                    | HostKeyVerificationRequest::ChangedHost { responder, .. } => {
                        let _ = responder.send(HostKeyVerificationResponse::Reject);
                    }
                }
            }
        }
    });

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

    assert!(
        result.is_err(),
        "Connection should fail when key is rejected"
    );
    assert!(
        matches!(result.unwrap_err(), SshError::HostKeyVerification(_)),
        "Should be HostKeyVerification error"
    );
}

/// Test that changed host key is detected (MITM scenario)
#[tokio::test]
async fn test_changed_host_key_detection() {
    skip_if_no_docker!();

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    // Pre-populate known_hosts with a FAKE key for the test server
    // This simulates having connected before but the key changed (potential MITM)
    let fake_key_entry = format!(
        "[{}]:{} ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeKeyDataThatDoesNotMatchTheRealKey123\n",
        env.server.host, env.server.port
    );
    fs::write(&env.known_hosts_path, &fake_key_entry).expect("Failed to write fake known_hosts");

    let host = env.create_test_host(AuthMethod::Password);
    let (event_tx, mut event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let changed_detected = Arc::new(AtomicBool::new(false));
    let changed_detected_clone = changed_detected.clone();

    // Handler that detects ChangedHost and rejects (safe behavior)
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            if let SshEvent::HostKeyVerification(req) = event {
                match *req {
                    HostKeyVerificationRequest::NewHost { responder, .. } => {
                        let _ = responder.send(HostKeyVerificationResponse::Accept);
                    }
                    HostKeyVerificationRequest::ChangedHost {
                        responder,
                        old_fingerprint,
                        ..
                    } => {
                        changed_detected_clone.store(true, Ordering::SeqCst);
                        // Verify old fingerprint is provided
                        assert!(
                            !old_fingerprint.is_empty(),
                            "Old fingerprint should be provided"
                        );
                        // Reject the changed key (secure behavior)
                        let _ = responder.send(HostKeyVerificationResponse::Reject);
                    }
                }
            }
        }
    });

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

    // Connection should fail because we rejected the changed key
    assert!(
        result.is_err(),
        "Connection should fail when changed key is rejected"
    );

    assert!(
        changed_detected.load(Ordering::SeqCst),
        "Changed host key should trigger ChangedHost verification"
    );
}

/// Test that accepting changed host key updates known_hosts
#[tokio::test]
async fn test_accepting_changed_key_updates_known_hosts() {
    skip_if_no_docker!();

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    // Pre-populate with fake key
    let fake_key_entry = format!(
        "[{}]:{} ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeKeyDataThatDoesNotMatchTheRealKey456\n",
        env.server.host, env.server.port
    );
    fs::write(&env.known_hosts_path, &fake_key_entry).expect("Failed to write fake known_hosts");

    let original_content = fs::read_to_string(&env.known_hosts_path).unwrap();

    let host = env.create_test_host(AuthMethod::Password);
    let (event_tx, mut event_rx) = mpsc::channel::<SshEvent>(32);

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    // Handler that accepts the changed key
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
    });

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

    assert!(
        result.is_ok(),
        "Connection should succeed when changed key is accepted: {:?}",
        result.err()
    );

    // Give time for known_hosts to be updated
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Verify known_hosts was updated (content should be different)
    let new_content = fs::read_to_string(&env.known_hosts_path).unwrap();
    assert_ne!(
        original_content, new_content,
        "known_hosts should be updated with new key"
    );
    // The fake key should no longer be present
    assert!(
        !new_content.contains("FakeKeyData"),
        "Old fake key should be replaced"
    );
}
