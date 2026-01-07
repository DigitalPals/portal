pub mod history;
pub mod hosts;
pub mod paths;
pub mod settings;
pub mod snippet_history;
pub mod snippets;

use std::io::Write;
use std::path::Path;
use uuid::Uuid;

pub use history::{HistoryConfig, HistoryEntry, SessionType};
#[allow(unused_imports)]
pub use hosts::{AuthMethod, DetectedOs, Host, HostsConfig};
pub use settings::SettingsConfig;
pub use snippet_history::{HistoricalHostResult, SnippetExecutionEntry, SnippetHistoryConfig};
pub use snippets::{Snippet, SnippetsConfig};

pub fn write_atomic(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Missing parent directory")
    })?;
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
    file.write_all(content.as_bytes())?;
    file.sync_all()?;

    if let Some(permissions) = original_permissions {
        let _ = std::fs::set_permissions(&temp_path, permissions);
    }

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
    use super::write_atomic;
    use std::fs;
    use tempfile::tempdir;

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
}
