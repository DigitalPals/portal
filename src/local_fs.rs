//! Local filesystem operations for the dual-pane SFTP browser

use std::path::Path;

use chrono::{TimeZone, Utc};

use crate::sftp::FileEntry;

/// List local directory contents
pub async fn list_local_dir(path: &Path) -> Result<Vec<FileEntry>, String> {
    let path = path.to_path_buf();

    tokio::task::spawn_blocking(move || list_local_dir_sync(&path))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

/// Synchronous version of directory listing
fn list_local_dir_sync(path: &Path) -> Result<Vec<FileEntry>, String> {
    let mut result = Vec::new();

    // Add parent directory entry if there is a real navigable parent.
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

    let entries =
        std::fs::read_dir(path).map_err(|e| format!("Failed to read directory: {}", e))?;

    for entry in entries {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let entry_path = entry.path();
        let metadata = std::fs::symlink_metadata(&entry_path)
            .map_err(|e| format!("Failed to read metadata: {}", e))?;
        let target_metadata = if metadata.file_type().is_symlink() {
            std::fs::metadata(&entry_path).ok()
        } else {
            None
        };

        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = target_metadata
            .as_ref()
            .map(|meta| meta.is_dir())
            .unwrap_or_else(|| metadata.is_dir());
        let is_symlink = metadata.file_type().is_symlink();
        let size = target_metadata
            .as_ref()
            .map(|meta| meta.len())
            .unwrap_or_else(|| metadata.len());

        let modified = target_metadata
            .as_ref()
            .and_then(|meta| meta.modified().ok())
            .or_else(|| metadata.modified().ok())
            .and_then(|mtime| {
                let duration = mtime.duration_since(std::time::UNIX_EPOCH).ok()?;
                let seconds = i64::try_from(duration.as_secs()).ok()?;
                Utc.timestamp_opt(seconds, 0).single()
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

fn parent_entry_path(path: &Path) -> Option<&Path> {
    let parent = path.parent()?;
    if parent.as_os_str().is_empty() {
        return None;
    }
    Some(parent)
}

#[cfg(test)]
mod tests {
    use super::parent_entry_path;
    use std::path::Path;

    #[test]
    fn parent_entry_path_skips_relative_empty_parent() {
        assert!(parent_entry_path(Path::new(".")).is_none());
        assert!(parent_entry_path(Path::new("relative")).is_none());
    }

    #[test]
    fn parent_entry_path_keeps_absolute_parent() {
        assert_eq!(parent_entry_path(Path::new("/home")), Some(Path::new("/")));
    }
}
