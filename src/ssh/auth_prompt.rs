//! Keyboard-interactive authentication prompt types.
//!
//! Mirrors the host key verification flow: the async SSH auth task sends a
//! request over the event channel and blocks on a oneshot responder while the
//! UI shows a dialog with the server-provided prompts.

use secrecy::SecretString;
use tokio::sync::oneshot;

/// A single prompt within a keyboard-interactive info request.
#[derive(Debug, Clone)]
pub struct AuthPrompt {
    /// Prompt text supplied by the server (e.g. "Password: ", "OTP code: ").
    pub prompt: String,
    /// Whether the user's input may be echoed. `false` means mask the input.
    pub echo: bool,
}

/// Request to show keyboard-interactive prompts to the user.
pub struct AuthPromptRequest {
    /// Host being authenticated (for display).
    pub host: String,
    pub port: u16,
    pub username: String,
    /// Server-provided name for this prompt round (may be empty).
    pub name: String,
    /// Server-provided instructions (may be empty).
    pub instructions: String,
    /// The prompts for this round.
    pub prompts: Vec<AuthPrompt>,
    /// Channel used to deliver the user's responses back to the auth task.
    pub responder: oneshot::Sender<AuthPromptResponse>,
}

impl std::fmt::Debug for AuthPromptRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthPromptRequest")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("username", &self.username)
            .field("name", &self.name)
            .field("prompt_count", &self.prompts.len())
            .finish()
    }
}

/// User's response to a keyboard-interactive prompt round.
pub enum AuthPromptResponse {
    /// Responses, one per prompt, in order. Values are secrets and must
    /// never be logged or persisted.
    Submit(Vec<SecretString>),
    /// Abort the authentication (and the connection) cleanly.
    Cancel,
}

impl std::fmt::Debug for AuthPromptResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthPromptResponse::Submit(responses) => f
                .debug_tuple("Submit")
                .field(&format!("[{} responses redacted]", responses.len()))
                .finish(),
            AuthPromptResponse::Cancel => f.debug_struct("Cancel").finish(),
        }
    }
}

/// Reuse the existing masked credential dialog for non-terminal SSH work.
pub async fn request_secret(
    host: &crate::config::Host,
    events: &tokio::sync::mpsc::Sender<super::SshEvent>,
    title: &str,
    prompt: &str,
) -> Result<SecretString, crate::error::SshError> {
    let (responder, response) = oneshot::channel();
    let request = AuthPromptRequest {
        host: host.hostname.clone(),
        port: host.port,
        username: host.effective_username(),
        name: title.into(),
        instructions: "Used for this connection only. It is never saved.".into(),
        prompts: vec![AuthPrompt {
            prompt: prompt.into(),
            echo: false,
        }],
        responder,
    };
    let cancelled = || {
        crate::error::SshError::AuthenticationFailed(
            "Credential request cancelled or timed out".into(),
        )
    };
    events
        .send(super::SshEvent::AuthPrompt(Box::new(request)))
        .await
        .map_err(|_| cancelled())?;
    match tokio::time::timeout(std::time::Duration::from_secs(120), response).await {
        Ok(Ok(AuthPromptResponse::Submit(mut values))) if values.len() == 1 => Ok(values.remove(0)),
        _ => Err(cancelled()),
    }
}

pub async fn resolve_interactive(
    host: &crate::config::Host,
    events: &tokio::sync::mpsc::Sender<super::SshEvent>,
) -> Result<super::auth::ResolvedAuth, crate::error::SshError> {
    use super::auth::ResolvedAuth;
    use crate::config::AuthMethod;
    use crate::error::SshError;
    let password = if matches!(host.auth, AuthMethod::Password) {
        Some(request_secret(host, events, "SSH sign-in", "SSH password").await?)
    } else {
        None
    };
    match ResolvedAuth::resolve(&host.auth, password, None).await {
        Err(SshError::KeyFilePassphraseRequired(_))
        | Err(SshError::KeyFilePassphraseInvalid(_)) => {
            let passphrase =
                request_secret(host, events, "Unlock SSH key", "Key passphrase").await?;
            ResolvedAuth::resolve(&host.auth, None, Some(passphrase)).await
        }
        result => result,
    }
}
