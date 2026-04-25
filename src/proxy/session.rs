//! Portal Proxy PTY session management.
//!
//! The proxy transport is intentionally backed by the local OpenSSH client for
//! the prototype. OpenSSH handles proxy authentication, Tailscale-only routing,
//! PTY allocation, and SSH agent forwarding; Portal handles terminal rendering.

use chrono::{DateTime, Utc};
use data_encoding::BASE64;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};
use serde::Deserialize;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::Stdio;
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::{Duration, timeout};
use uuid::Uuid;

use crate::config::Host;
use crate::config::paths;
use crate::config::settings::PortalProxySettings;
use crate::error::LocalError;

const MIN_SUPPORTED_PROXY_API_VERSION: u16 = 1;

#[derive(Debug)]
pub enum ProxyEvent {
    Data(Vec<u8>),
    Disconnected { clean: bool },
}

#[derive(Debug, Clone)]
pub struct ProxySessionTarget {
    pub session_id: Uuid,
    pub target_host: String,
    pub target_port: u16,
    pub target_user: String,
}

#[derive(Debug, Clone)]
pub struct ListedProxySession {
    pub session_id: Uuid,
    pub target_host: String,
    pub target_port: u16,
    pub target_user: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_output_at: Option<DateTime<Utc>>,
    pub preview: Vec<u8>,
    pub preview_truncated: bool,
}

#[derive(Debug, Clone)]
pub struct ProxyStatus {
    pub version: String,
    pub api_version: u16,
    pub metadata_schema_version: u16,
}

#[derive(Debug, Deserialize)]
struct RawProxyVersion {
    version: String,
    api_version: u16,
    metadata_schema_version: u16,
}

#[derive(Debug, Deserialize)]
struct RawListedProxySession {
    session_id: Uuid,
    target_host: String,
    target_port: u16,
    target_user: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    active: bool,
    last_output_at: Option<DateTime<Utc>>,
    preview_base64: Option<String>,
    #[serde(default)]
    preview_truncated: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawListResponse {
    Legacy(Vec<RawListedProxySession>),
    V1 {
        api_version: u16,
        sessions: Vec<RawListedProxySession>,
    },
}

enum ProxyCommand {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

#[derive(Debug)]
pub struct ProxySession {
    command_tx: mpsc::Sender<ProxyCommand>,
    child_killer: Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
}

impl ProxySession {
    pub fn spawn(
        settings: &PortalProxySettings,
        host: &Host,
        session_id: Uuid,
        cols: u16,
        rows: u16,
        event_tx: mpsc::Sender<ProxyEvent>,
    ) -> Result<Self, LocalError> {
        let target = ProxySessionTarget {
            session_id,
            target_host: host.hostname.clone(),
            target_port: host.port,
            target_user: host.effective_username(),
        };
        Self::spawn_target(settings, &target, cols, rows, event_tx)
    }

    pub fn spawn_target(
        settings: &PortalProxySettings,
        target: &ProxySessionTarget,
        cols: u16,
        rows: u16,
        event_tx: mpsc::Sender<ProxyEvent>,
    ) -> Result<Self, LocalError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| LocalError::PtyCreation(e.to_string()))?;

        let mut argv = proxy_ssh_argv(settings, true)?;
        argv.push(OsString::from("portal-proxy"));
        argv.push(OsString::from("attach"));
        argv.push(OsString::from("--session-id"));
        argv.push(OsString::from(target.session_id.to_string()));
        argv.push(OsString::from("--target-host"));
        argv.push(OsString::from(target.target_host.clone()));
        argv.push(OsString::from("--target-port"));
        argv.push(OsString::from(target.target_port.to_string()));
        argv.push(OsString::from("--target-user"));
        argv.push(OsString::from(target.target_user.clone()));
        argv.push(OsString::from("--cols"));
        argv.push(OsString::from(cols.to_string()));
        argv.push(OsString::from("--rows"));
        argv.push(OsString::from(rows.to_string()));

        let mut cmd = CommandBuilder::from_argv(argv);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "Portal");

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| LocalError::SpawnFailed(e.to_string()))?;
        let child_killer = child.clone_killer();

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| LocalError::Io(e.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| LocalError::Io(e.to_string()))?;
        let master = pair.master;

        let (command_tx, command_rx) = mpsc::channel::<ProxyCommand>(256);
        Self::spawn_io_task(reader, writer, master, command_rx, event_tx.clone());
        std::thread::spawn(move || match child.wait() {
            Ok(status) => {
                let _ = event_tx.blocking_send(ProxyEvent::Disconnected {
                    clean: status.success(),
                });
            }
            Err(error) => {
                tracing::error!("Portal Proxy ssh wait error: {}", error);
                let _ = event_tx.blocking_send(ProxyEvent::Disconnected { clean: false });
            }
        });

        Ok(Self {
            command_tx,
            child_killer: Some(child_killer),
        })
    }

    fn spawn_io_task(
        mut reader: Box<dyn Read + Send>,
        mut writer: Box<dyn Write + Send>,
        master: Box<dyn portable_pty::MasterPty + Send>,
        mut command_rx: mpsc::Receiver<ProxyCommand>,
        event_tx: mpsc::Sender<ProxyEvent>,
    ) {
        let event_tx_reader = event_tx.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        break;
                    }
                    Ok(n) => {
                        let _ = event_tx_reader.blocking_send(ProxyEvent::Data(buf[..n].to_vec()));
                    }
                    Err(error) => {
                        tracing::error!("Portal Proxy PTY read error: {}", error);
                        let _ = event_tx_reader
                            .blocking_send(ProxyEvent::Disconnected { clean: false });
                        break;
                    }
                }
            }
        });

        std::thread::spawn(move || {
            let _master = master;
            while let Some(cmd) = command_rx.blocking_recv() {
                match cmd {
                    ProxyCommand::Data(data) => {
                        if let Err(error) = writer.write_all(&data) {
                            tracing::error!("Portal Proxy PTY write error: {}", error);
                            break;
                        }
                        let _ = writer.flush();
                    }
                    ProxyCommand::Resize { cols, rows } => {
                        if let Err(error) = _master.resize(PtySize {
                            rows,
                            cols,
                            pixel_width: 0,
                            pixel_height: 0,
                        }) {
                            tracing::error!("Portal Proxy PTY resize error: {}", error);
                        }
                    }
                }
            }

            let _ = event_tx.blocking_send(ProxyEvent::Disconnected { clean: false });
        });
    }

    pub async fn send(&self, data: &[u8]) -> Result<(), LocalError> {
        self.command_tx
            .send(ProxyCommand::Data(data.to_vec()))
            .await
            .map_err(|e| LocalError::Io(e.to_string()))
    }

    pub async fn resize(&self, cols: u16, rows: u16) -> Result<(), LocalError> {
        self.command_tx
            .send(ProxyCommand::Resize { cols, rows })
            .await
            .map_err(|e| LocalError::Io(e.to_string()))
    }
}

pub async fn list_active_sessions(
    settings: &PortalProxySettings,
) -> Result<Vec<ListedProxySession>, String> {
    let mut argv = proxy_list_argv(settings, true)?;
    let output = run_proxy_list_command(&argv).await?;
    let raw = match output {
        ProxyListOutput::Success(stdout) => parse_list_response(&stdout)?,
        ProxyListOutput::Failure(stderr) if should_fallback_to_legacy_list(&stderr) => {
            argv = proxy_list_argv(settings, false)?;
            match run_proxy_list_command(&argv).await? {
                ProxyListOutput::Success(stdout) => parse_list_response(&stdout)?,
                ProxyListOutput::Failure(stderr) => {
                    return Err(format!(
                        "Portal Proxy list failed: {}",
                        stderr.trim().if_empty("unknown error")
                    ));
                }
            }
        }
        ProxyListOutput::Failure(stderr) => {
            return Err(format!(
                "Portal Proxy list failed: {}",
                stderr.trim().if_empty("unknown error")
            ));
        }
    };

    raw_sessions_to_listed(raw)
}

pub async fn check_proxy_status(settings: &PortalProxySettings) -> Result<ProxyStatus, String> {
    let mut argv = proxy_ssh_argv(settings, false).map_err(|error| error.to_string())?;
    argv.push(OsString::from("portal-proxy"));
    argv.push(OsString::from("version"));
    argv.push(OsString::from("--json"));

    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = timeout(Duration::from_secs(10), command.output())
        .await
        .map_err(|_| "Portal Proxy version check timed out".to_string())?
        .map_err(|error| format!("failed to run Portal Proxy version check: {}", error))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Portal Proxy version check failed: {}",
            stderr.trim().if_empty("unknown error")
        ));
    }

    let raw: RawProxyVersion = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("failed to parse Portal Proxy version: {}", error))?;
    if raw.api_version < MIN_SUPPORTED_PROXY_API_VERSION {
        return Err(format!(
            "Portal Proxy API version {} is too old; Portal requires {}",
            raw.api_version, MIN_SUPPORTED_PROXY_API_VERSION
        ));
    }

    Ok(ProxyStatus {
        version: raw.version,
        api_version: raw.api_version,
        metadata_schema_version: raw.metadata_schema_version,
    })
}

fn proxy_list_argv(
    settings: &PortalProxySettings,
    versioned: bool,
) -> Result<Vec<OsString>, String> {
    let mut argv = proxy_ssh_argv(settings, false).map_err(|error| error.to_string())?;
    argv.push(OsString::from("portal-proxy"));
    argv.push(OsString::from("list"));
    argv.push(OsString::from("--active"));
    argv.push(OsString::from("--include-preview"));
    argv.push(OsString::from("--preview-bytes"));
    argv.push(OsString::from("524288"));
    if versioned {
        argv.push(OsString::from("--format"));
        argv.push(OsString::from("v1"));
    }
    Ok(argv)
}

enum ProxyListOutput {
    Success(Vec<u8>),
    Failure(String),
}

async fn run_proxy_list_command(argv: &[OsString]) -> Result<ProxyListOutput, String> {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let output = timeout(Duration::from_secs(10), command.output())
        .await
        .map_err(|_| "Portal Proxy list timed out".to_string())?
        .map_err(|error| format!("failed to run Portal Proxy list: {}", error))?;

    if !output.status.success() {
        return Ok(ProxyListOutput::Failure(
            String::from_utf8_lossy(&output.stderr).to_string(),
        ));
    }

    Ok(ProxyListOutput::Success(output.stdout))
}

fn parse_list_response(bytes: &[u8]) -> Result<Vec<RawListedProxySession>, String> {
    let raw: RawListResponse = serde_json::from_slice(bytes)
        .map_err(|error| format!("failed to parse Portal Proxy sessions: {}", error))?;
    let raw = match raw {
        RawListResponse::Legacy(sessions) => sessions,
        RawListResponse::V1 {
            api_version,
            sessions,
        } => {
            if api_version != 1 {
                return Err(format!(
                    "unsupported Portal Proxy list API version: {}",
                    api_version
                ));
            }
            sessions
        }
    };

    Ok(raw)
}

fn should_fallback_to_legacy_list(stderr: &str) -> bool {
    stderr.contains("--format")
        && (stderr.contains("unexpected")
            || stderr.contains("wasn't expected")
            || stderr.contains("unrecognized"))
}

fn raw_sessions_to_listed(
    raw: Vec<RawListedProxySession>,
) -> Result<Vec<ListedProxySession>, String> {
    raw.into_iter()
        .filter(|session| session.active)
        .map(|session| {
            let preview = match session.preview_base64 {
                Some(encoded) => BASE64
                    .decode(encoded.as_bytes())
                    .map_err(|error| format!("failed to decode session preview: {}", error))?,
                None => Vec::new(),
            };

            Ok(ListedProxySession {
                session_id: session.session_id,
                target_host: session.target_host,
                target_port: session.target_port,
                target_user: session.target_user,
                created_at: session.created_at,
                updated_at: session.updated_at,
                last_output_at: session.last_output_at,
                preview,
                preview_truncated: session.preview_truncated,
            })
        })
        .collect()
}

fn proxy_ssh_argv(
    settings: &PortalProxySettings,
    allocate_tty: bool,
) -> Result<Vec<OsString>, LocalError> {
    let proxy_host = settings.host.trim();
    if proxy_host.is_empty() {
        return Err(LocalError::SpawnFailed(
            "Portal Proxy host is not configured".to_string(),
        ));
    }

    let mut argv = vec![OsString::from("ssh")];
    argv.push(OsString::from("-F"));
    argv.push(OsString::from("/dev/null"));
    if allocate_tty {
        argv.push(OsString::from("-tt"));
    }
    argv.push(OsString::from("-A"));
    argv.push(OsString::from("-o"));
    argv.push(OsString::from("StrictHostKeyChecking=accept-new"));
    if let Some(config_dir) = paths::config_dir() {
        argv.push(OsString::from("-o"));
        argv.push(OsString::from(format!(
            "UserKnownHostsFile={}",
            config_dir.join("portal_proxy_known_hosts").display()
        )));
    }
    argv.push(OsString::from("-p"));
    argv.push(OsString::from(settings.port.to_string()));
    argv.push(OsString::from("-l"));
    argv.push(OsString::from(settings.username.clone()));
    if let Some(identity) = settings
        .identity_file
        .as_ref()
        .filter(|path| !path.as_os_str().is_empty())
    {
        argv.push(OsString::from("-i"));
        argv.push(identity.as_os_str().to_os_string());
    }
    argv.push(OsString::from(proxy_host));
    Ok(argv)
}

trait EmptyFallback {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str;
}

impl EmptyFallback for str {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.is_empty() { fallback } else { self }
    }
}

impl Drop for ProxySession {
    fn drop(&mut self) {
        tracing::debug!("Portal Proxy session cleanup: detaching local ssh process");
        let (replacement_tx, _replacement_rx) = mpsc::channel(1);
        let _ = std::mem::replace(&mut self.command_tx, replacement_tx);
        if let Some(mut killer) = self.child_killer.take() {
            if let Err(error) = killer.kill() {
                tracing::debug!("Failed to kill Portal Proxy ssh process: {}", error);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_sessions_to_listed_decodes_preview_and_filters_inactive() {
        let now = Utc::now();
        let raw = vec![
            RawListedProxySession {
                session_id: Uuid::new_v4(),
                target_host: "example.com".to_string(),
                target_port: 22,
                target_user: "john".to_string(),
                created_at: now,
                updated_at: now,
                active: true,
                last_output_at: Some(now),
                preview_base64: Some(BASE64.encode(b"screen")),
                preview_truncated: true,
            },
            RawListedProxySession {
                session_id: Uuid::new_v4(),
                target_host: "old.example.com".to_string(),
                target_port: 22,
                target_user: "john".to_string(),
                created_at: now,
                updated_at: now,
                active: false,
                last_output_at: None,
                preview_base64: None,
                preview_truncated: false,
            },
        ];

        let sessions = raw_sessions_to_listed(raw).unwrap();

        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].preview, b"screen");
        assert!(sessions[0].preview_truncated);
    }

    #[test]
    fn parse_list_response_accepts_versioned_v1() {
        let session_id = Uuid::new_v4();
        let body = format!(
            r#"{{
                "api_version": 1,
                "generated_at": "2026-04-25T00:00:00Z",
                "sessions": [{{
                    "session_id": "{}",
                    "session_name": "portal-{}",
                    "target_host": "example.com",
                    "target_port": 22,
                    "target_user": "john",
                    "created_at": "2026-04-25T00:00:00Z",
                    "updated_at": "2026-04-25T00:00:00Z",
                    "ended_at": null,
                    "active": true
                }}]
            }}"#,
            session_id, session_id
        );

        let raw = parse_list_response(body.as_bytes()).unwrap();

        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].session_id, session_id);
    }

    #[test]
    fn parse_list_response_rejects_unknown_version() {
        let body =
            br#"{"api_version": 99, "generated_at": "2026-04-25T00:00:00Z", "sessions": []}"#;

        let error = parse_list_response(body).unwrap_err();

        assert!(error.contains("unsupported Portal Proxy list API version"));
    }

    #[test]
    fn legacy_list_fallback_only_handles_format_rejection() {
        assert!(should_fallback_to_legacy_list(
            "error: unexpected argument '--format' found"
        ));
        assert!(!should_fallback_to_legacy_list(
            "ssh: connect to host failed"
        ));
    }

    #[test]
    fn raw_proxy_version_deserializes() {
        let raw: RawProxyVersion = serde_json::from_slice(
            br#"{"version":"0.2.0","api_version":1,"metadata_schema_version":1,"min_portal_api_version":1}"#,
        )
        .unwrap();

        assert_eq!(raw.version, "0.2.0");
        assert_eq!(raw.api_version, 1);
        assert_eq!(raw.metadata_schema_version, 1);
    }
}
