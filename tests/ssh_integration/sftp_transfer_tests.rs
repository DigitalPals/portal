//! SFTP transfer integration tests.

use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use portal::config::AuthMethod;
use portal::sftp::SftpClient;
use portal::ssh::SshEvent;

use super::connection_tests::spawn_host_key_handler;
use super::fixtures::SshTestEnvironment;

#[tokio::test]
async fn test_sftp_upload_download_replaces_via_staging_paths() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let host = env.create_test_host(AuthMethod::Password);
    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SftpClient::with_known_hosts(60, known_hosts);

    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(64);
    let (_handler, _accept_count, _connected) = spawn_host_key_handler(event_rx);
    let password = SecretString::from(env.server.password.clone());
    let sftp = client
        .connect(
            &host,
            event_tx,
            Duration::from_secs(10),
            Some(password),
            None,
        )
        .await
        .expect("SFTP connect should succeed");

    let id = Uuid::new_v4();
    let remote_path = sftp
        .home_dir()
        .join(format!("portal-sftp-transfer-{id}.txt"));
    let upload_path = env.config_dir.path().join("upload.txt");
    let download_path = env.config_dir.path().join("download.txt");

    let first = b"first staged upload\n";
    tokio::fs::write(&upload_path, first)
        .await
        .expect("local upload fixture should be written");
    let uploaded = sftp
        .upload(&upload_path, &remote_path)
        .await
        .expect("initial upload should succeed");
    assert_eq!(uploaded, first.len() as u64);

    let second = b"replacement upload with a different length\n";
    tokio::fs::write(&upload_path, second)
        .await
        .expect("local replacement fixture should be written");
    let uploaded = sftp
        .upload(&upload_path, &remote_path)
        .await
        .expect("replacement upload should succeed");
    assert_eq!(uploaded, second.len() as u64);

    let downloaded = sftp
        .download(&remote_path, &download_path)
        .await
        .expect("download should succeed");
    assert_eq!(downloaded, second.len() as u64);
    let content = tokio::fs::read(&download_path)
        .await
        .expect("downloaded file should be readable");
    assert_eq!(content, second);

    let entries = sftp
        .list_dir(sftp.home_dir())
        .await
        .expect("home directory should be listable");
    assert!(
        !entries.iter().any(|entry| {
            entry.name.contains(".portal-part-") || entry.name.contains(".portal-backup-")
        }),
        "staging files should be cleaned up after completed transfers"
    );

    sftp.remove_file(&remote_path)
        .await
        .expect("remote test file should be removed");
}
