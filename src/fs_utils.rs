//! Local filesystem utilities
//!
//! This module provides common file operations used throughout the application,
//! consolidating functionality that was previously duplicated in app/actions.rs.

use std::path::Path;

/// Recursively copy a directory from source to target.
///
/// Creates the target directory and all parent directories if they don't exist.
/// Only copies regular files and directories; symlinks are skipped for safety.
pub fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
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

/// Count items in a directory recursively.
///
/// Counts all regular files in the directory tree. Directories themselves
/// are not counted, but their contents are.
pub fn count_items_in_dir(dir: &Path) -> Result<usize, String> {
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
}
