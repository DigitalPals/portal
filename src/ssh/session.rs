use std::sync::{Arc, OnceLock};
use std::time::Duration;

use russh::client::Handle;
use russh::{Channel, ChannelMsg, Disconnect};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
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

const DEFAULT_COMMAND_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;

fn command_output_limit() -> usize {
    static LIMIT: OnceLock<usize> = OnceLock::new();
    *LIMIT.get_or_init(|| {
        std::env::var("PORTAL_MAX_COMMAND_OUTPUT_BYTES")
            .ok()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_COMMAND_OUTPUT_LIMIT)
    })
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
        let output_limit = command_output_limit();
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
            let mut total_bytes = 0usize;

            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Data { data }) => {
                        total_bytes = total_bytes.saturating_add(data.len());
                        if total_bytes > output_limit {
                            tracing::warn!(
                                "Command output exceeded limit ({} bytes)",
                                output_limit
                            );
                            return Err(SshError::Channel(format!(
                                "Command output exceeded {} bytes",
                                output_limit
                            )));
                        }
                        if let Ok(s) = std::str::from_utf8(&data) {
                            output.push_str(s);
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        total_bytes = total_bytes.saturating_add(data.len());
                        if total_bytes > output_limit {
                            tracing::warn!(
                                "Command output exceeded limit ({} bytes)",
                                output_limit
                            );
                            return Err(SshError::Channel(format!(
                                "Command output exceeded {} bytes",
                                output_limit
                            )));
                        }
                        // Avoid logging stderr contents to prevent leaking secrets.
                        tracing::debug!("{} stderr ({} bytes)", command, data.len());
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
        let output_limit = command_output_limit();
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
            let mut total_bytes = 0usize;

            loop {
                match channel.wait().await {
                    Some(ChannelMsg::Data { data }) => {
                        total_bytes = total_bytes.saturating_add(data.len());
                        if total_bytes > output_limit {
                            tracing::warn!(
                                "Command output exceeded limit ({} bytes)",
                                output_limit
                            );
                            return Err(SshError::Channel(format!(
                                "Command output exceeded {} bytes",
                                output_limit
                            )));
                        }
                        if let Ok(s) = std::str::from_utf8(&data) {
                            stdout.push_str(s);
                        }
                    }
                    Some(ChannelMsg::ExtendedData { data, .. }) => {
                        total_bytes = total_bytes.saturating_add(data.len());
                        if total_bytes > output_limit {
                            tracing::warn!(
                                "Command output exceeded limit ({} bytes)",
                                output_limit
                            );
                            return Err(SshError::Channel(format!(
                                "Command output exceeded {} bytes",
                                output_limit
                            )));
                        }
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

pub async fn spawn_agent_forwarding(
    mut channel: Channel<russh::client::Msg>,
) -> Result<(), SshError> {
    let agent_path = std::env::var("SSH_AUTH_SOCK").map_err(|_| {
        SshError::Agent("SSH_AUTH_SOCK not set - is ssh-agent running?".to_string())
    })?;

    let stream = UnixStream::connect(&agent_path)
        .await
        .map_err(|e| SshError::Agent(format!("Failed to connect to SSH agent: {}", e)))?;

    let (mut agent_reader, mut agent_writer) = stream.into_split();

    tokio::spawn(async move {
        let mut buffer = vec![0u8; 16 * 1024];

        loop {
            tokio::select! {
                msg = channel.wait() => {
                    match msg {
                        Some(ChannelMsg::Data { data }) => {
                            if agent_writer.write_all(&data).await.is_err() {
                                break;
                            }
                        }
                        Some(ChannelMsg::ExtendedData { data, .. }) => {
                            if agent_writer.write_all(&data).await.is_err() {
                                break;
                            }
                        }
                        Some(ChannelMsg::Eof | ChannelMsg::Close) | None => {
                            break;
                        }
                        Some(_) => {}
                    }
                }
                read = agent_reader.read(&mut buffer) => {
                    match read {
                        Ok(0) => break,
                        Ok(count) => {
                            if channel.data(&buffer[..count]).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }
        }

        let _ = channel.eof().await;
        let _ = channel.close().await;
    });

    Ok(())
}

impl Drop for SshSession {
    fn drop(&mut self) {
        tracing::debug!("SSH session cleanup: closing command channel");
        let (replacement_tx, _replacement_rx) = mpsc::channel(1);
        let _ = std::mem::replace(&mut self.command_tx, replacement_tx);
        let handle = self.handle.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    let handle = handle.lock().await;
                    if let Err(e) = handle
                        .disconnect(Disconnect::ByApplication, "session dropped", "en")
                        .await
                    {
                        tracing::debug!("SSH disconnect failed: {}", e);
                    }
                });
            }
            Err(_) => {
                tracing::debug!("SSH session dropped without a Tokio runtime; disconnect skipped");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === CommandResult tests ===

    #[test]
    fn command_result_stores_stdout() {
        let result = CommandResult {
            stdout: "hello world".to_string(),
            stderr: String::new(),
            exit_code: 0,
        };
        assert_eq!(result.stdout, "hello world");
    }

    #[test]
    fn command_result_stores_stderr() {
        let result = CommandResult {
            stdout: String::new(),
            stderr: "error message".to_string(),
            exit_code: 1,
        };
        assert_eq!(result.stderr, "error message");
    }

    #[test]
    fn command_result_stores_exit_code() {
        let result = CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 42,
        };
        assert_eq!(result.exit_code, 42);
    }

    #[test]
    fn command_result_success_exit_code() {
        let result = CommandResult {
            stdout: "output".to_string(),
            stderr: String::new(),
            exit_code: 0,
        };
        assert_eq!(result.exit_code, 0);
    }

    #[test]
    fn command_result_failure_exit_code() {
        let result = CommandResult {
            stdout: String::new(),
            stderr: "command not found".to_string(),
            exit_code: 127,
        };
        assert_eq!(result.exit_code, 127);
    }

    #[test]
    fn command_result_negative_exit_code() {
        // Some systems use negative exit codes for signals
        let result = CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: -9, // SIGKILL
        };
        assert_eq!(result.exit_code, -9);
    }

    #[test]
    fn command_result_clone() {
        let original = CommandResult {
            stdout: "output".to_string(),
            stderr: "errors".to_string(),
            exit_code: 1,
        };
        let cloned = original.clone();

        assert_eq!(original.stdout, cloned.stdout);
        assert_eq!(original.stderr, cloned.stderr);
        assert_eq!(original.exit_code, cloned.exit_code);
    }

    #[test]
    fn command_result_debug() {
        let result = CommandResult {
            stdout: "out".to_string(),
            stderr: "err".to_string(),
            exit_code: 0,
        };
        let debug_str = format!("{:?}", result);

        assert!(debug_str.contains("CommandResult"));
        assert!(debug_str.contains("stdout"));
        assert!(debug_str.contains("stderr"));
        assert!(debug_str.contains("exit_code"));
    }

    #[test]
    fn command_result_with_multiline_output() {
        let result = CommandResult {
            stdout: "line1\nline2\nline3\n".to_string(),
            stderr: "warn1\nwarn2\n".to_string(),
            exit_code: 0,
        };

        assert!(result.stdout.contains('\n'));
        assert_eq!(result.stdout.lines().count(), 3);
        assert_eq!(result.stderr.lines().count(), 2);
    }

    #[test]
    fn command_result_with_empty_strings() {
        let result = CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
        };

        assert!(result.stdout.is_empty());
        assert!(result.stderr.is_empty());
    }

    #[test]
    fn command_result_with_unicode() {
        let result = CommandResult {
            stdout: "Hello 世界 🌍".to_string(),
            stderr: "Ошибка: файл не найден".to_string(),
            exit_code: 1,
        };

        assert!(result.stdout.contains("世界"));
        assert!(result.stdout.contains("🌍"));
        assert!(result.stderr.contains("Ошибка"));
    }

    #[test]
    fn command_result_with_large_output() {
        let large_stdout = "x".repeat(1_000_000); // 1MB of output
        let result = CommandResult {
            stdout: large_stdout.clone(),
            stderr: String::new(),
            exit_code: 0,
        };

        assert_eq!(result.stdout.len(), 1_000_000);
    }

    #[test]
    fn command_result_with_binary_like_content() {
        // Test with content that might appear in binary output converted to lossy string
        let result = CommandResult {
            stdout: "ELF\u{0}\u{0}\u{0}".to_string(),
            stderr: String::new(),
            exit_code: 0,
        };

        assert!(result.stdout.starts_with("ELF"));
    }

    #[test]
    fn command_result_common_exit_codes() {
        // Test common Unix exit codes
        let test_cases = [
            (0, "success"),
            (1, "general error"),
            (2, "misuse of shell builtin"),
            (126, "command not executable"),
            (127, "command not found"),
            (128, "invalid exit argument"),
            (130, "script terminated by Ctrl+C"),
            (255, "exit status out of range"),
        ];

        for (code, _description) in test_cases {
            let result = CommandResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: code,
            };
            assert_eq!(result.exit_code, code);
        }
    }

    // === SshSession Debug tests ===

    // Note: SshSession::new() requires actual SSH handles, so we can only test
    // that the Debug implementation exists and the struct is properly defined.
    // Full functional tests are done via integration tests.

    // === ChannelCommand coverage via documentation ===
    // ChannelCommand is private and tested implicitly through integration tests
    // that exercise send() and window_change() methods.
}
