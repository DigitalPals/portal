//! Portal Hub PTY session management.
//!
//! Portal connects to Portal Hub through OAuth-authenticated web APIs. Interactive
//! proxy sessions use a WebSocket terminal stream instead of a separate SSH login
//! to the Hub.

use chrono::{DateTime, Utc};
use data_encoding::BASE64;
use futures::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use uuid::Uuid;

use crate::config::settings::PortalHubSettings;
use crate::config::{AuthMethod, Host};
use crate::error::LocalError;

const MIN_SUPPORTED_WEB_PROXY_API_VERSION: u16 = 2;

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
    #[serde(default)]
    metadata_schema_version: u16,
    #[serde(default)]
    capabilities: ProxyCapabilities,
}

#[derive(Debug, Default, Deserialize)]
struct ProxyCapabilities {
    #[serde(default)]
    web_proxy: bool,
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
    Resize { cols: u16, rows: u16 },
}

#[derive(Debug, Serialize)]
struct WebTerminalStart {
    session_id: Uuid,
    target_host: String,
    target_port: u16,
    target_user: String,
    cols: u16,
    rows: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    private_key: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WebTerminalControl {
    Resize { cols: u16, rows: u16 },
}

#[derive(Debug)]
pub struct ProxySession {
    command_tx: mpsc::Sender<ProxyCommand>,
    child_killer: Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>,
}

impl ProxySession {
    pub fn spawn(
        settings: &PortalHubSettings,
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
        let private_key = proxy_private_key(&host.auth)?;
        Self::spawn_web_target(settings, &target, cols, rows, event_tx, private_key)
    }

    pub fn spawn_target(
        settings: &PortalHubSettings,
        target: &ProxySessionTarget,
        cols: u16,
        rows: u16,
        event_tx: mpsc::Sender<ProxyEvent>,
    ) -> Result<Self, LocalError> {
        Self::spawn_web_target(settings, target, cols, rows, event_tx, None)
    }

    fn spawn_web_target(
        settings: &PortalHubSettings,
        target: &ProxySessionTarget,
        cols: u16,
        rows: u16,
        event_tx: mpsc::Sender<ProxyEvent>,
        private_key: Option<String>,
    ) -> Result<Self, LocalError> {
        let (command_tx, command_rx) = mpsc::channel::<ProxyCommand>(256);
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
                    return;
                }
            };
            runtime.block_on(run_web_terminal(
                settings,
                target,
                cols,
                rows,
                private_key,
                command_rx,
                event_tx,
            ));
        });

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
}

async fn run_web_terminal(
    settings: PortalHubSettings,
    target: ProxySessionTarget,
    cols: u16,
    rows: u16,
    private_key: Option<String>,
    mut command_rx: mpsc::Receiver<ProxyCommand>,
    event_tx: mpsc::Sender<ProxyEvent>,
) {
    let result = async {
        let hub_url = settings.effective_web_url();
        if hub_url.is_empty() {
            return Err("Portal Hub host and web port are not configured".to_string());
        }
        let token = crate::hub::auth::load_access_token(&hub_url)?
            .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
        let mut request = terminal_ws_url(&hub_url)?
            .into_client_request()
            .map_err(|error| format!("failed to build Portal Hub terminal request: {}", error))?;
        request.headers_mut().insert(
            tokio_tungstenite::tungstenite::http::header::AUTHORIZATION,
            format!("Bearer {}", token)
                .parse()
                .map_err(|error| format!("invalid Portal Hub authorization header: {}", error))?,
        );

        let (stream, _) = connect_async(request)
            .await
            .map_err(|error| format!("failed to connect to Portal Hub terminal: {}", error))?;
        let (mut write, mut read) = stream.split();
        let start = WebTerminalStart {
            session_id: target.session_id,
            target_host: target.target_host,
            target_port: target.target_port,
            target_user: target.target_user,
            cols,
            rows,
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
            tokio::select! {
                message = read.next() => {
                    let Some(message) = message else {
                        return Ok(());
                    };
                    match message.map_err(|error| format!("Portal Hub terminal read failed: {}", error))? {
                        WsMessage::Binary(data) => {
                            let _ = event_tx.send(ProxyEvent::Data(data)).await;
                        }
                        WsMessage::Text(text) => {
                            if text.starts_with("{\"type\":\"error\"") {
                                let _ = event_tx.send(ProxyEvent::Data(format!("{}\r\n", text).into_bytes())).await;
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
                    }
                }
            }
        }
    }
    .await;

    if let Err(error) = result {
        let _ = event_tx
            .send(ProxyEvent::Data(format!("{}\r\n", error).into_bytes()))
            .await;
        let _ = event_tx
            .send(ProxyEvent::Disconnected { clean: false })
            .await;
    } else {
        let _ = event_tx
            .send(ProxyEvent::Disconnected { clean: true })
            .await;
    }
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
            vault_key_id: Some(vault_key_id),
            ..
        } => crate::hub::vault::load_decrypted_private_key(*vault_key_id)
            .map(|key| Some(key.expose_secret().to_string()))
            .map_err(LocalError::SpawnFailed),
        _ => Err(LocalError::SpawnFailed(
            "Portal Hub proxy requires a Key Vault private key for this host".to_string(),
        )),
    }
}

pub async fn list_active_sessions(
    settings: &PortalHubSettings,
) -> Result<Vec<ListedProxySession>, String> {
    let hub_url = settings.effective_web_url();
    let token = crate::hub::auth::load_access_token(&hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    let response: RawListResponse = reqwest::Client::new()
        .get(format!(
            "{}/api/sessions?active=true&include_preview=true&preview_bytes=524288",
            hub_url
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("failed to list Portal Hub sessions: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub session list failed: {}", error))?
        .json()
        .await
        .map_err(|error| format!("failed to parse Portal Hub sessions: {}", error))?;
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

    raw_sessions_to_listed(raw)
}

pub async fn check_proxy_status(settings: &PortalHubSettings) -> Result<ProxyStatus, String> {
    let hub_url = settings.effective_web_url();
    let raw: RawProxyVersion = reqwest::Client::new()
        .get(format!("{}/api/info", hub_url))
        .send()
        .await
        .map_err(|error| format!("failed to read Portal Hub info: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub info check failed: {}", error))?
        .json()
        .await
        .map_err(|error| format!("failed to parse Portal Hub info: {}", error))?;
    if raw.api_version < MIN_SUPPORTED_WEB_PROXY_API_VERSION {
        return Err(format!(
            "Portal Hub API version {} is too old; Portal requires {}",
            raw.api_version, MIN_SUPPORTED_WEB_PROXY_API_VERSION
        ));
    }
    if !raw.capabilities.web_proxy {
        return Err("Portal Hub does not advertise web proxy support".to_string());
    }

    Ok(ProxyStatus {
        version: raw.version,
        api_version: raw.api_version,
        metadata_schema_version: raw.metadata_schema_version,
    })
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

impl Drop for ProxySession {
    fn drop(&mut self) {
        tracing::debug!("Portal Hub session cleanup: detaching local ssh process");
        let (replacement_tx, _replacement_rx) = mpsc::channel(1);
        let _ = std::mem::replace(&mut self.command_tx, replacement_tx);
        if let Some(mut killer) = self.child_killer.take() {
            if let Err(error) = killer.kill() {
                tracing::debug!("Failed to kill Portal Hub ssh process: {}", error);
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
    fn raw_proxy_version_deserializes() {
        let raw: RawProxyVersion = serde_json::from_slice(
            br#"{"version":"0.5.0","api_version":2,"metadata_schema_version":1,"capabilities":{"web_proxy":true}}"#,
        )
        .unwrap();

        assert_eq!(raw.version, "0.5.0");
        assert_eq!(raw.api_version, 2);
        assert_eq!(raw.metadata_schema_version, 1);
        assert!(raw.capabilities.web_proxy);
    }
}
