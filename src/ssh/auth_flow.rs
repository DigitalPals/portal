//! SSH authentication with keyboard-interactive support and a fallback chain.
//!
//! The configured method is tried first; when the server rejects it (or the
//! method is unavailable), the remaining applicable methods are tried in
//! order: publickey/agent (if configured) -> keyboard-interactive -> password
//! (only when a password was already collected). The server-advertised
//! `remaining_methods` list from auth failures is honored when deciding the
//! next method.

use std::time::Duration;

use russh::client::{AuthResult, Handle, Handler, KeyboardInteractiveAuthResponse};
use russh::keys::HashAlg;
use russh::{MethodKind, MethodSet};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::{mpsc, oneshot};

use crate::error::SshError;
use crate::security_log;

use super::SshEvent;
use super::auth::ResolvedAuth;
use super::auth_prompt::{AuthPrompt, AuthPromptRequest, AuthPromptResponse};

/// How long to wait for the user to answer a keyboard-interactive prompt
/// round (consistent with the host key verification dialog).
const AUTH_PROMPT_TIMEOUT: Duration = Duration::from_secs(60);

/// Authentication methods Portal can attempt, in fallback order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    PublicKey,
    Agent,
    KeyboardInteractive,
    Password,
}

impl AuthKind {
    pub fn method_name(self) -> &'static str {
        match self {
            AuthKind::PublicKey => "publickey",
            AuthKind::Agent => "agent",
            AuthKind::KeyboardInteractive => "keyboard-interactive",
            AuthKind::Password => "password",
        }
    }

    /// The SSH wire-level method this maps to (agent auth is publickey).
    fn wire_kind(self) -> MethodKind {
        match self {
            AuthKind::PublicKey | AuthKind::Agent => MethodKind::PublicKey,
            AuthKind::KeyboardInteractive => MethodKind::KeyboardInteractive,
            AuthKind::Password => MethodKind::Password,
        }
    }
}

/// Build the ordered list of methods to attempt for a connection.
///
/// `primary` is the configured method; `has_password` indicates whether a
/// password was already collected (password prompting happens before the
/// connection starts, so a password fallback is only possible when one is
/// available). Keyboard-interactive is always appended as a fallback because
/// its prompts come from the server at auth time.
pub fn auth_fallback_chain(primary: AuthKind, has_password: bool) -> Vec<AuthKind> {
    let mut chain = Vec::with_capacity(3);
    if primary != AuthKind::Password || has_password {
        chain.push(primary);
    }
    if !chain.contains(&AuthKind::KeyboardInteractive) {
        chain.push(AuthKind::KeyboardInteractive);
    }
    if has_password && !chain.contains(&AuthKind::Password) {
        chain.push(AuthKind::Password);
    }
    chain
}

/// Decide the next method to attempt.
///
/// Returns the first entry of `chain` that has not been attempted yet and is
/// allowed by the server's advertised `remaining_methods` (when known).
pub fn next_auth_method(
    chain: &[AuthKind],
    attempted: &[AuthKind],
    remaining: Option<&MethodSet>,
) -> Option<AuthKind> {
    chain
        .iter()
        .copied()
        .find(|kind| {
            if attempted.contains(kind) {
                return false;
            }
            match remaining {
                Some(methods) => methods.iter().any(|m| *m == kind.wire_kind()),
                None => true,
            }
        })
}

/// Context shared by all auth attempts for one connection.
pub struct AuthContext<'a> {
    pub hostname: &'a str,
    pub port: u16,
    pub username: &'a str,
    pub event_tx: &'a mpsc::Sender<SshEvent>,
}

enum AttemptOutcome {
    Success,
    /// Rejected by the server; carries the advertised remaining methods.
    Rejected(Option<MethodSet>),
    /// The method could not be attempted (e.g. no agent available). The
    /// server's view of remaining methods is unchanged.
    Unavailable(String),
}

/// Authenticate using the resolved primary method with automatic fallback.
///
/// `password` is the pre-collected password (if any) used both for the
/// primary password attempt and as a fallback after other methods fail.
pub async fn authenticate<H: Handler>(
    handle: &mut Handle<H>,
    ctx: AuthContext<'_>,
    primary: ResolvedAuth,
) -> Result<(), SshError> {
    let (primary_kind, mut key, password) = match primary {
        ResolvedAuth::Password(password) => (AuthKind::Password, None, Some(password)),
        ResolvedAuth::PublicKey(key) => (AuthKind::PublicKey, Some(key), None),
        ResolvedAuth::Agent => (AuthKind::Agent, None, None),
        ResolvedAuth::KeyboardInteractive => (AuthKind::KeyboardInteractive, None, None),
    };

    let chain = auth_fallback_chain(primary_kind, password.is_some());
    let mut attempted: Vec<AuthKind> = Vec::new();
    let mut remaining: Option<MethodSet> = None;
    let mut last_reason = String::from("Authentication rejected by server");

    while let Some(kind) = next_auth_method(&chain, &attempted, remaining.as_ref()) {
        attempted.push(kind);
        security_log::log_auth_attempt(ctx.hostname, ctx.port, ctx.username, kind.method_name());

        let outcome = match kind {
            AuthKind::Password => {
                let Some(password) = password.as_ref() else {
                    continue;
                };
                match handle
                    .authenticate_password(ctx.username, password.expose_secret())
                    .await
                {
                    Ok(AuthResult::Success) => AttemptOutcome::Success,
                    Ok(AuthResult::Failure {
                        remaining_methods, ..
                    }) => AttemptOutcome::Rejected(Some(remaining_methods)),
                    Err(e) => return Err(SshError::AuthenticationFailed(e.to_string())),
                }
            }
            AuthKind::PublicKey => {
                let Some(key) = key.take() else {
                    continue;
                };
                match handle.authenticate_publickey(ctx.username, key).await {
                    Ok(AuthResult::Success) => AttemptOutcome::Success,
                    Ok(AuthResult::Failure {
                        remaining_methods, ..
                    }) => AttemptOutcome::Rejected(Some(remaining_methods)),
                    Err(e) => return Err(SshError::AuthenticationFailed(e.to_string())),
                }
            }
            AuthKind::Agent => match authenticate_with_agent(handle, ctx.username).await {
                Ok(AuthResult::Success) => AttemptOutcome::Success,
                Ok(AuthResult::Failure {
                    remaining_methods, ..
                }) => AttemptOutcome::Rejected(Some(remaining_methods)),
                Err(e) => AttemptOutcome::Unavailable(e.to_string()),
            },
            AuthKind::KeyboardInteractive => {
                match authenticate_keyboard_interactive(handle, &ctx).await? {
                    KbdInteractiveOutcome::Success => AttemptOutcome::Success,
                    KbdInteractiveOutcome::Rejected(methods) => {
                        AttemptOutcome::Rejected(Some(methods))
                    }
                }
            }
        };

        match outcome {
            AttemptOutcome::Success => {
                security_log::log_auth_success(
                    ctx.hostname,
                    ctx.port,
                    ctx.username,
                    kind.method_name(),
                );
                return Ok(());
            }
            AttemptOutcome::Rejected(methods) => {
                last_reason = format!(
                    "Authentication rejected by server ({} auth)",
                    kind.method_name()
                );
                security_log::log_auth_failure(
                    ctx.hostname,
                    ctx.port,
                    ctx.username,
                    kind.method_name(),
                    &last_reason,
                );
                if methods.is_some() {
                    remaining = methods;
                }
            }
            AttemptOutcome::Unavailable(reason) => {
                last_reason = reason.clone();
                security_log::log_auth_failure(
                    ctx.hostname,
                    ctx.port,
                    ctx.username,
                    kind.method_name(),
                    &reason,
                );
                tracing::debug!(
                    "Auth method {} unavailable for {}:{}: {}",
                    kind.method_name(),
                    ctx.hostname,
                    ctx.port,
                    reason
                );
            }
        }
    }

    Err(SshError::AuthenticationFailed(last_reason))
}

enum KbdInteractiveOutcome {
    Success,
    Rejected(MethodSet),
}

/// Run the keyboard-interactive exchange, surfacing server prompts to the UI.
///
/// The server may send multiple rounds of prompts; each round with prompts
/// opens a dialog and waits (bounded) for the user's responses. Rounds with
/// no prompts are answered automatically with an empty response list.
async fn authenticate_keyboard_interactive<H: Handler>(
    handle: &mut Handle<H>,
    ctx: &AuthContext<'_>,
) -> Result<KbdInteractiveOutcome, SshError> {
    let mut response = handle
        .authenticate_keyboard_interactive_start(ctx.username, None)
        .await
        .map_err(|e| SshError::AuthenticationFailed(e.to_string()))?;

    loop {
        match response {
            KeyboardInteractiveAuthResponse::Success => {
                return Ok(KbdInteractiveOutcome::Success);
            }
            KeyboardInteractiveAuthResponse::Failure {
                remaining_methods, ..
            } => {
                return Ok(KbdInteractiveOutcome::Rejected(remaining_methods));
            }
            KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } => {
                let responses: Vec<String> = if prompts.is_empty() {
                    Vec::new()
                } else {
                    let answers = request_prompt_responses(ctx, name, instructions, &prompts)
                        .await?;
                    // Expose secrets only at the point of sending to the server.
                    answers
                        .iter()
                        .map(|secret| secret.expose_secret().to_string())
                        .collect()
                };

                response = handle
                    .authenticate_keyboard_interactive_respond(responses)
                    .await
                    .map_err(|e| SshError::AuthenticationFailed(e.to_string()))?;
            }
        }
    }
}

/// Ask the UI for responses to one round of keyboard-interactive prompts.
async fn request_prompt_responses(
    ctx: &AuthContext<'_>,
    name: String,
    instructions: String,
    prompts: &[russh::client::Prompt],
) -> Result<Vec<SecretString>, SshError> {
    let (tx, rx) = oneshot::channel();
    let request = AuthPromptRequest {
        host: ctx.hostname.to_string(),
        port: ctx.port,
        username: ctx.username.to_string(),
        name,
        instructions,
        prompts: prompts
            .iter()
            .map(|p| AuthPrompt {
                prompt: p.prompt.clone(),
                echo: p.echo,
            })
            .collect(),
        responder: tx,
    };

    ctx.event_tx
        .send(SshEvent::AuthPrompt(Box::new(request)))
        .await
        .map_err(|_| {
            SshError::AuthenticationFailed("Failed to request authentication input".to_string())
        })?;

    match tokio::time::timeout(AUTH_PROMPT_TIMEOUT, rx).await {
        Ok(Ok(AuthPromptResponse::Submit(responses))) => {
            if responses.len() != prompts.len() {
                return Err(SshError::AuthenticationFailed(
                    "Authentication prompt response count mismatch".to_string(),
                ));
            }
            Ok(responses)
        }
        Ok(Ok(AuthPromptResponse::Cancel)) | Ok(Err(_)) => Err(SshError::AuthenticationFailed(
            "Authentication cancelled by user".to_string(),
        )),
        Err(_) => Err(SshError::AuthenticationFailed(
            "Authentication prompt timed out".to_string(),
        )),
    }
}

/// Try every identity offered by the local SSH agent.
pub async fn authenticate_with_agent<H: Handler>(
    handle: &mut Handle<H>,
    username: &str,
) -> Result<AuthResult, SshError> {
    let agent_path = std::env::var("SSH_AUTH_SOCK").map_err(|_| {
        SshError::Agent("SSH_AUTH_SOCK not set - is ssh-agent running?".to_string())
    })?;

    let stream = tokio::net::UnixStream::connect(&agent_path)
        .await
        .map_err(|e| SshError::Agent(format!("Failed to connect to SSH agent: {}", e)))?;

    let mut agent = russh::keys::agent::client::AgentClient::connect(stream);

    let identities = agent
        .request_identities()
        .await
        .map_err(|e| SshError::Agent(format!("Failed to get identities: {}", e)))?;

    if identities.is_empty() {
        return Err(SshError::Agent(
            "No identities found in SSH agent".to_string(),
        ));
    }

    let mut last_failure: Option<AuthResult> = None;

    // Try each identity with SHA-512 for RSA keys
    for identity in identities {
        let public_key = identity.public_key().into_owned();
        let hash_alg = if public_key.algorithm().is_rsa() {
            Some(HashAlg::Sha512)
        } else {
            None
        };

        match handle
            .authenticate_publickey_with(username, public_key, hash_alg, &mut agent)
            .await
        {
            Ok(result) if result.success() => return Ok(result),
            Ok(result) => {
                last_failure = Some(result);
                continue;
            }
            Err(e) => {
                tracing::debug!("Agent key failed: {}", e);
                continue;
            }
        }
    }

    // Report the server's remaining-methods hint when available so the
    // fallback chain can honor it.
    match last_failure {
        Some(result) => Ok(result),
        None => Err(SshError::Agent(
            "No agent key accepted by server".to_string(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn methods(kinds: &[MethodKind]) -> MethodSet {
        MethodSet::from(kinds)
    }

    #[test]
    fn chain_for_password_host_tries_password_then_keyboard_interactive() {
        assert_eq!(
            auth_fallback_chain(AuthKind::Password, true),
            vec![AuthKind::Password, AuthKind::KeyboardInteractive]
        );
    }

    #[test]
    fn chain_for_password_host_without_password_skips_password() {
        assert_eq!(
            auth_fallback_chain(AuthKind::Password, false),
            vec![AuthKind::KeyboardInteractive]
        );
    }

    #[test]
    fn chain_for_public_key_host_falls_back_to_keyboard_interactive() {
        assert_eq!(
            auth_fallback_chain(AuthKind::PublicKey, false),
            vec![AuthKind::PublicKey, AuthKind::KeyboardInteractive]
        );
    }

    #[test]
    fn chain_for_agent_host_falls_back_to_keyboard_interactive() {
        assert_eq!(
            auth_fallback_chain(AuthKind::Agent, false),
            vec![AuthKind::Agent, AuthKind::KeyboardInteractive]
        );
    }

    #[test]
    fn chain_for_keyboard_interactive_host() {
        assert_eq!(
            auth_fallback_chain(AuthKind::KeyboardInteractive, false),
            vec![AuthKind::KeyboardInteractive]
        );
    }

    #[test]
    fn chain_appends_password_fallback_when_password_available() {
        assert_eq!(
            auth_fallback_chain(AuthKind::PublicKey, true),
            vec![
                AuthKind::PublicKey,
                AuthKind::KeyboardInteractive,
                AuthKind::Password
            ]
        );
    }

    #[test]
    fn next_method_without_server_hint_takes_chain_order() {
        let chain = auth_fallback_chain(AuthKind::Agent, false);
        assert_eq!(
            next_auth_method(&chain, &[], None),
            Some(AuthKind::Agent)
        );
        assert_eq!(
            next_auth_method(&chain, &[AuthKind::Agent], None),
            Some(AuthKind::KeyboardInteractive)
        );
        assert_eq!(
            next_auth_method(
                &chain,
                &[AuthKind::Agent, AuthKind::KeyboardInteractive],
                None
            ),
            None
        );
    }

    #[test]
    fn next_method_honors_server_remaining_methods() {
        // Password rejected; server advertises only keyboard-interactive.
        let chain = auth_fallback_chain(AuthKind::Password, true);
        let remaining = methods(&[MethodKind::KeyboardInteractive]);
        assert_eq!(
            next_auth_method(&chain, &[AuthKind::Password], Some(&remaining)),
            Some(AuthKind::KeyboardInteractive)
        );
    }

    #[test]
    fn next_method_skips_methods_not_advertised() {
        // Public key rejected; server only allows password. No password was
        // collected, so nothing remains.
        let chain = auth_fallback_chain(AuthKind::PublicKey, false);
        let remaining = methods(&[MethodKind::Password]);
        assert_eq!(
            next_auth_method(&chain, &[AuthKind::PublicKey], Some(&remaining)),
            None
        );
    }

    #[test]
    fn next_method_allows_password_fallback_when_advertised() {
        let chain = auth_fallback_chain(AuthKind::PublicKey, true);
        let remaining = methods(&[MethodKind::Password]);
        assert_eq!(
            next_auth_method(&chain, &[AuthKind::PublicKey], Some(&remaining)),
            Some(AuthKind::Password)
        );
    }

    #[test]
    fn agent_and_publickey_share_wire_method() {
        let chain = auth_fallback_chain(AuthKind::Agent, false);
        let remaining = methods(&[MethodKind::PublicKey]);
        // Agent maps to the publickey wire method, so it stays eligible.
        assert_eq!(
            next_auth_method(&chain, &[], Some(&remaining)),
            Some(AuthKind::Agent)
        );
    }
}
