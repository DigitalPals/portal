//! Terminal session logging utilities

use std::path::{Path, PathBuf};

use chrono::Local;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::config::paths;
use crate::config::settings::SessionLogFormat;
use crate::fs_utils::{ensure_private_dir_no_follow, open_append_regular_file};

const LOG_QUEUE_CAPACITY: usize = 256;

enum LogCommand {
    Write(Vec<u8>),
    Shutdown,
}

/// Async session logger for terminal output.
pub struct SessionLogger {
    path: PathBuf,
    sender: mpsc::Sender<LogCommand>,
    join_handle: tokio::task::JoinHandle<()>,
}

impl SessionLogger {
    pub fn start(
        host_name: &str,
        log_dir: PathBuf,
        format: SessionLogFormat,
    ) -> std::io::Result<Self> {
        let log_dir = normalize_log_dir(log_dir);
        prepare_session_log_dir(&log_dir)?;

        let filename = session_log_filename(host_name);
        let path = log_dir.join(filename);
        let file = open_session_log_file(&path)?;

        let (sender, mut receiver) = mpsc::channel(LOG_QUEUE_CAPACITY);
        let path_for_task = path.clone();

        let join_handle = tokio::spawn(async move {
            let mut file = tokio::fs::File::from_std(file);
            let mut at_line_start = true;
            while let Some(command) = receiver.recv().await {
                match command {
                    LogCommand::Write(data) => {
                        if let Err(error) =
                            write_log_data(&mut file, &data, format, &mut at_line_start).await
                        {
                            tracing::error!(
                                "Failed writing session log {}: {}",
                                path_for_task.display(),
                                error
                            );
                        }
                    }
                    LogCommand::Shutdown => break,
                }
            }

            let _ = file.flush().await;
        });

        Ok(Self {
            path,
            sender,
            join_handle,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(&self, data: &[u8]) {
        if self
            .sender
            .try_send(LogCommand::Write(data.to_vec()))
            .is_err()
        {
            tracing::warn!("Session log queue full; dropping output chunk");
        }
    }

    pub async fn shutdown(self) {
        let _ = self.sender.send(LogCommand::Shutdown).await;
        let _ = self.join_handle.await;
    }
}

fn open_session_log_file(path: &Path) -> std::io::Result<std::fs::File> {
    open_append_regular_file(path)
}

fn prepare_session_log_dir(path: &Path) -> std::io::Result<()> {
    ensure_private_dir_no_follow(path)
}

fn session_log_filename(host_name: &str) -> String {
    let sanitized = sanitize_filename(host_name);
    let timestamp = Local::now().format("%Y-%m-%d_%H-%M-%S");
    format!("{}_{}.log", sanitized, timestamp)
}

fn sanitize_filename(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
            output.push(ch);
        } else {
            output.push('_');
        }
    }

    if output.is_empty() {
        "session".to_string()
    } else {
        output
    }
}

fn normalize_log_dir(log_dir: PathBuf) -> PathBuf {
    let raw = log_dir.to_string_lossy();
    if raw.starts_with("~/") || raw == "~" {
        paths::expand_tilde(&raw)
    } else {
        log_dir
    }
}

async fn write_log_data(
    file: &mut tokio::fs::File,
    data: &[u8],
    format: SessionLogFormat,
    at_line_start: &mut bool,
) -> std::io::Result<()> {
    match format {
        SessionLogFormat::Plain => {
            file.write_all(data).await?;
        }
        SessionLogFormat::Timestamped => {
            let mut output = Vec::with_capacity(data.len() + 16);
            let mut start = 0;

            for (idx, byte) in data.iter().enumerate() {
                if *byte == b'\n' {
                    if start <= idx {
                        if *at_line_start {
                            append_timestamp(&mut output);
                        }
                        output.extend_from_slice(&data[start..=idx]);
                    }
                    *at_line_start = true;
                    start = idx + 1;
                }
            }

            if start < data.len() {
                if *at_line_start {
                    append_timestamp(&mut output);
                }
                output.extend_from_slice(&data[start..]);
                *at_line_start = false;
            }

            file.write_all(&output).await?;
        }
    }

    Ok(())
}

fn append_timestamp(output: &mut Vec<u8>) {
    let timestamp = Local::now().format("%H:%M:%S").to_string();
    output.extend_from_slice(b"[");
    output.extend_from_slice(timestamp.as_bytes());
    output.extend_from_slice(b"] ");
}

#[cfg(test)]
mod tests {
    use super::{SessionLogger, open_session_log_file, prepare_session_log_dir};
    use crate::config::settings::SessionLogFormat;

    #[tokio::test]
    async fn session_logger_writes_plain_log() {
        let temp = tempfile::tempdir().unwrap();
        let logger = SessionLogger::start(
            "example-host",
            temp.path().to_path_buf(),
            SessionLogFormat::Plain,
        )
        .expect("session logger should start");
        let path = logger.path().to_path_buf();

        logger.write(b"hello\n");
        logger.shutdown().await;

        assert_eq!(std::fs::read_to_string(path).unwrap(), "hello\n");
    }

    #[test]
    fn prepare_session_log_dir_creates_missing_dir() {
        let temp = tempfile::tempdir().unwrap();
        let log_dir = temp.path().join("logs");

        prepare_session_log_dir(&log_dir).expect("missing log directory should be created");

        assert!(log_dir.is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn prepare_session_log_dir_makes_created_dir_private() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let log_dir = temp.path().join("logs");

        prepare_session_log_dir(&log_dir).expect("missing log directory should be created");

        let mode = std::fs::metadata(&log_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn prepare_session_log_dir_tightens_existing_dir_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp = tempfile::tempdir().unwrap();
        let log_dir = temp.path().join("logs");
        std::fs::create_dir(&log_dir).unwrap();
        std::fs::set_permissions(&log_dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        prepare_session_log_dir(&log_dir).expect("existing log directory should be accepted");

        let mode = std::fs::metadata(&log_dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn prepare_session_log_dir_rejects_regular_file() {
        let temp = tempfile::tempdir().unwrap();
        let log_dir = temp.path().join("logs");
        std::fs::write(&log_dir, "not a directory").unwrap();

        let error =
            prepare_session_log_dir(&log_dir).expect_err("file should not be used as log dir");

        assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
    }

    #[cfg(unix)]
    #[test]
    fn prepare_session_log_dir_rejects_symlinked_directory() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("logs");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error =
            prepare_session_log_dir(&link).expect_err("symlink should not be used as log dir");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(std::fs::read_dir(target).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn session_logger_rejects_symlinked_log_dir_without_writing_target() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("logs");
        std::fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert!(SessionLogger::start("example-host", link, SessionLogFormat::Plain).is_err());

        assert!(std::fs::read_dir(target).unwrap().next().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn open_session_log_file_rejects_symlink_without_writing_target() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target.log");
        let link = temp.path().join("session.log");
        std::fs::write(&target, "original\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let error = open_session_log_file(&link)
            .expect_err("session log should not append through symlink");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(std::fs::read_to_string(target).unwrap(), "original\n");
    }
}
