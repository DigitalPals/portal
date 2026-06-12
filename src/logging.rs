//! Logging initialization with file output support

use std::path::{Path, PathBuf};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::fs_utils::ensure_private_dir_no_follow;

const LOG_RETENTION_FILES: usize = 7;

/// Initialize logging with optional file output.
/// Returns a guard that must be kept alive for the duration of the program.
pub fn init_logging(log_dir: Option<PathBuf>) -> Option<WorkerGuard> {
    let default_level = if cfg!(debug_assertions) {
        tracing::Level::INFO
    } else {
        tracing::Level::WARN
    };
    let mut env_filter = EnvFilter::from_default_env().add_directive(default_level.into());
    if std::env::var_os("PORTAL_VNC_DEBUG").is_some() {
        if let Ok(directive) = "portal::vnc=debug".parse() {
            env_filter = env_filter.add_directive(directive);
        }
        if let Ok(directive) = "portal::app::update::vnc=debug".parse() {
            env_filter = env_filter.add_directive(directive);
        }
    }

    let console_layer = fmt::layer().with_target(true).with_thread_ids(false);

    if let Some(dir) = log_dir {
        match prepare_app_log_dir(&dir) {
            Ok(()) => {
                cleanup_old_logs(&dir, "portal.log", LOG_RETENTION_FILES);
                // Daily rotating log file
                let file_appender = tracing_appender::rolling::daily(&dir, "portal.log");
                let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

                let file_layer = fmt::layer()
                    .with_writer(non_blocking)
                    .with_ansi(false)
                    .with_target(true);

                tracing_subscriber::registry()
                    .with(env_filter)
                    .with(console_layer)
                    .with(file_layer)
                    .init();

                return Some(guard);
            }
            Err(error) => {
                eprintln!(
                    "Failed to prepare Portal log directory {}: {}; falling back to console logging",
                    dir.display(),
                    error
                );
            }
        }
    }

    // Console-only logging
    tracing_subscriber::registry()
        .with(env_filter)
        .with(console_layer)
        .init();
    None
}

fn prepare_app_log_dir(path: &Path) -> std::io::Result<()> {
    ensure_private_dir_no_follow(path)
}

fn cleanup_old_logs(dir: &Path, prefix: &str, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_rotated_log_name(name, prefix) {
            continue;
        }
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        files.push((modified, path));
    }

    if files.len() <= keep {
        return;
    }

    files.sort_by_key(|(modified, _)| *modified);
    let to_remove = files.len() - keep;
    for (_, path) in files.into_iter().take(to_remove) {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !metadata.is_file() {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}

fn is_rotated_log_name(name: &str, prefix: &str) -> bool {
    name.strip_prefix(prefix)
        .is_some_and(|rest| rest.starts_with('.') && rest.len() > 1)
}

#[cfg(test)]
mod tests {
    use super::{cleanup_old_logs, is_rotated_log_name, prepare_app_log_dir};

    #[test]
    fn rotated_log_name_rejects_similar_prefixes() {
        assert!(is_rotated_log_name("portal.log.2026-01-01", "portal.log"));
        assert!(!is_rotated_log_name("portal.log", "portal.log"));
        assert!(!is_rotated_log_name("portal.log.", "portal.log"));
        assert!(!is_rotated_log_name("portal.login", "portal.log"));
    }

    #[test]
    fn prepare_app_log_dir_creates_missing_dir() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("logs");

        prepare_app_log_dir(&log_dir).expect("missing log directory should be created");

        assert!(log_dir.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn prepare_app_log_dir_makes_created_dir_private() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("logs");

        prepare_app_log_dir(&log_dir).expect("missing log directory should be created");

        let mode = std::fs::metadata(&log_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn prepare_app_log_dir_tightens_existing_dir_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("logs");
        std::fs::create_dir(&log_dir).unwrap();
        std::fs::set_permissions(&log_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        prepare_app_log_dir(&log_dir).expect("existing log directory should be accepted");

        let mode = std::fs::metadata(&log_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn prepare_app_log_dir_rejects_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let log_dir = dir.path().join("logs");
        std::fs::write(&log_dir, "not a directory").unwrap();

        let error = prepare_app_log_dir(&log_dir).expect_err("file should not be used as log dir");

        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
    }

    #[cfg(unix)]
    #[test]
    fn prepare_app_log_dir_rejects_symlinked_directory() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target");
        let link = dir.path().join("logs");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = prepare_app_log_dir(&link).expect_err("symlink should not be used as log dir");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(std::fs::read_dir(target).unwrap().next().is_none());
    }

    #[test]
    fn cleanup_old_logs_ignores_similar_prefixes_and_base_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("portal.log"), "keep").unwrap();
        std::fs::write(dir.path().join("portal.log.1"), "old").unwrap();
        std::fs::write(dir.path().join("portal.log.2"), "old").unwrap();
        std::fs::write(dir.path().join("portal.login"), "keep").unwrap();

        cleanup_old_logs(dir.path(), "portal.log", 1);

        assert!(dir.path().join("portal.log").exists());
        assert!(dir.path().join("portal.login").exists());
    }

    #[cfg(unix)]
    #[test]
    fn cleanup_old_logs_ignores_rotated_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.log");
        let link = dir.path().join("portal.log.1");
        let regular = dir.path().join("portal.log.2");
        std::fs::write(&target, "target").unwrap();
        std::fs::write(&regular, "old").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        cleanup_old_logs(dir.path(), "portal.log", 0);

        assert!(link.exists());
        assert_eq!(std::fs::read_to_string(target).unwrap(), "target");
        assert!(!regular.exists());
    }
}
