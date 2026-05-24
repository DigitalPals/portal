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
    if !path.exists() {
        return Ok(T::default());
    }

    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
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
    let backup_path = corrupt_backup_path(path);
    std::fs::rename(path, &backup_path)?;
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
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Missing parent directory")
    })?;
    let path_exists = path.exists();
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("config");
    let temp_path = parent.join(format!(".{}.tmp-{}", file_name, Uuid::new_v4()));
    let backup_path = parent.join(format!(".{}.bak-{}", file_name, Uuid::new_v4()));
    let original_permissions = path.metadata().ok().map(|meta| meta.permissions());

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)?;

    if let Some(permissions) = original_permissions {
        let _ = std::fs::set_permissions(&temp_path, permissions);
    } else if !path_exists {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o600));
        }
    }

    file.write_all(content.as_bytes())?;
    file.sync_all()?;

    let mut backup_created = false;
    if path.exists() {
        std::fs::rename(path, &backup_path)?;
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
        Err(err) => {
            let _ = std::fs::remove_file(&temp_path);
            if backup_created {
                let _ = std::fs::rename(&backup_path, path);
            }
            sync_parent_dir(path);
            Err(err)
        }
    }
}

fn sync_parent_dir(path: &Path) {
    if let Some(parent) = path.parent() {
        if let Ok(dir) = std::fs::File::open(parent) {
            let _ = dir.sync_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{load_toml_or_recover, write_atomic};
    use serde::Deserialize;
    use std::fs;
    use tempfile::tempdir;

    #[derive(Debug, Default, Deserialize)]
    struct TestConfig {
        #[serde(default)]
        value: u32,
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
    fn write_atomic_creates_new_files_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("temp dir");
        let path = dir.path().join("secrets.toml");

        write_atomic(&path, "token=\"secret\"\n").expect("write");

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
