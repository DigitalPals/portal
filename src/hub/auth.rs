use std::collections::HashMap;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use data_encoding::BASE64URL_NOPAD;
use rand::RngCore;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::config::settings::PortalHubSettings;
use crate::hub::http;

const CLIENT_ID: &str = "portal-desktop";
pub(crate) const KEYCHAIN_SERVICE: &str = "com.digitalpals.portal";

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
    #[serde(default)]
    pub session_titles: bool,
    #[serde(default)]
    pub vault_enrollment: bool,
}

#[derive(Serialize, Deserialize)]
struct StoredTokens {
    access_token: String,
    refresh_token: String,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
}

#[derive(Debug, Deserialize)]
struct MeResponse {
    username: String,
}

/// Tokens acquired for a draft stay in memory until the user finishes setup.
#[derive(Debug, Clone)]
pub struct PendingHubAuth {
    pub summary: HubAuthSummary,
    access_token: SecretString,
    refresh_token: SecretString,
}

impl PendingHubAuth {
    #[cfg(test)]
    pub(crate) fn test_auth(hub_url: &str) -> Self {
        Self {
            summary: HubAuthSummary {
                username: "owner".into(),
                hub_url: hub_url.into(),
            },
            access_token: SecretString::from("test-access-secret"),
            refresh_token: SecretString::from("test-refresh-secret"),
        }
    }

    pub fn commit(&self) -> Result<(), String> {
        store_tokens(
            &self.summary.hub_url,
            self.access_token.expose_secret(),
            self.refresh_token.expose_secret(),
        )
    }
}

pub async fn authenticate(settings: PortalHubSettings) -> Result<PendingHubAuth, String> {
    browser_authorization(settings, None).await
}

/// Create the owner passkey through a one-time setup link from the deployment
/// connection, and sign this device in with the same browser visit.
pub async fn authenticate_with_setup(
    settings: PortalHubSettings,
    setup_token: SecretString,
) -> Result<PendingHubAuth, String> {
    browser_authorization(settings, Some(setup_token)).await
}

/// OAuth PKCE through the system browser, where the Hub verifies the owner's
/// passkey. Portal never sees the passkey; it receives a code on loopback.
async fn browser_authorization(
    settings: PortalHubSettings,
    setup_token: Option<SecretString>,
) -> Result<PendingHubAuth, String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub web URL is not configured".to_string());
    }

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|error| format!("failed to start local OAuth callback listener: {}", error))?;
    let callback_addr = listener
        .local_addr()
        .map_err(|error| format!("failed to read OAuth callback address: {}", error))?;
    let redirect_uri = format!("http://{}/callback", callback_addr);
    let state = random_url_token(24);
    let verifier = random_url_token(32);
    let challenge = BASE64URL_NOPAD.encode(&Sha256::digest(verifier.as_bytes()));
    let browser_url = authorization_url(
        &hub_url,
        &redirect_uri,
        &challenge,
        &state,
        setup_token.as_ref(),
    );

    open::that(browser_url.expose_secret())
        .map_err(|error| format!("failed to open browser for Portal Hub sign-in: {}", error))?;

    // Creating a passkey can take longer than using one.
    let limit = Duration::from_secs(if setup_token.is_some() { 900 } else { 300 });
    let code = tokio::time::timeout(limit, wait_for_callback(listener, &state))
        .await
        .map_err(|_| {
            if setup_token.is_some() {
                "Passkey setup timed out. Select Create owner passkey to get a new link."
                    .to_string()
            } else {
                "Sign-in timed out. Select Sign in to try again.".to_string()
            }
        })??;

    finish_authorization(hub_url, &code, &redirect_uri, &verifier).await
}

/// The browser address: the sign-in page, or the passkey setup page with the
/// one-time token in the fragment, which browsers never send to the server.
fn authorization_url(
    hub_url: &str,
    redirect_uri: &str,
    challenge: &str,
    state: &str,
    setup_token: Option<&SecretString>,
) -> SecretString {
    let query = format!(
        "response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}",
        CLIENT_ID,
        urlencoding::encode(redirect_uri),
        urlencoding::encode(challenge),
        urlencoding::encode(state),
    );
    match setup_token {
        Some(token) => format!(
            "{hub_url}/setup?{query}#token={}",
            urlencoding::encode(token.expose_secret())
        ),
        None => format!("{hub_url}/oauth/authorize?{query}"),
    }
    .into()
}

async fn finish_authorization(
    hub_url: String,
    code: &str,
    redirect_uri: &str,
    verifier: &str,
) -> Result<PendingHubAuth, String> {
    let client = http::client();
    let token: TokenResponse = client
        .post(format!("{}/oauth/token", hub_url))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier),
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

    Ok(PendingHubAuth {
        summary: HubAuthSummary {
            username: me.username,
            hub_url,
        },
        access_token: token.access_token.into(),
        refresh_token: token.refresh_token.into(),
    })
}

pub async fn fetch_hub_info(settings: &PortalHubSettings) -> Result<HubInfo, String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub URL or address is not configured".to_string());
    }
    http::json(
        true,
        |client| client.get(format!("{}/api/info", hub_url)),
        "failed to read Portal Hub info",
        "Portal Hub info check failed",
        "failed to parse Portal Hub info",
    )
    .await
}

pub fn load_access_token(hub_url: &str) -> Result<Option<String>, String> {
    load_tokens(hub_url).map(|tokens| tokens.map(|tokens| tokens.access_token))
}

pub async fn refresh_access_token(hub_url: &str) -> Result<Option<String>, String> {
    let Some(tokens) = load_tokens(hub_url)? else {
        return Ok(None);
    };
    let token: TokenResponse = http::client()
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

async fn wait_for_callback(listener: TcpListener, expected_state: &str) -> Result<String, String> {
    let (mut stream, _) = listener
        .accept()
        .await
        .map_err(|_| "Could not receive the sign-in callback")?;
    let mut buffer = [0u8; 4096];
    let read = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer))
        .await
        .map_err(|_| "Sign-in callback timed out")?
        .map_err(|_| "Could not read the sign-in callback")?;
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
    ).await;
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
    use serde_json::json;

    #[test]
    fn setup_token_travels_only_in_the_fragment_of_the_setup_page() {
        let token = SecretString::from("setup-token-value");
        let url = authorization_url(
            "https://hub.example.ts.net",
            "http://127.0.0.1:4000/callback",
            "challenge",
            "state",
            Some(&token),
        );
        let url = url::Url::parse(url.expose_secret()).unwrap();
        assert_eq!(url.path(), "/setup");
        assert_eq!(url.fragment(), Some("token=setup-token-value"));
        assert!(!url.query().unwrap().contains("setup-token-value"));
        assert!(url.query().unwrap().contains("client_id=portal-desktop"));
        let sign_in = authorization_url(
            "https://hub.example.ts.net",
            "http://127.0.0.1:4000/callback",
            "challenge",
            "state",
            None,
        );
        let sign_in = url::Url::parse(sign_in.expose_secret()).unwrap();
        assert_eq!(sign_in.path(), "/oauth/authorize");
        assert!(sign_in.fragment().is_none());
    }

    #[test]
    fn parse_query_decodes_callback_params() {
        let params = parse_query("code=abc%20123&state=xyz");
        assert_eq!(params.get("code"), Some(&"abc 123".to_string()));
        assert_eq!(params.get("state"), Some(&"xyz".to_string()));
    }

    #[test]
    fn portal_hub_info_matches_v2_contract() {
        let instance = json!({
            "api_version": 2,
            "version": "0.5.0-beta.13",
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
            "ssh_username": "portal-hub"
        });

        crate::contract_test_support::assert_portal_hub_contract("api-info-response", &instance);
        let info: HubInfo = serde_json::from_value(instance).unwrap();

        assert_eq!(info.api_version, 2);
        assert!(info.capabilities.sync_v2);
        assert!(info.capabilities.sync_events);
        assert!(info.capabilities.web_proxy);
        assert!(info.capabilities.session_titles);
        assert!(info.capabilities.key_vault);
        assert!(info.capabilities.vault_enrollment);
        assert_eq!(info.ssh_port, Some(2222));
        assert_eq!(info.ssh_username.as_deref(), Some("portal-hub"));
    }
}
