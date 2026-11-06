//! Opt-in real OpenSSH/SFTP/sudo tests. The harness owns a disposable fixture.
use super::*;
use crate::ssh::{
    auth_prompt::AuthPromptResponse,
    host_key_verification::{HostKeyVerificationRequest, HostKeyVerificationResponse},
    known_hosts::KnownHostsManager,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Mutex;

fn fixture_host(username: &str) -> Host {
    serde_json::from_value(serde_json::json!({
        "id": Uuid::new_v4(), "name": format!("Fixture {username}"),
        "hostname": "127.0.0.1", "port": std::env::var("PORTAL_MANAGEMENT_TEST_PORT").unwrap().parse::<u16>().unwrap(),
        "username": username, "auth": {"type":"public_key", "key_path":std::env::var("PORTAL_MANAGEMENT_TEST_KEY").unwrap()},
        "created_at":"2026-01-01T00:00:00Z", "updated_at":"2026-01-01T00:00:00Z"
    })).unwrap()
}

#[tokio::test]
#[ignore = "run scripts/test-hub-ssh.py to provision the leased SSH fixture"]
async fn real_management_ssh_sftp_sudo_and_jump_chain() {
    let known = tempfile::tempdir().unwrap();
    let known_hosts = Arc::new(Mutex::new(KnownHostsManager::with_paths(
        Some(known.path().join("known_hosts")),
        None,
    )));
    let expected_fingerprint = std::env::var("PORTAL_MANAGEMENT_TEST_FINGERPRINT").unwrap();
    let prompts = Arc::new(AtomicUsize::new(0));
    let seen_keys = Arc::new(AtomicUsize::new(0));
    let (events, mut receiver) = mpsc::channel(16);
    let prompt_count = prompts.clone();
    let key_count = seen_keys.clone();
    let responder = tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            match event {
                SshEvent::HostKeyVerification(request) => match *request {
                    HostKeyVerificationRequest::NewHost { info, responder } => {
                        assert_eq!(info.fingerprint, expected_fingerprint);
                        key_count.fetch_add(1, Ordering::SeqCst);
                        responder.send(HostKeyVerificationResponse::Accept).unwrap();
                    }
                    HostKeyVerificationRequest::ChangedHost { .. } => {
                        panic!("Fixture host key changed")
                    }
                },
                SshEvent::AuthPrompt(request) => {
                    assert_eq!(request.username, "portalpass");
                    assert_eq!(request.prompts.len(), 1);
                    assert!(!request.prompts[0].echo);
                    assert_eq!(request.prompts[0].prompt, "Sudo password");
                    prompt_count.fetch_add(1, Ordering::SeqCst);
                    request
                        .responder
                        .send(AuthPromptResponse::Submit(vec![SecretString::from(
                            "portal-fixture-password",
                        )]))
                        .unwrap();
                }
                _ => {}
            }
        }
    });
    for (user, jump) in [
        ("root", false),
        ("portalnopass", false),
        ("portalpass", false),
        ("portalpass", true),
    ] {
        let mut host = fixture_host(user);
        let chain = if jump {
            let bastion = fixture_host("portalnopass");
            host.port = 22; // loopback as seen by the bastion container
            vec![bastion]
        } else {
            vec![]
        };
        let connection = ManagementConnection::connect(&host, &chain, known_hosts.clone(), &events)
            .await
            .unwrap();
        let before = prompts.load(Ordering::SeqCst);
        // Use the exact production loader and complete embedded helper. The
        // unprivileged cwd contains a hostile hashlib.py to test isolated imports.
        let request = serde_json::to_vec(&serde_json::json!({"protocol_version":1,"action":"inspect","deployment_id":Uuid::new_v4()})).unwrap();
        let input = helper_input(HELPER, &request);
        let output = if user == "root" {
            connection
                .execute(&helper_command(HELPER), &input)
                .await
                .unwrap()
        } else {
            connection
                .execute_sudo(&helper_command(HELPER), &input, &host, &events)
                .await
                .unwrap()
        };
        let response = DeploymentResponse::parse(&output.stdout, &SecretString::default()).unwrap();
        assert!(response.ok, "{}", response.message);
        assert!(!response.bool("installed"));
        assert_eq!(
            prompts.load(Ordering::SeqCst) - before,
            usize::from(user == "portalpass")
        );
        // Exercise the same SFTP subsystem and exclusive file flags as uploads.
        let channel = connection.channel().await.unwrap();
        channel.request_subsystem(true, "sftp").await.unwrap();
        let sftp = russh_sftp::client::SftpSession::new(channel.into_stream())
            .await
            .unwrap();
        let path = format!("/tmp/portal-transport-{}", Uuid::new_v4());
        let mut file = sftp
            .open_with_flags(
                &path,
                russh_sftp::protocol::OpenFlags::WRITE
                    | russh_sftp::protocol::OpenFlags::CREATE
                    | russh_sftp::protocol::OpenFlags::EXCLUDE,
            )
            .await
            .unwrap();
        let bytes = b"arbitrary image bytes\0with binary content\n";
        file.write_all(bytes).await.unwrap();
        file.shutdown().await.unwrap();
        let checksum = connection
            .execute(&format!("sha256sum -- {path}"), b"")
            .await
            .unwrap();
        assert!(
            checksum
                .stdout
                .starts_with(&format!("{:x}", Sha256::digest(bytes)))
        );
        sftp.remove_file(&path).await.unwrap();
        sftp.close().await.unwrap();
        connection.disconnect().await;
    }
    assert_eq!(
        seen_keys.load(Ordering::SeqCst),
        2,
        "Direct and bastion target endpoints are verified independently"
    );
    responder.abort();
    let result = responder.await;
    assert!(result.is_err_and(|error| error.is_cancelled()));
}
