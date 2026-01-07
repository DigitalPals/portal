use std::sync::Arc;
use std::time::Duration;

use russh::client::Handle;
use russh::{Channel, ChannelMsg};
use tokio::sync::{Mutex, mpsc};
use tokio::time::timeout;

use crate::error::SshError;

use super::SshEvent;

/// Result of executing a command, including output and exit code
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}
use super::handler::ClientHandler;

/// Commands that can be sent to the channel task
enum ChannelCommand {
    Data(Vec<u8>),
    WindowChange { cols: u32, rows: u32 },
}

/// Active SSH session handle
pub struct SshSession {
    command_tx: mpsc::Sender<ChannelCommand>,
    handle: Arc<Mutex<Handle<ClientHandler>>>,
}

impl std::fmt::Debug for SshSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshSession")
            .field("command_tx", &"<channel>")
            .field("handle", &"<handle>")
            .finish()
    }
}

impl SshSession {
    /// Create a new session and spawn the channel I/O task
    pub fn new(
        handle: Handle<ClientHandler>,
        mut channel: Channel<russh::client::Msg>,
        event_tx: mpsc::Sender<SshEvent>,
    ) -> Self {
        let (command_tx, mut command_rx) = mpsc::channel::<ChannelCommand>(256);

        // Wrap handle in Arc<Mutex> so it can be shared
        let handle = Arc::new(Mutex::new(handle));
        let handle_for_task = handle.clone();

        // Spawn task that owns the channel, keeping the connection alive
        tokio::spawn(async move {
            // Keep handle reference alive for the duration of the session
            let _handle = handle_for_task;

            loop {
                tokio::select! {
                    // Handle incoming SSH data
                    msg = channel.wait() => {
                        match msg {
                            Some(ChannelMsg::Data { data }) => {
                                if event_tx.send(SshEvent::Data(data.to_vec())).await.is_err() {
                                    break;
                                }
                            }
                            Some(ChannelMsg::ExtendedData { data, .. }) => {
                                if event_tx.send(SshEvent::Data(data.to_vec())).await.is_err() {
                                    break;
                                }
                            }
                            Some(ChannelMsg::Eof) => {
                                let _ = event_tx.send(SshEvent::Disconnected).await;
                                break;
                            }
                            Some(ChannelMsg::Close) => {
                                let _ = event_tx.send(SshEvent::Disconnected).await;
                                break;
                            }
                            Some(ChannelMsg::ExitStatus { exit_status }) => {
                                tracing::debug!("Exit status: {}", exit_status);
                            }
                            Some(_) => {}
                            None => {
                                let _ = event_tx.send(SshEvent::Disconnected).await;
                                break;
                            }
                        }
                    }
                    // Handle outgoing commands from the main task
                    cmd = command_rx.recv() => {
                        match cmd {
                            Some(ChannelCommand::Data(data)) => {
                                if let Err(e) = channel.data(&data[..]).await {
                                    tracing::error!("Failed to send data: {}", e);
                                }
                            }
                            Some(ChannelCommand::WindowChange { cols, rows }) => {
                                if let Err(e) = channel.window_change(cols, rows, 0, 0).await {
                                    tracing::error!("Failed to send window change: {}", e);
                                }
                            }
                            None => {
                                // Command channel closed, exit
                                break;
                            }
                        }
                    }
                }
            }
        });

        Self { command_tx, handle }
    }

    /// Send data to the remote shell
    pub async fn send(&self, data: &[u8]) -> Result<(), SshError> {
        self.command_tx
            .send(ChannelCommand::Data(data.to_vec()))
            .await
            .map_err(|e| {
                tracing::debug!("SSH send failed: {}", e);
                SshError::Channel(e.to_string())
            })?;
        Ok(())
    }

    /// Notify the remote shell of a window size change
    pub async fn window_change(&self, cols: u16, rows: u16) -> Result<(), SshError> {
        self.command_tx
            .send(ChannelCommand::WindowChange {
                cols: cols as u32,
                rows: rows as u32,
            })
            .await
            .map_err(|e| {
                tracing::debug!("SSH window change failed: {}", e);
                SshError::Channel(e.to_string())
            })?;
        Ok(())
    }

    /// Execute a command on the remote host and return its stdout output.
    /// This opens a new exec channel separate from the interactive PTY.
    pub async fn execute_command(&self, command: &str) -> Result<String, SshError> {
        let timeout_result = timeout(Duration::from_secs(10), async {
            let handle = self.handle.lock().await;
            let mut channel = handle
                .channel_open_session()
                .await
                .map_err(|e| SshError::Channel(format!("Failed to open channel: {}", e)))?;
            drop(handle);

            channel
                .exec(true, command)
                .await
                .map_err(|e| SshError::Channel(format!("Failed to exec '{}': {}", command, e)))?;

            let mut output = String::new();

            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Data { data }) => {
                        if let Ok(s) = std::str::from_utf8(&data) {
                            output.push_str(s);
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        // Log stderr but don't include in output
                        tracing::debug!("{} stderr: {:?}", command, std::str::from_utf8(&data));
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        break;
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        if exit_status != 0 {
                            tracing::debug!("{} exited with status {}", command, exit_status);
                        }
                    }
                    Some(_) => {}
                }
            }

            Ok(output)
        })
        .await;

        match timeout_result {
            Ok(result) => result,
            Err(_) => Err(SshError::Channel(format!(
                "Command '{}' timed out",
                command
            ))),
        }
    }

    /// Execute a command and return full result including exit code
    /// This method captures stdout, stderr, and the exit code
    pub async fn execute_command_full(
        &self,
        command: &str,
        timeout_secs: u64,
    ) -> Result<CommandResult, SshError> {
        let timeout_result = timeout(Duration::from_secs(timeout_secs), async {
            let handle = self.handle.lock().await;
            let mut channel = handle
                .channel_open_session()
                .await
                .map_err(|e| SshError::Channel(format!("Failed to open channel: {}", e)))?;
            drop(handle);

            channel
                .exec(true, command)
                .await
                .map_err(|e| SshError::Channel(format!("Failed to exec '{}': {}", command, e)))?;

            let mut stdout = String::new();
            let mut stderr = String::new();
            let mut exit_code: i32 = 0;

            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Data { data }) => {
                        if let Ok(s) = std::str::from_utf8(&data) {
                            stdout.push_str(s);
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        if let Ok(s) = std::str::from_utf8(&data) {
                            stderr.push_str(s);
                        }
                    }
                    Some(ChannelMsg::ExitStatus { exit_status }) => {
                        exit_code = exit_status as i32;
                    }
                    Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => {
                        break;
                    }
                    Some(_) => {}
                }
            }

            Ok(CommandResult {
                stdout,
                stderr,
                exit_code,
            })
        })
        .await;

        match timeout_result {
            Ok(result) => result,
            Err(_) => Err(SshError::Channel(format!(
                "Command '{}' timed out after {} seconds",
                command, timeout_secs
            ))),
        }
    }
}
