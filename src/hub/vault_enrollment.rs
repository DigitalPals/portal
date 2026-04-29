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
    pub pairing_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub approved_at: Option<String>,
    pub revoked_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct VaultEnrollmentListResponse {
    enrollments: Vec<VaultEnrollment>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VaultAuditEvent {
    pub timestamp: String,
    pub event: String,
    pub detail: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct VaultAuditResponse {
    events: Vec<VaultAuditEvent>,
}

#[derive(Debug, Serialize)]
struct VaultEnrollmentApproveRequest {
    encrypted_secret_base64: String,
}

pub async fn list_all(settings: &PortalHubSettings) -> Result<Vec<VaultEnrollment>, String> {
    list_with_status(settings, "all").await
}

async fn list_with_status(
    settings: &PortalHubSettings,
    status: &str,
) -> Result<Vec<VaultEnrollment>, String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub web URL is not configured".to_string());
    }
    let token = crate::hub::auth::load_access_token(&hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    reqwest::Client::new()
        .get(format!(
            "{}/api/vault/enrollments?status={}",
            hub_url, status
        ))
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

pub async fn list_audit(settings: &PortalHubSettings) -> Result<Vec<VaultAuditEvent>, String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub web URL is not configured".to_string());
    }
    let token = crate::hub::auth::load_access_token(&hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    reqwest::Client::new()
        .get(format!("{}/api/vault/enrollments/audit?limit=20", hub_url))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("failed to list vault enrollment audit events: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub vault audit request failed: {}", error))?
        .json::<VaultAuditResponse>()
        .await
        .map(|response| response.events)
        .map_err(|error| format!("failed to parse vault audit events: {}", error))
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

pub async fn revoke(
    settings: PortalHubSettings,
    enrollment: VaultEnrollment,
) -> Result<VaultEnrollment, String> {
    let hub_url = settings.effective_web_url();
    if hub_url.is_empty() {
        return Err("Portal Hub web URL is not configured".to_string());
    }
    let token = crate::hub::auth::load_access_token(&hub_url)?
        .ok_or_else(|| "Portal Hub is not authenticated".to_string())?;
    reqwest::Client::new()
        .post(format!(
            "{}/api/vault/enrollments/{}/revoke",
            hub_url, enrollment.id
        ))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("failed to revoke vault enrollment: {}", error))?
        .error_for_status()
        .map_err(|error| format!("Portal Hub vault enrollment revocation failed: {}", error))?
        .json::<VaultEnrollment>()
        .await
        .map_err(|error| format!("failed to parse vault enrollment revocation: {}", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn portal_hub_vault_enrollment_response_matches_contract() {
        let instance = json!({
            "id": "00000000-0000-0000-0000-000000000001",
            "device_name": "Pixel",
            "public_key_algorithm": "RSA-OAEP-SHA256",
            "public_key_der_base64": "MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8A",
            "status": "pending",
            "encrypted_secret_base64": null,
            "pairing_id": "00000000-0000-0000-0000-000000000010",
            "created_at": "2026-04-29T12:00:00Z",
            "updated_at": "2026-04-29T12:00:00Z",
            "approved_at": null,
            "revoked_at": null
        });

        crate::contract_test_support::assert_portal_hub_contract(
            "vault-enrollment-response",
            &instance,
        );
        let enrollment: VaultEnrollment = serde_json::from_value(instance).unwrap();

        assert_eq!(enrollment.id, "00000000-0000-0000-0000-000000000001");
        assert_eq!(enrollment.public_key_algorithm, "RSA-OAEP-SHA256");
        assert_eq!(enrollment.status, "pending");
        assert!(enrollment.encrypted_secret_base64.is_none());
        assert!(enrollment.revoked_at.is_none());
    }

    #[test]
    fn portal_hub_vault_enrollment_approve_request_matches_contract() {
        let instance = serde_json::to_value(VaultEnrollmentApproveRequest {
            encrypted_secret_base64: "c2VjcmV0".to_string(),
        })
        .unwrap();

        crate::contract_test_support::assert_portal_hub_contract(
            "vault-enrollment-approve-request",
            &instance,
        );
    }
}
