//! Logging initialization with file output support

use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Initialize logging with optional file output.
/// Returns a guard that must be kept alive for the duration of the program.
pub fn init_logging(log_dir: Option<PathBuf>) -> Option<WorkerGuard> {
    let env_filter = EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into());

    let console_layer = fmt::layer().with_target(true).with_thread_ids(false);

    match log_dir {
        Some(dir) => {
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
