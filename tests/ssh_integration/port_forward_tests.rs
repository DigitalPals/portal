//! SSH port forwarding tests (-L and -D)

use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc};

use portal::config::{AuthMethod, PortForward, PortForwardKind};
use portal::ssh::host_key_verification::{HostKeyVerificationRequest, HostKeyVerificationResponse};
use portal::ssh::{SshClient, SshEvent};

use super::fixtures::SshTestEnvironment;

async fn accept_host_keys(mut event_rx: mpsc::Receiver<SshEvent>) {
    while let Some(event) = event_rx.recv().await {
        if let SshEvent::HostKeyVerification(req) = event {
            match *req {
                HostKeyVerificationRequest::NewHost { responder, .. } => {
                    let _ = responder.send(HostKeyVerificationResponse::Accept);
                }
                HostKeyVerificationRequest::ChangedHost { responder, .. } => {
                    let _ = responder.send(HostKeyVerificationResponse::Accept);
                }
            }
        }
    }
}

async fn find_free_local_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

#[tokio::test]
async fn test_local_forward_to_remote_sshd_banner() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(64);
    tokio::spawn(accept_host_keys(event_rx));

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let password = SecretString::from(env.server.password.clone());
    let (session, _detected_os) = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            Some(password),
            None,
            false,
            false, // no agent forwarding in tests
        )
        .await
        .expect("SSH connect failed");

    let bind_port = find_free_local_port().await;
    let forward = PortForward {
        id: uuid::Uuid::new_v4(),
        kind: PortForwardKind::Local,
        bind_host: "127.0.0.1".to_string(),
        bind_port,
        target_host: "127.0.0.1".to_string(),
        target_port: 22,
        enabled: true,
        description: Some("test local forward".to_string()),
    };

    session
        .create_local_forward(forward.clone())
        .await
        .expect("create_local_forward failed");

    let mut stream = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect(("127.0.0.1", bind_port)),
    )
    .await
    .expect("connect timeout")
    .expect("connect failed");

    let mut buf = [0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut buf))
        .await
        .expect("read timeout")
        .expect("read failed");
    let banner = String::from_utf8_lossy(&buf[..n]);
    assert!(
        banner.starts_with("SSH-"),
        "expected SSH banner, got: {:?}",
        banner
    );

    session
        .stop_forward(forward.id)
        .await
        .expect("stop_forward failed");
}

#[tokio::test]
async fn test_dynamic_forward_socks5_to_remote_sshd_banner() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(64);
    tokio::spawn(accept_host_keys(event_rx));

    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let password = SecretString::from(env.server.password.clone());
    let (session, _detected_os) = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            Some(password),
            None,
            false,
            false, // no agent forwarding in tests
        )
        .await
        .expect("SSH connect failed");

    let bind_port = find_free_local_port().await;
    let forward = PortForward {
        id: uuid::Uuid::new_v4(),
        kind: PortForwardKind::Dynamic,
        bind_host: "127.0.0.1".to_string(),
        bind_port,
        // Ignored for dynamic (-D)
        target_host: "socks".to_string(),
        target_port: 0,
        enabled: true,
        description: Some("test dynamic forward".to_string()),
    };

    session
        .create_dynamic_forward(forward.clone())
        .await
        .expect("create_dynamic_forward failed");

    let mut socks = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect(("127.0.0.1", bind_port)),
    )
    .await
    .expect("connect timeout")
    .expect("connect failed");

    // SOCKS5 greeting: VER=5, NMETHODS=1, METHODS=[NOAUTH]
    socks.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    socks.flush().await.unwrap();
    let mut method_select = [0u8; 2];
    socks.read_exact(&mut method_select).await.unwrap();
    assert_eq!(method_select, [0x05, 0x00]);

    // SOCKS5 CONNECT to 127.0.0.1:22
    let mut req = Vec::new();
    req.extend_from_slice(&[0x05, 0x01, 0x00, 0x01, 127, 0, 0, 1]);
    req.extend_from_slice(&22u16.to_be_bytes());
    socks.write_all(&req).await.unwrap();
    socks.flush().await.unwrap();

    let mut reply = [0u8; 10];
    socks.read_exact(&mut reply).await.unwrap();
    assert_eq!(&reply[..4], &[0x05, 0x00, 0x00, 0x01]);

    let mut buf = [0u8; 64];
    let n = tokio::time::timeout(Duration::from_secs(3), socks.read(&mut buf))
        .await
        .expect("read timeout")
        .expect("read failed");
    let banner = String::from_utf8_lossy(&buf[..n]);
    assert!(
        banner.starts_with("SSH-"),
        "expected SSH banner, got: {:?}",
        banner
    );

    session
        .stop_forward(forward.id)
        .await
        .expect("stop_forward failed");
}

