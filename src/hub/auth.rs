use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use data_encoding::BASE64URL_NOPAD;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::settings::PortalHubSettings;

const CLIENT_ID: &str = "portal-desktop";
const KEYCHAIN_SERVICE: &str = "com.digitalpals.portal";

#[derive(Debug, Clone)]
pub struct HubAuthSummary {
    pub username: String,
    pub hub_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HubInfo {
    pub api_version: u16,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub public_url: String,
    #[serde(default)]
    pub capabilities: HubCapabilities,
    #[serde(default)]
    pub ssh_port: Option<u16>,
    #[serde(default)]
    pub ssh_username: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct HubCapabilities {
    #[serde(default)]
    pub sync_v2: bool,
    #[serde(default)]
    pub sync_events: bool,
    #[serde(default)]
    pub key_vault: bool,
    #[serde(default)]
    pub web_proxy: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct StoredTokens {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct MeResponse {
    username: String,
}

pub async fn authenticate(settings: PortalHubSettings) -> Result<HubAuthSummary, String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub web URL is not configured".to_string());
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("failed to start local OAuth callback listener: {}", error))?;
    listener
        .set_nonblocking(false)
        .map_err(|error| format!("failed to configure OAuth callback listener: {}", error))?;
    let callback_addr = listener
        .local_addr()
        .map_err(|error| format!("failed to read OAuth callback address: {}", error))?;
    let redirect_uri = format!("http://{}/callback", callback_addr);
    let state = random_url_token(24);
    let verifier = random_url_token(32);
    let challenge = BASE64URL_NOPAD.encode(&Sha256::digest(verifier.as_bytes()));
    let authorize_url = format!(
        "{}/oauth/authorize?response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}",
        hub_url,
        CLIENT_ID,
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&challenge),
        urlencoding::encode(&state),
    );

    open::that(&authorize_url)
        .map_err(|error| format!("failed to open browser for Portal Hub sign-in: {}", error))?;

    let code = tokio::task::spawn_blocking(move || wait_for_callback(listener, &state))
        .await
        .map_err(|error| format!("OAuth callback task failed: {}", error))??;

    let client = reqwest::Client::new();
    let token: TokenResponse = client
        .post(format!("{}/oauth/token", hub_url))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect_uri.as_str()),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|error| {
            format!(
                "failed to exchange Portal Hub authorization code: {}",
                error
            )
        })?
        .error_for_status()
        .map_err(|error| format!("Portal Hub token exchange failed: {}", error))?
        .json()
        .await
        .map_err(|error| format!("failed to parse Portal Hub token response: {}", error))?;

    store_tokens(&hub_url, &token.access_token, &token.refresh_token)?;

    let me: MeResponse = client
        .get(format!("{}/api/me", hub_url))
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|error| format!("failed to read Portal Hub user: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub user check failed: {}", error))?
        .json()
        .await
        .map_err(|error| format!("failed to parse Portal Hub user response: {}", error))?;

    Ok(HubAuthSummary {
        username: me.username,
        hub_url,
    })
}

pub async fn fetch_hub_info(settings: &PortalHubSettings) -> Result<HubInfo, String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub host and web port are not configured".to_string());
    }
    reqwest::Client::new()
        .get(format!("{}/api/info", hub_url))
        .send()
        .await
        .map_err(|error| format!("failed to read Portal Hub info: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub info check failed: {}", error))?
        .json()
        .await
        .map_err(|error| format!("failed to parse Portal Hub info: {}", error))
}

pub fn load_access_token(hub_url: &str) -> Result<Option<String>, String> {
    load_tokens(hub_url).map(|tokens| tokens.map(|tokens| tokens.access_token))
}

pub async fn refresh_access_token(hub_url: &str) -> Result<Option<String>, String> {
    let Some(tokens) = load_tokens(hub_url)? else {
        return Ok(None);
    };
    let token: TokenResponse = reqwest::Client::new()
        .post(format!("{}/oauth/token", hub_url))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", tokens.refresh_token.as_str()),
            ("client_id", CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|error| format!("failed to refresh Portal Hub token: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub token refresh failed: {}", error))?
        .json()
        .await
        .map_err(|error| {
            format!(
                "failed to parse Portal Hub token refresh response: {}",
                error
            )
        })?;

    store_tokens(hub_url, &token.access_token, &token.refresh_token)?;
    Ok(Some(token.access_token))
}

fn load_tokens(hub_url: &str) -> Result<Option<StoredTokens>, String> {
    let entry = keyring_entry(hub_url)?;
    match entry.get_password() {
        Ok(raw) => {
            let tokens: StoredTokens = serde_json::from_str(&raw)
                .map_err(|error| format!("failed to parse stored Portal Hub tokens: {}", error))?;
            Ok(Some(tokens))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!(
            "failed to read Portal Hub tokens from OS keychain: {}",
            error
        )),
    }
}

pub fn logout(settings: &PortalHubSettings) -> Result<(), String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Ok(());
    }
    let entry = keyring_entry(&hub_url)?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!(
            "failed to remove Portal Hub tokens from OS keychain: {}",
            error
        )),
    }
}

fn store_tokens(hub_url: &str, access_token: &str, refresh_token: &str) -> Result<(), String> {
    let tokens = StoredTokens {
        access_token: access_token.to_string(),
        refresh_token: refresh_token.to_string(),
    };
    keyring_entry(hub_url)?
        .set_password(
            &serde_json::to_string(&tokens)
                .map_err(|error| format!("failed to serialize Portal Hub tokens: {}", error))?,
        )
        .map_err(|error| {
            format!(
                "failed to store Portal Hub tokens in OS keychain: {}",
                error
            )
        })
}

fn keyring_entry(hub_url: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &format!("portal-hub:{}", hub_url))
        .map_err(|error| format!("failed to open OS keychain: {}", error))
}

fn wait_for_callback(listener: TcpListener, expected_state: &str) -> Result<String, String> {
    listener
        .set_nonblocking(false)
        .map_err(|error| format!("failed to configure OAuth callback listener: {}", error))?;
    listener
        .set_ttl(64)
        .map_err(|error| format!("failed to configure OAuth callback listener: {}", error))?;

    let (mut stream, _) = listener
        .accept()
        .map_err(|error| format!("failed to accept OAuth callback: {}", error))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|error| format!("failed to configure OAuth callback timeout: {}", error))?;

    let mut buffer = [0u8; 4096];
    let read = stream
        .read(&mut buffer)
        .map_err(|error| format!("failed to read OAuth callback: {}", error))?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let first_line = request.lines().next().unwrap_or_default();
    let path = first_line
        .split_whitespace()
        .nth(1)
        .ok_or_else(|| "invalid OAuth callback request".to_string())?;
    let query = path
        .split_once('?')
        .map(|(_, query)| query)
        .ok_or_else(|| "OAuth callback is missing query".to_string())?;
    let params = parse_query(query);
    let state = params
        .get("state")
        .ok_or_else(|| "OAuth callback is missing state".to_string())?;
    if state != expected_state {
        return Err("OAuth state mismatch".to_string());
    }
    let code = params
        .get("code")
        .ok_or_else(|| "OAuth callback is missing code".to_string())?
        .to_string();

    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n\r\n<html><body><h1>Portal Hub sign-in complete</h1><p>You can return to Portal.</p></body></html>",
    );
    Ok(code)
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            Some((
                urlencoding::decode(key).ok()?.to_string(),
                urlencoding::decode(value).ok()?.to_string(),
            ))
        })
        .collect()
}

fn random_url_token(len: usize) -> String {
    let mut bytes = vec![0u8; len];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    BASE64URL_NOPAD.encode(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_decodes_callback_params() {
        let params = parse_query("code=abc%20123&state=xyz");
        assert_eq!(params.get("code"), Some(&"abc 123".to_string()));
        assert_eq!(params.get("state"), Some(&"xyz".to_string()));
    }
}
