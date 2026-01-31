//! Logging initialization with file output support

use std::path::{Path, PathBuf};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

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

    match log_dir {
        Some(dir) => {
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

            Some(guard)
        }
        None => {
            // Console-only logging
            tracing_subscriber::registry()
                .with(env_filter)
                .with(console_layer)
                .init();
            None
        }
    }
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
        if !name.starts_with(prefix) {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
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
        let _ = std::fs::remove_file(path);
    }
}
