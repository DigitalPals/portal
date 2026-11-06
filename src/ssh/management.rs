//! Direct, non-pooled management SSH. Every hop uses normal host-key verification
//! and credential dialogs; Hub routing, vault enrollment and agent forwarding
//! are deliberately outside this transport's dependency graph.
use super::{
    SshConnection, SshEvent,
    auth_flow::{self, AuthContext},
    handler::ClientHandler,
    known_hosts::KnownHostsManager,
    tunnel::TunnelStream,
};
use crate::{config::Host, error::SshError};
use russh::{Channel, ChannelMsg, client};
use secrecy::{ExposeSecret, SecretString};
use std::{
    collections::HashMap,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tokio::{
    net::TcpStream,
    sync::{Mutex, mpsc},
};

pub struct ManagementConnection {
    connections: Vec<Arc<SshConnection>>,
}

impl ManagementConnection {
    pub async fn connect(
        host: &Host,
        chain: &[Host],
        known_hosts: Arc<Mutex<KnownHostsManager>>,
        events: &mpsc::Sender<SshEvent>,
    ) -> Result<Self, String> {
        let mut this = Self {
            connections: Vec::new(),
        };
        let config = Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(900)),
            keepalive_interval: Some(Duration::from_secs(30)),
            keepalive_max: 3,
            ..Default::default()
        });
        for hop in chain.iter().chain(std::iter::once(host)) {
            let result = async {
                let stream = if let Some(parent) = this.connections.last() {
                    let handle = parent.handle();
                    let handle = handle.lock().await;
                    TunnelStream::Channel(
                        handle
                            .channel_open_direct_tcpip(
                                &hop.hostname,
                                hop.port as u32,
                                "127.0.0.1",
                                0,
                            )
                            .await?
                            .into_stream(),
                    )
                } else {
                    let socket = tokio::time::timeout(
                        Duration::from_secs(30),
                        TcpStream::connect((hop.hostname.as_str(), hop.port)),
                    )
                    .await
                    .map_err(|_| SshError::Timeout(hop.hostname.clone()))?
                    .map_err(|_| SshError::Channel("Could not reach the SSH server".into()))?;
                    TunnelStream::Tcp(socket)
                };
                let forwarding = Arc::new(AtomicBool::new(false));
                let forwards = Arc::new(Mutex::new(HashMap::new()));
                let handler = ClientHandler::new(
                    hop.hostname.clone(),
                    hop.port,
                    known_hosts.clone(),
                    events.clone(),
                    forwarding.clone(),
                    forwards.clone(),
                );
                let mut handle = client::connect_stream(config.clone(), stream, handler).await?;
                let auth = super::auth_prompt::resolve_interactive(hop, events).await?;
                let username = hop.effective_username();
                auth_flow::authenticate(
                    &mut handle,
                    AuthContext {
                        hostname: &hop.hostname,
                        port: hop.port,
                        username: &username,
                        event_tx: events,
                    },
                    auth,
                )
                .await?;
                Ok::<_, SshError>(SshConnection::new_via(
                    handle,
                    forwards,
                    forwarding,
                    Arc::from(hop.hostname.clone()),
                    hop.port,
                    this.connections.last().cloned(),
                ))
            }
            .await;
            match result {
                Ok(connection) => this.connections.push(connection),
                Err(_) => {
                    this.disconnect().await;
                    // SSH server errors may contain arbitrary echoed authentication data.
                    return Err(format!(
                        "SSH connection to {} failed. Check its address, host key and credentials, then retry.",
                        hop.name
                    ));
                }
            }
        }
        Ok(this)
    }

    pub async fn channel(&self) -> Result<Channel<client::Msg>, String> {
        let connection = self.connections.last().ok_or("SSH connection is closed")?;
        connection
            .handle()
            .lock()
            .await
            .channel_open_session()
            .await
            .map_err(|_| "Could not open an SSH command channel".into())
    }

    pub async fn disconnect(&self) {
        for connection in self.connections.iter().rev() {
            let _ = connection.disconnect().await;
        }
    }

    /// No PTY: stdout/stderr cannot echo terminal input. Output is bounded and
    /// errors never include remote output or stdin. Commands are fixed or built
    /// solely from our generated UUID paths, never user values or credentials.
    pub async fn execute(&self, command: &str, input: &[u8]) -> Result<CommandOutput, String> {
        let mut channel = self.channel().await?;
        channel
            .exec(true, command)
            .await
            .map_err(|_| "Could not start the remote command")?;
        channel
            .data(input)
            .await
            .map_err(|_| "Could not send remote command input")?;
        channel
            .eof()
            .await
            .map_err(|_| "Could not finish remote command input")?;
        read_output(&mut channel).await
    }

    pub async fn execute_sudo(
        &self,
        command: &str,
        input: &[u8],
        host: &Host,
        events: &mpsc::Sender<SshEvent>,
    ) -> Result<CommandOutput, String> {
        let mut channel = self.channel().await?;
        // The wrapper announces successful elevation before reading the helper's
        // stdin. It distinguishes NOPASSWD/cached sudo from a real prompt.
        // command contains only a fixed isolated-Python loader, no secrets.
        let command = format!(
            "sudo -S -p 'PORTAL_SUDO_PASSWORD:' -- /bin/sh -c {}",
            shell_quote(&format!("printf PORTAL_SUDO_READY: >&2; exec {command}"))
        );
        channel
            .exec(true, command)
            .await
            .map_err(|_| "Could not start sudo")?;
        let mut handshake = SudoHandshake::default();
        let mut password: Option<SecretString> = None;
        loop {
            match channel.wait().await {
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    match handshake.feed(&data)? {
                        SudoStep::Ready => break,
                        SudoStep::Wait => continue,
                        SudoStep::Password => {
                            let value = super::auth_prompt::request_secret(host, events, "Server administrator access", "Sudo password").await.map_err(|_| "Administrator password request cancelled or timed out")?;
                            validate_sudo_password(&value)?;
                            channel.data(value.expose_secret().as_bytes()).await.map_err(|_| "Could not send sudo password")?;
                            channel.data(&b"\n"[..]).await.map_err(|_| "Could not finish sudo password")?;
                            password = Some(value);
                        }
                    }
                }
                Some(ChannelMsg::ExitStatus { .. } | ChannelMsg::Close) | None => return Err("Sudo needs permission to run the installer without a terminal. Check administrator access on this server.".into()),
                _ => {}
            }
        }
        channel
            .data(input)
            .await
            .map_err(|_| "Could not send deployment request")?;
        channel
            .eof()
            .await
            .map_err(|_| "Could not finish deployment request")?;
        let mut output = read_output(&mut channel).await?;
        if let Some(password) = password {
            output.stdout = output
                .stdout
                .replace(password.expose_secret(), "[redacted]");
        }
        Ok(output)
    }
}

pub(crate) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[derive(Default)]
struct SudoHandshake {
    bytes: Vec<u8>,
    prompted: bool,
}
#[derive(Debug, PartialEq, Eq)]
enum SudoStep {
    Wait,
    Password,
    Ready,
}
impl SudoHandshake {
    fn feed(&mut self, data: &[u8]) -> Result<SudoStep, String> {
        if self.bytes.len() + data.len() > 4096 {
            return Err("Sudo did not accept administrator access".into());
        }
        self.bytes.extend_from_slice(data);
        if self
            .bytes
            .windows(SUDO_READY.len())
            .any(|w| w == SUDO_READY)
        {
            return Ok(SudoStep::Ready);
        }
        if self
            .bytes
            .windows(SUDO_PROMPT.len())
            .any(|w| w == SUDO_PROMPT)
        {
            if self.prompted {
                return Err(
                    "Sudo rejected the password. Retry with the server administrator password."
                        .into(),
                );
            }
            self.prompted = true;
            self.bytes.clear();
            return Ok(SudoStep::Password);
        }
        Ok(SudoStep::Wait)
    }
}

const SUDO_READY: &[u8] = b"PORTAL_SUDO_READY:";
const SUDO_PROMPT: &[u8] = b"PORTAL_SUDO_PASSWORD:";
const MAX_OUTPUT: usize = 128 * 1024;
pub struct CommandOutput {
    pub status: u32,
    pub stdout: String,
}

fn validate_sudo_password(password: &SecretString) -> Result<(), String> {
    let password = password.expose_secret();
    if password.is_empty() || password.contains(['\n', '\r', '\0']) {
        return Err("Sudo requires a nonempty password without line breaks".into());
    }
    Ok(())
}

async fn read_output(channel: &mut Channel<client::Msg>) -> Result<CommandOutput, String> {
    let mut stdout = Vec::new();
    let mut count = 0;
    let mut status = None;
    while let Some(message) = channel.wait().await {
        match message {
            ChannelMsg::Data { data } => {
                count += data.len();
                if count > MAX_OUTPUT {
                    return Err("Remote output exceeded the diagnostic limit".into());
                }
                stdout.extend_from_slice(&data);
            }
            ChannelMsg::ExtendedData { data, .. } => {
                count += data.len();
                if count > MAX_OUTPUT {
                    return Err("Remote output exceeded the diagnostic limit".into());
                }
            }
            ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
            ChannelMsg::Close => break,
            _ => {}
        }
    }
    Ok(CommandOutput {
        status: status
            .ok_or("SSH closed before the command finished. Check status before retrying.")?,
        stdout: String::from_utf8(stdout).map_err(|_| "Invalid remote response encoding")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sudo_input_rejects_line_injection_without_echoing_secrets() {
        for value in ["", "secret\ncommand", "secret\r", "secret\0"] {
            let error = validate_sudo_password(&SecretString::from(value)).unwrap_err();
            assert!(!error.contains("secret"));
        }
        assert!(validate_sudo_password(&SecretString::from("safe '$ password")).is_ok());
    }
    #[test]
    fn passwordless_sudo_sends_only_the_request_after_ready() {
        let mut handshake = SudoHandshake::default();
        assert_eq!(handshake.feed(b"PORTAL_SUDO_").unwrap(), SudoStep::Wait);
        assert_eq!(handshake.feed(b"READY:").unwrap(), SudoStep::Ready);
        assert!(!handshake.prompted);
    }
    #[test]
    fn prompted_sudo_waits_for_confirmation_before_request_input() {
        let mut handshake = SudoHandshake::default();
        assert_eq!(
            handshake.feed(b"lecture\nPORTAL_SUDO_PASS").unwrap(),
            SudoStep::Wait
        );
        assert_eq!(handshake.feed(b"WORD:").unwrap(), SudoStep::Password);
        assert_eq!(
            handshake.feed(b"PORTAL_SUDO_READY:").unwrap(),
            SudoStep::Ready
        );
        let mut rejected = SudoHandshake::default();
        assert_eq!(rejected.feed(SUDO_PROMPT).unwrap(), SudoStep::Password);
        assert!(rejected.feed(SUDO_PROMPT).is_err());
    }
}
