//! Terminal session logging utilities

use std::path::{Path, PathBuf};

use chrono::Local;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::config::paths;
use crate::config::settings::SessionLogFormat;

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
        std::fs::create_dir_all(&log_dir)?;

        let filename = session_log_filename(host_name);
        let path = log_dir.join(filename);

        let (sender, mut receiver) = mpsc::channel(LOG_QUEUE_CAPACITY);
        let path_for_task = path.clone();

        let join_handle = tokio::spawn(async move {
            let file_result = tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path_for_task)
                .await;

            let mut file = match file_result {
                Ok(file) => file,
                Err(error) => {
                    tracing::error!(
                        "Failed to open session log file {}: {}",
                        path_for_task.display(),
                        error
                    );
                    return;
                }
            };

            let mut at_line_start = true;
            while let Some(command) = receiver.recv().await {
                match command {
                    LogCommand::Write(data) => {
                        if let Err(error) = write_log_data(&mut file, &data, format, &mut at_line_start).await
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
        } else if ch.is_whitespace() {
            output.push('_');
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
