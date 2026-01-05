use std::sync::Arc;

use russh::client::Handle;
use russh::{Channel, ChannelMsg};
use tokio::sync::{mpsc, Mutex};

use crate::error::SshError;

use super::handler::ClientHandler;
use super::SshEvent;

/// Commands that can be sent to the channel task
enum ChannelCommand {
    Data(Vec<u8>),
    WindowChange { cols: u32, rows: u32 },
}

/// Active SSH session handle
pub struct SshSession {
    command_tx: mpsc::UnboundedSender<ChannelCommand>,
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
        event_tx: mpsc::UnboundedSender<SshEvent>,
    ) -> Self {
        let (command_tx, mut command_rx) = mpsc::unbounded_channel::<ChannelCommand>();

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
                                let _ = event_tx.send(SshEvent::Data(data.to_vec()));
                            }
                            Some(ChannelMsg::ExtendedData { data, .. }) => {
                                let _ = event_tx.send(SshEvent::Data(data.to_vec()));
                            }
                            Some(ChannelMsg::Eof) => {
                                let _ = event_tx.send(SshEvent::Disconnected);
                                break;
                            }
                            Some(ChannelMsg::Close) => {
                                let _ = event_tx.send(SshEvent::Disconnected);
                                break;
                            }
                            Some(ChannelMsg::ExitStatus { exit_status }) => {
                                tracing::debug!("Exit status: {}", exit_status);
                            }
                            Some(_) => {}
                            None => {
                                let _ = event_tx.send(SshEvent::Disconnected);
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

        Self {
            command_tx,
            handle,
        }
    }

    /// Send data to the remote shell
    pub async fn send(&self, data: &[u8]) -> Result<(), SshError> {
        self.command_tx
            .send(ChannelCommand::Data(data.to_vec()))
            .map_err(|e| SshError::Channel(e.to_string()))?;
        Ok(())
    }

    /// Notify the remote shell of a window size change
    pub fn window_change(&self, cols: u16, rows: u16) -> Result<(), SshError> {
        self.command_tx
            .send(ChannelCommand::WindowChange {
                cols: cols as u32,
                rows: rows as u32,
            })
            .map_err(|e| SshError::Channel(e.to_string()))?;
        Ok(())
    }

    /// Execute a command on the remote host and return its stdout output.
    /// This opens a new exec channel separate from the interactive PTY.
    pub async fn execute_command(&self, command: &str) -> Result<String, SshError> {
        let handle = self.handle.lock().await;

        let mut channel = handle
            .channel_open_session()
            .await
            .map_err(|e| SshError::Channel(format!("Failed to open channel: {}", e)))?;

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
    }
}
