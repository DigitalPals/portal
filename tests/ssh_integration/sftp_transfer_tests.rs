//! SFTP transfer integration tests.

use std::sync::Arc;
use std::time::Duration;

use secrecy::SecretString;
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;

use portal::config::AuthMethod;
use portal::sftp::{SftpClient, SharedSftpSession};
use portal::ssh::{SshClient, SshEvent, SshSession};

use super::connection_tests::spawn_host_key_handler;
use super::fixtures::SshTestEnvironment;

async fn connect_password_sftp(env: &SshTestEnvironment) -> SharedSftpSession {
    let host = env.create_test_host(AuthMethod::Password);
    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SftpClient::with_known_hosts(60, known_hosts);

    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(64);
    let (_handler, _accept_count, _connected) = spawn_host_key_handler(event_rx);
    let password = SecretString::from(env.server.password.clone());
    client
        .connect(
            &host,
            event_tx,
            Duration::from_secs(10),
            Some(password),
            None,
        )
        .await
        .expect("SFTP connect should succeed")
}

async fn connect_password_ssh(env: &SshTestEnvironment) -> Arc<SshSession> {
    let host = env.create_test_host(AuthMethod::Password);
    let known_hosts = Arc::new(Mutex::new(env.create_known_hosts_manager()));
    let client = SshClient::with_known_hosts(60, known_hosts);

    let (event_tx, event_rx) = mpsc::channel::<SshEvent>(64);
    let (_handler, _accept_count, _connected) = spawn_host_key_handler(event_rx);
    let password = SecretString::from(env.server.password.clone());
    let (session, _) = client
        .connect(
            &host,
            (80, 24),
            event_tx,
            Duration::from_secs(10),
            Some(password),
            None,
            false,
            false,
        )
        .await
        .expect("SSH connect should succeed");
    session
}

fn shell_quote(path: &std::path::Path) -> String {
    let value = path.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[tokio::test]
async fn test_sftp_upload_download_replaces_via_staging_paths() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let sftp = connect_password_sftp(&env).await;

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

#[tokio::test]
async fn test_sftp_create_dir_rejects_existing_but_recursive_upload_reuses_it() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let sftp = connect_password_sftp(&env).await;

    let id = Uuid::new_v4();
    let remote_dir = sftp.home_dir().join(format!("portal-sftp-dir-{id}"));
    let local_dir = env.config_dir.path().join(format!("upload-dir-{id}"));
    let local_file = local_dir.join("file.txt");

    tokio::fs::create_dir(&local_dir)
        .await
        .expect("local upload directory should be created");
    tokio::fs::write(&local_file, b"content")
        .await
        .expect("local upload file should be written");

    sftp.create_dir(&remote_dir)
        .await
        .expect("initial remote directory create should succeed");
    let error = sftp
        .create_dir(&remote_dir)
        .await
        .expect_err("creating an existing remote directory should fail");
    assert!(error.to_string().contains("already exists"));

    let uploaded = sftp
        .upload_recursive(&local_dir, &remote_dir)
        .await
        .expect("recursive upload should reuse existing remote directory");
    assert_eq!(uploaded, 1);

    let remote_file = remote_dir.join("file.txt");
    let download_path = env.config_dir.path().join(format!("download-{id}.txt"));
    sftp.download(&remote_file, &download_path)
        .await
        .expect("uploaded file should be downloadable");
    assert_eq!(
        tokio::fs::read(&download_path)
            .await
            .expect("read download"),
        b"content"
    );

    sftp.remove_recursive(&remote_dir)
        .await
        .expect("remote test directory should be removed");
}

#[cfg(unix)]
#[tokio::test]
async fn test_sftp_upload_recursive_skips_local_symlinks() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let sftp = connect_password_sftp(&env).await;
    let id = Uuid::new_v4();
    let remote_dir = sftp
        .home_dir()
        .join(format!("portal-sftp-recursive-upload-{id}"));
    let local_dir = env.config_dir.path().join(format!("upload-dir-{id}"));
    let local_file = local_dir.join("file.txt");
    let local_link = local_dir.join("linked.txt");
    let download_path = env.config_dir.path().join(format!("download-{id}.txt"));

    tokio::fs::create_dir(&local_dir)
        .await
        .expect("local upload directory should be created");
    tokio::fs::write(&local_file, b"content")
        .await
        .expect("local upload file should be written");
    std::os::unix::fs::symlink(&local_file, &local_link)
        .expect("local recursive upload symlink should be created");

    let uploaded = sftp
        .upload_recursive(&local_dir, &remote_dir)
        .await
        .expect("recursive upload should skip local symlink entries");
    assert_eq!(uploaded, 1);

    sftp.download(&remote_dir.join("file.txt"), &download_path)
        .await
        .expect("regular recursively uploaded file should be downloadable");
    assert_eq!(
        tokio::fs::read(&download_path)
            .await
            .expect("read recursive upload download"),
        b"content"
    );

    let entries = sftp
        .list_dir(&remote_dir)
        .await
        .expect("recursive upload remote directory should be listable");
    assert!(
        !entries.iter().any(|entry| entry.name == "linked.txt"),
        "recursive upload should not create a remote file for skipped local symlink"
    );

    sftp.remove_recursive(&remote_dir)
        .await
        .expect("remote recursive upload directory should be removed");
}

#[cfg(unix)]
#[tokio::test]
async fn test_sftp_upload_rejects_local_symlink_without_creating_remote_file() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let sftp = connect_password_sftp(&env).await;
    let id = Uuid::new_v4();
    let target_path = env.config_dir.path().join(format!("target-{id}.txt"));
    let link_path = env.config_dir.path().join(format!("link-{id}.txt"));
    let remote_path = sftp
        .home_dir()
        .join(format!("portal-sftp-symlink-{id}.txt"));

    tokio::fs::write(&target_path, b"secret")
        .await
        .expect("local upload target should be written");
    std::os::unix::fs::symlink(&target_path, &link_path)
        .expect("local upload symlink should be created");

    let error = sftp
        .upload(&link_path, &remote_path)
        .await
        .expect_err("symlink upload should be rejected");
    assert!(error.to_string().contains("symbolic link"));

    let entries = sftp
        .list_dir(sftp.home_dir())
        .await
        .expect("home directory should be listable");
    assert!(
        !entries.iter().any(|entry| entry.path == remote_path),
        "rejected symlink upload should not create remote destination"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn test_sftp_download_rejects_remote_symlink_without_creating_local_file() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let sftp = connect_password_sftp(&env).await;
    let ssh = connect_password_ssh(&env).await;
    let id = Uuid::new_v4();
    let remote_target = sftp.home_dir().join(format!("portal-sftp-target-{id}.txt"));
    let remote_link = sftp.home_dir().join(format!("portal-sftp-link-{id}.txt"));
    let local_download = env.config_dir.path().join(format!("download-{id}.txt"));

    let setup = format!(
        "printf %s secret > {} && ln -s {} {}",
        shell_quote(&remote_target),
        shell_quote(&remote_target),
        shell_quote(&remote_link)
    );
    let result = ssh
        .execute_command_full(&setup, 10)
        .await
        .expect("remote symlink setup command should run");
    assert_eq!(
        result.exit_code, 0,
        "remote symlink setup failed: {}",
        result.stderr
    );

    let error = sftp
        .download(&remote_link, &local_download)
        .await
        .expect_err("remote symlink download should be rejected");
    assert!(error.to_string().contains("symbolic link"));
    assert!(
        !local_download.exists(),
        "rejected remote symlink download should not create local destination"
    );

    let cleanup = format!(
        "rm -f {} {}",
        shell_quote(&remote_link),
        shell_quote(&remote_target)
    );
    let _ = ssh.execute_command_full(&cleanup, 10).await;
}

#[cfg(unix)]
#[tokio::test]
async fn test_sftp_download_recursive_skips_remote_symlinks() {
    skip_if_no_docker!();
    let _guard = super::fixtures::acquire_test_lock().await;

    let env = SshTestEnvironment::new()
        .await
        .expect("Failed to create test environment");

    let sftp = connect_password_sftp(&env).await;
    let ssh = connect_password_ssh(&env).await;
    let id = Uuid::new_v4();
    let remote_dir = sftp.home_dir().join(format!("portal-sftp-recursive-{id}"));
    let remote_file = remote_dir.join("file.txt");
    let remote_link = remote_dir.join("linked.txt");
    let local_dir = env.config_dir.path().join(format!("download-dir-{id}"));

    let setup = format!(
        "mkdir -p {} && printf %s content > {} && ln -s {} {}",
        shell_quote(&remote_dir),
        shell_quote(&remote_file),
        shell_quote(&remote_file),
        shell_quote(&remote_link)
    );
    let result = ssh
        .execute_command_full(&setup, 10)
        .await
        .expect("remote recursive symlink setup command should run");
    assert_eq!(
        result.exit_code, 0,
        "remote recursive symlink setup failed: {}",
        result.stderr
    );

    let downloaded = sftp
        .download_recursive(&remote_dir, &local_dir)
        .await
        .expect("recursive download should skip remote symlink entries");
    assert_eq!(downloaded, 1);
    assert_eq!(
        tokio::fs::read(local_dir.join("file.txt"))
            .await
            .expect("regular recursive download should be readable"),
        b"content"
    );
    assert!(
        !local_dir.join("linked.txt").exists(),
        "recursive download should not materialize skipped remote symlink"
    );

    sftp.remove_recursive(&remote_dir)
        .await
        .expect("remote recursive symlink fixture should be removed");
}
