//! Local PTY session management
//!
//! Spawns a local shell with a pseudo-terminal and manages I/O.

use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use std::io::{Read, Write};
use tokio::sync::mpsc;

use crate::error::LocalError;

/// Events emitted by local PTY sessions
#[derive(Debug)]
pub enum LocalEvent {
    /// Data received from the PTY
    Data(Vec<u8>),
    /// PTY session has ended
    Disconnected,
}

/// Commands that can be sent to the PTY task
enum PtyCommand {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

/// Handle to an active local terminal session
pub struct LocalSession {
    command_tx: mpsc::Sender<PtyCommand>,
}

impl std::fmt::Debug for LocalSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalSession")
            .field("command_tx", &"<channel>")
            .finish()
    }
}

impl LocalSession {
    /// Spawn a new local terminal session
    ///
    /// Uses `$SHELL` environment variable or falls back to `/bin/sh`.
    /// Returns a session handle and spawns a background task for PTY I/O.
    pub fn spawn(
        cols: u16,
        rows: u16,
        event_tx: mpsc::Sender<LocalEvent>,
    ) -> Result<Self, LocalError> {
        // Get user's shell
        let shell = if cfg!(target_os = "windows") {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
        };
        tracing::info!("Spawning local terminal");

        // Create PTY system
        let pty_system = native_pty_system();

        // Create PTY pair with initial size
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| LocalError::PtyCreation(e.to_string()))?;

        // Build command for shell as login shell to source profile/rc files
        let mut cmd = CommandBuilder::new(&shell);
        if !cfg!(target_os = "windows") {
            cmd.arg("-l");
        }
        // Set TERM for proper terminal emulation
        cmd.env("TERM", "xterm-256color");

        // Spawn shell process
        let _child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| LocalError::SpawnFailed(e.to_string()))?;

        // Get master for I/O
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| LocalError::Io(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| LocalError::Io(e.to_string()))?;

        // Keep master alive for resize operations
        let master = pair.master;

        let (command_tx, command_rx) = mpsc::channel::<PtyCommand>(256);

        // Spawn background task for PTY I/O
        Self::spawn_io_task(reader, writer, master, command_rx, event_tx);

        Ok(Self { command_tx })
    }

    /// Spawn the background I/O task
    fn spawn_io_task(
        mut reader: Box<dyn Read + Send>,
        mut writer: Box<dyn Write + Send>,
        master: Box<dyn portable_pty::MasterPty + Send>,
        mut command_rx: mpsc::Receiver<PtyCommand>,
        event_tx: mpsc::Sender<LocalEvent>,
    ) {
        // Reader task - reads from PTY in a blocking thread
        let event_tx_reader = event_tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        // EOF - PTY closed
                        let _ = event_tx_reader.blocking_send(LocalEvent::Disconnected);
                        break;
                    }
                    Ok(n) => {
                        let _ = event_tx_reader.blocking_send(LocalEvent::Data(buf[..n].to_vec()));
                    }
                    Err(e) => {
                        tracing::error!("PTY read error: {}", e);
                        let _ = event_tx_reader.blocking_send(LocalEvent::Disconnected);
                        break;
                    }
                }
            }
        });

        // Writer task - receives commands and writes to PTY
        std::thread::spawn(move || {
            // Keep master alive for resize
            let _master = master;

            while let Some(cmd) = command_rx.blocking_recv() {
                match cmd {
                    PtyCommand::Data(data) => {
                        if let Err(e) = writer.write_all(&data) {
                            tracing::error!("PTY write error: {}", e);
                            break;
                        }
                        let _ = writer.flush();
                    }
                    PtyCommand::Resize { cols, rows } => {
                        if let Err(e) = _master.resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        }) {
                            tracing::error!("PTY resize error: {}", e);
                        }
                    }
                }
            }

            // Channel closed, send disconnect
            let _ = event_tx.blocking_send(LocalEvent::Disconnected);
        });
    }

    /// Send data to the local shell
    pub async fn send(&self, data: &[u8]) -> Result<(), LocalError> {
        self.command_tx
            .send(PtyCommand::Data(data.to_vec()))
            .await
            .map_err(|e| {
                tracing::debug!("Local PTY send failed: {}", e);
                LocalError::Io(e.to_string())
            })?;
        Ok(())
    }

    /// Notify the local shell of a window size change
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), LocalError> {
        self.command_tx
            .send(PtyCommand::Resize { cols, rows })
            .await
            .map_err(|e| {
                tracing::debug!("Local PTY resize failed: {}", e);
                LocalError::Io(e.to_string())
            })?;
        Ok(())
    }

    /// Create a stub LocalSession for testing purposes.
    /// The returned session has a disconnected channel and cannot perform real operations.
    #[cfg(test)]
    pub fn new_test_stub() -> Self {
        let (command_tx, _rx) = mpsc::channel(1);
        Self { command_tx }
    }
}
