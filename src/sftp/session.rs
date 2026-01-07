//! SFTP session for file operations

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use russh_sftp::client::SftpSession as RusshSftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::fs::OpenOptions;
use tokio::io;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use tokio::sync::Mutex;

use crate::error::SftpError;

use super::types::FileEntry;

/// SFTP session wrapper for file operations
pub struct SftpSession {
    sftp: Mutex<RusshSftpSession>,
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
    pub fn new(sftp: RusshSftpSession, home_dir: PathBuf) -> Self {
        Self {
            sftp: Mutex::new(sftp),
            home_dir,
        }
    }

    /// Get the remote home directory
    pub fn home_dir(&self) -> &Path {
        &self.home_dir
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
        if path.parent().is_some() && path_str != "/" {
            result.push(FileEntry {
                name: "..".to_string(),
                path: path.parent().unwrap_or(Path::new("/")).to_path_buf(),
                is_dir: true,
                is_symlink: false,
                size: 0,
                modified: None,
            });
        }

        for entry in read_dir {
            let name = entry.file_name();
            let metadata = entry.metadata();
            let entry_path = path.join(&name);

            let is_dir = metadata.is_dir();
            let is_symlink = metadata.is_symlink();
            let size = metadata.size.unwrap_or(0);

            // Convert mtime to DateTime if available
            let modified = metadata.mtime.map(|mtime| {
                Utc.timestamp_opt(mtime as i64, 0)
                    .single()
                    .unwrap_or_else(Utc::now)
            });

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
        Ok(metadata.size.unwrap_or(0))
    }

    /// Create a directory
    pub async fn create_dir(&self, path: &Path) -> Result<(), SftpError> {
        let sftp = self.sftp.lock().await;
        let path_str = path.to_string_lossy().to_string();

        match sftp.try_exists(path_str.clone()).await {
            Ok(true) => return Ok(()),
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
        if let Some(parent) = local_path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                SftpError::LocalIo(format!(
                    "Failed to create local directory {}: {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        let sftp = self.sftp.lock().await;
        let remote_str = remote_path.to_string_lossy().to_string();

        let mut remote = sftp.open(remote_str.clone()).await.map_err(|e| {
            SftpError::Transfer(format!("Failed to open remote file {}: {}", remote_str, e))
        })?;

        let mut local = {
            let mut options = OpenOptions::new();
            options.create(true).write(true).truncate(true);
            #[cfg(unix)]
            {
                options.mode(0o600);
            }
            options.open(local_path).await.map_err(|e| {
                SftpError::LocalIo(format!(
                    "Failed to write local file {}: {}",
                    local_path.display(),
                    e
                ))
            })?
        };

        let bytes = io::copy(&mut remote, &mut local).await.map_err(|e| {
            SftpError::Transfer(format!(
                "Failed to download {} to {}: {}",
                remote_str,
                local_path.display(),
                e
            ))
        })?;

        Ok(bytes)
    }

    /// Upload a file from local to remote
    pub async fn upload(&self, local_path: &Path, remote_path: &Path) -> Result<u64, SftpError> {
        let mut local = tokio::fs::File::open(local_path).await.map_err(|e| {
            SftpError::LocalIo(format!(
                "Failed to read local file {}: {}",
                local_path.display(),
                e
            ))
        })?;

        let sftp = self.sftp.lock().await;
        let remote_str = remote_path.to_string_lossy().to_string();

        let mut remote = sftp
            .open_with_flags(
                remote_str.clone(),
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
            )
            .await
            .map_err(|e| {
                SftpError::Transfer(format!("Failed to open remote file {}: {}", remote_str, e))
            })?;

        let bytes = io::copy(&mut local, &mut remote).await.map_err(|e| {
            SftpError::Transfer(format!(
                "Failed to upload {} to {}: {}",
                local_path.display(),
                remote_str,
                e
            ))
        })?;

        Ok(bytes)
    }

    /// Download a directory recursively from remote to local
    pub async fn download_recursive(
        &self,
        remote_path: &Path,
        local_path: &Path,
    ) -> Result<usize, SftpError> {
        // Create local directory
        tokio::fs::create_dir_all(local_path).await.map_err(|e| {
            SftpError::LocalIo(format!(
                "Failed to create local directory {}: {}",
                local_path.display(),
                e
            ))
        })?;

        let entries = self.list_dir(remote_path).await?;
        let mut count = 0;

        for entry in entries {
            if entry.name == ".." {
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
        // Create remote directory
        self.create_dir(remote_path).await?;

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
            let file_type = entry
                .file_type()
                .await
                .map_err(|e| SftpError::LocalIo(format!("Failed to get file type: {}", e)))?;

            let entry_name = entry.file_name();
            let local_entry_path = entry.path();
            let remote_entry_path = remote_path.join(&entry_name);

            if file_type.is_dir() {
                count +=
                    Box::pin(self.upload_recursive(&local_entry_path, &remote_entry_path)).await?;
            } else if file_type.is_file() {
                self.upload(&local_entry_path, &remote_entry_path).await?;
                count += 1;
            }
            // Skip symlinks for now
        }

        Ok(count)
    }
}

/// Thread-safe wrapper for SFTP session
pub type SharedSftpSession = Arc<SftpSession>;
