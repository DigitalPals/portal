use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{HostsConfig, SettingsConfig, SnippetsConfig};
use crate::hub::vault::HubVaultConfig;
use crate::proxy::{HubSyncPutRequest, HubSyncResponse};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HubProfile {
    pub hosts: Value,
    pub settings: Value,
    pub snippets: Value,
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
    reqwest::Client::new()
        .get(format!("{}/api/sync", hub_url))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("failed to read Portal Hub sync profile: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub sync get failed: {}", error))?
        .json()
        .await
        .map_err(|error| format!("failed to parse Portal Hub sync profile: {}", error))
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
    reqwest::Client::new()
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
        .map_err(|error| format!("failed to parse Portal Hub sync response: {}", error))
}

fn web_url(settings: &crate::config::settings::PortalHubSettings) -> Result<String, String> {
    let hub_url = settings.web_url.trim().trim_end_matches('/').to_string();
    if hub_url.is_empty() {
        return Err("Portal Hub web URL is not configured".to_string());
    }
    Ok(hub_url)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
