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
    ensure_local_dir_root(path)?;

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

fn ensure_local_dir_root(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read directory metadata: {}", e))?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Err(format!(
            "Refusing to read local directory through symbolic link {}",
            path.display()
        ));
    }

    if !file_type.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }

    Ok(())
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
    use super::{ensure_local_dir_root, list_local_dir_sync, parent_entry_path};
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

    #[test]
    fn list_local_dir_marks_regular_directory_navigable() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir(temp.path().join("dir")).unwrap();

        let entries = list_local_dir_sync(temp.path()).unwrap();
        let dir = entries
            .iter()
            .find(|entry| entry.name == "dir")
            .expect("directory should be listed");

        assert!(dir.is_dir);
        assert!(!dir.is_symlink);
        assert!(dir.is_navigable_dir());
    }

    #[test]
    fn ensure_local_dir_root_rejects_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("file");
        std::fs::write(&path, "content").unwrap();

        let error = ensure_local_dir_root(&path).expect_err("file should not be listed as a dir");

        assert!(error.contains("not a directory"));
    }

    #[cfg(unix)]
    #[test]
    fn list_local_dir_marks_symlinked_directory_non_navigable() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let entries = list_local_dir_sync(temp.path()).unwrap();
        let link_entry = entries
            .iter()
            .find(|entry| entry.name == "link")
            .expect("symlink should be listed");

        assert!(link_entry.is_dir);
        assert!(link_entry.is_symlink);
        assert!(!link_entry.is_navigable_dir());
    }

    #[cfg(unix)]
    #[test]
    fn list_local_dir_lists_broken_symlink_as_non_navigable_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let link = temp.path().join("broken");
        std::os::unix::fs::symlink(temp.path().join("missing"), &link).unwrap();

        let entries = list_local_dir_sync(temp.path()).unwrap();
        let link_entry = entries
            .iter()
            .find(|entry| entry.name == "broken")
            .expect("broken symlink should be listed");

        assert!(!link_entry.is_dir);
        assert!(link_entry.is_symlink);
        assert!(!link_entry.is_navigable_dir());
    }

    #[cfg(unix)]
    #[test]
    fn list_local_dir_rejects_symlinked_root_without_listing_target() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = list_local_dir_sync(&link).expect_err("symlink root should not be listed");

        assert!(error.contains("symbolic link"));
    }
}
