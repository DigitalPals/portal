//! SFTP session for file operations

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use russh::client::Handle;
use russh::Disconnect;
use russh_sftp::client::SftpSession as RusshSftpSession;
use tokio::sync::Mutex;

use crate::error::SftpError;
use crate::ssh::handler::ClientHandler;

use super::types::FileEntry;

/// SFTP session wrapper for file operations
pub struct SftpSession {
    sftp: Mutex<RusshSftpSession>,
    handle: Mutex<Handle<ClientHandler>>,
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
    pub fn new(sftp: RusshSftpSession, handle: Handle<ClientHandler>, home_dir: PathBuf) -> Self {
        Self {
            sftp: Mutex::new(sftp),
            handle: Mutex::new(handle),
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

    /// Create a directory
    pub async fn create_dir(&self, path: &Path) -> Result<(), SftpError> {
        let sftp = self.sftp.lock().await;
        let path_str = path.to_string_lossy().to_string();

        sftp.create_dir(path_str.clone()).await.map_err(|e| {
            SftpError::FileOperation(format!("Failed to create directory {}: {}", path_str, e))
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
        // First check if it's a directory
        let is_dir = {
            let sftp = self.sftp.lock().await;
            let path_str = path.to_string_lossy().to_string();
            let metadata = sftp.metadata(path_str.clone()).await.map_err(|e| {
                SftpError::FileOperation(format!("Failed to get metadata for {}: {}", path_str, e))
            })?;
            metadata.is_dir()
        };

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

    /// Rename a file or directory
    pub async fn rename(&self, from: &Path, to: &Path) -> Result<(), SftpError> {
        let sftp = self.sftp.lock().await;
        let from_str = from.to_string_lossy().to_string();
        let to_str = to.to_string_lossy().to_string();

        sftp.rename(from_str.clone(), to_str.clone())
            .await
            .map_err(|e| {
                SftpError::FileOperation(format!(
                    "Failed to rename {} to {}: {}",
                    from_str, to_str, e
                ))
            })
    }

    /// Download a file from remote to local
    pub async fn download(&self, remote_path: &Path, local_path: &Path) -> Result<u64, SftpError> {
        let sftp = self.sftp.lock().await;
        let remote_str = remote_path.to_string_lossy().to_string();

        // Read file from remote
        let contents = sftp.read(remote_str.clone()).await.map_err(|e| {
            SftpError::Transfer(format!("Failed to read remote file {}: {}", remote_str, e))
        })?;

        // Write to local file
        tokio::fs::write(local_path, &contents).await.map_err(|e| {
            SftpError::LocalIo(format!(
                "Failed to write local file {}: {}",
                local_path.display(),
                e
            ))
        })?;

        Ok(contents.len() as u64)
    }

    /// Upload a file from local to remote
    pub async fn upload(&self, local_path: &Path, remote_path: &Path) -> Result<u64, SftpError> {
        // Read local file
        let contents = tokio::fs::read(local_path).await.map_err(|e| {
            SftpError::LocalIo(format!(
                "Failed to read local file {}: {}",
                local_path.display(),
                e
            ))
        })?;

        let sftp = self.sftp.lock().await;
        let remote_str = remote_path.to_string_lossy().to_string();

        // Write to remote file
        sftp.write(remote_str.clone(), &contents)
            .await
            .map_err(|e| {
                SftpError::Transfer(format!("Failed to write remote file {}: {}", remote_str, e))
            })?;

        Ok(contents.len() as u64)
    }

    /// Download a file or directory recursively from remote to local
    pub async fn download_recursive(
        &self,
        remote_path: &Path,
        local_path: &Path,
    ) -> Result<(), SftpError> {
        let is_dir = {
            let sftp = self.sftp.lock().await;
            let remote_str = remote_path.to_string_lossy().to_string();
            let metadata = sftp.metadata(remote_str.clone()).await.map_err(|e| {
                SftpError::FileOperation(format!(
                    "Failed to get metadata for {}: {}",
                    remote_str, e
                ))
            })?;
            metadata.is_dir()
        };

        if is_dir {
            // Create local directory
            tokio::fs::create_dir_all(local_path).await.map_err(|e| {
                SftpError::LocalIo(format!(
                    "Failed to create directory {}: {}",
                    local_path.display(),
                    e
                ))
            })?;

            // Download contents
            let entries = self.list_dir(remote_path).await?;
            for entry in entries {
                if entry.name == ".." {
                    continue;
                }
                let local_dest = local_path.join(&entry.name);
                Box::pin(self.download_recursive(&entry.path, &local_dest)).await?;
            }
            Ok(())
        } else {
            // Ensure parent directory exists
            if let Some(parent) = local_path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    SftpError::LocalIo(format!(
                        "Failed to create directory {}: {}",
                        parent.display(),
                        e
                    ))
                })?;
            }
            self.download(remote_path, local_path).await?;
            Ok(())
        }
    }

    /// Upload a file or directory recursively from local to remote
    pub async fn upload_recursive(
        &self,
        local_path: &Path,
        remote_path: &Path,
    ) -> Result<(), SftpError> {
        let metadata = tokio::fs::metadata(local_path).await.map_err(|e| {
            SftpError::LocalIo(format!(
                "Failed to get metadata for {}: {}",
                local_path.display(),
                e
            ))
        })?;

        if metadata.is_dir() {
            // Create remote directory (ignore error if exists)
            let _ = self.create_dir(remote_path).await;

            // Upload contents
            let mut read_dir = tokio::fs::read_dir(local_path).await.map_err(|e| {
                SftpError::LocalIo(format!(
                    "Failed to read directory {}: {}",
                    local_path.display(),
                    e
                ))
            })?;

            while let Some(entry) = read_dir
                .next_entry()
                .await
                .map_err(|e| SftpError::LocalIo(format!("Failed to read directory entry: {}", e)))?
            {
                let local_entry_path = entry.path();
                let remote_dest = remote_path.join(entry.file_name());
                Box::pin(self.upload_recursive(&local_entry_path, &remote_dest)).await?;
            }
            Ok(())
        } else {
            self.upload(local_path, remote_path).await?;
            Ok(())
        }
    }

    /// Close the SFTP session
    pub async fn close(self) -> Result<(), SftpError> {
        let handle = self.handle.into_inner();
        handle
            .disconnect(Disconnect::ByApplication, "SFTP session closed", "")
            .await
            .map_err(|e| SftpError::ConnectionFailed(format!("Failed to disconnect: {}", e)))?;
        Ok(())
    }
}

/// Thread-safe wrapper for SFTP session
pub type SharedSftpSession = Arc<SftpSession>;
