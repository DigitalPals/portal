use std::collections::HashMap;
use std::time::Duration;

use futures::{StreamExt, stream::BoxStream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::settings::PortalHubSettings;
use crate::config::{HostsConfig, SettingsConfig, SnippetsConfig, paths};
use crate::hub::vault::HubVaultConfig;
use crate::proxy::{HubSyncPutRequest, HubSyncResponse};

const HOSTS: &str = "hosts";
const SETTINGS: &str = "settings";
const SNIPPETS: &str = "snippets";
const VAULT: &str = "vault";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortalHubSyncService {
    Hosts,
    Settings,
    Snippets,
    Vault,
}

impl PortalHubSyncService {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Hosts => HOSTS,
            Self::Settings => SETTINGS,
            Self::Snippets => SNIPPETS,
            Self::Vault => VAULT,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Hosts => "Hosts sync",
            Self::Settings => "Settings sync",
            Self::Snippets => "Snippets sync",
            Self::Vault => "Key vault",
        }
    }

    pub const fn stored_data_label(self) -> &'static str {
        match self {
            Self::Hosts => "stored hosts",
            Self::Settings => "stored settings",
            Self::Snippets => "stored snippets",
            Self::Vault => "stored key vault data",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubProfile {
    pub hosts: Value,
    pub settings: Value,
    pub snippets: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubSyncV2Response {
    pub api_version: u16,
    pub services: HashMap<String, HubSyncServiceState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubSyncRevisionEvent {
    pub api_version: u16,
    pub services: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubSyncServiceState {
    pub revision: String,
    pub payload: Value,
    #[serde(default)]
    pub tombstones: Value,
}

#[derive(Debug, Clone)]
pub struct LocalSyncProfile {
    pub hosts: HostsConfig,
    pub settings: SettingsConfig,
    pub snippets: SnippetsConfig,
    pub vault: HubVaultConfig,
}

#[derive(Debug, Clone)]
pub struct SyncConflict {
    pub service: String,
    pub local: Value,
    pub hub: Value,
    pub expected_revision: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    Local,
    Hub,
}

#[derive(Debug, Clone)]
pub enum SyncRunResult {
    Synced(SyncRunSummary),
    Conflicts(Vec<SyncConflict>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncRunOrigin {
    Manual,
    Background,
    Startup,
    RemoteEvent,
    Login,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncRunActivity {
    NoChanges,
    UploadedLocalChanges,
    AppliedRemoteChanges,
    UploadedAndApplied,
    Disabled,
}

#[derive(Debug, Clone)]
pub struct SyncRunSummary {
    pub activity: SyncRunActivity,
}

impl SyncRunSummary {
    pub const fn new(activity: SyncRunActivity) -> Self {
        Self { activity }
    }

    pub const fn message(&self) -> &'static str {
        match self.activity {
            SyncRunActivity::NoChanges => "Portal Hub is up to date",
            SyncRunActivity::UploadedLocalChanges => "Uploaded local changes to Portal Hub",
            SyncRunActivity::AppliedRemoteChanges => "Applied Portal Hub changes",
            SyncRunActivity::UploadedAndApplied => "Synced local and Portal Hub changes",
            SyncRunActivity::Disabled => "Portal Hub sync is disabled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LocalSyncState {
    hub_url: String,
    #[serde(default)]
    services: HashMap<String, LocalServiceSyncState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LocalServiceSyncState {
    revision: String,
    baseline: Value,
}

pub fn build_sync_request(
    hosts: &HostsConfig,
    settings: &SettingsConfig,
    snippets: &SnippetsConfig,
    vault: &HubVaultConfig,
) -> Result<HubSyncPutRequest, String> {
    Ok(HubSyncPutRequest {
        profile: json!({
            "hosts": serde_json::to_value(hosts)
                .map_err(|error| format!("failed to serialize hosts: {}", error))?,
            "settings": serde_json::to_value(settings)
                .map_err(|error| format!("failed to serialize settings: {}", error))?,
            "snippets": serde_json::to_value(snippets)
                .map_err(|error| format!("failed to serialize snippets: {}", error))?,
        }),
        vault: serde_json::to_value(vault)
            .map_err(|error| format!("failed to serialize vault: {}", error))?,
    })
}

pub fn parse_profile(response: &HubSyncResponse) -> Result<HubProfile, String> {
    Ok(HubProfile {
        hosts: response
            .profile
            .get("hosts")
            .cloned()
            .ok_or_else(|| "Portal Hub sync profile is missing hosts".to_string())?,
        settings: response
            .profile
            .get("settings")
            .cloned()
            .ok_or_else(|| "Portal Hub sync profile is missing settings".to_string())?,
        snippets: response
            .profile
            .get("snippets")
            .cloned()
            .ok_or_else(|| "Portal Hub sync profile is missing snippets".to_string())?,
    })
}

pub async fn http_sync_get(
    settings: &crate::config::settings::PortalHubSettings,
) -> Result<HubSyncResponse, String> {
    let hub_url = web_url(settings)?;
    let token = crate::hub::auth::load_access_token(&hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    let response: HubSyncResponse = reqwest::Client::new()
        .get(format!("{}/api/sync", hub_url))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("failed to read Portal Hub sync profile: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub sync get failed: {}", error))?
        .json()
        .await
        .map_err(|error| format!("failed to parse Portal Hub sync profile: {}", error))?;
    validate_legacy_sync_response(&response)?;
    Ok(response)
}

pub async fn http_sync_put(
    settings: &crate::config::settings::PortalHubSettings,
    expected_revision: String,
    request: HubSyncPutRequest,
) -> Result<HubSyncResponse, String> {
    let hub_url = web_url(settings)?;
    let token = crate::hub::auth::load_access_token(&hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    let body = serde_json::json!({
        "expected_revision": expected_revision,
        "profile": request.profile,
        "vault": request.vault,
    });
    let response: HubSyncResponse = reqwest::Client::new()
        .put(format!("{}/api/sync", hub_url))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("failed to upload Portal Hub sync profile: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub sync put failed: {}", error))?
        .json()
        .await
        .map_err(|error| format!("failed to parse Portal Hub sync response: {}", error))?;
    validate_legacy_sync_response(&response)?;
    Ok(response)
}

fn validate_legacy_sync_response(response: &HubSyncResponse) -> Result<(), String> {
    if response.api_version < 1 {
        return Err(format!(
            "Portal Hub sync API version {} is too old; Portal requires 1",
            response.api_version
        ));
    }
    Ok(())
}

pub async fn run_bidirectional_sync(
    settings: PortalHubSettings,
    profile: LocalSyncProfile,
) -> Result<SyncRunResult, String> {
    if !settings.sync_configured() {
        return Ok(SyncRunResult::Synced(SyncRunSummary::new(
            SyncRunActivity::Disabled,
        )));
    }

    let hub_url = web_url(&settings)?;
    let mut local_state = load_local_sync_state(&hub_url)?;
    let hub = http_sync_v2_get(&settings).await?;
    let mut updates = HashMap::new();
    let mut conflicts = Vec::new();
    let mut final_payloads: HashMap<String, Value> = HashMap::new();
    let mut uploaded_local = false;
    let mut applied_remote = false;

    for (service, local_payload) in enabled_service_payloads(&settings, &profile)? {
        let hub_service = hub
            .services
            .get(service)
            .cloned()
            .unwrap_or_else(|| default_hub_service(service));
        let baseline = local_state
            .services
            .get(service)
            .map(|state| state.baseline.clone())
            .unwrap_or_else(|| default_service_payload(service));
        let stored_revision = local_state
            .services
            .get(service)
            .map(|state| state.revision.as_str())
            .unwrap_or("0");

        if hub_service.payload == local_payload {
            final_payloads.insert(service.to_string(), local_payload.clone());
            local_state.services.insert(
                service.to_string(),
                LocalServiceSyncState {
                    revision: hub_service.revision,
                    baseline: local_payload,
                },
            );
            continue;
        }

        let hub_changed =
            hub_service.revision != stored_revision && hub_service.payload != baseline;
        let local_changed = local_payload != baseline;
        match (local_changed, hub_changed) {
            (false, true) => {
                applied_remote = true;
                final_payloads.insert(service.to_string(), hub_service.payload.clone());
                local_state.services.insert(
                    service.to_string(),
                    LocalServiceSyncState {
                        revision: hub_service.revision,
                        baseline: hub_service.payload,
                    },
                );
            }
            (true, false) => {
                uploaded_local = true;
                updates.insert(
                    service.to_string(),
                    json!({
                        "expected_revision": hub_service.revision,
                        "payload": local_payload,
                        "tombstones": hub_service.tombstones,
                    }),
                );
            }
            (true, true) => conflicts.push(SyncConflict {
                service: service.to_string(),
                local: local_payload,
                hub: hub_service.payload,
                expected_revision: hub_service.revision,
            }),
            (false, false) => {
                final_payloads.insert(service.to_string(), local_payload.clone());
            }
        }
    }

    if !conflicts.is_empty() {
        return Ok(SyncRunResult::Conflicts(conflicts));
    }

    if !updates.is_empty() {
        let response = http_sync_v2_put_values(&settings, updates).await?;
        for (service, service_state) in response.services {
            if enabled_service_names(&settings).contains(&service.as_str()) {
                local_state.services.insert(
                    service,
                    LocalServiceSyncState {
                        revision: service_state.revision,
                        baseline: service_state.payload,
                    },
                );
            }
        }
    }

    apply_payloads(settings, profile, final_payloads)?;
    save_local_sync_state(&local_state)?;
    let activity = match (uploaded_local, applied_remote) {
        (true, true) => SyncRunActivity::UploadedAndApplied,
        (true, false) => SyncRunActivity::UploadedLocalChanges,
        (false, true) => SyncRunActivity::AppliedRemoteChanges,
        (false, false) => SyncRunActivity::NoChanges,
    };
    Ok(SyncRunResult::Synced(SyncRunSummary::new(activity)))
}

pub fn local_sync_changes_pending(
    settings: &PortalHubSettings,
    profile: &LocalSyncProfile,
) -> Result<bool, String> {
    if !settings.sync_configured() {
        return Ok(false);
    }
    let hub_url = web_url(settings)?;
    let local_state = load_local_sync_state(&hub_url)?;
    for (service, local_payload) in enabled_service_payloads(settings, profile)? {
        let baseline = local_state
            .services
            .get(service)
            .map(|state| state.baseline.clone())
            .unwrap_or_else(|| default_service_payload(service));
        if local_payload != baseline {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn remote_revisions_require_sync(
    settings: &PortalHubSettings,
    revisions: &HashMap<String, String>,
) -> Result<bool, String> {
    if !settings.sync_configured() {
        return Ok(false);
    }
    let hub_url = web_url(settings)?;
    let local_state = load_local_sync_state(&hub_url)?;
    for service in enabled_service_names(settings) {
        let Some(remote_revision) = revisions.get(service) else {
            continue;
        };
        let local_revision = local_state
            .services
            .get(service)
            .map(|state| state.revision.as_str())
            .unwrap_or("0");
        if remote_revision != local_revision {
            return Ok(true);
        }
    }
    Ok(false)
}

pub async fn resolve_sync_conflicts(
    settings: PortalHubSettings,
    profile: LocalSyncProfile,
    conflicts: Vec<(SyncConflict, ConflictChoice)>,
) -> Result<String, String> {
    let hub_url = web_url(&settings)?;
    let mut local_state = load_local_sync_state(&hub_url)?;
    let mut updates = HashMap::new();
    let mut final_payloads = HashMap::new();

    for (conflict, choice) in conflicts {
        let payload = match choice {
            ConflictChoice::Local => conflict.local,
            ConflictChoice::Hub => conflict.hub,
        };
        final_payloads.insert(conflict.service.clone(), payload.clone());
        updates.insert(
            conflict.service,
            json!({
                "expected_revision": conflict.expected_revision,
                "payload": payload,
                "tombstones": [],
            }),
        );
    }

    let response = http_sync_v2_put_values(&settings, updates).await?;
    for (service, service_state) in response.services {
        if final_payloads.contains_key(&service) {
            local_state.services.insert(
                service,
                LocalServiceSyncState {
                    revision: service_state.revision,
                    baseline: service_state.payload,
                },
            );
        }
    }
    apply_payloads(settings, profile, final_payloads)?;
    save_local_sync_state(&local_state)?;
    Ok("Portal Hub conflicts resolved".to_string())
}

pub async fn clear_remote_service(
    settings: &PortalHubSettings,
    service: PortalHubSyncService,
) -> Result<String, String> {
    let hub_url = web_url(settings)?;
    let mut local_state = load_local_sync_state(&hub_url)?;
    let hub = http_sync_v2_get(settings).await?;
    let service_key = service.key();
    let current = hub
        .services
        .get(service_key)
        .cloned()
        .unwrap_or_else(|| default_hub_service(service_key));
    let payload = default_service_payload(service_key);
    let response = http_sync_v2_put_values(
        settings,
        HashMap::from([(
            service_key.to_string(),
            json!({
                "expected_revision": current.revision,
                "payload": payload,
                "tombstones": [],
            }),
        )]),
    )
    .await?;

    if let Some(service_state) = response.services.get(service_key) {
        local_state.services.insert(
            service_key.to_string(),
            LocalServiceSyncState {
                revision: service_state.revision.clone(),
                baseline: service_state.payload.clone(),
            },
        );
    }
    save_local_sync_state(&local_state)?;
    Ok(format!(
        "Deleted {} from Portal Hub",
        service.stored_data_label()
    ))
}

pub async fn http_sync_v2_get(settings: &PortalHubSettings) -> Result<HubSyncV2Response, String> {
    let hub_url = web_url(settings)?;
    let token = crate::hub::auth::load_access_token(&hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    reqwest::Client::new()
        .get(format!("{}/api/sync/v2", hub_url))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("failed to read Portal Hub sync state: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub sync get failed: {}", error))?
        .json()
        .await
        .map_err(|error| format!("failed to parse Portal Hub sync state: {}", error))
}

pub fn sync_revision_event_stream(
    hub_url: String,
) -> BoxStream<'static, Result<HubSyncRevisionEvent, String>> {
    async_stream::stream! {
        loop {
            match connect_sync_revision_events(&hub_url).await {
                Ok(mut response) => {
                    let mut buffer = String::new();
                    loop {
                        match response.chunk().await {
                            Ok(Some(chunk)) => {
                                buffer.push_str(&String::from_utf8_lossy(&chunk));
                                while let Some((index, separator_len)) = next_sse_event_boundary(&buffer) {
                                    let raw = buffer[..index].to_string();
                                    buffer.drain(..index + separator_len);
                                    match parse_sync_revision_event(&raw) {
                                        Ok(Some(event)) => yield Ok(event),
                                        Ok(None) => {}
                                        Err(error) => yield Err(error),
                                    }
                                }
                            }
                            Ok(None) => {
                                yield Err("Portal Hub sync event stream closed".to_string());
                                break;
                            }
                            Err(error) => {
                                yield Err(format!("failed to read Portal Hub sync events: {}", error));
                                break;
                            }
                        }
                    }
                }
                Err(error) => yield Err(error),
            }

            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    .boxed()
}

async fn connect_sync_revision_events(hub_url: &str) -> Result<reqwest::Response, String> {
    let token = crate::hub::auth::load_access_token(hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    reqwest::Client::new()
        .get(format!("{}/api/sync/v2/events", hub_url))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("failed to connect Portal Hub sync events: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub sync events failed: {}", error))
}

async fn http_sync_v2_put_values(
    settings: &PortalHubSettings,
    services: HashMap<String, Value>,
) -> Result<HubSyncV2Response, String> {
    let hub_url = web_url(settings)?;
    let token = crate::hub::auth::load_access_token(&hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    let body = json!({ "services": services });
    reqwest::Client::new()
        .put(format!("{}/api/sync/v2", hub_url))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("failed to update Portal Hub sync state: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub sync update failed: {}", error))?
        .json()
        .await
        .map_err(|error| format!("failed to parse Portal Hub sync response: {}", error))
}

fn enabled_service_payloads(
    settings: &PortalHubSettings,
    profile: &LocalSyncProfile,
) -> Result<Vec<(&'static str, Value)>, String> {
    let mut services = Vec::new();
    if settings.hosts_sync_enabled {
        let hosts = canonical_hosts_payload(&profile.hosts)?;
        services.push((HOSTS, hosts));
    }
    if settings.settings_sync_enabled {
        let mut synced_settings = profile.settings.clone();
        synced_settings.portal_hub = PortalHubSettings::default();
        services.push((
            SETTINGS,
            serde_json::to_value(&synced_settings)
                .map_err(|error| format!("failed to serialize settings: {}", error))?,
        ));
    }
    if settings.snippets_sync_enabled {
        services.push((
            SNIPPETS,
            serde_json::to_value(&profile.snippets)
                .map_err(|error| format!("failed to serialize snippets: {}", error))?,
        ));
    }
    if settings.key_vault_enabled {
        services.push((
            VAULT,
            serde_json::to_value(&profile.vault)
                .map_err(|error| format!("failed to serialize vault: {}", error))?,
        ));
    }
    Ok(services)
}

fn enabled_service_names(settings: &PortalHubSettings) -> Vec<&'static str> {
    let mut services = Vec::new();
    if settings.hosts_sync_enabled {
        services.push(HOSTS);
    }
    if settings.settings_sync_enabled {
        services.push(SETTINGS);
    }
    if settings.snippets_sync_enabled {
        services.push(SNIPPETS);
    }
    if settings.key_vault_enabled {
        services.push(VAULT);
    }
    services
}

fn apply_payloads(
    settings: PortalHubSettings,
    profile: LocalSyncProfile,
    payloads: HashMap<String, Value>,
) -> Result<(), String> {
    if let Some(value) = payloads.get(HOSTS) {
        let mut hosts: HostsConfig = serde_json::from_value(value.clone())
            .map_err(|error| format!("failed to parse synced hosts: {}", error))?;
        preserve_runtime_host_metadata(&mut hosts, &profile.hosts);
        hosts.save().map_err(|error| error.to_string())?;
    }
    if let Some(value) = payloads.get(SETTINGS) {
        let mut synced_settings: SettingsConfig = serde_json::from_value(value.clone())
            .map_err(|error| format!("failed to parse synced settings: {}", error))?;
        synced_settings.portal_hub = settings;
        synced_settings.save().map_err(|error| error.to_string())?;
    }
    if let Some(value) = payloads.get(SNIPPETS) {
        let snippets: SnippetsConfig = serde_json::from_value(value.clone())
            .map_err(|error| format!("failed to parse synced snippets: {}", error))?;
        snippets.save().map_err(|error| error.to_string())?;
    }
    if let Some(value) = payloads.get(VAULT) {
        let vault: HubVaultConfig = serde_json::from_value(value.clone())
            .map_err(|error| format!("failed to parse synced vault: {}", error))?;
        vault.save()?;
    }
    let _ = profile;
    Ok(())
}

fn load_local_sync_state(hub_url: &str) -> Result<LocalSyncState, String> {
    let path = paths::hub_sync_state_file()
        .ok_or_else(|| "could not determine sync state path".to_string())?;
    if !path.exists() {
        return Ok(LocalSyncState {
            hub_url: hub_url.to_string(),
            services: HashMap::new(),
        });
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {}", path.display(), error))?;
    let mut state: LocalSyncState = serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse {}: {}", path.display(), error))?;
    if state.hub_url != hub_url {
        state = LocalSyncState {
            hub_url: hub_url.to_string(),
            services: HashMap::new(),
        };
    }
    Ok(state)
}

fn save_local_sync_state(state: &LocalSyncState) -> Result<(), String> {
    paths::ensure_config_dir()
        .map_err(|error| format!("failed to create config directory: {}", error))?;
    let path = paths::hub_sync_state_file()
        .ok_or_else(|| "could not determine sync state path".to_string())?;
    let content = serde_json::to_string_pretty(state)
        .map_err(|error| format!("failed to serialize sync state: {}", error))?;
    crate::config::write_atomic(&path, &content)
        .map_err(|error| format!("failed to write {}: {}", path.display(), error))
}

fn default_hub_service(service: &str) -> HubSyncServiceState {
    HubSyncServiceState {
        revision: "0".to_string(),
        payload: default_service_payload(service),
        tombstones: json!([]),
    }
}

fn canonical_hosts_payload(hosts: &HostsConfig) -> Result<Value, String> {
    let mut value = serde_json::to_value(hosts)
        .map_err(|error| format!("failed to serialize hosts: {}", error))?;
    if let Some(hosts) = value.get_mut("hosts").and_then(Value::as_array_mut) {
        for host in hosts {
            if let Some(host) = host.as_object_mut() {
                host.remove("last_connected");
                host.remove("detected_os");
                if let Some(created_at) = host.get("created_at").cloned() {
                    host.insert("updated_at".to_string(), created_at);
                }
            }
        }
    }
    Ok(value)
}

fn preserve_runtime_host_metadata(remote: &mut HostsConfig, local: &HostsConfig) {
    for host in &mut remote.hosts {
        if let Some(local_host) = local.find_host(host.id) {
            host.last_connected = local_host.last_connected;
            host.detected_os = local_host.detected_os.clone();
            host.updated_at = local_host.updated_at;
        }
    }
}

fn parse_sync_revision_event(raw: &str) -> Result<Option<HubSyncRevisionEvent>, String> {
    let mut event_name = "message";
    let mut data = String::new();

    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_name = value.trim();
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.trim_start());
        }
    }

    if event_name != "sync" || data.trim().is_empty() {
        return Ok(None);
    }

    serde_json::from_str(&data)
        .map(Some)
        .map_err(|error| format!("failed to parse Portal Hub sync event: {}", error))
}

fn next_sse_event_boundary(buffer: &str) -> Option<(usize, usize)> {
    match (buffer.find("\n\n"), buffer.find("\r\n\r\n")) {
        (Some(lf), Some(crlf)) if crlf < lf => Some((crlf, 4)),
        (Some(lf), _) => Some((lf, 2)),
        (None, Some(crlf)) => Some((crlf, 4)),
        (None, None) => None,
    }
}

fn default_service_payload(service: &str) -> Value {
    match service {
        HOSTS => json!({"hosts": [], "groups": []}),
        SETTINGS => json!({}),
        SNIPPETS => json!({"snippets": []}),
        VAULT => json!({"keys": []}),
        _ => json!(null),
    }
}

fn web_url(settings: &crate::config::settings::PortalHubSettings) -> Result<String, String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub web URL is not configured".to_string());
    }
    Ok(hub_url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthMethod, Host, Protocol};

    #[test]
    fn sync_request_contains_profile_and_vault_sections() {
        let request = build_sync_request(
            &HostsConfig::default(),
            &SettingsConfig::default(),
            &SnippetsConfig::default(),
            &HubVaultConfig::default(),
        )
        .unwrap();

        assert!(request.profile.get("hosts").is_some());
        assert!(request.profile.get("settings").is_some());
        assert!(request.profile.get("snippets").is_some());
        assert!(request.vault.get("keys").is_some());
    }

    #[test]
    fn canonical_hosts_payload_ignores_runtime_metadata() {
        let created_at = chrono::DateTime::parse_from_rfc3339("2026-04-25T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let updated_at = chrono::DateTime::parse_from_rfc3339("2026-04-26T00:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let host = Host {
            id: uuid::Uuid::new_v4(),
            name: "Test".to_string(),
            hostname: "example.com".to_string(),
            port: 22,
            username: "john".to_string(),
            protocol: Protocol::Ssh,
            vnc_port: None,
            vnc_password_id: None,
            auth: AuthMethod::Agent,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            portal_hub_enabled: true,
            group_id: None,
            notes: None,
            tags: Vec::new(),
            created_at,
            updated_at,
            detected_os: Some(crate::config::DetectedOs::Linux),
            last_connected: Some(updated_at),
        };

        let payload = canonical_hosts_payload(&HostsConfig {
            hosts: vec![host],
            groups: Vec::new(),
        })
        .unwrap();
        let host = payload["hosts"][0].as_object().unwrap();

        assert!(!host.contains_key("last_connected"));
        assert!(!host.contains_key("detected_os"));
        assert_eq!(host.get("updated_at"), host.get("created_at"));
    }

    #[test]
    fn parses_sync_revision_sse_event() {
        let event = parse_sync_revision_event(
            "event: sync\ndata: {\"api_version\":2,\"services\":{\"hosts\":\"rev-1\"}}\n",
        )
        .unwrap()
        .unwrap();

        assert_eq!(event.api_version, 2);
        assert_eq!(event.services.get("hosts"), Some(&"rev-1".to_string()));
    }

    #[test]
    fn finds_lf_and_crlf_sse_boundaries() {
        assert_eq!(next_sse_event_boundary("event: sync\n\n"), Some((11, 2)));
        assert_eq!(
            next_sse_event_boundary("event: sync\r\n\r\n"),
            Some((11, 4))
        );
    }
}
