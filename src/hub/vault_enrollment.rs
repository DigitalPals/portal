use data_encoding::BASE64;
use rsa::pkcs8::DecodePublicKey;
use rsa::rand_core::OsRng;
use rsa::{Oaep, RsaPublicKey};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use sha1::Sha1;
use sha2::Sha256;

use crate::config::settings::PortalHubSettings;
use crate::hub::vault::load_stored_vault_secret;

#[derive(Debug, Clone, Deserialize)]
pub struct VaultEnrollment {
    pub id: String,
    pub device_name: String,
    pub public_key_algorithm: String,
    pub public_key_der_base64: String,
    pub status: String,
    pub encrypted_secret_base64: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub approved_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VaultEnrollmentListResponse {
    enrollments: Vec<VaultEnrollment>,
}

#[derive(Debug, Serialize)]
struct VaultEnrollmentApproveRequest {
    encrypted_secret_base64: String,
}

pub async fn list_pending(settings: &PortalHubSettings) -> Result<Vec<VaultEnrollment>, String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub web URL is not configured".to_string());
    }
    let token = crate::hub::auth::load_access_token(&hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    reqwest::Client::new()
        .get(format!("{}/api/vault/enrollments?status=pending", hub_url))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("failed to list vault enrollment requests: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub vault enrollment request failed: {}", error))?
        .json::<VaultEnrollmentListResponse>()
        .await
        .map(|response| response.enrollments)
        .map_err(|error| format!("failed to parse vault enrollment requests: {}", error))
}

pub async fn approve(
    settings: PortalHubSettings,
    enrollment: VaultEnrollment,
) -> Result<VaultEnrollment, String> {
    if enrollment.public_key_algorithm != "RSA-OAEP-SHA256" {
        return Err(format!(
            "unsupported vault enrollment public key algorithm: {}",
            enrollment.public_key_algorithm
        ));
    }
    let vault_secret = load_stored_vault_secret()?.ok_or_else(|| {
        "Portal vault is locked because no vault secret was found in the OS keychain".to_string()
    })?;
    let public_key_der = BASE64
        .decode(enrollment.public_key_der_base64.as_bytes())
        .map_err(|error| format!("invalid enrollment public key: {}", error))?;
    let public_key = RsaPublicKey::from_public_key_der(&public_key_der)
        .map_err(|error| format!("invalid enrollment RSA public key: {}", error))?;
    let ciphertext = public_key
        .encrypt(
            &mut OsRng,
            Oaep::new_with_mgf_hash::<Sha256, Sha1>(),
            vault_secret.expose_secret().as_bytes(),
        )
        .map_err(|error| format!("failed to encrypt vault unlock key for device: {}", error))?;

    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub web URL is not configured".to_string());
    }
    let token = crate::hub::auth::load_access_token(&hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    reqwest::Client::new()
        .post(format!(
            "{}/api/vault/enrollments/{}/approve",
            hub_url, enrollment.id
        ))
        .bearer_auth(token)
        .json(&VaultEnrollmentApproveRequest {
            encrypted_secret_base64: BASE64.encode(&ciphertext),
        })
        .send()
        .await
        .map_err(|error| format!("failed to approve vault enrollment: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub vault enrollment approval failed: {}", error))?
        .json::<VaultEnrollment>()
        .await
        .map_err(|error| format!("failed to parse vault enrollment approval: {}", error))
}
