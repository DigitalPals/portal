//! Portal Hub PTY session management.
//!
//! Portal connects to Portal Hub through OAuth-authenticated web APIs. Interactive
//! proxy sessions use a WebSocket terminal stream instead of a separate SSH login
//! to the Hub.

use chrono::{DateTime, Utc};
use data_encoding::BASE64;
use futures::{Sink, SinkExt, StreamExt};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::mpsc as std_mpsc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use uuid::Uuid;

use crate::config::settings::PortalHubSettings;
use crate::config::{AuthMethod, DetectedOs, Host};
use crate::error::LocalError;
use crate::hub::http;
use crate::ssh::host_key_verification::{
    HostKeyInfo, HostKeyVerificationRequest, HostKeyVerificationResponse,
};

const MIN_SUPPORTED_WEB_PROXY_API_VERSION: u16 = 2;
// Keep the dashboard cheap: previews are only thumbnails. Full-ish replay is
// requested when actually resuming a session.
const SESSION_LIST_PREVIEW_BYTES: u64 = 16 * 1024;
const SESSION_RESUME_REPLAY_BYTES: u64 = 64 * 1024;
const HUB_UPLOAD_RESPONSE_TIMEOUT: Duration = Duration::from_secs(45);

#[derive(Debug)]
pub enum ProxyEvent {
    Data(Vec<u8>),
    Disconnected { clean: bool },
    HostKeyVerification(Box<HostKeyVerificationRequest>),
    OsDetected(DetectedOs),
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
    pub display_name: Option<String>,
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
    pub public_url: String,
    pub ssh_port: Option<u16>,
    pub ssh_username: Option<String>,
    pub sync_v2: bool,
    pub sync_events: bool,
    pub web_proxy: bool,
    pub session_titles: bool,
    pub key_vault: bool,
    pub vault_enrollment: bool,
}

#[derive(Debug, Deserialize)]
struct RawProxyVersion {
    version: String,
    api_version: u16,
    #[serde(default)]
    public_url: String,
    #[serde(default)]
    metadata_schema_version: u16,
    #[serde(default)]
    ssh_port: Option<u16>,
    #[serde(default)]
    ssh_username: Option<String>,
    #[serde(default)]
    capabilities: ProxyCapabilities,
}

#[derive(Debug, Default, Deserialize)]
struct ProxyCapabilities {
    #[serde(default)]
    web_proxy: bool,
    #[serde(default)]
    session_titles: bool,
    #[serde(default)]
    sync_v2: bool,
    #[serde(default)]
    sync_events: bool,
    #[serde(default)]
    key_vault: bool,
    #[serde(default)]
    vault_enrollment: bool,
}

#[derive(Debug, Deserialize)]
struct RawListedProxySession {
    session_id: Uuid,
    #[serde(default)]
    display_name: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubSyncPutRequest {
    pub profile: Value,
    pub vault: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HubSyncResponse {
    pub api_version: u16,
    pub revision: String,
    pub profile: Value,
    pub vault: Value,
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
    Resize {
        cols: u16,
        rows: u16,
    },
    UploadFile {
        filename: String,
        contents: Vec<u8>,
        response_tx: oneshot::Sender<Result<String, String>>,
    },
}

#[derive(Debug, Serialize)]
struct WebTerminalStart {
    session_id: Uuid,
    target_host: String,
    target_port: u16,
    target_user: String,
    cols: u16,
    rows: u16,
    replay_bytes: u64,
    detect_os: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    private_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WebTerminalControl {
    Resize {
        cols: u16,
        rows: u16,
    },
    HostKeyResponse {
        accepted: bool,
    },
    UploadFile {
        request_id: Uuid,
        filename: String,
        contents_base64: String,
    },
}

#[derive(Debug, Deserialize)]
struct WebTerminalUploadFileResult {
    request_id: Uuid,
    path: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WebTerminalHostKeyVerification {
    host: String,
    port: u16,
    fingerprint: String,
    key_type: String,
    #[serde(default)]
    old_fingerprint: Option<String>,
}

#[derive(Debug)]
pub struct ProxySession {
    command_tx: mpsc::Sender<ProxyCommand>,
    child_killer: Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
}

struct WebTerminalSpawnOptions {
    cols: u16,
    rows: u16,
    detect_os: bool,
    private_key: Option<String>,
    replay_bytes: u64,
}

impl ProxySession {
    pub fn spawn(
        settings: &PortalHubSettings,
        host: &Host,
        session_id: Uuid,
        cols: u16,
        rows: u16,
        detect_os: bool,
        event_tx: mpsc::Sender<ProxyEvent>,
    ) -> Result<Self, LocalError> {
        let target = ProxySessionTarget {
            session_id,
            target_host: host.hostname.clone(),
            target_port: host.port,
            target_user: host.effective_username(),
        };
        let private_key = proxy_private_key(&host.auth)?;
        Self::spawn_web_target(
            settings,
            &target,
            event_tx,
            WebTerminalSpawnOptions {
                cols,
                rows,
                detect_os,
                private_key,
                replay_bytes: 0,
            },
        )
    }

    pub fn spawn_target(
        settings: &PortalHubSettings,
        target: &ProxySessionTarget,
        cols: u16,
        rows: u16,
        detect_os: bool,
        event_tx: mpsc::Sender<ProxyEvent>,
    ) -> Result<Self, LocalError> {
        Self::spawn_web_target(
            settings,
            target,
            event_tx,
            WebTerminalSpawnOptions {
                cols,
                rows,
                detect_os,
                private_key: None,
                replay_bytes: SESSION_RESUME_REPLAY_BYTES,
            },
        )
    }

    fn spawn_web_target(
        settings: &PortalHubSettings,
        target: &ProxySessionTarget,
        event_tx: mpsc::Sender<ProxyEvent>,
        options: WebTerminalSpawnOptions,
    ) -> Result<Self, LocalError> {
        let WebTerminalSpawnOptions {
            cols,
            rows,
            detect_os,
            private_key,
            replay_bytes,
        } = options;
        let (command_tx, command_rx) = mpsc::channel::<ProxyCommand>(256);
        let (ready_tx, ready_rx) = std_mpsc::sync_channel::<Result<(), String>>(1);
        let settings = settings.clone();
        let target = target.clone();
        std::thread::spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = event_tx.blocking_send(ProxyEvent::Data(
                        format!("Failed to start Portal Hub web transport: {}\r\n", error)
                            .into_bytes(),
                    ));
                    let _ = event_tx.blocking_send(ProxyEvent::Disconnected { clean: false });
                    let _ = ready_tx.send(Err(error.to_string()));
                    return;
                }
            };
            runtime.block_on(run_web_terminal(
                settings,
                target,
                cols,
                rows,
                detect_os,
                private_key,
                replay_bytes,
                command_rx,
                event_tx,
                Some(ready_tx),
            ));
        });

        match ready_rx.recv_timeout(Duration::from_secs(75)) {
            Ok(Ok(())) => {}
            Ok(Err(error)) => return Err(LocalError::SpawnFailed(error)),
            Err(std_mpsc::RecvTimeoutError::Timeout) => {
                return Err(LocalError::SpawnFailed(
                    "timed out waiting for Portal Hub terminal to start".to_string(),
                ));
            }
            Err(std_mpsc::RecvTimeoutError::Disconnected) => {
                return Err(LocalError::SpawnFailed(
                    "Portal Hub terminal startup ended before it reported readiness".to_string(),
                ));
            }
        }

        Ok(Self {
            command_tx,
            child_killer: None,
        })
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

    pub async fn upload_file(&self, filename: String, contents: Vec<u8>) -> Result<String, String> {
        let (response_tx, response_rx) = oneshot::channel();
        self.command_tx
            .send(ProxyCommand::UploadFile {
                filename,
                contents,
                response_tx,
            })
            .await
            .map_err(|error| format!("Portal Hub terminal is not connected: {error}"))?;

        tokio::time::timeout(HUB_UPLOAD_RESPONSE_TIMEOUT, response_rx)
            .await
            .map_err(|_| "Timed out waiting for Portal Hub upload result".to_string())?
            .map_err(|_| "Portal Hub upload response channel closed".to_string())?
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_web_terminal(
    settings: PortalHubSettings,
    target: ProxySessionTarget,
    cols: u16,
    rows: u16,
    detect_os: bool,
    private_key: Option<String>,
    replay_bytes: u64,
    mut command_rx: mpsc::Receiver<ProxyCommand>,
    event_tx: mpsc::Sender<ProxyEvent>,
    mut ready_tx: Option<std_mpsc::SyncSender<Result<(), String>>>,
) {
    let result = async {
        let hub_url = settings.effective_web_url();
        if hub_url.is_empty() {
            return Err("Portal Hub host and web port are not configured".to_string());
        }
        let token = refreshed_portal_hub_access_token(&hub_url).await?;
        let mut request = terminal_ws_url(&hub_url)?
            .into_client_request()
            .map_err(|error| format!("failed to build Portal Hub terminal request: {}", error))?;
        request.headers_mut().insert(
            tokio_tungstenite::tungstenite::http::header::AUTHORIZATION,
            format!("Bearer {}", token)
                .parse()
                .map_err(|error| format!("invalid Portal Hub authorization header: {}", error))?,
        );

        let (stream, _) = tokio::time::timeout(
            http::WEBSOCKET_CONNECT_TIMEOUT,
            connect_async(request),
        )
            .await
            .map_err(|_| "timed out connecting to Portal Hub terminal".to_string())?
            .map_err(|error| format!("failed to connect to Portal Hub terminal: {}", error))?;
        let (mut write, mut read) = stream.split();
        let mut pending_uploads: std::collections::HashMap<
            Uuid,
            oneshot::Sender<Result<String, String>>,
        > = std::collections::HashMap::new();
        let start = WebTerminalStart {
            session_id: target.session_id,
            target_host: target.target_host,
            target_port: target.target_port,
            target_user: target.target_user,
            cols,
            rows,
            replay_bytes,
            detect_os,
            private_key,
        };
        write
            .send(WsMessage::Text(
                serde_json::to_string(&start)
                    .map_err(|error| format!("failed to serialize terminal request: {}", error))?,
            ))
            .await
            .map_err(|error| format!("failed to start Portal Hub terminal: {}", error))?;

        loop {
            let Some(message) = read.next().await else {
                return Err("Portal Hub terminal closed before it started".to_string());
            };
            match message.map_err(|error| format!("Portal Hub terminal read failed: {}", error))? {
                WsMessage::Text(text) => {
                    if let Ok(value) = serde_json::from_str::<Value>(&text) {
                        match value.get("type").and_then(Value::as_str) {
                            Some("started") => {
                                if let Some(ready_tx) = ready_tx.take() {
                                    let _ = ready_tx.send(Ok(()));
                                }
                                break;
                            }
                            Some("error") => {
                                let message = value
                                    .get("message")
                                    .and_then(Value::as_str)
                                    .unwrap_or(&text)
                                    .to_string();
                                return Err(message);
                            }
                            Some("host_key_verification") => {
                                respond_to_host_key_verification(&text, &event_tx, &mut write)
                                    .await?;
                            }
                            _ => {}
                        }
                    }
                }
                WsMessage::Binary(data) => {
                    if let Some(ready_tx) = ready_tx.take() {
                        let _ = ready_tx.send(Ok(()));
                    }
                    let _ = event_tx.send(ProxyEvent::Data(data)).await;
                    break;
                }
                WsMessage::Close(_) => {
                    return Err("Portal Hub terminal closed before it started".to_string());
                }
                WsMessage::Ping(data) => {
                    let _ = write.send(WsMessage::Pong(data)).await;
                }
                _ => {}
            }
        }

        loop {
            tokio::select! {
                message = read.next() => {
                    let Some(message) = message else {
                        return Ok(());
                    };
                    match message.map_err(|error| format!("Portal Hub terminal read failed: {}", error))? {
                        WsMessage::Binary(data) => {
                            let _ = event_tx.send(ProxyEvent::Data(data)).await;
                        }
                        WsMessage::Text(text) if text.starts_with("{\"type\":\"error\"") => {
                            let _ = event_tx
                                .send(ProxyEvent::Data(format!("{}\r\n", text).into_bytes()))
                                .await;
                        }
                        WsMessage::Text(text) => {
                            if let Ok(value) = serde_json::from_str::<Value>(&text)
                                && let Some(message_type) = value.get("type").and_then(Value::as_str)
                            {
                                match message_type {
                                    "host_key_verification" => {
                                        respond_to_host_key_verification(&text, &event_tx, &mut write)
                                            .await?;
                                    }
                                    "upload_file_result" => {
                                        handle_upload_file_result(
                                            &text,
                                            &mut pending_uploads,
                                        )?;
                                    }
                                    "os_detected" => {
                                        if let Some(os) = detected_os_from_message(&value) {
                                            let _ = event_tx.send(ProxyEvent::OsDetected(os)).await;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        WsMessage::Close(_) => return Ok(()),
                        WsMessage::Ping(data) => {
                            let _ = write.send(WsMessage::Pong(data)).await;
                        }
                        _ => {}
                    }
                }
                command = command_rx.recv() => {
                    let Some(command) = command else {
                        let _ = write.send(WsMessage::Close(None)).await;
                        return Ok(());
                    };
                    match command {
                        ProxyCommand::Data(data) => {
                            write
                                .send(WsMessage::Binary(data))
                                .await
                                .map_err(|error| format!("Portal Hub terminal write failed: {}", error))?;
                        }
                        ProxyCommand::Resize { cols, rows } => {
                            let control = WebTerminalControl::Resize { cols, rows };
                            write
                                .send(WsMessage::Text(
                                    serde_json::to_string(&control).map_err(|error| {
                                        format!("failed to serialize terminal resize: {}", error)
                                    })?,
                                ))
                                .await
                                .map_err(|error| format!("Portal Hub terminal resize failed: {}", error))?;
                        }
                        ProxyCommand::UploadFile {
                            filename,
                            contents,
                            response_tx,
                        } => {
                            let request_id = Uuid::new_v4();
                            let control = WebTerminalControl::UploadFile {
                                request_id,
                                filename,
                                contents_base64: BASE64.encode(&contents),
                            };
                            write
                                .send(WsMessage::Text(
                                    serde_json::to_string(&control).map_err(|error| {
                                        format!("failed to serialize terminal upload: {}", error)
                                    })?,
                                ))
                                .await
                                .map_err(|error| format!("Portal Hub terminal upload failed: {}", error))?;
                            pending_uploads.insert(request_id, response_tx);
                        }
                    }
                }
            }
        }
    }
    .await;

    if let Err(error) = result {
        if let Some(ready_tx) = ready_tx.take() {
            let _ = ready_tx.send(Err(error.clone()));
        }
        let _ = event_tx
            .send(ProxyEvent::Data(format!("{}\r\n", error).into_bytes()))
            .await;
        let _ = event_tx
            .send(ProxyEvent::Disconnected { clean: false })
            .await;
    } else {
        if let Some(ready_tx) = ready_tx.take() {
            let _ = ready_tx.send(Ok(()));
        }
        let _ = event_tx
            .send(ProxyEvent::Disconnected { clean: true })
            .await;
    }
}

fn detected_os_from_message(value: &Value) -> Option<DetectedOs> {
    let uname = value.get("uname")?.as_str()?.trim();
    if uname.is_empty() {
        return None;
    }

    let mut detected_os = DetectedOs::from_uname(uname);
    if detected_os.is_linux()
        && let Some(os_release) = value.get("os_release").and_then(Value::as_str)
        && let Some(distribution) = DetectedOs::from_os_release(os_release)
    {
        detected_os = distribution;
    }
    Some(detected_os)
}

fn handle_upload_file_result(
    text: &str,
    pending_uploads: &mut std::collections::HashMap<Uuid, oneshot::Sender<Result<String, String>>>,
) -> Result<(), String> {
    let result: WebTerminalUploadFileResult = serde_json::from_str(text)
        .map_err(|error| format!("failed to parse Portal Hub upload result: {error}"))?;
    let Some(response_tx) = pending_uploads.remove(&result.request_id) else {
        return Ok(());
    };

    let result = match (result.path, result.error) {
        (Some(path), _) => Ok(path),
        (_, Some(error)) => Err(error),
        (None, None) => Err("Portal Hub upload result did not include a path".to_string()),
    };
    let _ = response_tx.send(result);
    Ok(())
}

async fn respond_to_host_key_verification<S>(
    text: &str,
    event_tx: &mpsc::Sender<ProxyEvent>,
    write: &mut S,
) -> Result<(), String>
where
    S: Sink<WsMessage> + Unpin,
    S::Error: std::fmt::Display,
{
    let challenge: WebTerminalHostKeyVerification =
        serde_json::from_str(text).map_err(|error| {
            format!("failed to parse Portal Hub host key verification request: {error}")
        })?;
    let (response_tx, response_rx) = oneshot::channel();
    let info = HostKeyInfo {
        host: challenge.host,
        port: challenge.port,
        fingerprint: challenge.fingerprint,
        key_type: challenge.key_type,
    };
    let request = match challenge.old_fingerprint {
        Some(old_fingerprint) => HostKeyVerificationRequest::ChangedHost {
            info,
            old_fingerprint,
            responder: response_tx,
        },
        None => HostKeyVerificationRequest::NewHost {
            info,
            responder: response_tx,
        },
    };

    event_tx
        .send(ProxyEvent::HostKeyVerification(Box::new(request)))
        .await
        .map_err(|_| "failed to show host key verification dialog".to_string())?;

    let accepted = match tokio::time::timeout(Duration::from_secs(60), response_rx).await {
        Ok(Ok(HostKeyVerificationResponse::Accept)) => true,
        Ok(Ok(HostKeyVerificationResponse::Reject)) | Ok(Err(_)) => false,
        Err(_) => false,
    };

    let response = WebTerminalControl::HostKeyResponse { accepted };
    write
        .send(WsMessage::Text(serde_json::to_string(&response).map_err(
            |error| format!("failed to serialize host key response: {error}"),
        )?))
        .await
        .map_err(|error| format!("failed to send host key response to Portal Hub: {error}"))?;

    Ok(())
}

fn terminal_ws_url(hub_url: &str) -> Result<String, String> {
    if let Some(rest) = hub_url.strip_prefix("https://") {
        Ok(format!(
            "wss://{}/api/sessions/terminal",
            rest.trim_end_matches('/')
        ))
    } else if let Some(rest) = hub_url.strip_prefix("http://") {
        Ok(format!(
            "ws://{}/api/sessions/terminal",
            rest.trim_end_matches('/')
        ))
    } else {
        Err("Portal Hub web URL must start with http:// or https://".to_string())
    }
}

fn proxy_private_key(auth: &AuthMethod) -> Result<Option<String>, LocalError> {
    match auth {
        AuthMethod::PublicKey {
            key_path,
            vault_key_id: Some(vault_key_id),
            ..
        } => crate::hub::vault::load_decrypted_private_key_or_local_file(
            *vault_key_id,
            key_path.as_deref(),
        )
        .map(|key| Some(key.expose_secret().to_string()))
        .map_err(LocalError::SpawnFailed),
        AuthMethod::PublicKey {
            key_path: Some(key_path),
            ..
        } => crate::hub::vault::read_private_key_file(key_path)
            .map(Some)
            .map_err(|error| LocalError::SpawnFailed(error.to_string())),
        _ => Ok(None),
    }
}

pub async fn list_active_sessions(
    settings: &PortalHubSettings,
) -> Result<Vec<ListedProxySession>, String> {
    let started = Instant::now();
    let hub_url = settings.effective_web_url();
    let url = format!(
        "{}/api/sessions?active=true&include_preview=true&preview_bytes={}",
        hub_url, SESSION_LIST_PREVIEW_BYTES
    );
    let response: RawListResponse = http::authenticated_json(
        &hub_url,
        true,
        |client, token| client.get(&url).bearer_auth(token),
        "failed to list Portal Hub sessions",
        "Portal Hub session list failed",
        "failed to parse Portal Hub sessions",
    )
    .await?;
    let raw = match response {
        RawListResponse::Legacy(sessions) => sessions,
        RawListResponse::V1 {
            api_version,
            sessions,
        } => {
            if api_version < MIN_SUPPORTED_WEB_PROXY_API_VERSION {
                return Err(format!(
                    "Portal Hub session API version {} is too old; Portal requires {}",
                    api_version, MIN_SUPPORTED_WEB_PROXY_API_VERSION
                ));
            }
            sessions
        }
    };

    let sessions = raw_sessions_to_listed(raw)?;
    tracing::debug!(
        sessions = sessions.len(),
        preview_bytes = SESSION_LIST_PREVIEW_BYTES,
        elapsed_ms = started.elapsed().as_millis(),
        "listed Portal Hub sessions"
    );
    Ok(sessions)
}

pub async fn kill_session(settings: &PortalHubSettings, session_id: Uuid) -> Result<(), String> {
    let hub_url = settings.effective_web_url();
    let url = format!("{}/api/sessions/{}", hub_url, session_id);
    http::authenticated_empty(
        &hub_url,
        true,
        |client, token| client.delete(&url).bearer_auth(token),
        "failed to kill Portal Hub session",
        "Portal Hub session kill failed",
    )
    .await
}

#[derive(Debug, Serialize)]
struct SessionUpdateRequest<'a> {
    display_name: Option<&'a str>,
}

pub async fn rename_session(
    settings: &PortalHubSettings,
    session_id: Uuid,
    display_name: Option<&str>,
) -> Result<(), String> {
    let hub_url = settings.effective_web_url();
    let url = format!("{}/api/sessions/{}", hub_url, session_id);
    let request = SessionUpdateRequest { display_name };
    http::authenticated_empty(
        &hub_url,
        true,
        |client, token| client.patch(&url).bearer_auth(token).json(&request),
        "failed to rename Portal Hub session",
        "Portal Hub session rename failed",
    )
    .await
}

pub async fn check_terminal_websocket(settings: &PortalHubSettings) -> Result<(), String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub host and web port are not configured".to_string());
    }
    let token = refreshed_portal_hub_access_token(&hub_url).await?;
    let mut request = terminal_ws_url(&hub_url)?
        .into_client_request()
        .map_err(|error| format!("failed to build Portal Hub terminal request: {}", error))?;
    request.headers_mut().insert(
        tokio_tungstenite::tungstenite::http::header::AUTHORIZATION,
        format!("Bearer {}", token)
            .parse()
            .map_err(|error| format!("invalid Portal Hub authorization header: {}", error))?,
    );

    let (mut stream, _) =
        tokio::time::timeout(http::WEBSOCKET_CONNECT_TIMEOUT, connect_async(request))
            .await
            .map_err(|_| "timed out connecting to Portal Hub terminal".to_string())?
            .map_err(|error| format!("failed to connect to Portal Hub terminal: {}", error))?;
    let _ = stream.close(None).await;
    Ok(())
}

pub async fn check_proxy_status(settings: &PortalHubSettings) -> Result<ProxyStatus, String> {
    let hub_url = settings.effective_web_url();
    let raw: RawProxyVersion = http::json(
        true,
        |client| client.get(format!("{}/api/info", hub_url)),
        "failed to read Portal Hub info",
        "Portal Hub info check failed",
        "failed to parse Portal Hub info",
    )
    .await?;
    if raw.api_version < MIN_SUPPORTED_WEB_PROXY_API_VERSION {
        return Err(format!(
            "Portal Hub API version {} is too old; Portal requires {}",
            raw.api_version, MIN_SUPPORTED_WEB_PROXY_API_VERSION
        ));
    }
    if !raw.capabilities.web_proxy {
        return Err("Portal Hub does not advertise web proxy support".to_string());
    }
    if !raw.capabilities.sync_v2 {
        return Err("Portal Hub does not advertise sync v2 support".to_string());
    }

    Ok(ProxyStatus {
        version: raw.version,
        api_version: raw.api_version,
        metadata_schema_version: raw.metadata_schema_version,
        public_url: raw.public_url,
        ssh_port: raw.ssh_port,
        ssh_username: raw.ssh_username,
        sync_v2: raw.capabilities.sync_v2,
        sync_events: raw.capabilities.sync_events,
        web_proxy: raw.capabilities.web_proxy,
        session_titles: raw.capabilities.session_titles,
        key_vault: raw.capabilities.key_vault,
        vault_enrollment: raw.capabilities.vault_enrollment,
    })
}

fn raw_sessions_to_listed(
    raw: Vec<RawListedProxySession>,
) -> Result<Vec<ListedProxySession>, String> {
    let started = Instant::now();
    let raw_count = raw.len();
    let mut active_count = 0usize;
    let mut preview_count = 0usize;
    let mut preview_bytes = 0usize;
    let mut sessions = Vec::new();

    for session in raw.into_iter().filter(|session| session.active) {
        active_count += 1;
        let preview = match session.preview_base64 {
            Some(encoded) => BASE64
                .decode(encoded.as_bytes())
                .map_err(|error| format!("failed to decode session preview: {}", error))?,
            None => Vec::new(),
        };
        if !preview.is_empty() {
            preview_count += 1;
            preview_bytes += preview.len();
        }

        sessions.push(ListedProxySession {
            session_id: session.session_id,
            display_name: session.display_name,
            target_host: session.target_host,
            target_port: session.target_port,
            target_user: session.target_user,
            created_at: session.created_at,
            updated_at: session.updated_at,
            last_output_at: session.last_output_at,
            preview,
            preview_truncated: session.preview_truncated,
        });
    }

    tracing::debug!(
        raw_sessions = raw_count,
        active_sessions = active_count,
        previews = preview_count,
        preview_bytes,
        elapsed_ms = started.elapsed().as_millis(),
        "decoded Portal Hub session previews"
    );

    Ok(sessions)
}

async fn refreshed_portal_hub_access_token(hub_url: &str) -> Result<String, String> {
    crate::hub::auth::refresh_access_token(hub_url)
        .await?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())
}

impl Drop for ProxySession {
    fn drop(&mut self) {
        tracing::debug!("Portal Hub session cleanup: detaching local ssh process");
        let (replacement_tx, _replacement_rx) = mpsc::channel(1);
        let _ = std::mem::replace(&mut self.command_tx, replacement_tx);
        if let Some(mut killer) = self.child_killer.take()
            && let Err(error) = killer.kill()
        {
            tracing::debug!("Failed to kill Portal Hub ssh process: {}", error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn raw_sessions_to_listed_decodes_preview_and_filters_inactive() {
        let now = Utc::now();
        let raw = vec![
            RawListedProxySession {
                session_id: Uuid::new_v4(),
                display_name: Some("Production deploy".to_string()),
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
                display_name: None,
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
        assert_eq!(
            sessions[0].display_name.as_deref(),
            Some("Production deploy")
        );
        assert_eq!(sessions[0].preview, b"screen");
        assert!(sessions[0].preview_truncated);
    }

    #[test]
    fn raw_proxy_version_deserializes() {
        let instance = json!({
            "api_version": 2,
            "version": "0.5.0",
            "public_url": "https://portal-hub.example.ts.net",
            "capabilities": {
                "sync_v2": true,
                "sync_events": true,
                "web_proxy": true,
                "session_titles": true,
                "key_vault": true,
                "vault_enrollment": true
            },
            "ssh_port": 2222,
            "ssh_username": "portal-hub",
            "metadata_schema_version": 1
        });
        crate::contract_test_support::assert_portal_hub_contract("api-info-response", &instance);
        let raw: RawProxyVersion = serde_json::from_value(instance).unwrap();

        assert_eq!(raw.version, "0.5.0");
        assert_eq!(raw.api_version, 2);
        assert_eq!(raw.metadata_schema_version, 1);
        assert_eq!(raw.public_url, "https://portal-hub.example.ts.net");
        assert_eq!(raw.ssh_port, Some(2222));
        assert_eq!(raw.ssh_username.as_deref(), Some("portal-hub"));
        assert!(raw.capabilities.web_proxy);
        assert!(raw.capabilities.session_titles);
        assert!(raw.capabilities.sync_v2);
        assert!(raw.capabilities.sync_events);
        assert!(raw.capabilities.key_vault);
        assert!(raw.capabilities.vault_enrollment);
    }

    #[test]
    fn proxy_private_key_reads_regular_public_key_auth_file() {
        let dir = tempfile::tempdir().unwrap();
        let key_path = dir.path().join("id_ed25519");
        std::fs::write(&key_path, "-----BEGIN OPENSSH PRIVATE KEY-----\nexample\n").unwrap();

        let private_key = proxy_private_key(&AuthMethod::PublicKey {
            key_path: Some(key_path),
            vault_key_id: None,
        })
        .unwrap();

        assert_eq!(
            private_key.as_deref(),
            Some("-----BEGIN OPENSSH PRIVATE KEY-----\nexample\n")
        );
    }

    #[test]
    fn proxy_private_key_rejects_non_file_public_key_auth_path() {
        let dir = tempfile::tempdir().unwrap();

        let error = proxy_private_key(&AuthMethod::PublicKey {
            key_path: Some(dir.path().to_path_buf()),
            vault_key_id: None,
        })
        .expect_err("directory key path should be rejected");

        match error {
            LocalError::SpawnFailed(message) => {
                assert!(message.contains("not a regular file"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn portal_hub_sessions_response_matches_contract_and_deserializes() {
        let session_id = Uuid::new_v4();
        let instance = json!({
            "api_version": 2,
            "generated_at": "2026-04-29T12:00:00Z",
            "sessions": [{
                "schema_version": 1,
                "session_id": session_id,
                "session_name": format!("portal-{session_id}"),
                "display_name": "Production deploy",
                "target_host": "example.internal",
                "target_port": 22,
                "target_user": "john",
                "created_at": "2026-04-29T11:00:00Z",
                "updated_at": "2026-04-29T11:30:00Z",
                "ended_at": null,
                "active": true,
                "last_output_at": "2026-04-29T11:29:59Z",
                "preview_base64": BASE64.encode(b"screen"),
                "preview_truncated": false
            }]
        });

        crate::contract_test_support::assert_portal_hub_contract("sessions-response", &instance);
        let raw: RawListResponse = serde_json::from_value(instance).unwrap();

        match raw {
            RawListResponse::V1 {
                api_version,
                sessions,
            } => {
                assert_eq!(api_version, 2);
                let listed = raw_sessions_to_listed(sessions).unwrap();
                assert_eq!(listed.len(), 1);
                assert_eq!(listed[0].session_id, session_id);
                assert_eq!(listed[0].display_name.as_deref(), Some("Production deploy"));
                assert_eq!(listed[0].preview, b"screen");
            }
            RawListResponse::Legacy(_) => panic!("expected v1 sessions response"),
        }
    }

    #[test]
    fn portal_hub_session_update_request_matches_contract() {
        let instance = serde_json::to_value(SessionUpdateRequest {
            display_name: Some("Production deploy"),
        })
        .unwrap();

        crate::contract_test_support::assert_portal_hub_contract(
            "session-update-request",
            &instance,
        );
    }

    #[test]
    fn portal_hub_terminal_start_request_matches_contract() {
        let start = WebTerminalStart {
            session_id: Uuid::new_v4(),
            target_host: "example.internal".to_string(),
            target_port: 22,
            target_user: "john".to_string(),
            cols: 120,
            rows: 30,
            replay_bytes: 0,
            detect_os: true,
            private_key: Some("-----BEGIN OPENSSH PRIVATE KEY-----\n...\n".to_string()),
        };
        let instance = serde_json::to_value(start).unwrap();

        crate::contract_test_support::assert_portal_hub_contract(
            "terminal-start-request",
            &instance,
        );
    }

    #[test]
    fn portal_hub_host_key_verification_message_matches_contract_and_deserializes() {
        let instance = json!({
            "type": "host_key_verification",
            "host": "example.internal",
            "port": 22,
            "fingerprint": "SHA256:abc123",
            "key_type": "ssh-ed25519",
            "old_fingerprint": "SHA256:old123"
        });

        crate::contract_test_support::assert_portal_hub_contract(
            "terminal-control-message",
            &instance,
        );
        let request: WebTerminalHostKeyVerification = serde_json::from_value(instance).unwrap();

        assert_eq!(request.host, "example.internal");
        assert_eq!(request.old_fingerprint.as_deref(), Some("SHA256:old123"));
    }

    #[test]
    fn portal_hub_os_detected_message_matches_contract_and_maps_distribution() {
        let instance = json!({
            "type": "os_detected",
            "uname": "Linux",
            "os_release": "NAME=Ubuntu\nID=ubuntu\nID_LIKE=debian\n"
        });

        crate::contract_test_support::assert_portal_hub_contract(
            "terminal-control-message",
            &instance,
        );
        assert_eq!(
            detected_os_from_message(&instance),
            Some(DetectedOs::Ubuntu)
        );
    }

    #[test]
    fn portal_hub_os_detected_message_maps_non_linux_family() {
        let instance = json!({
            "type": "os_detected",
            "uname": "FreeBSD"
        });

        assert_eq!(
            detected_os_from_message(&instance),
            Some(DetectedOs::FreeBSD)
        );
    }

    #[test]
    fn portal_hub_host_key_response_message_matches_contract() {
        let response = WebTerminalControl::HostKeyResponse { accepted: true };
        let instance = serde_json::to_value(response).unwrap();

        crate::contract_test_support::assert_portal_hub_contract(
            "terminal-control-message",
            &instance,
        );
    }

    #[test]
    fn portal_hub_upload_file_message_matches_contract() {
        let request_id = Uuid::new_v4();
        let message = WebTerminalControl::UploadFile {
            request_id,
            filename: "portal-paste-20260703-120000-abc.png".to_string(),
            contents_base64: BASE64.encode(b"png"),
        };
        let instance = serde_json::to_value(message).unwrap();

        crate::contract_test_support::assert_portal_hub_contract(
            "terminal-control-message",
            &instance,
        );
        assert_eq!(instance["type"], "upload_file");
        assert_eq!(instance["request_id"], request_id.to_string());
    }

    #[test]
    fn portal_hub_upload_file_result_matches_contract_and_completes_pending_request() {
        let request_id = Uuid::new_v4();
        let instance = json!({
            "type": "upload_file_result",
            "request_id": request_id,
            "path": "/home/john/.cache/portal/pastes/image.png"
        });
        crate::contract_test_support::assert_portal_hub_contract(
            "terminal-control-message",
            &instance,
        );

        let (response_tx, mut response_rx) = oneshot::channel();
        let mut pending = std::collections::HashMap::new();
        pending.insert(request_id, response_tx);

        handle_upload_file_result(&instance.to_string(), &mut pending).unwrap();

        assert!(pending.is_empty());
        assert_eq!(
            response_rx.try_recv().unwrap().unwrap(),
            "/home/john/.cache/portal/pastes/image.png"
        );
    }

    #[test]
    fn terminal_ws_url_uses_wss_for_tailscale_https() {
        assert_eq!(
            terminal_ws_url("https://portal-hub.example.ts.net").unwrap(),
            "wss://portal-hub.example.ts.net/api/sessions/terminal"
        );
    }
}
