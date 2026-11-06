//! Isolated draft state. Network completion never publishes settings or tokens.
use std::collections::HashSet;
use uuid::Uuid;

use super::auth::PendingHubAuth;
use super::deployment::{ManagedHubState, OwnerSetupLink};
use crate::config::settings::PortalHubSettings;
use crate::proxy::ProxyStatus;

#[derive(Debug, Default)]
pub struct PortalHubWizardState {
    pub active: bool,
    pub generation: Uuid,
    pub draft: PortalHubSettings,
    pub hub_url_input: String,
    pub connection_dirty: bool,
    pub status: Option<ProxyStatus>,
    pub status_error: Option<String>,
    pub status_loading: bool,
    pub auth: Option<PendingHubAuth>,
    pub auth_user: Option<String>,
    /// Active account generation borrowed by this draft (no draft credentials).
    pub inherited_auth: Option<(Uuid, String)>,
    pub auth_error: Option<String>,
    pub auth_loading: bool,
    pub auth_abort: Option<futures::future::AbortHandle>,
    pub excluded_hosts: HashSet<Uuid>,
    pub route_default: bool,
    pub prefer_vault: bool,
    pub sync_all: bool,
    pub managed: ManagedHubState,
    /// The Hub this device was signed in to when setup opened.
    pub signed_in_url: Option<String>,
    /// Set when a one-click deployment reached ready: verify the endpoint,
    /// sign in and save the defaults without further clicks.
    pub auto_finish: Option<AutoFinish>,
}

#[derive(Debug, Clone, Default)]
pub struct AutoFinish {
    /// One-time link for creating the owner passkey; taken by the first
    /// sign-in attempt. None when the owner already has a passkey.
    pub setup: Option<OwnerSetupLink>,
}

impl PortalHubWizardState {
    pub fn begin(settings: PortalHubSettings) -> Self {
        Self {
            active: true,
            generation: Uuid::new_v4(),
            hub_url_input: settings.web_url_input_value(),
            route_default: settings.default_for_new_ssh_hosts,
            prefer_vault: settings.prefer_vault_keys,
            sync_all: settings.hosts_sync_enabled
                && settings.settings_sync_enabled
                && settings.snippets_sync_enabled
                && settings.key_vault_enabled,
            draft: settings,
            ..Self::default()
        }
    }

    pub fn accepts(&self, generation: Uuid) -> bool {
        self.active && self.generation == generation
    }

    pub fn edit_endpoint(&mut self, value: String) {
        if let Some(abort) = self.auth_abort.take() {
            abort.abort();
        }
        self.generation = Uuid::new_v4();
        self.hub_url_input = value;
        self.connection_dirty = true;
        self.status = None;
        self.status_error = None;
        self.status_loading = false;
        self.auth = None;
        self.auth_user = None;
        self.inherited_auth = None;
        self.auth_error = None;
        self.auth_loading = false;
    }

    pub fn checked_settings(&mut self) -> Result<PortalHubSettings, String> {
        let url = self.draft.normalize_web_url_input(&self.hub_url_input)?;
        self.draft.apply_discovered_web_url(&url);
        self.hub_url_input = url;
        self.connection_dirty = false;
        Ok(self.draft.clone())
    }

    pub fn cancel(&mut self) {
        if let Some(abort) = self.auth_abort.take() {
            abort.abort();
        }
        self.managed.cancel();
        *self = Self::default();
    }

    pub fn receive_status(
        &mut self,
        generation: Uuid,
        result: Result<ProxyStatus, String>,
    ) -> bool {
        if !self.accepts(generation) {
            return false;
        }
        self.status_loading = false;
        match result {
            Ok(status) => {
                self.status = Some(status);
                self.status_error = None;
            }
            Err(error) => {
                self.status = None;
                self.status_error = Some(error);
            }
        }
        true
    }

    /// Receiving credentials changes only the draft. This method has no keychain
    /// or sync side effects, even when authenticating the currently active URL.
    pub fn receive_auth(&mut self, generation: Uuid, auth: PendingHubAuth) -> bool {
        if !self.accepts(generation) || auth.summary.hub_url != self.draft.effective_web_url() {
            return false;
        }
        self.auth_loading = false;
        self.auth_abort = None;
        self.auth_user = Some(format!(
            "{} @ {}",
            auth.summary.username, auth.summary.hub_url
        ));
        self.auth = Some(auth);
        self.inherited_auth = None;
        self.auth_error = None;
        true
    }

    pub fn inherit_auth(&mut self, generation: Uuid, user: Option<String>) {
        self.inherited_auth = user
            .as_ref()
            .map(|_| (generation, self.draft.effective_web_url()));
        self.auth_user = user;
    }

    pub fn invalidate_inherited_auth(&mut self, generation: Uuid, endpoint: &str) {
        if self
            .inherited_auth
            .as_ref()
            .is_some_and(|(id, url)| *id == generation && url == endpoint)
        {
            self.inherited_auth = None;
            self.auth_user = None;
            self.auth_error =
                Some("This account was signed out. Sign in again before saving.".into());
        }
    }

    pub fn can_finish(&self) -> bool {
        self.active
            && !self.connection_dirty
            && self.status.is_some()
            && self.auth_user.is_some()
            && !self.auth_loading
            && !self.status_loading
            && !self.managed.busy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checking_a_draft_does_not_change_the_working_endpoint_or_preferences() {
        let mut settings = PortalHubSettings::default();
        settings.apply_discovered_web_url("https://old.example");
        settings.hosts_sync_enabled = false;
        let mut draft = PortalHubWizardState::begin(settings.clone());
        draft.edit_endpoint("https://new.example".into());
        let checked = draft.checked_settings().unwrap();
        assert_eq!(checked.effective_web_url(), "https://new.example");
        assert_eq!(settings.effective_web_url(), "https://old.example");
        assert!(!checked.hosts_sync_enabled);
        assert!(!draft.can_finish());
    }

    #[test]
    fn editing_cancelling_and_reopening_invalidate_old_results() {
        let mut draft = PortalHubWizardState::begin(PortalHubSettings::default());
        let original = draft.generation;
        draft.edit_endpoint("https://new.example".into());
        assert!(!draft.accepts(original));
        let edited = draft.generation;
        draft.cancel();
        assert!(!draft.accepts(edited));
        draft = PortalHubWizardState::begin(PortalHubSettings::default());
        assert!(!draft.accepts(edited));
    }
    #[test]
    fn authenticated_draft_does_not_enable_sync_or_publish_tokens() {
        let mut current = PortalHubSettings::default();
        current.apply_discovered_web_url("https://working.example");
        current.hosts_sync_enabled = false;
        current.settings_sync_enabled = false;
        current.snippets_sync_enabled = false;
        current.key_vault_enabled = false;
        let mut draft = PortalHubWizardState::begin(current.clone());
        draft.edit_endpoint("https://new.example".into());
        draft.checked_settings().unwrap();
        draft.auth_loading = true;
        assert!(draft.receive_auth(
            draft.generation,
            PendingHubAuth::test_auth("https://new.example")
        ));
        assert!(!current.sync_configured());
        assert!(!draft.draft.sync_configured());
        assert_eq!(current.effective_web_url(), "https://working.example");
        assert!(!draft.can_finish()); // Authentication alone is insufficient.
        assert!(!format!("{draft:?}").contains("test-access-secret"));
        draft.cancel();
        assert!(draft.auth.is_none());
        assert_eq!(current.effective_web_url(), "https://working.example");
    }

    #[test]
    fn cancelled_and_wrong_origin_authentication_cannot_reactivate_setup() {
        let mut draft = PortalHubWizardState::begin(PortalHubSettings::default());
        draft.edit_endpoint("https://new.example".into());
        draft.checked_settings().unwrap();
        let generation = draft.generation;
        assert!(!draft.receive_auth(
            generation,
            PendingHubAuth::test_auth("https://other.example")
        ));
        draft.cancel();
        assert!(!draft.receive_auth(generation, PendingHubAuth::test_auth("https://new.example")));
        assert!(!draft.receive_status(generation, Err("stale failure".into())));
        assert!(draft.auth.is_none());
        assert!(draft.status_error.is_none());
    }

    #[tokio::test]
    async fn cancellation_aborts_the_oauth_future() {
        let mut draft = PortalHubWizardState::begin(PortalHubSettings::default());
        let (abort, registration) = futures::future::AbortHandle::new_pair();
        draft.auth_abort = Some(abort);
        draft.cancel();
        assert!(
            futures::future::Abortable::new(std::future::pending::<()>(), registration)
                .await
                .is_err()
        );
    }
}
