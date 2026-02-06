use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use russh::client::Handle;
use russh::{Channel, ChannelMsg};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, UnixStream};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::timeout;
use uuid::Uuid;

use crate::config::{PortForward, PortForwardKind};
use crate::error::SshError;
use crate::security_log;

use super::SshEvent;
use super::connection_pool::SshConnection;

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
    // Keeps the underlying SSH transport alive while this interactive channel exists.
    _connection: Arc<SshConnection>,
    handle: Arc<Mutex<Handle<ClientHandler>>>,
    forward_handles: Arc<Mutex<HashMap<Uuid, ForwardHandle>>>,
    remote_forwards: Arc<Mutex<HashMap<Uuid, PortForward>>>,
    host: String,
    port: u16,
    disconnect_logged: Arc<AtomicBool>,
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
        connection: Arc<SshConnection>,
        mut channel: Channel<russh::client::Msg>,
        event_tx: mpsc::Sender<SshEvent>,
    ) -> Self {
        let (command_tx, mut command_rx) = mpsc::channel::<ChannelCommand>(256);
        let disconnect_logged = Arc::new(AtomicBool::new(false));

        let handle = connection.handle();
        let remote_forwards = connection.remote_forwards();
        let host = connection.host().to_string();
        let port = connection.port();

        let handle_for_task = handle.clone();
        let disconnect_logged_for_task = disconnect_logged.clone();
        let host_for_task = host.clone();
        let connection_for_task = connection.clone();

        // Spawn task that owns the channel, keeping the connection alive
        tokio::spawn(async move {
            // Keep the underlying connection alive for the duration of this channel.
            let _connection = connection_for_task;
            let _handle = handle_for_task;

            // Track if we received a clean exit (exit status 0)
            let mut clean_exit = false;

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
                                let _ = event_tx.send(SshEvent::Disconnected { clean: clean_exit }).await;
                                if !disconnect_logged_for_task
                                    .swap(true, Ordering::SeqCst)
                                {
                                    security_log::log_ssh_disconnect(
                                        &host_for_task,
                                        port,
                                        clean_exit,
                                    );
                                }
                                break;
                            }
                            Some(ChannelMsg::Close) => {
                                let _ = event_tx.send(SshEvent::Disconnected { clean: clean_exit }).await;
                                if !disconnect_logged_for_task
                                    .swap(true, Ordering::SeqCst)
                                {
                                    security_log::log_ssh_disconnect(
                                        &host_for_task,
                                        port,
                                        clean_exit,
                                    );
                                }
                                break;
                            }
                            Some(ChannelMsg::ExitStatus { exit_status }) => {
                                tracing::debug!("Exit status: {}", exit_status);
                                // Clean exit if the shell returned 0
                                clean_exit = exit_status == 0;
                            }
                            Some(_) => {}
                            None => {
                                // Connection dropped unexpectedly (no EOF/Close received)
                                let _ = event_tx.send(SshEvent::Disconnected { clean: false }).await;
                                if !disconnect_logged_for_task
                                    .swap(true, Ordering::SeqCst)
                                {
                                    security_log::log_ssh_disconnect(&host_for_task, port, false);
                                }
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
                                // Command channel closed; close only this channel and exit.
                                let _ = event_tx.send(SshEvent::Disconnected { clean: false }).await;
                                let _ = channel.eof().await;
                                let _ = channel.close().await;
                                break;
                            }
                        }
                    }
                }
            }
        });

        Self {
            command_tx,
            _connection: connection,
            handle,
            forward_handles: Arc::new(Mutex::new(HashMap::new())),
            remote_forwards,
            host,
            port,
            disconnect_logged,
        }
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

    pub async fn create_local_forward(&self, forward: PortForward) -> Result<(), SshError> {
        if forward.kind != PortForwardKind::Local {
            return Err(SshError::Channel("Forward kind is not local".to_string()));
        }

        let bind_addr = format!("{}:{}", forward.bind_host, forward.bind_port);
        let listener = TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| SshError::Channel(format!("Failed to bind {}: {}", bind_addr, e)))?;

        let actual_port = listener
            .local_addr()
            .map(|addr| addr.port())
            .unwrap_or(forward.bind_port);

        let (stop_tx, mut stop_rx) = oneshot::channel();
        let handle = self.handle.clone();
        let target_host = forward.target_host.clone();
        let target_port = forward.target_port;
        let bind_host = forward.bind_host.clone();
        let forward_id = forward.id;

        self.forward_handles.lock().await.insert(
            forward.id,
            ForwardHandle {
                id: forward.id,
                kind: PortForwardKind::Local,
                bind_host: bind_host.clone(),
                bind_port: actual_port,
                stop_tx: Some(stop_tx),
            },
        );

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => {
                        break;
                    }
                    accept_result = listener.accept() => {
                        let (mut socket, origin) = match accept_result {
                            Ok(result) => result,
                            Err(e) => {
                                tracing::debug!("Local forward {} accept error: {}", forward_id, e);
                                continue;
                            }
                        };

                        let handle = handle.clone();
                        let target_host = target_host.clone();
                        tokio::spawn(async move {
                            let origin_addr = origin.ip().to_string();
                            let origin_port = origin.port() as u32;
                            let channel = {
                                let handle_guard = handle.lock().await;
                                handle_guard
                                    .channel_open_direct_tcpip(
                                        target_host.clone(),
                                        target_port as u32,
                                        origin_addr,
                                        origin_port,
                                    )
                                    .await
                            };

                            let channel = match channel {
                                Ok(channel) => channel,
                                Err(e) => {
                                    tracing::debug!(
                                        "Local forward {} failed to open channel: {}",
                                        forward_id,
                                        e
                                    );
                                    let _ = socket.shutdown().await;
                                    return;
                                }
                            };

                            let mut channel_stream = channel.into_stream();
                            let _ = tokio::io::copy_bidirectional(&mut channel_stream, &mut socket)
                                .await;
                            let _ = channel_stream.shutdown().await;
                            let _ = socket.shutdown().await;
                        });
                    }
                }
            }
        });

        tracing::info!(
            "Local forward {} listening on {}:{}",
            forward_id,
            bind_host,
            actual_port
        );

        Ok(())
    }

    pub async fn create_remote_forward(&self, mut forward: PortForward) -> Result<(), SshError> {
        if forward.kind != PortForwardKind::Remote {
            return Err(SshError::Channel("Forward kind is not remote".to_string()));
        }

        let assigned_port = {
            let mut handle_guard = self.handle.lock().await;
            handle_guard
                .tcpip_forward(forward.bind_host.clone(), forward.bind_port as u32)
                .await
                .map_err(|e| {
                    SshError::Channel(format!(
                        "Failed to request remote forward {}:{}: {}",
                        forward.bind_host, forward.bind_port, e
                    ))
                })?
        };

        let actual_port = normalize_remote_forward_port(forward.bind_port, assigned_port);
        forward.bind_port = actual_port;

        self.remote_forwards
            .lock()
            .await
            .insert(forward.id, forward.clone());

        self.forward_handles.lock().await.insert(
            forward.id,
            ForwardHandle {
                id: forward.id,
                kind: PortForwardKind::Remote,
                bind_host: forward.bind_host.clone(),
                bind_port: actual_port,
                stop_tx: None,
            },
        );

        tracing::info!(
            "Remote forward {} requested on {}:{}",
            forward.id,
            forward.bind_host,
            actual_port
        );

        Ok(())
    }

    pub async fn create_dynamic_forward(&self, forward: PortForward) -> Result<(), SshError> {
        if forward.kind != PortForwardKind::Dynamic {
            return Err(SshError::Channel("Forward kind is not dynamic".to_string()));
        }

        let bind_addr = format!("{}:{}", forward.bind_host, forward.bind_port);
        let listener = TcpListener::bind(&bind_addr)
            .await
            .map_err(|e| SshError::Channel(format!("Failed to bind {}: {}", bind_addr, e)))?;

        let actual_port = listener
            .local_addr()
            .map(|addr| addr.port())
            .unwrap_or(forward.bind_port);

        let (stop_tx, mut stop_rx) = oneshot::channel();
        let handle = self.handle.clone();
        let bind_host = forward.bind_host.clone();
        let forward_id = forward.id;

        self.forward_handles.lock().await.insert(
            forward.id,
            ForwardHandle {
                id: forward.id,
                kind: PortForwardKind::Dynamic,
                bind_host: bind_host.clone(),
                bind_port: actual_port,
                stop_tx: Some(stop_tx),
            },
        );

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut stop_rx => {
                        break;
                    }
                    accept_result = listener.accept() => {
                        let (mut socket, origin) = match accept_result {
                            Ok(result) => result,
                            Err(e) => {
                                tracing::debug!("Dynamic forward {} accept error: {}", forward_id, e);
                                continue;
                            }
                        };

                        let handle = handle.clone();
                        tokio::spawn(async move {
                            let peer_ip = origin.ip().to_string();
                            let peer_port = origin.port() as u32;

                            let (target_host, target_port) = match socks5_handshake(&mut socket).await {
                                Ok(v) => v,
                                Err(e) => {
                                    tracing::debug!("Dynamic forward {} SOCKS handshake failed: {}", forward_id, e);
                                    let _ = socket.shutdown().await;
                                    return;
                                }
                            };

                            let channel = {
                                let handle_guard = handle.lock().await;
                                handle_guard
                                    .channel_open_direct_tcpip(
                                        target_host.clone(),
                                        target_port as u32,
                                        peer_ip,
                                        peer_port,
                                    )
                                    .await
                            };

                            let channel = match channel {
                                Ok(channel) => channel,
                                Err(e) => {
                                    tracing::debug!(
                                        "Dynamic forward {} failed to open channel to {}:{}: {}",
                                        forward_id,
                                        target_host,
                                        target_port,
                                        e
                                    );
                                    let _ = socket.shutdown().await;
                                    return;
                                }
                            };

                            let mut channel_stream = channel.into_stream();
                            let _ = tokio::io::copy_bidirectional(&mut channel_stream, &mut socket).await;
                            let _ = channel_stream.shutdown().await;
                            let _ = socket.shutdown().await;
                        });
                    }
                }
            }
        });

        tracing::info!(
            "Dynamic forward {} listening (SOCKS5) on {}:{}",
            forward_id,
            bind_host,
            actual_port
        );

        Ok(())
    }

    pub async fn stop_forward(&self, forward_id: Uuid) -> Result<(), SshError> {
        let mut handles = self.forward_handles.lock().await;
        let Some(mut handle) = handles.remove(&forward_id) else {
            return Ok(());
        };

        if handle.kind == PortForwardKind::Remote {
            self.remote_forwards.lock().await.remove(&forward_id);
        }

        handle.stop(self.handle.clone()).await
    }

    pub async fn stop_all_forwards(&self) {
        let mut handles = self.forward_handles.lock().await;
        let drained: Vec<ForwardHandle> = handles.drain().map(|(_, handle)| handle).collect();
        drop(handles);

        for mut handle in drained {
            if handle.kind == PortForwardKind::Remote {
                self.remote_forwards.lock().await.remove(&handle.id);
            }
            if let Err(e) = handle.stop(self.handle.clone()).await {
                tracing::debug!("Failed to stop forward {}: {}", handle.id, e);
            }
        }
    }
}

#[derive(Debug)]
pub struct ForwardHandle {
    pub id: Uuid,
    pub kind: PortForwardKind,
    pub bind_host: String,
    pub bind_port: u16,
    stop_tx: Option<oneshot::Sender<()>>,
}

impl ForwardHandle {
    async fn stop(&mut self, handle: Arc<Mutex<Handle<ClientHandler>>>) -> Result<(), SshError> {
        match self.kind {
            PortForwardKind::Local => {
                if let Some(stop_tx) = self.stop_tx.take() {
                    let _ = stop_tx.send(());
                }
                Ok(())
            }
            PortForwardKind::Remote => {
                let handle_guard = handle.lock().await;
                handle_guard
                    .cancel_tcpip_forward(self.bind_host.clone(), self.bind_port as u32)
                    .await
                    .map_err(|e| {
                        SshError::Channel(format!(
                            "Failed to cancel remote forward {}:{}: {}",
                            self.bind_host, self.bind_port, e
                        ))
                    })
            }
            PortForwardKind::Dynamic => {
                if let Some(stop_tx) = self.stop_tx.take() {
                    let _ = stop_tx.send(());
                }
                Ok(())
            }
        }
    }
}

fn normalize_remote_forward_port(requested: u16, assigned: u32) -> u16 {
    if requested == 0 {
        assigned as u16
    } else {
        requested
    }
}

async fn socks5_handshake<S>(stream: &mut S) -> Result<(String, u16), SshError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // Greeting: VER, NMETHODS, METHODS...
    let mut hdr = [0u8; 2];
    stream
        .read_exact(&mut hdr)
        .await
        .map_err(|e| SshError::Channel(format!("SOCKS greeting read failed: {}", e)))?;
    if hdr[0] != 0x05 {
        return Err(SshError::Channel(format!(
            "Unsupported SOCKS version: {}",
            hdr[0]
        )));
    }

    let nmethods = hdr[1] as usize;
    let mut methods = vec![0u8; nmethods];
    stream
        .read_exact(&mut methods)
        .await
        .map_err(|e| SshError::Channel(format!("SOCKS methods read failed: {}", e)))?;

    // Only support "no authentication" (0x00).
    if !methods.contains(&0x00) {
        let _ = stream.write_all(&[0x05, 0xff]).await;
        let _ = stream.flush().await;
        return Err(SshError::Channel(
            "SOCKS client offered no supported auth methods".to_string(),
        ));
    }
    stream
        .write_all(&[0x05, 0x00])
        .await
        .map_err(|e| SshError::Channel(format!("SOCKS method write failed: {}", e)))?;
    stream
        .flush()
        .await
        .map_err(|e| SshError::Channel(format!("SOCKS flush failed: {}", e)))?;

    // Request: VER CMD RSV ATYP ...
    let mut req_hdr = [0u8; 4];
    stream
        .read_exact(&mut req_hdr)
        .await
        .map_err(|e| SshError::Channel(format!("SOCKS request header read failed: {}", e)))?;
    if req_hdr[0] != 0x05 {
        return Err(SshError::Channel(format!(
            "Unsupported SOCKS request version: {}",
            req_hdr[0]
        )));
    }
    let cmd = req_hdr[1];
    let atyp = req_hdr[3];

    if cmd != 0x01 {
        // Command not supported
        let _ = stream
            .write_all(&[0x05, 0x07, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await;
        let _ = stream.flush().await;
        return Err(SshError::Channel(format!(
            "Unsupported SOCKS command: {}",
            cmd
        )));
    }

    let target_host =
        match atyp {
            0x01 => {
                // IPv4
                let mut ip = [0u8; 4];
                stream.read_exact(&mut ip).await.map_err(|e| {
                    SshError::Channel(format!("SOCKS IPv4 address read failed: {}", e))
                })?;
                format!("{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3])
            }
            0x03 => {
                // Domain
                let mut len = [0u8; 1];
                stream.read_exact(&mut len).await.map_err(|e| {
                    SshError::Channel(format!("SOCKS domain length read failed: {}", e))
                })?;
                let mut name = vec![0u8; len[0] as usize];
                stream
                    .read_exact(&mut name)
                    .await
                    .map_err(|e| SshError::Channel(format!("SOCKS domain read failed: {}", e)))?;
                String::from_utf8(name)
                    .map_err(|_| SshError::Channel("SOCKS domain is not UTF-8".to_string()))?
            }
            0x04 => {
                // IPv6
                let mut ip = [0u8; 16];
                stream.read_exact(&mut ip).await.map_err(|e| {
                    SshError::Channel(format!("SOCKS IPv6 address read failed: {}", e))
                })?;
                std::net::Ipv6Addr::from(ip).to_string()
            }
            _ => {
                // Address type not supported
                let _ = stream
                    .write_all(&[0x05, 0x08, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
                    .await;
                let _ = stream.flush().await;
                return Err(SshError::Channel(format!(
                    "Unsupported SOCKS address type: {}",
                    atyp
                )));
            }
        };

    let mut port_bytes = [0u8; 2];
    stream
        .read_exact(&mut port_bytes)
        .await
        .map_err(|e| SshError::Channel(format!("SOCKS port read failed: {}", e)))?;
    let target_port = u16::from_be_bytes(port_bytes);

    // Success reply with BND.ADDR = 0.0.0.0 and BND.PORT = 0.
    stream
        .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
        .await
        .map_err(|e| SshError::Channel(format!("SOCKS reply write failed: {}", e)))?;
    stream
        .flush()
        .await
        .map_err(|e| SshError::Channel(format!("SOCKS reply flush failed: {}", e)))?;

    Ok((target_host, target_port))
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
        if !self.disconnect_logged.swap(true, Ordering::SeqCst) {
            security_log::log_ssh_disconnect(&self.host, self.port, false);
        }
        tracing::debug!("SSH session cleanup: closing command channel");
        let (replacement_tx, _replacement_rx) = mpsc::channel(1);
        let _ = std::mem::replace(&mut self.command_tx, replacement_tx);
        let handle = self.handle.clone();
        let forward_handles = self.forward_handles.clone();
        let remote_forwards = self.remote_forwards.clone();
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move {
                    {
                        let mut handles = forward_handles.lock().await;
                        let drained: Vec<ForwardHandle> =
                            handles.drain().map(|(_, handle)| handle).collect();
                        drop(handles);

                        for mut forward in drained {
                            if forward.kind == PortForwardKind::Remote {
                                remote_forwards.lock().await.remove(&forward.id);
                            }
                            if let Err(e) = forward.stop(handle.clone()).await {
                                tracing::debug!("Failed to stop forward {}: {}", forward.id, e);
                            }
                        }
                    }
                });
            }
            Err(_) => {
                tracing::debug!("SSH session dropped without a Tokio runtime; cleanup skipped");
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

    // === SOCKS5 tests ===

    #[tokio::test]
    async fn socks5_handshake_domain_connect() {
        let (mut client, mut server) = tokio::io::duplex(1024);

        let server_task = tokio::spawn(async move { socks5_handshake(&mut server).await });

        // Greeting: VER=5, NMETHODS=1, METHODS=[NOAUTH]
        client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
        client.flush().await.unwrap();

        let mut method_select = [0u8; 2];
        client.read_exact(&mut method_select).await.unwrap();
        assert_eq!(method_select, [0x05, 0x00]);

        // Request: CONNECT to example.com:80
        let host = b"example.com";
        let mut req = Vec::new();
        req.extend_from_slice(&[0x05, 0x01, 0x00, 0x03, host.len() as u8]);
        req.extend_from_slice(host);
        req.extend_from_slice(&80u16.to_be_bytes());
        client.write_all(&req).await.unwrap();
        client.flush().await.unwrap();

        let mut reply = [0u8; 10];
        client.read_exact(&mut reply).await.unwrap();
        assert_eq!(&reply[..4], &[0x05, 0x00, 0x00, 0x01]);

        let (dst_host, dst_port) = server_task.await.unwrap().unwrap();
        assert_eq!(dst_host, "example.com");
        assert_eq!(dst_port, 80);
    }
}
