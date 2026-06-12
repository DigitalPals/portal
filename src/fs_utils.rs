//! Local filesystem utilities
//!
//! This module provides common file operations used throughout the application,
//! consolidating functionality that was previously duplicated in app/actions.rs.

use std::io::{Read, Write};
use std::path::Path;
use uuid::Uuid;

/// Reject copying a file over itself.
///
/// This protects the single-file copy path; directory copies use
/// `copy_dir_recursive`, which has stricter descendant checks.
pub fn ensure_not_same_path(source: &Path, target: &Path) -> Result<(), String> {
    let source = source
        .canonicalize()
        .map_err(|e| format!("Failed to resolve source {}: {}", source.display(), e))?;

    if let Ok(target) = target.canonicalize()
        && target == source
    {
        return Err(format!("Cannot copy {} onto itself", source.display()));
    }

    Ok(())
}

/// Reject an existing destination symlink before copy helpers write through it.
pub fn ensure_destination_not_symlink(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("Cannot copy over symbolic link {}", path.display()))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Failed to read metadata for {}: {}",
            path.display(),
            error
        )),
    }
}

fn invalid_input(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message)
}

fn invalid_data(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

fn file_too_large(message: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::FileTooLarge, message)
}

fn read_to_end_enforcing_limit<R: Read>(
    reader: &mut R,
    initial_size: u64,
    limit: u64,
    label: &str,
) -> std::io::Result<Vec<u8>> {
    let capacity_limit = if limit == 0 {
        initial_size
    } else {
        initial_size.min(limit)
    };
    let capacity = usize::try_from(capacity_limit).unwrap_or(usize::MAX);
    let mut data = Vec::with_capacity(capacity);

    if limit == 0 {
        reader.read_to_end(&mut data)?;
        return Ok(data);
    }

    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut data)?;
    if u64::try_from(data.len()).unwrap_or(u64::MAX) > limit {
        return Err(file_too_large(format!(
            "{} file is too large (read more than {} bytes)",
            label, limit
        )));
    }

    Ok(data)
}

/// Make a directory owner-only without following a symlink at the final path component.
#[cfg(unix)]
pub fn set_private_dir_permissions_no_follow(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    set_directory_permissions_no_follow(path, std::fs::Permissions::from_mode(0o700))
}

/// Set directory permissions without following a symlink at the final path component.
#[cfg(unix)]
fn set_directory_permissions_no_follow(
    path: &Path,
    permissions: std::fs::Permissions,
) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;

    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("{} is not a directory", path.display()),
        ));
    }
    file.set_permissions(permissions)
}

#[cfg(not(unix))]
fn set_directory_permissions_no_follow(
    path: &Path,
    permissions: std::fs::Permissions,
) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(invalid_input(format!(
            "{} is a symbolic link",
            path.display()
        )));
    }
    if !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("{} is not a directory", path.display()),
        ));
    }
    std::fs::set_permissions(path, permissions)
}

fn ensure_regular_file_source(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {}", path.display(), e))?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Err(format!("Cannot copy symbolic link {}", path.display()));
    }

    if !file_type.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }

    Ok(())
}

fn ensure_readable_regular_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to stat {} file: {}", label, e))?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Err(format!("{} file is a symbolic link", label));
    }

    if !file_type.is_file() {
        return Err(format!("{} file is not a regular file", label));
    }

    Ok(())
}

fn ensure_directory_root(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {}", path.display(), e))?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Err(format!("Cannot copy symbolic link {}", path.display()));
    }

    if !file_type.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }

    Ok(())
}

/// Open a log file for appending without writing through symlinks or special files.
pub fn open_append_regular_file(path: &Path) -> std::io::Result<std::fs::File> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(invalid_input(format!(
                "Refusing to append through symbolic link {}",
                path.display()
            )));
        }
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(invalid_input(format!(
                "Refusing to append to non-regular file {}",
                path.display()
            )));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let file = open_append_regular_file_unchecked(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(invalid_input(format!(
            "Refusing to append to non-regular file {}",
            path.display()
        )));
    }

    Ok(file)
}

/// Open a directory handle for fsync without blocking on special files.
pub fn open_directory_for_sync(path: &Path) -> std::io::Result<std::fs::File> {
    let file = open_directory_for_sync_unchecked(path)?;
    if !file.metadata()?.is_dir() {
        return Err(invalid_input(format!(
            "{} is not a directory",
            path.display()
        )));
    }
    Ok(file)
}

/// Best-effort fsync of a path's parent directory after entry changes.
pub fn sync_parent_dir(path: &Path) {
    if let Some(parent) = path.parent()
        && let Ok(dir) = open_directory_for_sync(parent)
    {
        let _ = dir.sync_all();
    }
}

/// Create or validate a directory without following a symlink at the final path.
pub fn ensure_dir_no_follow(path: &Path) -> std::io::Result<()> {
    ensure_dir_no_follow_with(path, |_| Ok(()))
}

/// Create or validate an owner-only directory without following a symlink at the final path.
pub fn ensure_private_dir_no_follow(path: &Path) -> std::io::Result<()> {
    ensure_dir_no_follow_with(path, |path| {
        #[cfg(unix)]
        {
            set_private_dir_permissions_no_follow(path)
        }

        #[cfg(not(unix))]
        {
            let _ = path;
            Ok(())
        }
    })
}

fn ensure_dir_no_follow_with<F>(path: &Path, harden: F) -> std::io::Result<()>
where
    F: Fn(&Path) -> std::io::Result<()>,
{
    match std::fs::symlink_metadata(path) {
        Ok(metadata) => {
            validate_directory_metadata_no_follow(path, &metadata)?;
            harden(path)?;
            return Ok(());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    std::fs::create_dir_all(path)?;

    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            let _ = cleanup_created_path(path);
            return Err(error);
        }
    };
    if let Err(error) = validate_directory_metadata_no_follow(path, &metadata) {
        let _ = cleanup_created_path(path);
        return Err(error);
    }

    if let Err(error) = harden(path) {
        let _ = cleanup_created_path(path);
        return Err(error);
    }

    sync_parent_dir(path);
    Ok(())
}

fn validate_directory_metadata_no_follow(
    path: &Path,
    metadata: &std::fs::Metadata,
) -> std::io::Result<()> {
    if metadata.file_type().is_symlink() {
        return Err(invalid_input(format!(
            "Refusing to use symbolic link directory {}",
            path.display()
        )));
    }
    if !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("{} is not a directory", path.display()),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn open_directory_for_sync_unchecked(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_directory_for_sync_unchecked(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// Create a new regular file without following a symlink at the final path component.
pub fn create_new_regular_file_no_follow(path: &Path) -> std::io::Result<std::fs::File> {
    let file = create_new_regular_file_no_follow_unchecked(path)?;
    if !file.metadata()?.file_type().is_file() {
        return Err(invalid_input(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    Ok(file)
}

#[cfg(unix)]
fn create_new_regular_file_no_follow_unchecked(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn create_new_regular_file_no_follow_unchecked(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(unix)]
fn open_append_regular_file_unchecked(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_append_regular_file_unchecked(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// Read a regular file without following a symlink at the final path component.
pub fn open_read_regular_file(path: &Path, label: &str) -> Result<std::fs::File, String> {
    ensure_readable_regular_file(path, label)?;

    let file = open_read_regular_file_unchecked(path)
        .map_err(|e| format!("Failed to open {} file: {}", label, e))?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("Failed to stat {} file: {}", label, e))?;
    if !metadata.file_type().is_file() {
        return Err(format!("{} file is not a regular file", label));
    }

    Ok(file)
}

/// Read a regular file without following a symlink at the final path component.
pub fn read_regular_file_limited(path: &Path, limit: u64, label: &str) -> Result<Vec<u8>, String> {
    let mut file = open_read_regular_file(path, label)?;
    let metadata = file
        .metadata()
        .map_err(|e| format!("Failed to stat {} file: {}", label, e))?;
    let size = metadata.len();
    if limit > 0 && size > limit {
        return Err(format!(
            "{} file too large ({} bytes, limit {})",
            label, size, limit
        ));
    }

    read_to_end_enforcing_limit(&mut file, size, limit, label)
        .map_err(|e| format!("Failed to read {} file: {}", label, e))
}

/// Read a UTF-8 regular file without following a symlink at the final path component.
pub fn read_regular_file_to_string(path: &Path, label: &str) -> std::io::Result<String> {
    read_regular_file_to_string_limited_io(path, 0, label)
}

/// Read a UTF-8 regular file without following a symlink at the final path component.
pub fn read_regular_file_to_string_limited_io(
    path: &Path,
    limit: u64,
    label: &str,
) -> std::io::Result<String> {
    let metadata = std::fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();

    if file_type.is_symlink() {
        return Err(invalid_input(format!(
            "{} file is a symbolic link: {}",
            label,
            path.display()
        )));
    }

    if !file_type.is_file() {
        return Err(invalid_input(format!(
            "{} file is not a regular file: {}",
            label,
            path.display()
        )));
    }

    let mut file = open_read_regular_file_unchecked(path)?;
    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() {
        return Err(invalid_input(format!(
            "{} file is not a regular file: {}",
            label,
            path.display()
        )));
    }
    if limit > 0 && metadata.len() > limit {
        return Err(file_too_large(format!(
            "{} file is too large ({} bytes, limit {})",
            label,
            metadata.len(),
            limit
        )));
    }

    let data = read_to_end_enforcing_limit(&mut file, metadata.len(), limit, label)?;
    String::from_utf8(data).map_err(|error| invalid_data(error.to_string()))
}

/// Read a UTF-8 regular file without following a symlink at the final path component.
pub fn read_regular_file_to_string_limited(
    path: &Path,
    limit: u64,
    label: &str,
) -> Result<String, String> {
    read_regular_file_to_string_limited_io(path, limit, label)
        .map_err(|error| format!("Failed to read {} file: {}", label, error))
}

/// Read a UTF-8 regular file while allowing a symlink at the final path component.
///
/// This is for OpenSSH-compatible paths where following user-created symlinks is expected.
/// The opened handle is rechecked before reading to avoid trusting path metadata alone.
pub fn read_regular_file_follow_symlink_to_string_limited(
    path: &Path,
    limit: u64,
    label: &str,
) -> std::io::Result<String> {
    let metadata = std::fs::metadata(path)?;
    validate_followed_regular_file_metadata(&metadata, limit, label)?;

    let mut file = open_followed_read_file_unchecked(path)?;
    let metadata = file.metadata()?;
    validate_followed_regular_file_metadata(&metadata, limit, label)?;

    let data = read_to_end_enforcing_limit(&mut file, metadata.len(), limit, label)?;
    String::from_utf8(data).map_err(|error| invalid_data(error.to_string()))
}

fn validate_followed_regular_file_metadata(
    metadata: &std::fs::Metadata,
    limit: u64,
    label: &str,
) -> std::io::Result<()> {
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} path is not a regular file", label),
        ));
    }
    if limit > 0 && metadata.len() > limit {
        return Err(file_too_large(format!(
            "{} file is too large ({} bytes, limit {})",
            label,
            metadata.len(),
            limit
        )));
    }
    Ok(())
}

#[cfg(unix)]
fn open_read_regular_file_unchecked(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(unix)]
fn open_followed_read_file_unchecked(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
}

#[cfg(not(unix))]
fn open_read_regular_file_unchecked(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().read(true).open(path)
}

#[cfg(not(unix))]
fn open_followed_read_file_unchecked(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

/// Write a regular file without following a symlink at the final path component.
pub fn write_regular_file(path: &Path, data: &[u8], label: &str) -> Result<(), String> {
    write_regular_file_with(path, label, |file| file.write_all(data))
}

fn write_regular_file_with<F>(path: &Path, label: &str, write_fn: F) -> Result<(), String>
where
    F: FnOnce(&mut std::fs::File) -> std::io::Result<()>,
{
    write_regular_file_with_permissions(path, label, None, write_fn)
}

fn write_regular_file_with_permissions<F>(
    path: &Path,
    label: &str,
    permissions: Option<std::fs::Permissions>,
    write_fn: F,
) -> Result<(), String>
where
    F: FnOnce(&mut std::fs::File) -> std::io::Result<()>,
{
    let target_metadata = writable_regular_file_metadata(path, label)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temp_path = parent.join(format!(".{}.tmp-{}", file_name, Uuid::new_v4()));
    let backup_path = parent.join(format!(".{}.bak-{}", file_name, Uuid::new_v4()));

    let mut file = create_new_regular_file_no_follow(&temp_path)
        .map_err(|e| format!("Failed to open {} for save: {}", label, e))?;
    let target_permissions =
        permissions.or_else(|| target_metadata.as_ref().map(std::fs::Metadata::permissions));
    if let Some(permissions) = target_permissions
        && let Err(error) = file.set_permissions(permissions)
    {
        drop(file);
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to set {} permissions: {}", label, error));
    }

    if let Err(error) = write_fn(&mut file) {
        drop(file);
        let _ = std::fs::remove_file(&temp_path);
        return Err(format!("Failed to write {}: {}", label, error));
    }
    file.sync_all()
        .map_err(|e| format!("Failed to sync {}: {}", label, e))?;
    drop(file);

    let mut backup_created = false;
    if target_metadata.is_some() {
        if let Err(error) = std::fs::rename(path, &backup_path) {
            let _ = std::fs::remove_file(&temp_path);
            sync_parent_dir(path);
            return Err(format!("Failed to stage previous {}: {}", label, error));
        }
        backup_created = true;
    }

    match std::fs::rename(&temp_path, path) {
        Ok(()) => {
            if backup_created {
                let _ = std::fs::remove_file(&backup_path);
            }
            sync_parent_dir(path);
            Ok(())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&temp_path);
            if backup_created {
                let _ = std::fs::rename(&backup_path, path);
            }
            sync_parent_dir(path);
            Err(format!("Failed to save {}: {}", label, error))
        }
    }
}

fn writable_regular_file_metadata(
    path: &Path,
    label: &str,
) -> Result<Option<std::fs::Metadata>, String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("Refusing to save {} through symbolic link", label))
        }
        Ok(metadata) if metadata.is_dir() => {
            Err(format!("Refusing to save {} over directory", label))
        }
        Ok(metadata) if !metadata.file_type().is_file() => {
            Err(format!("Refusing to save {} over non-regular file", label))
        }
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!(
            "Failed to inspect {} before save: {}",
            label, error
        )),
    }
}

/// Copy one regular file without following source or destination symlinks.
pub fn copy_regular_file(source: &Path, target: &Path) -> Result<(), String> {
    ensure_regular_file_source(source)?;
    ensure_not_same_path(source, target)?;
    copy_file(source, target)
}

/// Recursively copy a directory from source to target.
///
/// Creates the target directory and all parent directories if they don't exist.
/// Only copies regular files and directories; symlinks are skipped for safety.
pub fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), String> {
    copy_dir_recursive_with(source, target, &copy_file)
}

fn copy_dir_recursive_with<F>(source: &Path, target: &Path, copy_file_fn: &F) -> Result<(), String>
where
    F: Fn(&Path, &Path) -> Result<(), String>,
{
    ensure_directory_root(source)?;
    let source_metadata = std::fs::symlink_metadata(source)
        .map_err(|e| format!("Failed to read metadata for {}: {}", source.display(), e))?;
    ensure_destination_not_symlink(target)?;
    ensure_target_not_inside_source(source, target)?;

    let target_existed = path_exists(target)?;
    std::fs::create_dir_all(target)
        .map_err(|e| format!("Failed to create directory {}: {}", target.display(), e))?;
    ensure_directory_destination(target)?;
    if !target_existed {
        sync_parent_dir(target);
    }

    let result = copy_dir_contents(source, target, copy_file_fn);
    if result.is_err() && !target_existed {
        let _ = cleanup_created_path(target);
    }
    result?;

    if !target_existed {
        set_directory_permissions_no_follow(target, source_metadata.permissions())
            .map_err(|e| format!("Failed to set permissions on {}: {}", target.display(), e))?;
    }

    Ok(())
}

fn cleanup_created_path(path: &Path) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("Failed to inspect {}: {}", path.display(), error)),
    };

    let result = if metadata.is_dir() && !metadata.file_type().is_symlink() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };

    match result {
        Ok(()) => {
            sync_parent_dir(path);
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Failed to remove {}: {}", path.display(), error)),
    }
}

fn path_exists(path: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(format!(
            "Failed to read metadata for {}: {}",
            path.display(),
            error
        )),
    }
}

fn ensure_directory_destination(path: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(path)
        .map_err(|e| format!("Failed to read metadata for {}: {}", path.display(), e))?;

    if metadata.file_type().is_symlink() {
        return Err(format!(
            "Cannot copy directory through symbolic link {}",
            path.display()
        ));
    }

    if !metadata.is_dir() {
        return Err(format!("{} is not a directory", path.display()));
    }

    Ok(())
}

fn copy_dir_contents<F>(source: &Path, target: &Path, copy_file_fn: &F) -> Result<(), String>
where
    F: Fn(&Path, &Path) -> Result<(), String>,
{
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
            copy_dir_recursive_with(&source_path, &target_path, copy_file_fn)?;
        } else if file_type.is_file() {
            copy_file_fn(&source_path, &target_path)?;
        }
        // Skip symlinks for safety
    }

    Ok(())
}

fn copy_file(source: &Path, target: &Path) -> Result<(), String> {
    ensure_regular_file_source(source)?;
    let mut source_file = open_read_regular_file(source, "source")?;
    let source_metadata = source_file
        .metadata()
        .map_err(|e| format!("Failed to stat source file: {}", e))?;
    let target_metadata = writable_regular_file_metadata(target, "copy target")?;
    if target_metadata
        .as_ref()
        .is_some_and(|target_metadata| same_file_metadata(&source_metadata, target_metadata))
    {
        return Err(format!(
            "Cannot copy {} onto itself at {}",
            source.display(),
            target.display()
        ));
    }

    write_regular_file_with_permissions(
        target,
        "copy target",
        Some(source_metadata.permissions()),
        |target_file| {
            std::io::copy(&mut source_file, target_file)?;
            Ok(())
        },
    )
    .map_err(|e| format!("Failed to copy {}: {}", source.display(), e))
}

#[cfg(unix)]
fn same_file_metadata(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino()
}

#[cfg(not(unix))]
fn same_file_metadata(_left: &std::fs::Metadata, _right: &std::fs::Metadata) -> bool {
    false
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
    ensure_directory_root(dir)?;

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
    let metadata = match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => metadata,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("Failed to inspect {}: {}", path.display(), e)),
    };

    let result = if metadata.is_dir() && !metadata.file_type().is_symlink() {
        tokio::fs::remove_dir_all(path).await
    } else {
        tokio::fs::remove_file(path).await
    };

    match result {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Failed to remove {}: {}", path.display(), e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, io, io::Write};
    use tempfile::tempdir;

    #[cfg(unix)]
    fn make_fifo(path: &std::path::Path) {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        let result = unsafe { libc::mkfifo(path.as_ptr(), 0o600) };
        assert_eq!(result, 0, "mkfifo failed: {}", io::Error::last_os_error());
    }

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

    #[test]
    fn copy_dir_recursive_rejects_file_source_without_creating_target() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.txt");
        let target = temp.path().join("target");
        fs::write(&source, "content").unwrap();

        let result = copy_dir_recursive(&source, &target);

        assert!(result.is_err());
        assert!(!target.exists());
    }

    #[test]
    fn copy_regular_file_copies_file() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.txt");
        let target = temp.path().join("target.txt");
        fs::write(&source, "content").unwrap();

        copy_regular_file(&source, &target).unwrap();

        assert_eq!(fs::read_to_string(target).unwrap(), "content");
    }

    #[cfg(unix)]
    #[test]
    fn copy_regular_file_preserves_source_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let source = temp.path().join("source.txt");
        let target = temp.path().join("target.txt");
        fs::write(&source, "content").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();

        copy_regular_file(&source, &target).unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn copy_regular_file_replaces_existing_target_with_source_permissions_without_artifacts() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let source = temp.path().join("source.txt");
        let target = temp.path().join("target.txt");
        fs::write(&source, "new content").unwrap();
        fs::write(&target, "old content").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o644)).unwrap();

        copy_regular_file(&source, &target).unwrap();

        assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let artifacts: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(".target.txt."))
            })
            .collect();
        assert!(artifacts.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn copy_regular_file_rejects_hard_link_target_without_truncating_source() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.txt");
        let target = temp.path().join("target.txt");
        fs::write(&source, "content").unwrap();
        fs::hard_link(&source, &target).unwrap();

        let error = copy_regular_file(&source, &target)
            .expect_err("hard-linked target should be treated as same file");

        assert!(error.contains("Cannot copy"));
        assert_eq!(fs::read_to_string(&source).unwrap(), "content");
        assert_eq!(fs::read_to_string(&target).unwrap(), "content");
    }

    #[test]
    fn copy_regular_file_rejects_directory_source_without_creating_target() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target.txt");
        fs::create_dir(&source).unwrap();

        assert!(copy_regular_file(&source, &target).is_err());
        assert!(!target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_recursive_preserves_new_directory_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let subdir = source.join("subdir");
        let target = temp.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&subdir).unwrap();
        fs::write(subdir.join("file.txt"), "content").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(&subdir, fs::Permissions::from_mode(0o710)).unwrap();

        copy_dir_recursive(&source, &target).unwrap();

        let root_mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        let subdir_mode = fs::metadata(target.join("subdir"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(root_mode, 0o750);
        assert_eq!(subdir_mode, 0o710);
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_recursive_keeps_existing_target_directory_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("file.txt"), "content").unwrap();
        fs::set_permissions(&source, fs::Permissions::from_mode(0o750)).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();

        copy_dir_recursive(&source, &target).unwrap();

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        assert_eq!(
            fs::read_to_string(target.join("file.txt")).unwrap(),
            "content"
        );
    }

    #[test]
    fn ensure_dir_no_follow_creates_directory() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("download");

        ensure_dir_no_follow(&path).unwrap();

        assert!(path.is_dir());
    }

    #[test]
    fn ensure_dir_no_follow_rejects_existing_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("download");
        fs::write(&path, "content").unwrap();

        let error = ensure_dir_no_follow(&path).expect_err("regular file should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
        assert!(path.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_dir_no_follow_rejects_symlink_without_changing_target() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error =
            ensure_dir_no_follow(&link).expect_err("symlinked directory should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn ensure_private_dir_no_follow_creates_directory() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("private");

        ensure_private_dir_no_follow(&path).unwrap();

        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_private_dir_no_follow_sets_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let path = temp.path().join("private");

        ensure_private_dir_no_follow(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn ensure_private_dir_no_follow_rejects_existing_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("private");
        fs::write(&path, "content").unwrap();

        let error =
            ensure_private_dir_no_follow(&path).expect_err("regular file should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
        assert!(path.is_file());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_private_dir_no_follow_rejects_symlink_without_changing_target() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = ensure_private_dir_no_follow(&link)
            .expect_err("symlinked directory should be rejected");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[cfg(unix)]
    #[test]
    fn set_private_dir_permissions_no_follow_tightens_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let path = temp.path().join("private");
        fs::create_dir(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();

        set_private_dir_permissions_no_follow(&path).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    #[cfg(unix)]
    fn set_private_dir_permissions_no_follow_rejects_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("file");
        fs::write(&path, "content").unwrap();

        let error = set_private_dir_permissions_no_follow(&path)
            .expect_err("regular file should not be chmodded as directory");

        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
    }

    #[cfg(unix)]
    #[test]
    fn set_private_dir_permissions_no_follow_rejects_fifo_without_blocking() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("pipe");
        make_fifo(&path);

        let error = set_private_dir_permissions_no_follow(&path)
            .expect_err("fifo should not be chmodded as directory");

        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
    }

    #[cfg(unix)]
    #[test]
    fn set_private_dir_permissions_no_follow_rejects_symlink_without_changing_target() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        set_private_dir_permissions_no_follow(&link).expect_err("symlink should not be followed");

        let mode = fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o755);
    }

    #[test]
    fn open_append_regular_file_creates_and_appends_regular_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("portal.log");

        {
            let mut file = open_append_regular_file(&path).unwrap();
            writeln!(file, "first").unwrap();
        }
        {
            let mut file = open_append_regular_file(&path).unwrap();
            writeln!(file, "second").unwrap();
        }

        assert_eq!(fs::read_to_string(path).unwrap(), "first\nsecond\n");
    }

    #[test]
    fn open_append_regular_file_rejects_directory() {
        let temp = tempdir().unwrap();

        let error = open_append_regular_file(temp.path())
            .expect_err("directory should not be opened as append log");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[test]
    fn open_directory_for_sync_opens_directory() {
        let temp = tempdir().unwrap();

        let dir = open_directory_for_sync(temp.path()).unwrap();

        assert!(dir.metadata().unwrap().is_dir());
    }

    #[test]
    fn open_directory_for_sync_rejects_regular_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("file");
        fs::write(&path, "content").unwrap();

        open_directory_for_sync(&path).expect_err("regular file should not open as directory");
    }

    #[cfg(unix)]
    #[test]
    fn open_directory_for_sync_rejects_fifo_without_blocking() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("pipe");
        make_fifo(&path);

        open_directory_for_sync(&path).expect_err("fifo should not open as directory");
    }

    #[test]
    fn create_new_regular_file_no_follow_creates_regular_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("new-file");

        {
            let mut file = create_new_regular_file_no_follow(&path).unwrap();
            file.write_all(b"content").unwrap();
        }

        assert_eq!(fs::read_to_string(path).unwrap(), "content");
    }

    #[test]
    fn create_new_regular_file_no_follow_rejects_existing_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("existing-file");
        fs::write(&path, "original").unwrap();

        create_new_regular_file_no_follow(&path).expect_err("existing file should not be replaced");

        assert_eq!(fs::read_to_string(path).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn create_new_regular_file_no_follow_rejects_symlink_without_writing_target() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("link");
        fs::write(&target, "original").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        create_new_regular_file_no_follow(&link).expect_err("symlink should not be followed");

        assert_eq!(fs::read_to_string(target).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn create_new_regular_file_no_follow_rejects_fifo_without_blocking() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("pipe");
        make_fifo(&path);

        create_new_regular_file_no_follow(&path)
            .expect_err("fifo should not be opened as a new regular file");
    }

    #[cfg(unix)]
    #[test]
    fn unchecked_read_open_opens_fifo_nonblocking_for_recheck() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("pipe");
        make_fifo(&path);

        let file = open_read_regular_file_unchecked(&path)
            .expect("fifo read open should not block when opened for metadata recheck");

        assert!(!file.metadata().unwrap().file_type().is_file());
    }

    #[cfg(unix)]
    #[test]
    fn unchecked_followed_read_open_opens_fifo_nonblocking_for_recheck() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("pipe");
        make_fifo(&path);

        let file = open_followed_read_file_unchecked(&path)
            .expect("fifo read open should not block when opened for metadata recheck");

        assert!(!file.metadata().unwrap().file_type().is_file());
    }

    #[cfg(unix)]
    #[test]
    fn unchecked_append_open_rejects_fifo_without_blocking() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("pipe");
        make_fifo(&path);

        open_append_regular_file_unchecked(&path)
            .expect_err("fifo append open without a reader should fail instead of blocking");
    }

    #[test]
    fn read_regular_file_limited_reads_regular_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("note.txt");
        fs::write(&path, "content").unwrap();

        let data = read_regular_file_limited(&path, 1024, "Text").unwrap();

        assert_eq!(data, b"content");
    }

    #[test]
    fn read_regular_file_to_string_preserves_invalid_utf8_error() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("settings.toml");
        fs::write(&path, [0xff, 0xfe, b'a']).unwrap();

        let error = read_regular_file_to_string(&path, "config")
            .expect_err("invalid UTF-8 should be reported as InvalidData");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }

    #[cfg(unix)]
    #[test]
    fn read_regular_file_to_string_rejects_symlink() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("target.toml");
        let link = temp.path().join("settings.toml");
        fs::write(&target, "value = 1").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error =
            read_regular_file_to_string(&link, "config").expect_err("symlink should be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn read_regular_file_limited_rejects_oversized_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("note.txt");
        fs::write(&path, "content").unwrap();

        let error = read_regular_file_limited(&path, 3, "Text")
            .expect_err("oversized file should be rejected");

        assert!(error.contains("too large"));
    }

    #[test]
    fn read_to_end_enforcing_limit_rejects_reader_that_exceeds_limit() {
        let mut reader = io::Cursor::new(b"content".to_vec());

        let error = read_to_end_enforcing_limit(&mut reader, 3, 3, "Text")
            .expect_err("reader should be capped even when initial size fits");

        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
        assert!(error.to_string().contains("too large"));
    }

    #[test]
    fn read_regular_file_to_string_limited_rejects_invalid_utf8() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("note.txt");
        fs::write(&path, [0xff, 0xfe]).unwrap();

        let error = read_regular_file_to_string_limited(&path, 1024, "Text")
            .expect_err("invalid UTF-8 should be rejected");

        assert!(error.contains("Failed to read Text file"));
    }

    #[test]
    fn read_regular_file_follow_symlink_to_string_limited_reads_regular_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config");
        fs::write(&path, "Host api\n").unwrap();

        let content =
            read_regular_file_follow_symlink_to_string_limited(&path, 1024, "SSH config").unwrap();

        assert_eq!(content, "Host api\n");
    }

    #[cfg(unix)]
    #[test]
    fn read_regular_file_follow_symlink_to_string_limited_allows_symlinked_file() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("config");
        fs::write(&target, "Host api\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let content =
            read_regular_file_follow_symlink_to_string_limited(&link, 1024, "SSH config").unwrap();

        assert_eq!(content, "Host api\n");
    }

    #[test]
    fn read_regular_file_follow_symlink_to_string_limited_rejects_directory() {
        let temp = tempdir().unwrap();

        let error =
            read_regular_file_follow_symlink_to_string_limited(temp.path(), 1024, "SSH config")
                .expect_err("directory should be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn read_regular_file_follow_symlink_to_string_limited_rejects_oversized_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config");
        fs::write(&path, "too large").unwrap();

        let error = read_regular_file_follow_symlink_to_string_limited(&path, 3, "SSH config")
            .expect_err("oversized file should be rejected");

        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
    }

    #[cfg(unix)]
    #[test]
    fn read_regular_file_follow_symlink_to_string_limited_rejects_socket() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("config.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let error = read_regular_file_follow_symlink_to_string_limited(&path, 1024, "SSH config")
            .expect_err("socket should be rejected");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[test]
    fn write_regular_file_creates_and_updates_regular_file() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("note.txt");

        write_regular_file(&path, b"first", "file").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first");

        write_regular_file(&path, b"second", "file").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn write_regular_file_preserves_existing_file_on_write_failure() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("note.txt");
        fs::write(&path, "original").unwrap();

        let error = write_regular_file_with(&path, "file", |file| {
            file.write_all(b"partial")?;
            Err(io::Error::other("simulated write failure"))
        })
        .expect_err("injected write failure should propagate");

        assert!(error.contains("simulated write failure"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
        let artifacts: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with(".note.txt."))
            })
            .collect();
        assert!(artifacts.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn write_regular_file_preserves_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempdir().unwrap();
        let path = temp.path().join("note.txt");
        fs::write(&path, "original").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();

        write_regular_file(&path, b"updated", "file").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "updated");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn write_regular_file_rejects_directory() {
        let temp = tempdir().unwrap();

        let error = write_regular_file(temp.path(), b"content", "file")
            .expect_err("directory should not be writable as a file");

        assert!(error.contains("directory"));
    }

    #[test]
    fn copy_dir_recursive_removes_new_target_after_copy_failure() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("ok.txt"), "ok").unwrap();
        fs::write(source.join("fail.txt"), "fail").unwrap();

        let result = copy_dir_recursive_with(&source, &target, &|source, target| {
            if source.file_name().is_some_and(|name| name == "fail.txt") {
                return Err("simulated copy failure".to_string());
            }
            copy_file(source, target)
        });

        assert!(result.is_err());
        assert!(!target.exists());
    }

    #[test]
    fn copy_dir_recursive_cleans_replaced_new_target_after_copy_failure() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("fail.txt"), "fail").unwrap();

        let result = copy_dir_recursive_with(&source, &target, &|source, target_path| {
            if source.file_name().is_some_and(|name| name == "fail.txt") {
                fs::remove_dir_all(target_path.parent().unwrap()).unwrap();
                fs::write(target_path.parent().unwrap(), "replacement").unwrap();
                return Err("simulated copy failure".to_string());
            }
            copy_file(source, target_path)
        });

        assert!(result.is_err());
        assert!(!target.exists());
    }

    #[test]
    fn copy_dir_recursive_preserves_existing_target_after_copy_failure() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("fail.txt"), "fail").unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), "keep").unwrap();

        let result = copy_dir_recursive_with(&source, &target, &|source, _target| {
            if source.file_name().is_some_and(|name| name == "fail.txt") {
                return Err("simulated copy failure".to_string());
            }
            Ok(())
        });

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(target.join("keep.txt")).unwrap(), "keep");
    }

    #[test]
    fn count_items_in_dir_rejects_file_source() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.txt");
        fs::write(&source, "content").unwrap();

        assert!(count_items_in_dir(&source).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn ensure_destination_not_symlink_rejects_symlink_target() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        fs::write(&target, "content").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(ensure_destination_not_symlink(&link).is_err());
    }

    #[test]
    fn ensure_directory_destination_accepts_real_directory() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();

        ensure_directory_destination(&target).expect("real directory should be accepted");
    }

    #[test]
    fn ensure_directory_destination_rejects_regular_file() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("target");
        fs::write(&target, "content").unwrap();

        let error = ensure_directory_destination(&target)
            .expect_err("regular file should not be a directory destination");

        assert!(error.contains("not a directory"));
    }

    #[cfg(unix)]
    #[test]
    fn ensure_directory_destination_rejects_symlinked_directory() {
        let temp = tempdir().unwrap();
        let real_target = temp.path().join("real_target");
        let link_target = temp.path().join("link_target");
        fs::create_dir(&real_target).unwrap();
        std::os::unix::fs::symlink(&real_target, &link_target).unwrap();

        let error = ensure_directory_destination(&link_target)
            .expect_err("symlinked directory should not be a directory destination");

        assert!(error.contains("symbolic link"));
    }

    #[cfg(unix)]
    #[test]
    fn copy_regular_file_rejects_symlink_source_without_creating_target() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.txt");
        let link = temp.path().join("link.txt");
        let target = temp.path().join("target.txt");
        fs::write(&source, "content").unwrap();
        std::os::unix::fs::symlink(&source, &link).unwrap();

        assert!(copy_regular_file(&link, &target).is_err());
        assert!(!target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn copy_regular_file_rejects_socket_source_without_creating_target() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source.sock");
        let target = temp.path().join("target.txt");
        let _listener = std::os::unix::net::UnixListener::bind(&source).unwrap();

        assert!(copy_regular_file(&source, &target).is_err());
        assert!(!target.exists());
    }

    #[cfg(unix)]
    #[test]
    fn open_append_regular_file_rejects_symlink_without_writing_target() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("target.log");
        let link = temp.path().join("portal.log");
        fs::write(&target, "original\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = open_append_regular_file(&link)
            .expect_err("symlink should not be opened as append log");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(fs::read_to_string(target).unwrap(), "original\n");
    }

    #[cfg(unix)]
    #[test]
    fn open_append_regular_file_rejects_socket() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("portal.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let error =
            open_append_regular_file(&path).expect_err("socket should not be opened as append log");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[cfg(unix)]
    #[test]
    fn read_regular_file_limited_rejects_symlink_without_reading_target() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        fs::write(&target, "secret").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error =
            read_regular_file_limited(&link, 1024, "Text").expect_err("symlink should not be read");

        assert!(error.contains("symbolic link"));
    }

    #[cfg(unix)]
    #[test]
    fn read_regular_file_limited_rejects_socket() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("portal.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let error =
            read_regular_file_limited(&path, 1024, "Text").expect_err("socket should not be read");

        assert!(error.contains("not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn write_regular_file_rejects_symlink_without_writing_target() {
        let temp = tempdir().unwrap();
        let target = temp.path().join("target.txt");
        let link = temp.path().join("link.txt");
        fs::write(&target, "original").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error =
            write_regular_file(&link, b"changed", "file").expect_err("symlink should be rejected");

        assert!(error.contains("symbolic link"));
        assert_eq!(fs::read_to_string(target).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn write_regular_file_rejects_socket() {
        let temp = tempdir().unwrap();
        let path = temp.path().join("portal.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let error =
            write_regular_file(&path, b"content", "file").expect_err("socket should be rejected");

        assert!(error.contains("non-regular"));
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_recursive_rejects_symlinked_target_without_writing_target() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let real_target = temp.path().join("real_target");
        let link_target = temp.path().join("link_target");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("file.txt"), "content").unwrap();
        fs::create_dir(&real_target).unwrap();
        std::os::unix::fs::symlink(&real_target, &link_target).unwrap();

        assert!(copy_dir_recursive(&source, &link_target).is_err());
        assert!(!real_target.join("file.txt").exists());
    }

    #[cfg(unix)]
    #[test]
    fn copy_dir_recursive_rejects_symlinked_file_inside_existing_target() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");
        let target = temp.path().join("target");
        let outside = temp.path().join("outside.txt");
        fs::create_dir(&source).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(source.join("file.txt"), "new").unwrap();
        fs::write(&outside, "original").unwrap();
        std::os::unix::fs::symlink(&outside, target.join("file.txt")).unwrap();

        assert!(copy_dir_recursive(&source, &target).is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "original");
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

    #[tokio::test]
    async fn cleanup_temp_dir_removes_file_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("scratch");
        fs::write(&path, "content").unwrap();

        cleanup_temp_dir(&path).await.unwrap();

        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cleanup_temp_dir_removes_symlink_without_touching_target() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("scratch");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.txt"), "keep").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        cleanup_temp_dir(&link).await.unwrap();

        assert!(!link.exists());
        assert_eq!(fs::read_to_string(target.join("keep.txt")).unwrap(), "keep");
    }
}
