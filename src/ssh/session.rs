use russh::client::Handle;
use russh::{Channel, ChannelMsg, Disconnect};
use tokio::sync::mpsc;
use tokio::sync::Mutex;

use crate::error::SshError;

use super::handler::ClientHandler;
use super::SshEvent;

/// Commands that can be sent to the channel task
enum ChannelCommand {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
}

/// Active SSH session handle
pub struct SshSession {
    handle: Mutex<Handle<ClientHandler>>,
    command_tx: mpsc::UnboundedSender<ChannelCommand>,
}

impl std::fmt::Debug for SshSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshSession")
            .field("command_tx", &"<channel>")
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

        // Spawn task that owns the channel and handles both read and write
        tokio::spawn(async move {
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
                            Some(ChannelCommand::Resize(cols, rows)) => {
                                if let Err(e) = channel.window_change(cols as u32, rows as u32, 0, 0).await {
                                    tracing::error!("Failed to resize: {}", e);
                                }
                            }
                            Some(ChannelCommand::Close) => {
                                let _ = channel.eof().await;
                                let _ = channel.close().await;
                                break;
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
            handle: Mutex::new(handle),
            command_tx,
        }
    }

    /// Send data to the remote shell
    pub async fn send(&self, data: &[u8]) -> Result<(), SshError> {
        self.command_tx
            .send(ChannelCommand::Data(data.to_vec()))
            .map_err(|e| SshError::Channel(e.to_string()))?;
        Ok(())
    }

    /// Request terminal resize
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), SshError> {
        self.command_tx
            .send(ChannelCommand::Resize(cols, rows))
            .map_err(|e| SshError::Channel(e.to_string()))?;
        Ok(())
    }

    /// Gracefully close the session
    pub async fn close(self) -> Result<(), SshError> {
        self.close_shared().await
    }

    /// Gracefully close the session without consuming the handle
    pub async fn close_shared(&self) -> Result<(), SshError> {
        let _ = self.command_tx.send(ChannelCommand::Close);

        let handle = self.handle.lock().await;
        handle
            .disconnect(Disconnect::ByApplication, "User disconnected", "")
            .await
            .map_err(|e| SshError::Session(e.to_string()))?;

        Ok(())
    }

    /// Execute a command on the remote server using a new channel
    /// Returns the command output (stdout + stderr combined)
    pub async fn execute_command(&self, command: &str) -> Result<String, SshError> {
        // Open channel and start command - release lock quickly
        let mut channel = {
            let handle = self.handle.lock().await;
            handle
                .channel_open_session()
                .await
                .map_err(|e| SshError::Channel(format!("Failed to open exec channel: {}", e)))?
        };

        channel
            .exec(true, command)
            .await
            .map_err(|e| SshError::Channel(format!("Failed to execute command: {}", e)))?;

        // Collect output without holding the handle lock
        let mut output = Vec::new();
        let mut exit_code: Option<u32> = None;

        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => output.extend_from_slice(&data),
                Some(ChannelMsg::ExtendedData { data, .. }) => output.extend_from_slice(&data),
                Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                _ => continue,
            }
        }

        // Explicitly close the exec channel
        let _ = channel.close().await;

        // Only fail if we received an explicit non-zero exit code
        if let Some(code) = exit_code {
            if code != 0 {
                return Err(SshError::Channel(format!(
                    "Command exited with code {}: {}",
                    code,
                    String::from_utf8_lossy(&output)
                )));
            }
        }

        Ok(String::from_utf8_lossy(&output).to_string())
    }
}
