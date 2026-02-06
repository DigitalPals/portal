//! SFTP session for file operations

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{TimeZone, Utc};
use russh_sftp::client::SftpSession as RusshSftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::fs::OpenOptions;
use tokio::io;

use tokio::sync::Mutex;

use crate::error::SftpError;

use super::types::FileEntry;

/// SFTP session wrapper for file operations
pub struct SftpSession {
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
    pub fn new(sftp: RusshSftpSession, home_dir: PathBuf) -> Self {
        Self {
            sftp: Arc::new(Mutex::new(sftp)),
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
        let mtime: u64 = 1704067200; // 2024-01-01 00:00:00 UTC
        let datetime = Utc
            .timestamp_opt(mtime as i64, 0)
            .single()
            .unwrap_or_else(Utc::now);

        assert_eq!(datetime.year(), 2024);
        assert_eq!(datetime.month(), 1);
        assert_eq!(datetime.day(), 1);
    }

    #[test]
    fn timestamp_conversion_zero() {
        let mtime: u64 = 0; // Unix epoch
        let datetime = Utc
            .timestamp_opt(mtime as i64, 0)
            .single()
            .unwrap_or_else(Utc::now);

        assert_eq!(datetime.year(), 1970);
        assert_eq!(datetime.month(), 1);
        assert_eq!(datetime.day(), 1);
    }

    #[test]
    fn timestamp_conversion_recent() {
        let mtime: u64 = 1700000000; // Nov 2023
        let datetime = Utc
            .timestamp_opt(mtime as i64, 0)
            .single()
            .unwrap_or_else(Utc::now);

        assert_eq!(datetime.year(), 2023);
        assert_eq!(datetime.month(), 11);
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
        let path_str = path.to_string_lossy().to_string();

        let should_add = path.parent().is_some() && path_str != "/";
        assert!(should_add);
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
