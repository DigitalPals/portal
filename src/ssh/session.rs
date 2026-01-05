use russh::client::Handle;
use russh::{Channel, ChannelMsg};
use tokio::sync::mpsc;

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

        // Spawn task that owns the channel and handle, keeping the connection alive
        tokio::spawn(async move {
            // Keep handle alive for the duration of the session
            let _handle = handle;

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
}
