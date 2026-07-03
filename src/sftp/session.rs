//! SFTP session for file operations

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use russh_sftp::client::SftpSession as RusshSftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::time::timeout;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::error::SftpError;
use crate::fs_utils::{ensure_dir_no_follow, open_directory_for_sync, open_read_regular_file};
use crate::ssh::SshConnection;
use crate::ssh::SshSession;

use super::types::{FileEntry, is_safe_sftp_entry_name};

const TRANSFER_BUFFER_SIZE: usize = 64 * 1024;

/// SFTP session wrapper for file operations
pub struct SftpSession {
    // Keeps the underlying SSH connection alive while this SFTP channel exists.
    _connection: Arc<SshConnection>,
    sftp: Arc<Mutex<RusshSftpSession>>,
    home_dir: PathBuf,
}

impl std::fmt::Debug for SftpSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SftpSession")
            .field("home_dir", &self.home_dir)
            .finish_non_exhaustive()
    }
}

impl SftpSession {
    /// Create a new SFTP session
    pub fn new(connection: Arc<SshConnection>, sftp: RusshSftpSession, home_dir: PathBuf) -> Self {
        Self {
            _connection: connection,
            sftp: Arc::new(Mutex::new(sftp)),
            home_dir,
        }
    }

    /// Get the remote home directory
    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }

    /// Open a new SFTP channel on an existing authenticated SSH terminal session.
    pub async fn from_ssh_session(
        ssh_session: &SshSession,
    ) -> Result<SharedSftpSession, SftpError> {
        let connection = ssh_session.connection();
        let channel = {
            let handle = connection.handle();
            let handle_guard = handle.lock().await;
            handle_guard.channel_open_session().await
        }
        .map_err(|e| SftpError::ConnectionFailed(format!("Failed to open SFTP channel: {e}")))?;

        channel
            .request_subsystem(false, "sftp")
            .await
            .map_err(|e| {
                SftpError::ConnectionFailed(format!("Failed to request SFTP subsystem: {e}"))
            })?;

        let sftp = RusshSftpSession::new(channel.into_stream())
            .await
            .map_err(|e| {
                SftpError::ConnectionFailed(format!("Failed to initialize SFTP session: {e}"))
            })?;

        let home_dir = match timeout(Duration::from_secs(5), sftp.canonicalize(".")).await {
            Ok(Ok(path)) => PathBuf::from(path),
            Ok(Err(_)) | Err(_) => PathBuf::from("/"),
        };

        Ok(Arc::new(SftpSession::new(connection, sftp, home_dir)))
    }

    /// List directory contents
    pub async fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>, SftpError> {
        let sftp = self.sftp.lock().await;
        let path_str = path.to_string_lossy().to_string();

        let read_dir = sftp.read_dir(path_str.clone()).await.map_err(|e| {
            SftpError::FileOperation(format!("Failed to read directory {}: {}", path_str, e))
        })?;

        let mut result = Vec::new();

        // Add parent directory entry
        if let Some(parent) = parent_entry_path(path) {
            result.push(FileEntry {
                name: "..".to_string(),
                path: parent.to_path_buf(),
                is_dir: true,
                is_symlink: false,
                size: 0,
                modified: None,
            });
        }

        for entry in read_dir {
            let name = entry.file_name();
            if !is_safe_sftp_entry_name(&name) {
                tracing::warn!("Skipping unsafe SFTP directory entry name: {:?}", name);
                continue;
            }
            let metadata = entry.metadata();
            let entry_path = path.join(&name);

            let is_dir = metadata.is_dir();
            let is_symlink = metadata.is_symlink();
            let size = metadata.size.unwrap_or(0);

            // Convert mtime to DateTime if available
            let modified = metadata.mtime.and_then(unix_timestamp_to_utc);

            result.push(FileEntry {
                name,
                path: entry_path,
                is_dir,
                is_symlink,
                size,
                modified,
            });
        }

        Ok(result)
    }

    /// Get file size for a remote path.
    pub async fn file_size(&self, path: &Path) -> Result<u64, SftpError> {
        let sftp = self.sftp.lock().await;
        let path_str = path.to_string_lossy().to_string();
        let metadata = sftp.symlink_metadata(path_str.clone()).await.map_err(|e| {
            SftpError::FileOperation(format!("Failed to get metadata for {}: {}", path_str, e))
        })?;
        ensure_remote_file_source(
            &path_str,
            metadata.is_dir(),
            metadata.is_symlink(),
            "inspect",
        )
        .map_err(SftpError::FileOperation)?;
        Ok(metadata.size.unwrap_or(0))
    }

    /// Create a directory
    pub async fn create_dir(&self, path: &Path) -> Result<(), SftpError> {
        let sftp = self.sftp.lock().await;
        let path_str = path.to_string_lossy().to_string();

        match sftp.try_exists(path_str.clone()).await {
            Ok(exists) => reject_existing_remote_create_dir(&path_str, exists)
                .map_err(SftpError::FileOperation)?,
            Err(e) => {
                return Err(SftpError::FileOperation(format!(
                    "Failed to check directory {}: {}",
                    path_str, e
                )));
            }
        }

        sftp.create_dir(path_str.clone()).await.map_err(|e| {
            SftpError::FileOperation(format!("Failed to create directory {}: {}", path_str, e))
        })
    }

    /// Create a directory and any missing parents without following symlinks.
    pub async fn ensure_dir_all(&self, path: &Path) -> Result<(), SftpError> {
        let mut current = PathBuf::new();

        for component in path.components() {
            match component {
                std::path::Component::Prefix(prefix) => current.push(prefix.as_os_str()),
                std::path::Component::RootDir => current.push(Path::new("/")),
                std::path::Component::CurDir => {}
                std::path::Component::ParentDir => {
                    return Err(SftpError::FileOperation(format!(
                        "Refusing to create remote directory with parent traversal: {}",
                        path.display()
                    )));
                }
                std::path::Component::Normal(part) => {
                    current.push(part);
                    self.ensure_remote_dir(&current).await?;
                }
            }
        }

        Ok(())
    }

    async fn ensure_remote_dir(&self, path: &Path) -> Result<(), SftpError> {
        let sftp = self.sftp.lock().await;
        let path_str = path.to_string_lossy().to_string();

        match sftp.try_exists(path_str.clone()).await {
            Ok(true) => {
                let metadata = sftp.symlink_metadata(path_str.clone()).await.map_err(|e| {
                    SftpError::FileOperation(format!(
                        "Failed to get metadata for {}: {}",
                        path_str, e
                    ))
                })?;
                ensure_existing_remote_directory(
                    &path_str,
                    metadata.is_dir(),
                    metadata.is_symlink(),
                )
                .map_err(SftpError::FileOperation)?;
                return Ok(());
            }
            Ok(false) => {}
            Err(e) => {
                return Err(SftpError::FileOperation(format!(
                    "Failed to check directory {}: {}",
                    path_str, e
                )));
            }
        }

        sftp.create_dir(path_str.clone()).await.map_err(|e| {
            SftpError::FileOperation(format!("Failed to create directory {}: {}", path_str, e))
        })
    }

    /// Rename a file or directory
    pub async fn rename(&self, old_path: &Path, new_path: &Path) -> Result<(), SftpError> {
        let sftp = self.sftp.lock().await;
        let old_path_str = old_path.to_string_lossy().to_string();
        let new_path_str = new_path.to_string_lossy().to_string();

        match sftp.try_exists(new_path_str.clone()).await {
            Ok(exists) => reject_existing_remote_rename_destination(&new_path_str, exists)
                .map_err(SftpError::FileOperation)?,
            Err(e) => {
                return Err(SftpError::FileOperation(format!(
                    "Failed to check destination {}: {}",
                    new_path_str, e
                )));
            }
        }

        sftp.rename(old_path_str.clone(), new_path_str.clone())
            .await
            .map_err(|e| {
                SftpError::FileOperation(format!(
                    "Failed to rename {} to {}: {}",
                    old_path_str, new_path_str, e
                ))
            })
    }

    /// Set file/directory permissions (chmod)
    pub async fn set_permissions(&self, path: &Path, mode: u32) -> Result<(), SftpError> {
        let sftp = self.sftp.lock().await;
        let path_str = path.to_string_lossy().to_string();

        let metadata = sftp.symlink_metadata(path_str.clone()).await.map_err(|e| {
            SftpError::FileOperation(format!("Failed to get metadata for {}: {}", path_str, e))
        })?;
        reject_remote_permissions_target(&path_str, metadata.is_symlink())
            .map_err(SftpError::FileOperation)?;

        // Create file attributes with only permissions set
        let attrs = russh_sftp::protocol::FileAttributes {
            permissions: Some(mode),
            ..Default::default()
        };

        sftp.set_metadata(path_str.clone(), attrs)
            .await
            .map_err(|e| {
                SftpError::FileOperation(format!(
                    "Failed to set permissions on {}: {}",
                    path_str, e
                ))
            })
    }

    /// Remove a file
    pub async fn remove_file(&self, path: &Path) -> Result<(), SftpError> {
        let sftp = self.sftp.lock().await;
        let path_str = path.to_string_lossy().to_string();

        sftp.remove_file(path_str.clone()).await.map_err(|e| {
            SftpError::FileOperation(format!("Failed to remove file {}: {}", path_str, e))
        })
    }

    /// Remove a directory (must be empty)
    pub async fn remove_dir(&self, path: &Path) -> Result<(), SftpError> {
        let sftp = self.sftp.lock().await;
        let path_str = path.to_string_lossy().to_string();

        sftp.remove_dir(path_str.clone()).await.map_err(|e| {
            SftpError::FileOperation(format!("Failed to remove directory {}: {}", path_str, e))
        })
    }

    /// Remove a file or directory recursively
    pub async fn remove_recursive(&self, path: &Path) -> Result<(), SftpError> {
        // First check if it's a directory or symlink (do not follow symlinks)
        let (is_dir, is_symlink) = {
            let sftp = self.sftp.lock().await;
            let path_str = path.to_string_lossy().to_string();
            let metadata = sftp.symlink_metadata(path_str.clone()).await.map_err(|e| {
                SftpError::FileOperation(format!("Failed to get metadata for {}: {}", path_str, e))
            })?;
            (metadata.is_dir(), metadata.is_symlink())
        };

        if is_symlink {
            return self.remove_file(path).await;
        }

        if is_dir {
            // List and recursively delete contents
            let entries = self.list_dir(path).await?;
            for entry in entries {
                if entry.name == ".." {
                    continue;
                }
                Box::pin(self.remove_recursive(&entry.path)).await?;
            }
            // Now remove the empty directory
            self.remove_dir(path).await
        } else {
            self.remove_file(path).await
        }
    }

    /// Download a file from remote to local
    pub async fn download(&self, remote_path: &Path, local_path: &Path) -> Result<u64, SftpError> {
        self.download_with_progress(remote_path, local_path, |_| {}, || false)
            .await
    }

    /// Download a file and report cumulative bytes written.
    pub async fn download_with_progress<F, C>(
        &self,
        remote_path: &Path,
        local_path: &Path,
        mut on_progress: F,
        is_cancelled: C,
    ) -> Result<u64, SftpError>
    where
        F: FnMut(u64),
        C: Fn() -> bool,
    {
        ensure_local_download_parent(local_path).await?;
        ensure_local_file_download_target(local_path).await?;

        let partial_path = local_staging_path(local_path, STAGING_PARTIAL_MARKER)?;
        let sftp = self.sftp.lock().await;
        let remote_str = remote_path.to_string_lossy().to_string();

        let metadata = sftp
            .symlink_metadata(remote_str.clone())
            .await
            .map_err(|e| {
                SftpError::Transfer(format!(
                    "Failed to get metadata for remote file {}: {}",
                    remote_str, e
                ))
            })?;
        ensure_remote_file_source(
            &remote_str,
            metadata.is_dir(),
            metadata.is_symlink(),
            "download",
        )
        .map_err(SftpError::Transfer)?;

        let mut remote = sftp.open(remote_str.clone()).await.map_err(|e| {
            SftpError::Transfer(format!("Failed to open remote file {}: {}", remote_str, e))
        })?;

        let mut local = {
            let mut options = OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                options.mode(0o600);
            }
            options.open(&partial_path).await.map_err(|e| {
                SftpError::LocalIo(format!(
                    "Failed to open local staging file {}: {}",
                    partial_path.display(),
                    e
                ))
            })?
        };

        let mut bytes = 0u64;
        let mut buffer = vec![0u8; TRANSFER_BUFFER_SIZE];
        loop {
            if is_cancelled() {
                drop(local);
                cleanup_local_staging(&partial_path).await;
                return Err(SftpError::Transfer("Transfer cancelled".to_string()));
            }

            let read = match remote.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => read,
                Err(e) => {
                    drop(local);
                    cleanup_local_staging(&partial_path).await;
                    return Err(SftpError::Transfer(format!(
                        "Failed to download {} to {}: {}",
                        remote_str,
                        local_path.display(),
                        e
                    )));
                }
            };

            if let Err(e) = local.write_all(&buffer[..read]).await {
                drop(local);
                cleanup_local_staging(&partial_path).await;
                return Err(SftpError::LocalIo(format!(
                    "Failed to write local staging file {}: {}",
                    partial_path.display(),
                    e
                )));
            }
            bytes = bytes.saturating_add(read as u64);
            on_progress(bytes);
        }

        if let Err(e) = local.flush().await {
            drop(local);
            cleanup_local_staging(&partial_path).await;
            return Err(SftpError::LocalIo(format!(
                "Failed to flush local staging file {}: {}",
                partial_path.display(),
                e
            )));
        }

        if let Err(e) = local.sync_all().await {
            drop(local);
            cleanup_local_staging(&partial_path).await;
            return Err(SftpError::LocalIo(format!(
                "Failed to sync local staging file {}: {}",
                partial_path.display(),
                e
            )));
        }

        drop(local);

        if let Err(e) = tokio::fs::rename(&partial_path, local_path).await {
            cleanup_local_staging(&partial_path).await;
            return Err(SftpError::LocalIo(format!(
                "Failed to promote local staging file {} to {}: {}",
                partial_path.display(),
                local_path.display(),
                e
            )));
        }

        sync_local_parent_dir(local_path).await;

        Ok(bytes)
    }

    /// Upload a file from local to remote
    pub async fn upload(&self, local_path: &Path, remote_path: &Path) -> Result<u64, SftpError> {
        self.upload_with_progress(local_path, remote_path, |_| {}, || false)
            .await
    }

    /// Upload bytes to a new remote file. Existing destinations are not overwritten.
    pub async fn upload_bytes(
        &self,
        contents: &[u8],
        remote_path: &Path,
    ) -> Result<u64, SftpError> {
        let sftp = self.sftp.lock().await;
        let remote_str = remote_path.to_string_lossy().to_string();

        let mut remote = sftp
            .open_with_flags(
                remote_str.clone(),
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
            )
            .await
            .map_err(|e| {
                SftpError::Transfer(format!("Failed to open remote file {}: {}", remote_str, e))
            })?;

        let mut bytes = 0u64;
        for chunk in contents.chunks(TRANSFER_BUFFER_SIZE) {
            if let Err(e) = remote.write_all(chunk).await {
                let _ = remote.shutdown().await;
                drop(remote);
                cleanup_remote_staging(&sftp, &remote_str).await;
                return Err(SftpError::Transfer(format!(
                    "Failed to upload clipboard image to {}: {}",
                    remote_str, e
                )));
            }
            bytes = bytes.saturating_add(chunk.len() as u64);
        }

        if let Err(e) = remote.flush().await {
            let _ = remote.shutdown().await;
            drop(remote);
            cleanup_remote_staging(&sftp, &remote_str).await;
            return Err(SftpError::Transfer(format!(
                "Failed to flush remote file {}: {}",
                remote_str, e
            )));
        }

        if let Err(e) = remote.sync_all().await {
            let _ = remote.shutdown().await;
            drop(remote);
            cleanup_remote_staging(&sftp, &remote_str).await;
            return Err(SftpError::Transfer(format!(
                "Failed to sync remote file {}: {}",
                remote_str, e
            )));
        }

        if let Err(e) = remote.shutdown().await {
            drop(remote);
            cleanup_remote_staging(&sftp, &remote_str).await;
            return Err(SftpError::Transfer(format!(
                "Failed to close remote file {}: {}",
                remote_str, e
            )));
        }

        drop(remote);
        drop(sftp);

        if let Err(error) = self.set_permissions(remote_path, 0o600).await {
            tracing::warn!(
                "Failed to set clipboard image permissions on {}: {}",
                remote_str,
                error
            );
        }

        Ok(bytes)
    }

    /// Upload a file and report cumulative bytes written.
    pub async fn upload_with_progress<F, C>(
        &self,
        local_path: &Path,
        remote_path: &Path,
        mut on_progress: F,
        is_cancelled: C,
    ) -> Result<u64, SftpError>
    where
        F: FnMut(u64),
        C: Fn() -> bool,
    {
        let mut local = open_local_upload_file_source(local_path).await?;

        let sftp = self.sftp.lock().await;
        let remote_str = remote_path.to_string_lossy().to_string();
        let partial_path = remote_staging_path(remote_path, STAGING_PARTIAL_MARKER)?;
        let backup_path = remote_staging_path(remote_path, STAGING_BACKUP_MARKER)?;
        let partial_str = partial_path.to_string_lossy().to_string();
        let backup_str = backup_path.to_string_lossy().to_string();

        let mut remote = sftp
            .open_with_flags(
                partial_str.clone(),
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::EXCLUDE,
            )
            .await
            .map_err(|e| {
                SftpError::Transfer(format!(
                    "Failed to open remote staging file {}: {}",
                    partial_str, e
                ))
            })?;

        let mut bytes = 0u64;
        let mut buffer = vec![0u8; TRANSFER_BUFFER_SIZE];
        loop {
            if is_cancelled() {
                let _ = remote.shutdown().await;
                drop(remote);
                cleanup_remote_staging(&sftp, &partial_str).await;
                return Err(SftpError::Transfer("Transfer cancelled".to_string()));
            }

            let read = match local.read(&mut buffer).await {
                Ok(0) => break,
                Ok(read) => read,
                Err(e) => {
                    let _ = remote.shutdown().await;
                    drop(remote);
                    cleanup_remote_staging(&sftp, &partial_str).await;
                    return Err(SftpError::LocalIo(format!(
                        "Failed to read local file {}: {}",
                        local_path.display(),
                        e
                    )));
                }
            };

            if let Err(e) = remote.write_all(&buffer[..read]).await {
                let _ = remote.shutdown().await;
                drop(remote);
                cleanup_remote_staging(&sftp, &partial_str).await;
                return Err(SftpError::Transfer(format!(
                    "Failed to upload {} to {}: {}",
                    local_path.display(),
                    remote_str,
                    e
                )));
            }
            bytes = bytes.saturating_add(read as u64);
            on_progress(bytes);
        }

        if let Err(e) = remote.flush().await {
            let _ = remote.shutdown().await;
            drop(remote);
            cleanup_remote_staging(&sftp, &partial_str).await;
            return Err(SftpError::Transfer(format!(
                "Failed to flush remote staging file {}: {}",
                partial_str, e
            )));
        }

        if let Err(e) = remote.sync_all().await {
            let _ = remote.shutdown().await;
            drop(remote);
            cleanup_remote_staging(&sftp, &partial_str).await;
            return Err(SftpError::Transfer(format!(
                "Failed to sync remote staging file {}: {}",
                partial_str, e
            )));
        }

        if let Err(e) = remote.shutdown().await {
            drop(remote);
            cleanup_remote_staging(&sftp, &partial_str).await;
            return Err(SftpError::Transfer(format!(
                "Failed to close remote staging file {}: {}",
                partial_str, e
            )));
        }

        drop(remote);

        if let Err(e) = promote_remote_staging(&sftp, &partial_str, &remote_str, &backup_str).await
        {
            cleanup_remote_staging(&sftp, &partial_str).await;
            return Err(e);
        }

        Ok(bytes)
    }

    /// Download a directory recursively from remote to local
    pub async fn download_recursive(
        &self,
        remote_path: &Path,
        local_path: &Path,
    ) -> Result<usize, SftpError> {
        ensure_local_download_parent(local_path).await?;
        ensure_local_download_directory(local_path).await?;

        let entries = self.list_dir(remote_path).await?;
        let mut count = 0;

        for entry in entries {
            if should_skip_recursive_download_entry(&entry) {
                continue;
            }

            let target_path = local_path.join(&entry.name);

            if entry.is_dir {
                count += Box::pin(self.download_recursive(&entry.path, &target_path)).await?;
            } else {
                self.download(&entry.path, &target_path).await?;
                count += 1;
            }
        }

        Ok(count)
    }

    /// Upload a directory recursively from local to remote
    pub async fn upload_recursive(
        &self,
        local_path: &Path,
        remote_path: &Path,
    ) -> Result<usize, SftpError> {
        ensure_local_upload_directory_root(local_path).await?;

        // Ensure remote directory exists; recursive upload can target an existing directory.
        self.ensure_remote_dir(remote_path).await?;

        let mut count = 0;
        let mut entries = tokio::fs::read_dir(local_path).await.map_err(|e| {
            SftpError::LocalIo(format!(
                "Failed to read local directory {}: {}",
                local_path.display(),
                e
            ))
        })?;

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| SftpError::LocalIo(format!("Failed to read directory entry: {}", e)))?
        {
            let entry_name = entry.file_name();
            let local_entry_path = entry.path();
            let remote_entry_path = remote_path.join(&entry_name);

            match local_upload_entry_kind(&local_entry_path).await? {
                Some(LocalUploadEntryKind::Directory) => {
                    count += Box::pin(self.upload_recursive(&local_entry_path, &remote_entry_path))
                        .await?;
                }
                Some(LocalUploadEntryKind::File) => {
                    self.upload(&local_entry_path, &remote_entry_path).await?;
                    count += 1;
                }
                None => {}
            }
        }

        Ok(count)
    }
}

fn unix_timestamp_to_utc(mtime: u32) -> Option<chrono::DateTime<Utc>> {
    Utc.timestamp_opt(i64::from(mtime), 0).single()
}

fn parent_entry_path(path: &Path) -> Option<&Path> {
    let parent = path.parent()?;
    if parent.as_os_str().is_empty() {
        return None;
    }
    Some(parent)
}

fn should_skip_recursive_download_entry(entry: &FileEntry) -> bool {
    entry.name == ".." || entry.is_symlink
}

fn ensure_remote_file_source(
    path: &str,
    is_dir: bool,
    is_symlink: bool,
    operation: &str,
) -> Result<(), String> {
    if is_symlink {
        return Err(format!("Cannot {} symbolic link {}", operation, path));
    }

    if is_dir {
        return Err(format!("Cannot {} directory {}", operation, path));
    }

    Ok(())
}

fn reject_existing_remote_rename_destination(path: &str, exists: bool) -> Result<(), String> {
    if exists {
        Err(format!("{} already exists", path))
    } else {
        Ok(())
    }
}

fn reject_existing_remote_create_dir(path: &str, exists: bool) -> Result<(), String> {
    if exists {
        Err(format!("{} already exists", path))
    } else {
        Ok(())
    }
}

fn ensure_existing_remote_directory(
    path: &str,
    is_dir: bool,
    is_symlink: bool,
) -> Result<(), String> {
    if is_symlink {
        return Err(format!("Cannot use symbolic link {} as directory", path));
    }

    if !is_dir {
        return Err(format!(
            "Cannot use {}; a non-directory already exists",
            path
        ));
    }

    Ok(())
}

fn reject_remote_permissions_target(path: &str, is_symlink: bool) -> Result<(), String> {
    if is_symlink {
        Err(format!(
            "Refusing to set permissions on symbolic link {}",
            path
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalUploadEntryKind {
    Directory,
    File,
}

const STAGING_PARTIAL_MARKER: &str = ".portal-part-";
const STAGING_BACKUP_MARKER: &str = ".portal-backup-";

async fn ensure_local_file_download_target(local_path: &Path) -> Result<(), SftpError> {
    match tokio::fs::symlink_metadata(local_path).await {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(SftpError::LocalIo(format!(
            "Refusing to overwrite local symbolic link {}",
            local_path.display()
        ))),
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(SftpError::LocalIo(format!(
            "Refusing to overwrite non-regular local file {}",
            local_path.display()
        ))),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(SftpError::LocalIo(format!(
            "Failed to inspect local file {}: {}",
            local_path.display(),
            e
        ))),
    }
}

async fn ensure_local_download_directory(local_path: &Path) -> Result<(), SftpError> {
    let path = local_path.to_path_buf();
    tokio::task::spawn_blocking(move || ensure_dir_no_follow(&path))
        .await
        .map_err(|e| SftpError::LocalIo(format!("Local directory task failed: {}", e)))?
        .map_err(|e| local_download_directory_error(local_path, e))
}

async fn ensure_local_download_parent(local_path: &Path) -> Result<(), SftpError> {
    let Some(parent) = local_path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }

    let parent = parent.to_path_buf();
    tokio::task::spawn_blocking(move || ensure_dir_no_follow(&parent))
        .await
        .map_err(|e| SftpError::LocalIo(format!("Local directory task failed: {}", e)))?
        .map_err(|e| local_download_parent_error(local_path, e))
}

fn local_download_directory_error(path: &Path, error: std::io::Error) -> SftpError {
    match error.kind() {
        ErrorKind::InvalidInput => SftpError::LocalIo(format!(
            "Refusing to write directory through local symbolic link {}",
            path.display()
        )),
        ErrorKind::NotADirectory => SftpError::LocalIo(format!(
            "Cannot create local directory {}; not a directory",
            path.display()
        )),
        _ => SftpError::LocalIo(format!(
            "Failed to create local directory {}: {}",
            path.display(),
            error
        )),
    }
}

fn local_download_parent_error(local_path: &Path, error: std::io::Error) -> SftpError {
    match error.kind() {
        ErrorKind::InvalidInput => SftpError::LocalIo(format!(
            "Refusing to write through local symbolic link directory {}",
            local_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(local_path)
                .display()
        )),
        ErrorKind::NotADirectory => SftpError::LocalIo(format!(
            "Cannot create local file {}; parent is not a directory",
            local_path.display()
        )),
        _ => SftpError::LocalIo(format!(
            "Failed to create local directory {}: {}",
            local_path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(local_path)
                .display(),
            error
        )),
    }
}

async fn ensure_local_upload_directory_root(local_path: &Path) -> Result<(), SftpError> {
    let metadata = tokio::fs::symlink_metadata(local_path).await.map_err(|e| {
        SftpError::LocalIo(format!(
            "Failed to read metadata for {}: {}",
            local_path.display(),
            e
        ))
    })?;

    if metadata.file_type().is_symlink() {
        return Err(SftpError::LocalIo(format!(
            "Cannot upload symbolic link {}",
            local_path.display()
        )));
    }

    if !metadata.is_dir() {
        return Err(SftpError::LocalIo(format!(
            "Cannot upload directory {}; not a directory",
            local_path.display()
        )));
    }

    Ok(())
}

async fn local_upload_entry_kind(
    local_path: &Path,
) -> Result<Option<LocalUploadEntryKind>, SftpError> {
    let metadata = tokio::fs::symlink_metadata(local_path).await.map_err(|e| {
        SftpError::LocalIo(format!(
            "Failed to read metadata for {}: {}",
            local_path.display(),
            e
        ))
    })?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Ok(None);
    }

    if file_type.is_dir() {
        Ok(Some(LocalUploadEntryKind::Directory))
    } else if file_type.is_file() {
        Ok(Some(LocalUploadEntryKind::File))
    } else {
        Ok(None)
    }
}

async fn ensure_local_upload_file_source(local_path: &Path) -> Result<(), SftpError> {
    let metadata = tokio::fs::symlink_metadata(local_path).await.map_err(|e| {
        SftpError::LocalIo(format!(
            "Failed to read metadata for {}: {}",
            local_path.display(),
            e
        ))
    })?;

    if metadata.file_type().is_symlink() {
        return Err(SftpError::LocalIo(format!(
            "Cannot upload symbolic link {}",
            local_path.display()
        )));
    }

    if !metadata.is_file() {
        return Err(SftpError::LocalIo(format!(
            "Cannot upload file {}; not a regular file",
            local_path.display()
        )));
    }

    Ok(())
}

async fn open_local_upload_file_source(local_path: &Path) -> Result<tokio::fs::File, SftpError> {
    ensure_local_upload_file_source(local_path).await?;

    let path = local_path.to_path_buf();
    let path_for_open = path.clone();
    let file =
        tokio::task::spawn_blocking(move || open_read_regular_file(&path_for_open, "local upload"))
            .await
            .map_err(|e| {
                SftpError::LocalIo(format!(
                    "Failed to open local file {}: {}",
                    path.display(),
                    e
                ))
            })?
            .map_err(|e| {
                SftpError::LocalIo(format!(
                    "Failed to open local file {}: {}",
                    local_path.display(),
                    e
                ))
            })?;

    Ok(tokio::fs::File::from_std(file))
}

fn local_staging_path(path: &Path, marker: &str) -> Result<PathBuf, SftpError> {
    sibling_staging_path(path, marker).map_err(SftpError::LocalIo)
}

fn remote_staging_path(path: &Path, marker: &str) -> Result<PathBuf, SftpError> {
    sibling_staging_path(path, marker).map_err(SftpError::Transfer)
}

fn sibling_staging_path(path: &Path, marker: &str) -> Result<PathBuf, String> {
    let file_name = path
        .file_name()
        .ok_or_else(|| format!("Cannot create staging path for {}", path.display()))?
        .to_string_lossy();

    Ok(path.with_file_name(staging_file_name(&file_name, marker, Uuid::new_v4())))
}

fn staging_file_name(final_name: &str, marker: &str, id: Uuid) -> String {
    format!(".{}{}{}", final_name, marker, id)
}

async fn cleanup_local_staging(path: &Path) {
    match tokio::fs::remove_file(path).await {
        Ok(()) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => {
            tracing::debug!(
                "Failed to remove local staging file {}: {}",
                path.display(),
                e
            );
        }
    }
}

async fn sync_local_parent_dir(path: &Path) {
    let Some(parent) = path.parent() else {
        return;
    };

    let parent = if parent.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        parent.to_path_buf()
    };

    match tokio::task::spawn_blocking(move || {
        let dir = open_directory_for_sync(&parent)?;
        dir.sync_all()
    })
    .await
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::debug!("Failed to sync local parent directory: {}", e);
        }
        Err(e) => {
            tracing::debug!("Failed to join local parent directory sync task: {}", e);
        }
    }
}

async fn cleanup_remote_staging(sftp: &RusshSftpSession, path: &str) {
    if let Err(e) = sftp.remove_file(path.to_string()).await {
        tracing::debug!("Failed to remove remote staging file {}: {}", path, e);
    }
}

async fn promote_remote_staging(
    sftp: &RusshSftpSession,
    partial_path: &str,
    final_path: &str,
    backup_path: &str,
) -> Result<(), SftpError> {
    match sftp
        .rename(partial_path.to_string(), final_path.to_string())
        .await
    {
        Ok(()) => Ok(()),
        Err(first_error) => match sftp.try_exists(final_path.to_string()).await {
            Ok(true) => {
                sftp.rename(final_path.to_string(), backup_path.to_string())
                    .await
                    .map_err(|backup_error| {
                        SftpError::Transfer(format!(
                            "Failed to stage replacement for remote file {} after upload to {}: initial promote failed: {}; backup rename failed: {}",
                            final_path, partial_path, first_error, backup_error
                        ))
                    })?;

                match sftp
                    .rename(partial_path.to_string(), final_path.to_string())
                    .await
                {
                    Ok(()) => {
                        cleanup_remote_staging(sftp, backup_path).await;
                        Ok(())
                    }
                    Err(promote_error) => {
                        let rollback = sftp
                            .rename(backup_path.to_string(), final_path.to_string())
                            .await;
                        let rollback_message = match rollback {
                            Ok(()) => "original remote file was restored".to_string(),
                            Err(rollback_error) => format!(
                                "rollback failed; original remote file remains at {}: {}",
                                backup_path, rollback_error
                            ),
                        };

                        Err(SftpError::Transfer(format!(
                            "Failed to promote remote staging file {} to {}: {}; {}",
                            partial_path, final_path, promote_error, rollback_message
                        )))
                    }
                }
            }
            Ok(false) => Err(SftpError::Transfer(format!(
                "Failed to promote remote staging file {} to {}: {}",
                partial_path, final_path, first_error
            ))),
            Err(check_error) => Err(SftpError::Transfer(format!(
                "Failed to promote remote staging file {} to {}: {}; target existence check failed: {}",
                partial_path, final_path, first_error, check_error
            ))),
        },
    }
}

impl Drop for SftpSession {
    fn drop(&mut self) {
        tracing::debug!("SFTP session cleanup: closing session");
        let sftp = self.sftp.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    let sftp = sftp.lock().await;
                    if let Err(e) = sftp.close().await {
                        tracing::debug!("SFTP close failed: {}", e);
                    }
                });
            }
            Err(_) => {
                tracing::debug!("SFTP session dropped without a Tokio runtime; close skipped");
            }
        }
    }
}

/// Thread-safe wrapper for SFTP session
pub type SharedSftpSession = Arc<SftpSession>;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    // Note: Most SftpSession methods require an actual RusshSftpSession which
    // requires a live SSH/SFTP connection. These are tested via integration tests
    // in tests/ssh_integration/. The unit tests here cover type definitions and
    // any pure helper functions.

    // === Type alias tests ===

    #[test]
    fn shared_sftp_session_is_arc_type() {
        // Verify the type alias is correctly defined
        // This is a compile-time check that SharedSftpSession = Arc<SftpSession>
        fn _accepts_arc(_: Arc<SftpSession>) {}
        fn _returns_shared() -> Option<SharedSftpSession> {
            None
        }
        // If this compiles, the type alias is correct
        let _: Option<Arc<SftpSession>> = _returns_shared();
    }

    // === Path handling tests ===
    // These test the path conversion patterns used throughout the module

    #[test]
    fn path_to_string_lossy_preserves_valid_utf8() {
        let path = Path::new("/home/user/documents/file.txt");
        let path_str = path.to_string_lossy().to_string();
        assert_eq!(path_str, "/home/user/documents/file.txt");
    }

    #[test]
    fn path_to_string_lossy_handles_root() {
        let path = Path::new("/");
        let path_str = path.to_string_lossy().to_string();
        assert_eq!(path_str, "/");
    }

    #[test]
    fn path_to_string_lossy_handles_relative() {
        let path = Path::new("relative/path/file.txt");
        let path_str = path.to_string_lossy().to_string();
        assert_eq!(path_str, "relative/path/file.txt");
    }

    #[test]
    fn path_parent_for_root_returns_none_content() {
        let path = Path::new("/");
        // Root path's parent is still Some("/") on Unix, but we check our logic
        assert!(path.parent().is_some() || path.to_string_lossy() == "/");
    }

    #[test]
    fn path_parent_for_nested_returns_parent() {
        let path = Path::new("/home/user/file.txt");
        let parent = path.parent().unwrap();
        assert_eq!(parent, Path::new("/home/user"));
    }

    #[test]
    fn path_join_creates_correct_path() {
        let base = Path::new("/home/user");
        let joined = base.join("documents");
        assert_eq!(joined, PathBuf::from("/home/user/documents"));
    }

    #[test]
    fn path_join_with_filename() {
        let base = Path::new("/var/log");
        let joined = base.join("syslog");
        assert_eq!(joined, PathBuf::from("/var/log/syslog"));
    }

    // === FileEntry creation pattern tests ===
    // Test the patterns used when creating FileEntry structs

    #[test]
    fn parent_entry_pattern() {
        let path = Path::new("/home/user/documents");
        let parent = path.parent().unwrap_or(Path::new("/"));

        let entry = FileEntry {
            name: "..".to_string(),
            path: parent.to_path_buf(),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
        };

        assert_eq!(entry.name, "..");
        assert!(entry.is_dir);
        assert!(!entry.is_symlink);
        assert_eq!(entry.size, 0);
        assert!(entry.modified.is_none());
        assert_eq!(entry.path, PathBuf::from("/home/user"));
    }

    #[test]
    fn file_entry_pattern() {
        let base_path = Path::new("/home/user");
        let name = "test.txt".to_string();
        let entry_path = base_path.join(&name);

        let entry = FileEntry {
            name: name.clone(),
            path: entry_path.clone(),
            is_dir: false,
            is_symlink: false,
            size: 1024,
            modified: Some(Utc::now()),
        };

        assert_eq!(entry.name, "test.txt");
        assert_eq!(entry.path, PathBuf::from("/home/user/test.txt"));
        assert!(!entry.is_dir);
        assert!(!entry.is_symlink);
        assert_eq!(entry.size, 1024);
        assert!(entry.modified.is_some());
    }

    #[test]
    fn directory_entry_pattern() {
        let base_path = Path::new("/var");
        let name = "log".to_string();

        let entry = FileEntry {
            name: name.clone(),
            path: base_path.join(&name),
            is_dir: true,
            is_symlink: false,
            size: 4096,
            modified: None,
        };

        assert_eq!(entry.name, "log");
        assert!(entry.is_dir);
        assert!(!entry.is_symlink);
    }

    #[test]
    fn symlink_entry_pattern() {
        let entry = FileEntry {
            name: "link".to_string(),
            path: PathBuf::from("/usr/bin/link"),
            is_dir: false,
            is_symlink: true,
            size: 0,
            modified: None,
        };

        assert!(entry.is_symlink);
        assert!(!entry.is_dir);
    }

    // === Timestamp conversion tests ===
    // Test the mtime to DateTime conversion pattern used in list_dir

    #[test]
    fn timestamp_conversion_valid() {
        let mtime: u32 = 1704067200; // 2024-01-01 00:00:00 UTC
        let datetime = unix_timestamp_to_utc(mtime).unwrap();

        assert_eq!(datetime.year(), 2024);
        assert_eq!(datetime.month(), 1);
        assert_eq!(datetime.day(), 1);
    }

    #[test]
    fn timestamp_conversion_zero() {
        let mtime: u32 = 0; // Unix epoch
        let datetime = unix_timestamp_to_utc(mtime).unwrap();

        assert_eq!(datetime.year(), 1970);
        assert_eq!(datetime.month(), 1);
        assert_eq!(datetime.day(), 1);
    }

    #[test]
    fn timestamp_conversion_recent() {
        let mtime: u32 = 1700000000; // Nov 2023
        let datetime = unix_timestamp_to_utc(mtime).unwrap();

        assert_eq!(datetime.year(), 2023);
        assert_eq!(datetime.month(), 11);
    }

    #[test]
    fn timestamp_conversion_max_u32_is_valid() {
        let datetime = unix_timestamp_to_utc(u32::MAX).unwrap();

        assert_eq!(datetime.year(), 2106);
    }

    // === Root path detection tests ===
    // Test the logic used to determine if we should add parent entry

    #[test]
    fn should_add_parent_entry_for_nested_path() {
        let path = Path::new("/home/user");
        let path_str = path.to_string_lossy().to_string();

        let should_add = path.parent().is_some() && path_str != "/";
        assert!(should_add);
    }

    #[test]
    fn should_not_add_parent_entry_for_root() {
        let path = Path::new("/");
        let path_str = path.to_string_lossy().to_string();

        let should_add = path.parent().is_some() && path_str != "/";
        assert!(!should_add);
    }

    #[test]
    fn should_add_parent_entry_for_single_level() {
        let path = Path::new("/home");

        assert_eq!(parent_entry_path(path), Some(Path::new("/")));
    }

    #[test]
    fn parent_entry_path_skips_relative_empty_parent() {
        assert!(parent_entry_path(Path::new(".")).is_none());
        assert!(parent_entry_path(Path::new("relative")).is_none());
    }

    // === Size handling tests ===

    fn get_size(has_size: bool, value: u64) -> Option<u64> {
        if has_size { Some(value) } else { None }
    }

    #[test]
    fn size_unwrap_or_zero_with_none() {
        let size = get_size(false, 0);
        assert_eq!(size.unwrap_or(0), 0);
    }

    #[test]
    fn size_unwrap_or_zero_with_some() {
        let size = get_size(true, 12345);
        assert_eq!(size.unwrap_or(0), 12345);
    }

    #[test]
    fn size_unwrap_or_zero_with_large_value() {
        let size = get_size(true, u64::MAX);
        assert_eq!(size.unwrap_or(0), u64::MAX);
    }

    // === PathBuf tests for home_dir storage ===

    #[test]
    fn pathbuf_stores_home_dir() {
        let home = PathBuf::from("/home/testuser");
        assert_eq!(home.as_path(), Path::new("/home/testuser"));
    }

    #[test]
    fn pathbuf_from_tilde_expansion() {
        // Simulating what would happen after tilde expansion
        let expanded = PathBuf::from("/home/user");
        assert!(expanded.is_absolute());
    }

    #[test]
    fn path_reference_from_pathbuf() {
        let home_dir = PathBuf::from("/root");
        let path_ref: &Path = &home_dir;
        assert_eq!(path_ref, Path::new("/root"));
    }

    // === Skip parent entry in iteration ===

    #[test]
    fn skip_parent_entry_pattern() {
        let entries = [".", "..", "file.txt", "dir"];
        let filtered: Vec<_> = entries.iter().filter(|e| **e != "..").collect();

        assert_eq!(filtered.len(), 3);
        assert!(!filtered.contains(&&".."));
    }

    #[test]
    fn recursive_download_skips_parent_and_symlink_entries() {
        let parent = FileEntry {
            name: "..".to_string(),
            path: PathBuf::from("/remote"),
            is_dir: true,
            is_symlink: false,
            size: 0,
            modified: None,
        };
        let symlink = FileEntry {
            name: "linked".to_string(),
            path: PathBuf::from("/remote/linked"),
            is_dir: false,
            is_symlink: true,
            size: 0,
            modified: None,
        };
        let file = FileEntry {
            name: "file.txt".to_string(),
            path: PathBuf::from("/remote/file.txt"),
            is_dir: false,
            is_symlink: false,
            size: 1,
            modified: None,
        };

        assert!(should_skip_recursive_download_entry(&parent));
        assert!(should_skip_recursive_download_entry(&symlink));
        assert!(!should_skip_recursive_download_entry(&file));
    }

    #[test]
    fn remote_file_source_guard_allows_regular_file_candidate() {
        ensure_remote_file_source("/remote/file.txt", false, false, "download")
            .expect("regular file candidates should be allowed");
    }

    #[test]
    fn remote_file_source_guard_rejects_symlink() {
        let error = ensure_remote_file_source("/remote/link", false, true, "download")
            .expect_err("symlink should be rejected");

        assert!(error.contains("symbolic link"));
    }

    #[test]
    fn remote_file_source_guard_rejects_directory() {
        let error = ensure_remote_file_source("/remote/dir", true, false, "inspect")
            .expect_err("directory should be rejected");

        assert!(error.contains("directory"));
    }

    #[test]
    fn remote_rename_destination_guard_allows_missing_path() {
        reject_existing_remote_rename_destination("/remote/new-name.txt", false)
            .expect("missing destination should be allowed");
    }

    #[test]
    fn remote_rename_destination_guard_rejects_existing_path() {
        let error = reject_existing_remote_rename_destination("/remote/existing.txt", true)
            .expect_err("existing destination should be rejected");

        assert!(error.contains("already exists"));
    }

    #[test]
    fn remote_create_dir_guard_allows_missing_path() {
        reject_existing_remote_create_dir("/remote/new-dir", false)
            .expect("missing directory target should be creatable");
    }

    #[test]
    fn remote_create_dir_guard_rejects_existing_path() {
        let error = reject_existing_remote_create_dir("/remote/existing-dir", true)
            .expect_err("existing directory target should be rejected");

        assert!(error.contains("already exists"));
    }

    #[test]
    fn existing_remote_directory_guard_allows_real_directory() {
        ensure_existing_remote_directory("/remote/dir", true, false)
            .expect("existing real directory should be reusable");
    }

    #[test]
    fn existing_remote_directory_guard_rejects_file() {
        let error = ensure_existing_remote_directory("/remote/file.txt", false, false)
            .expect_err("existing file should not be accepted as directory");

        assert!(error.contains("non-directory"));
    }

    #[test]
    fn existing_remote_directory_guard_rejects_symlink() {
        let error = ensure_existing_remote_directory("/remote/link", true, true)
            .expect_err("existing symlink should not be accepted as directory");

        assert!(error.contains("symbolic link"));
    }

    #[test]
    fn remote_permissions_target_guard_allows_non_symlink() {
        reject_remote_permissions_target("/remote/file.txt", false)
            .expect("non-symlink permissions target should be allowed");
    }

    #[test]
    fn remote_permissions_target_guard_rejects_symlink() {
        let error = reject_remote_permissions_target("/remote/link.txt", true)
            .expect_err("symlink permissions target should be rejected");

        assert!(error.contains("symbolic link"));
    }

    #[test]
    fn staging_file_name_uses_hidden_sibling_prefix() {
        let name = staging_file_name("notes.txt", STAGING_PARTIAL_MARKER, Uuid::nil());

        assert_eq!(
            name,
            ".notes.txt.portal-part-00000000-0000-0000-0000-000000000000"
        );
    }

    #[test]
    fn sibling_staging_path_stays_in_same_directory() {
        let staging = sibling_staging_path(Path::new("/tmp/notes.txt"), STAGING_PARTIAL_MARKER)
            .expect("staging path should be created");

        assert_eq!(staging.parent(), Some(Path::new("/tmp")));
        let file_name = staging.file_name().unwrap().to_string_lossy();
        assert!(file_name.starts_with(".notes.txt.portal-part-"));
    }

    #[test]
    fn sibling_staging_path_rejects_paths_without_file_names() {
        let error = sibling_staging_path(Path::new("/"), STAGING_PARTIAL_MARKER)
            .expect_err("root cannot have a sibling staging file");

        assert!(error.contains("Cannot create staging path"));
    }

    #[tokio::test]
    async fn local_file_download_target_allows_missing_path() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("download.txt");

        ensure_local_file_download_target(&missing)
            .await
            .expect("missing file target should be creatable later");
    }

    #[tokio::test]
    async fn local_file_download_target_allows_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("download.txt");
        std::fs::write(&file, "old").unwrap();

        ensure_local_file_download_target(&file)
            .await
            .expect("regular file target should be replaceable");
    }

    #[tokio::test]
    async fn local_file_download_target_rejects_directory() {
        let temp = tempfile::tempdir().unwrap();

        let error = ensure_local_file_download_target(temp.path())
            .await
            .expect_err("directory should not be accepted as file download target");

        assert!(error.to_string().contains("non-regular"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_file_download_target_rejects_symlinked_file() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("download.txt");
        std::fs::write(&target, "old").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = ensure_local_file_download_target(&link)
            .await
            .expect_err("symlinked file target should be rejected");

        assert!(error.to_string().contains("symbolic link"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_file_download_target_rejects_socket() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("download.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let error = ensure_local_file_download_target(&path)
            .await
            .expect_err("socket should not be accepted as file download target");

        assert!(error.to_string().contains("non-regular"));
    }

    #[tokio::test]
    async fn local_download_parent_allows_existing_directory() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("download.txt");

        ensure_local_download_parent(&path)
            .await
            .expect("existing parent directory should be accepted");
    }

    #[tokio::test]
    async fn local_download_parent_creates_missing_directory() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("nested").join("download.txt");

        ensure_local_download_parent(&path)
            .await
            .expect("missing parent directory should be created");

        assert!(path.parent().unwrap().is_dir());
    }

    #[tokio::test]
    async fn local_download_parent_rejects_file_parent() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path().join("not-a-dir");
        let path = parent.join("download.txt");
        std::fs::write(&parent, "content").unwrap();

        let error = ensure_local_download_parent(&path)
            .await
            .expect_err("file parent should not be accepted");

        assert!(error.to_string().contains("parent is not a directory"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_download_parent_rejects_symlinked_directory() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        let path = link.join("download.txt");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = ensure_local_download_parent(&path)
            .await
            .expect_err("symlinked parent should be rejected");

        assert!(error.to_string().contains("symbolic link"));
        assert!(!target.join("download.txt").exists());
    }

    #[tokio::test]
    async fn local_download_directory_allows_existing_directory() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("download-dir");
        std::fs::create_dir(&path).unwrap();

        ensure_local_download_directory(&path)
            .await
            .expect("existing directory should be accepted");
    }

    #[tokio::test]
    async fn local_download_directory_creates_missing_directory() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("download-dir");

        ensure_local_download_directory(&path)
            .await
            .expect("missing directory should be created");

        assert!(path.is_dir());
    }

    #[tokio::test]
    async fn local_download_directory_rejects_file_target() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("download-dir");
        std::fs::write(&path, "content").unwrap();

        let error = ensure_local_download_directory(&path)
            .await
            .expect_err("file should not be used as download directory");

        assert!(error.to_string().contains("not a directory"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_download_directory_rejects_symlinked_directory() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("download-dir");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = ensure_local_download_directory(&link)
            .await
            .expect_err("symlinked directory should be rejected");

        assert!(error.to_string().contains("symbolic link"));
        assert!(std::fs::read_dir(target).unwrap().next().is_none());
    }

    #[tokio::test]
    async fn local_upload_directory_root_allows_directory() {
        let temp = tempfile::tempdir().unwrap();

        ensure_local_upload_directory_root(temp.path())
            .await
            .expect("directory root should be uploadable");
    }

    #[tokio::test]
    async fn local_upload_directory_root_rejects_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file.txt");
        std::fs::write(&file, "content").unwrap();

        let error = ensure_local_upload_directory_root(&file)
            .await
            .expect_err("regular file should not be accepted as a recursive upload root");

        assert!(error.to_string().contains("not a directory"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_upload_directory_root_rejects_symlinked_directory() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = ensure_local_upload_directory_root(&link)
            .await
            .expect_err("symlinked directory root should be rejected");

        assert!(error.to_string().contains("symbolic link"));
    }

    #[tokio::test]
    async fn local_upload_entry_kind_classifies_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file.txt");
        std::fs::write(&file, "content").unwrap();

        let kind = local_upload_entry_kind(&file)
            .await
            .expect("regular file should be classifiable");

        assert_eq!(kind, Some(LocalUploadEntryKind::File));
    }

    #[tokio::test]
    async fn local_upload_entry_kind_classifies_directory() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("dir");
        std::fs::create_dir(&dir).unwrap();

        let kind = local_upload_entry_kind(&dir)
            .await
            .expect("directory should be classifiable");

        assert_eq!(kind, Some(LocalUploadEntryKind::Directory));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_upload_entry_kind_skips_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        std::fs::write(&target, "content").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let kind = local_upload_entry_kind(&link)
            .await
            .expect("symlink metadata should be readable");

        assert_eq!(kind, None);
    }

    #[tokio::test]
    async fn local_upload_file_source_allows_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file.txt");
        std::fs::write(&file, "content").unwrap();

        ensure_local_upload_file_source(&file)
            .await
            .expect("regular file should be uploadable");
    }

    #[tokio::test]
    async fn local_upload_file_source_rejects_directory() {
        let temp = tempfile::tempdir().unwrap();

        let error = ensure_local_upload_file_source(temp.path())
            .await
            .expect_err("directory should not be accepted as a file upload source");

        assert!(error.to_string().contains("not a regular file"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn local_upload_file_source_rejects_symlinked_file() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        std::fs::write(&target, "content").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = ensure_local_upload_file_source(&link)
            .await
            .expect_err("symlinked file should be rejected");

        assert!(error.to_string().contains("symbolic link"));
    }

    #[tokio::test]
    async fn open_local_upload_file_source_reads_regular_file() {
        use tokio::io::AsyncReadExt as _;

        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file.txt");
        std::fs::write(&file, "content").unwrap();

        let mut opened = open_local_upload_file_source(&file)
            .await
            .expect("regular file should open for upload");
        let mut content = String::new();
        opened.read_to_string(&mut content).await.unwrap();

        assert_eq!(content, "content");
    }

    #[tokio::test]
    async fn open_local_upload_file_source_rejects_directory() {
        let temp = tempfile::tempdir().unwrap();

        let error = open_local_upload_file_source(temp.path())
            .await
            .expect_err("directory should not open for upload");

        assert!(error.to_string().contains("not a regular file"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn open_local_upload_file_source_rejects_symlinked_file() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        std::fs::write(&target, "content").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = open_local_upload_file_source(&link)
            .await
            .expect_err("symlinked file should not open for upload");

        assert!(error.to_string().contains("symbolic link"));
    }

    // === Error message formatting patterns ===

    #[test]
    fn error_message_format_read_dir() {
        let path_str = "/nonexistent/path";
        let error = "Permission denied";
        let msg = format!("Failed to read directory {}: {}", path_str, error);

        assert!(msg.contains(path_str));
        assert!(msg.contains(error));
        assert!(msg.starts_with("Failed to read directory"));
    }

    #[test]
    fn error_message_format_create_dir() {
        let path_str = "/readonly/newdir";
        let error = "Read-only file system";
        let msg = format!("Failed to create directory {}: {}", path_str, error);

        assert!(msg.contains(path_str));
        assert!(msg.contains(error));
    }

    #[test]
    fn error_message_format_rename() {
        let old = "/tmp/old";
        let new = "/tmp/new";
        let error = "Cross-device link";
        let msg = format!("Failed to rename {} to {}: {}", old, new, error);

        assert!(msg.contains(old));
        assert!(msg.contains(new));
        assert!(msg.contains(error));
    }

    #[test]
    fn error_message_format_transfer() {
        let remote = "/remote/file.txt";
        let local = "/local/file.txt";
        let error = "Connection reset";
        let msg = format!("Failed to download {} to {}: {}", remote, local, error);

        assert!(msg.contains(remote));
        assert!(msg.contains(local));
        assert!(msg.contains(error));
    }
}
