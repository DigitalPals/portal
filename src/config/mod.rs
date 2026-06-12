pub mod history;
pub mod hosts;
pub mod paths;
pub mod settings;
pub mod snippet_history;
pub mod snippets;
pub mod ssh_config;

use std::io::Write;
use std::path::Path;
use uuid::Uuid;

use crate::error::ConfigError;
use crate::fs_utils;

const CONFIG_FILE_MAX_BYTES: u64 = 8 * 1024 * 1024;

pub use history::{HistoryConfig, HistoryEntry, SessionType};
pub use hosts::{
    AuthMethod, DetectedOs, Host, HostsConfig, PortForward, PortForwardKind, Protocol,
};
pub use settings::SettingsConfig;
pub use snippet_history::{HistoricalHostResult, SnippetExecutionEntry, SnippetHistoryConfig};
pub use snippets::{Snippet, SnippetsConfig};

pub(crate) fn load_toml_or_recover<T>(path: &Path, label: &str) -> Result<T, ConfigError>
where
    T: serde::de::DeserializeOwned + Default,
{
    let content = match fs_utils::read_regular_file_to_string_limited_io(
        path,
        CONFIG_FILE_MAX_BYTES,
        label,
    ) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(source) if source.kind() == std::io::ErrorKind::FileTooLarge => {
            return Err(ConfigError::ReadFile {
                path: path.to_path_buf(),
                source,
            });
        }
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            recover_corrupt_config(path, label, &error.to_string()).map_err(|source| {
                ConfigError::WriteFile {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            return Ok(T::default());
        }
        Err(source) => {
            return Err(ConfigError::ReadFile {
                path: path.to_path_buf(),
                source,
            });
        }
    };

    match toml::from_str(&content) {
        Ok(config) => Ok(config),
        Err(error) => {
            recover_corrupt_config(path, label, &error.to_string()).map_err(|source| {
                ConfigError::WriteFile {
                    path: path.to_path_buf(),
                    source,
                }
            })?;
            Ok(T::default())
        }
    }
}

pub(crate) fn recover_corrupt_config(
    path: &Path,
    label: &str,
    reason: &str,
) -> std::io::Result<std::path::PathBuf> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is a symbolic link", path.display()),
        ));
    }
    if metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::IsADirectory,
            format!("{} is a directory", path.display()),
        ));
    }
    if !metadata.file_type().is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }

    let backup_path = corrupt_backup_path(path);
    std::fs::rename(path, &backup_path)?;
    fs_utils::sync_parent_dir(path);
    tracing::warn!(
        "Recovered from corrupt {} config at {}: {}; moved original to {}",
        label,
        path.display(),
        reason,
        backup_path.display()
    );
    Ok(backup_path)
}

fn corrupt_backup_path(path: &Path) -> std::path::PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    let timestamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    parent.join(format!(
        "{}.corrupt-{}-{}",
        file_name,
        timestamp,
        Uuid::new_v4()
    ))
}

pub fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    write_atomic_with(path, content.as_bytes(), write_and_sync)
}

fn write_atomic_with<F>(path: &Path, content: &[u8], write_fn: F) -> std::io::Result<()>
where
    F: FnOnce(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
{
    write_atomic_with_permissions(path, content, write_fn, |path, permissions| {
        std::fs::set_permissions(path, permissions)
    })
}

fn write_atomic_with_permissions<F, P>(
    path: &Path,
    content: &[u8],
    write_fn: F,
    set_permissions_fn: P,
) -> std::io::Result<()>
where
    F: FnOnce(&mut std::fs::File, &[u8]) -> std::io::Result<()>,
    P: Fn(&Path, std::fs::Permissions) -> std::io::Result<()>,
{
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Missing parent directory")
    })?;
    let target_metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    if target_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is a symbolic link", path.display()),
        ));
    }
    if target_metadata
        .as_ref()
        .is_some_and(std::fs::Metadata::is_dir)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::IsADirectory,
            format!("{} is a directory", path.display()),
        ));
    }
    if target_metadata
        .as_ref()
        .is_some_and(|metadata| !metadata.file_type().is_file())
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    let path_exists = target_metadata.is_some();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    let temp_path = parent.join(format!(".{}.tmp-{}", file_name, Uuid::new_v4()));
    let backup_path = parent.join(format!(".{}.bak-{}", file_name, Uuid::new_v4()));
    let original_permissions = target_metadata
        .as_ref()
        .map(|metadata| metadata.permissions());

    let mut file = open_atomic_temp_file(&temp_path)?;

    if let Some(permissions) = original_permissions {
        if let Err(error) = set_permissions_fn(&temp_path, permissions) {
            drop(file);
            let _ = std::fs::remove_file(&temp_path);
            return Err(error);
        }
    } else if !path_exists {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(error) =
                set_permissions_fn(&temp_path, std::fs::Permissions::from_mode(0o600))
            {
                drop(file);
                let _ = std::fs::remove_file(&temp_path);
                return Err(error);
            }
        }
    }

    if let Err(error) = write_fn(&mut file, content) {
        drop(file);
        let _ = std::fs::remove_file(&temp_path);
        return Err(error);
    }
    drop(file);

    let mut backup_created = false;
    if path_exists {
        if let Err(error) = std::fs::rename(path, &backup_path) {
            let _ = std::fs::remove_file(&temp_path);
            fs_utils::sync_parent_dir(path);
            return Err(error);
        }
        backup_created = true;
    }

    match std::fs::rename(&temp_path, path) {
        Ok(()) => {
            if backup_created {
                let _ = std::fs::remove_file(&backup_path);
            }
            fs_utils::sync_parent_dir(path);
            Ok(())
        }
        Err(err) => {
            let _ = std::fs::remove_file(&temp_path);
            if backup_created {
                let _ = std::fs::rename(&backup_path, path);
            }
            fs_utils::sync_parent_dir(path);
            Err(err)
        }
    }
}

fn write_and_sync(file: &mut std::fs::File, content: &[u8]) -> std::io::Result<()> {
    file.write_all(content)?;
    file.sync_all()
}

#[cfg(unix)]
fn open_atomic_temp_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn open_atomic_temp_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::{
        CONFIG_FILE_MAX_BYTES, load_toml_or_recover, recover_corrupt_config, write_atomic,
        write_atomic_with, write_atomic_with_permissions,
    };
    use crate::error::ConfigError;
    use serde::Deserialize;
    use std::{fs, io, io::Write};
    use tempfile::tempdir;

    #[derive(Debug, Default, Deserialize)]
    struct TestConfig {
        #[serde(default)]
        value: u32,
    }

    #[test]
    fn load_toml_or_recover_returns_default_for_missing_file() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");

        let config: TestConfig =
            load_toml_or_recover(&path, "settings").expect("missing config should default");

        assert_eq!(config.value, 0);
        assert!(!path.exists());
    }

    #[test]
    fn load_toml_or_recover_moves_corrupt_file_aside() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        fs::write(&path, "this is not = valid = toml").expect("write corrupt config");

        let config: TestConfig =
            load_toml_or_recover(&path, "settings").expect("corrupt config should recover");

        assert_eq!(config.value, 0);
        assert!(!path.exists());

        let backups: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| {
                        name.starts_with("settings.toml.corrupt-")
                            && fs::read_to_string(entry.path())
                                .expect("backup should be readable")
                                .contains("this is not")
                    })
                    .unwrap_or(false)
            })
            .collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn load_toml_or_recover_moves_invalid_utf8_file_aside() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        fs::write(&path, [0xff, 0xfe, b'a']).expect("write invalid UTF-8 config");

        let config: TestConfig =
            load_toml_or_recover(&path, "settings").expect("invalid UTF-8 should recover");

        assert_eq!(config.value, 0);
        assert!(!path.exists());

        let backups = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with("settings.toml.corrupt-"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn load_toml_or_recover_rejects_oversized_file_without_recovery() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        fs::write(&path, vec![b'a'; CONFIG_FILE_MAX_BYTES as usize + 1])
            .expect("write oversized config");

        let error =
            load_toml_or_recover::<TestConfig>(&path, "settings").expect_err("oversized fails");

        match error {
            ConfigError::ReadFile { source, .. } => {
                assert_eq!(source.kind(), io::ErrorKind::FileTooLarge);
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(path.exists());
        let backups = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with("settings.toml.corrupt-"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(backups, 0);
    }

    #[cfg(unix)]
    #[test]
    fn load_toml_or_recover_rejects_symlink_without_recovery() {
        let dir = tempdir().expect("temp dir");
        let target = dir.path().join("target.toml");
        let link = dir.path().join("settings.toml");
        fs::write(&target, "value = 42\n").expect("write target");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");

        let error =
            load_toml_or_recover::<TestConfig>(&link, "settings").expect_err("symlink fails");

        match error {
            ConfigError::ReadFile { source, .. } => {
                assert_eq!(source.kind(), io::ErrorKind::InvalidInput);
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(
            fs::symlink_metadata(&link)
                .expect("link metadata")
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "value = 42\n");
    }

    #[cfg(unix)]
    #[test]
    fn load_toml_or_recover_rejects_broken_symlink_without_defaulting() {
        let dir = tempdir().expect("temp dir");
        let link = dir.path().join("settings.toml");
        std::os::unix::fs::symlink(dir.path().join("missing.toml"), &link)
            .expect("create broken symlink");

        let error =
            load_toml_or_recover::<TestConfig>(&link, "settings").expect_err("symlink fails");

        match error {
            ConfigError::ReadFile { source, .. } => {
                assert_eq!(source.kind(), io::ErrorKind::InvalidInput);
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert!(
            fs::symlink_metadata(&link)
                .expect("link metadata")
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn recover_corrupt_config_rejects_directory() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        fs::create_dir(&path).expect("create directory");

        let error = recover_corrupt_config(&path, "settings", "invalid")
            .expect_err("directory should not be recovered as corrupt config");

        assert_eq!(error.kind(), io::ErrorKind::IsADirectory);
        assert!(path.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn recover_corrupt_config_rejects_symlink_without_moving_it() {
        let dir = tempdir().expect("temp dir");
        let target = dir.path().join("target.toml");
        let link = dir.path().join("settings.toml");
        fs::write(&target, "value = 42\n").expect("write target");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");

        let error = recover_corrupt_config(&link, "settings", "invalid")
            .expect_err("symlink should not be recovered as corrupt config");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            fs::symlink_metadata(&link)
                .expect("link metadata")
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&target).unwrap(), "value = 42\n");
    }

    #[cfg(unix)]
    #[test]
    fn recover_corrupt_config_rejects_special_file() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).expect("bind socket");

        let error = recover_corrupt_config(&path, "settings", "invalid")
            .expect_err("socket should not be recovered as corrupt config");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(path.exists());
    }

    #[test]
    fn write_atomic_creates_file_and_overwrites() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");

        write_atomic(&path, "first=1\n").expect("write first");
        assert_eq!(fs::read_to_string(&path).expect("read first"), "first=1\n");

        write_atomic(&path, "second=2\n").expect("write second");
        assert_eq!(
            fs::read_to_string(&path).expect("read second"),
            "second=2\n"
        );
    }

    #[test]
    fn write_atomic_cleans_temp_files() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("history.toml");

        write_atomic(&path, "ok=true\n").expect("write");

        let tmp_prefix = ".history.toml.tmp-";
        let tmp_count = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with(tmp_prefix))
                    .unwrap_or(false)
            })
            .count();

        assert_eq!(tmp_count, 0);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_creates_temp_file_private_before_write() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("secrets.toml");

        write_atomic_with(&path, b"token=\"secret\"\n", |file, content| {
            let mode = file.metadata()?.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            file.write_all(content)
        })
        .expect("write should succeed");

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_preserves_existing_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        fs::write(&path, "original=true\n").expect("write original");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640))
            .expect("set original permissions");

        write_atomic(&path, "updated=true\n").expect("write updated config");

        assert_eq!(
            fs::read_to_string(&path).expect("read updated"),
            "updated=true\n"
        );
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o640);
    }

    #[test]
    fn write_atomic_removes_temp_file_when_write_fails() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("history.toml");

        let error = write_atomic_with(&path, b"ok=true\n", |file, content| {
            file.write_all(content)?;
            Err(io::Error::other("simulated write failure"))
        })
        .expect_err("injected write failure should propagate");

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(!path.exists());

        let entries: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .collect();
        assert!(entries.is_empty());
    }

    #[test]
    fn write_atomic_preserves_target_and_cleans_temp_when_permissions_fail() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        fs::write(&path, "original=true\n").expect("write original");

        let error = write_atomic_with_permissions(
            &path,
            b"updated=true\n",
            |file, content| file.write_all(content),
            |_path, _permissions| Err(io::Error::other("simulated permission failure")),
        )
        .expect_err("injected permission failure should propagate");

        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert_eq!(
            fs::read_to_string(&path).expect("read original"),
            "original=true\n"
        );

        let artifacts: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with(".settings.toml."))
                    .unwrap_or(false)
            })
            .collect();
        assert!(artifacts.is_empty());
    }

    #[test]
    fn write_atomic_rejects_directory_target_without_artifacts() {
        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        fs::create_dir(&path).expect("create directory target");

        let error = write_atomic(&path, "value=1\n").expect_err("directory target should fail");

        assert_eq!(error.kind(), io::ErrorKind::IsADirectory);
        assert!(path.is_dir());

        let artifacts: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with(".settings.toml."))
                    .unwrap_or(false)
            })
            .collect();
        assert!(artifacts.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_rejects_symlink_target_without_artifacts() {
        let dir = tempdir().expect("temp dir");
        let target = dir.path().join("real-settings.toml");
        let link = dir.path().join("settings.toml");
        fs::write(&target, "original=true\n").expect("write target");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");

        let error = write_atomic(&link, "updated=true\n").expect_err("symlink target should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            fs::symlink_metadata(&link)
                .expect("link metadata")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(&target).expect("read target"),
            "original=true\n"
        );

        let artifacts: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with(".settings.toml."))
                    .unwrap_or(false)
            })
            .collect();
        assert!(artifacts.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_rejects_special_file_target_without_artifacts() {
        use std::os::unix::fs::FileTypeExt;

        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("settings.toml");
        let _listener = std::os::unix::net::UnixListener::bind(&path).expect("bind socket");

        let error = write_atomic(&path, "updated=true\n").expect_err("socket target should fail");

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(
            fs::symlink_metadata(&path)
                .expect("socket metadata")
                .file_type()
                .is_socket()
        );

        let artifacts: Vec<_> = fs::read_dir(dir.path())
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .map(|name| name.starts_with(".settings.toml."))
                    .unwrap_or(false)
            })
            .collect();
        assert!(artifacts.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn write_atomic_creates_new_files_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("secrets.toml");

        write_atomic(&path, "token=\"secret\"\n").expect("write");

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
