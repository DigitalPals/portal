//! Local filesystem utilities
//!
//! This module provides common file operations used throughout the application,
//! consolidating functionality that was previously duplicated in app/actions.rs.

use std::path::Path;

/// Reject copying a file over itself.
///
/// This protects the single-file copy path; directory copies use
/// `copy_dir_recursive`, which has stricter descendant checks.
pub fn ensure_not_same_path(source: &Path, target: &Path) -> Result<(), String> {
    let source = source
        .canonicalize()
        .map_err(|e| format!("Failed to resolve source {}: {}", source.display(), e))?;

    if let Ok(target) = target.canonicalize() {
        if target == source {
            return Err(format!("Cannot copy {} onto itself", source.display()));
        }
    }

    Ok(())
}

/// Reject symlink paths before copy helpers recurse or copy target contents.
pub fn ensure_not_symlink(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {}", path.display(), e))?;

    if metadata.file_type().is_symlink() {
        return Err(format!("Cannot copy symbolic link {}", path.display()));
    }

    Ok(())
}

/// Recursively copy a directory from source to target.
///
/// Creates the target directory and all parent directories if they don't exist.
/// Only copies regular files and directories; symlinks are skipped for safety.
pub fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
    ensure_not_symlink(source)?;
    ensure_target_not_inside_source(source, target)?;

    std::fs::create_dir_all(target)
        .map_err(|e| format!("Failed to create directory {}: {}", target.display(), e))?;

    for entry in std::fs::read_dir(source)
        .map_err(|e| format!("Failed to read directory {}: {}", source.display(), e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Failed to get file type: {}", e))?;

        let source_path = entry.path();
        let target_path = target.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&source_path, &target_path)?;
        } else if file_type.is_file() {
            std::fs::copy(&source_path, &target_path)
                .map_err(|e| format!("Failed to copy {}: {}", source_path.display(), e))?;
        }
        // Skip symlinks for safety
    }

    Ok(())
}

fn ensure_target_not_inside_source(source: &Path, target: &Path) -> Result<(), String> {
    let source = source
        .canonicalize()
        .map_err(|e| format!("Failed to resolve source {}: {}", source.display(), e))?;

    if let Ok(target) = target.canonicalize() {
        if target == source || target.starts_with(&source) {
            return Err(format!(
                "Cannot copy {} into itself at {}",
                source.display(),
                target.display()
            ));
        }
        return Ok(());
    }

    for ancestor in target.ancestors().skip(1) {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        if let Ok(ancestor) = ancestor.canonicalize() {
            if ancestor == source || ancestor.starts_with(&source) {
                return Err(format!(
                    "Cannot copy {} into itself at {}",
                    source.display(),
                    target.display()
                ));
            }
            break;
        }
    }

    Ok(())
}

/// Count items in a directory recursively.
///
/// Counts all regular files in the directory tree. Directories themselves
/// are not counted, but their contents are.
pub fn count_items_in_dir(dir: &Path) -> Result<usize, String> {
    ensure_not_symlink(dir)?;

    let mut count = 0;

    for entry in std::fs::read_dir(dir)
        .map_err(|e| format!("Failed to read directory {}: {}", dir.display(), e))?
    {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("Failed to get file type: {}", e))?;

        if file_type.is_dir() {
            count += count_items_in_dir(&entry.path())?;
        } else if file_type.is_file() {
            count += 1;
        }
    }

    Ok(count)
}

/// Remove a directory tree (async) for temp cleanup.
pub async fn cleanup_temp_dir(path: &Path) -> Result<(), String> {
    match tokio::fs::remove_dir_all(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to remove {}: {}", path.display(), e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_count_empty_dir() {
        let dir = tempdir().unwrap();
        assert_eq!(count_items_in_dir(dir.path()).unwrap(), 0);
    }

    #[test]
    fn test_count_items() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("file1.txt"), "content").unwrap();
        fs::write(dir.path().join("file2.txt"), "content").unwrap();
        fs::create_dir(dir.path().join("subdir")).unwrap();
        fs::write(dir.path().join("subdir/file3.txt"), "content").unwrap();

        assert_eq!(count_items_in_dir(dir.path()).unwrap(), 3);
    }

    #[test]
    fn test_copy_dir_recursive() {
        let source = tempdir().unwrap();
        let target = tempdir().unwrap();

        // Create source structure
        fs::write(source.path().join("file1.txt"), "content1").unwrap();
        fs::create_dir(source.path().join("subdir")).unwrap();
        fs::write(source.path().join("subdir/file2.txt"), "content2").unwrap();

        // Copy
        let target_path = target.path().join("copied");
        copy_dir_recursive(source.path(), &target_path).unwrap();

        // Verify
        assert!(target_path.join("file1.txt").exists());
        assert!(target_path.join("subdir/file2.txt").exists());
        assert_eq!(
            fs::read_to_string(target_path.join("file1.txt")).unwrap(),
            "content1"
        );
    }

    #[test]
    fn ensure_not_same_path_rejects_same_file() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("file.txt");
        fs::write(&file, "content").unwrap();

        assert!(ensure_not_same_path(&file, &file).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_not_same_path_rejects_symlink_to_source() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("file.txt");
        let link = dir.path().join("link.txt");
        fs::write(&file, "content").unwrap();
        std::os::unix::fs::symlink(&file, &link).unwrap();

        assert!(ensure_not_same_path(&file, &link).is_err());
    }

    #[test]
    fn copy_dir_recursive_rejects_target_inside_source() {
        let source = tempdir().unwrap();
        fs::write(source.path().join("file1.txt"), "content1").unwrap();

        let target_path = source.path().join("copied");
        let result = copy_dir_recursive(source.path(), &target_path);

        assert!(result.is_err());
        assert!(!target_path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn copy_helpers_reject_symlinked_directory_roots() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("secret.txt"), "secret").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(copy_dir_recursive(&link, &temp.path().join("copied")).is_err());
        assert!(count_items_in_dir(&link).is_err());
    }

    #[tokio::test]
    async fn cleanup_temp_dir_removes_tree() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("file.txt"), "content").unwrap();

        cleanup_temp_dir(dir.path()).await.unwrap();
        assert!(!dir.path().exists());
    }

    #[tokio::test]
    async fn cleanup_temp_dir_ignores_missing_dir() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("missing");

        cleanup_temp_dir(&missing).await.unwrap();
    }
}
