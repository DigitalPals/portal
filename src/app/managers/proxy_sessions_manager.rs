use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::time::Instant;
use uuid::Uuid;

use crate::config::{HostsConfig, Protocol};
use crate::proxy::ListedProxySession;
use crate::views::terminal_view::TerminalSession;

pub struct ProxySessionCard {
    pub session_id: Uuid,
    pub host_id: Option<Uuid>,
    pub display_name: String,
    pub target_host: String,
    pub target_port: u16,
    pub target_user: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_output_at: Option<DateTime<Utc>>,
    pub preview: Vec<u8>,
    pub preview_truncated: bool,
    pub terminal: TerminalSession,
}

pub struct ProxySessionsState {
    pub sessions: Vec<ProxySessionCard>,
    pub loading: bool,
    pub error: Option<String>,
    pub last_loaded_at: Option<DateTime<Utc>>,
    pub refresh_generation: u64,
    pub kill_requested: Option<Uuid>,
}

impl ProxySessionsState {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            loading: false,
            error: None,
            last_loaded_at: None,
            refresh_generation: 0,
            kill_requested: None,
        }
    }

    pub fn start_loading(&mut self) {
        self.refresh_generation = self.refresh_generation.wrapping_add(1);
        self.loading = true;
        self.error = None;
    }

    pub fn set_error(&mut self, error: String) {
        self.loading = false;
        self.error = Some(error);
        self.kill_requested = None;
        self.last_loaded_at = Some(Utc::now());
    }

    /// Replace the session list while preserving preview terminal state for
    /// cards whose rendered preview has not changed. Returns true when the
    /// dashboard contents changed enough to justify fast follow-up polling.
    pub fn set_sessions(&mut self, sessions: Vec<ListedProxySession>, hosts: &HostsConfig) -> bool {
        let started = Instant::now();
        let previous_order: Vec<Uuid> = self
            .sessions
            .iter()
            .map(|session| session.session_id)
            .collect();
        let incoming_order: Vec<Uuid> = sessions.iter().map(|session| session.session_id).collect();
        let mut existing_by_id: HashMap<Uuid, ProxySessionCard> = self
            .sessions
            .drain(..)
            .map(|session| (session.session_id, session))
            .collect();
        let previous_count = existing_by_id.len();
        let incoming_count = sessions.len();
        let mut changed = previous_count != incoming_count || previous_order != incoming_order;
        let mut rebuilt_previews = 0usize;
        let mut reused_previews = 0usize;

        let mut next_sessions = Vec::with_capacity(incoming_count);
        for session in sessions {
            if let Some(mut existing) = existing_by_id.remove(&session.session_id) {
                if existing.preview_needs_rebuild(&session) {
                    rebuilt_previews += 1;
                    changed = true;
                    next_sessions.push(ProxySessionCard::from_listed(session, hosts));
                } else {
                    reused_previews += 1;
                    changed |= existing.update_metadata(session, hosts);
                    next_sessions.push(existing);
                }
            } else {
                rebuilt_previews += 1;
                changed = true;
                next_sessions.push(ProxySessionCard::from_listed(session, hosts));
            }
        }

        if !existing_by_id.is_empty() {
            changed = true;
        }

        self.sessions = next_sessions;
        self.loading = false;
        self.error = None;
        if self
            .kill_requested
            .is_some_and(|session_id| self.get(session_id).is_none())
        {
            self.kill_requested = None;
        }
        self.last_loaded_at = Some(Utc::now());
        tracing::debug!(
            sessions = self.sessions.len(),
            changed,
            rebuilt_previews,
            reused_previews,
            elapsed_ms = started.elapsed().as_millis(),
            "updated Portal Hub session cards"
        );
        changed
    }

    pub fn get(&self, session_id: Uuid) -> Option<&ProxySessionCard> {
        self.sessions
            .iter()
            .find(|session| session.session_id == session_id)
    }

    pub fn start_action(&mut self) {
        self.loading = true;
        self.error = None;
    }

    pub fn finish_action(&mut self) {
        self.loading = false;
        self.kill_requested = None;
    }
}

impl Default for ProxySessionsState {
    fn default() -> Self {
        Self::new()
    }
}

impl ProxySessionCard {
    fn from_listed(session: ListedProxySession, hosts: &HostsConfig) -> Self {
        let (display_name, host_id) = display_name_for_session(&session, hosts);
        let (terminal, _events) = TerminalSession::new(display_name.clone());
        if !session.preview.is_empty() {
            terminal.process_output(&session.preview);
        }

        Self {
            session_id: session.session_id,
            host_id,
            display_name,
            target_host: session.target_host,
            target_port: session.target_port,
            target_user: session.target_user,
            created_at: session.created_at,
            updated_at: session.updated_at,
            last_output_at: session.last_output_at,
            preview: session.preview,
            preview_truncated: session.preview_truncated,
            terminal,
        }
    }

    fn preview_needs_rebuild(&self, session: &ListedProxySession) -> bool {
        self.updated_at != session.updated_at
            || self.last_output_at != session.last_output_at
            || self.preview_truncated != session.preview_truncated
            || self.preview != session.preview
    }

    fn update_metadata(&mut self, session: ListedProxySession, hosts: &HostsConfig) -> bool {
        let (display_name, host_id) = display_name_for_session(&session, hosts);
        let changed = self.host_id != host_id
            || self.display_name != display_name
            || self.target_host != session.target_host
            || self.target_port != session.target_port
            || self.target_user != session.target_user
            || self.created_at != session.created_at
            || self.updated_at != session.updated_at
            || self.last_output_at != session.last_output_at
            || self.preview_truncated != session.preview_truncated;

        self.host_id = host_id;
        self.display_name = display_name;
        self.target_host = session.target_host;
        self.target_port = session.target_port;
        self.target_user = session.target_user;
        self.created_at = session.created_at;
        self.updated_at = session.updated_at;
        self.last_output_at = session.last_output_at;
        self.preview_truncated = session.preview_truncated;

        changed
    }
}

fn display_name_for_session(
    session: &ListedProxySession,
    hosts: &HostsConfig,
) -> (String, Option<Uuid>) {
    let matching_host = hosts.hosts.iter().find(|host| {
        host.protocol == Protocol::Ssh
            && host.hostname.trim() == session.target_host.trim()
            && host.port == session.target_port
            && host.effective_username() == session.target_user
    });

    if let Some(host) = matching_host {
        return (host.name.clone(), Some(host.id));
    }

    (
        format!(
            "{}@{}:{}",
            session.target_user, session.target_host, session.target_port
        ),
        None,
    )
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use std::sync::Arc;

    use super::*;
    use crate::config::hosts::HubRouting;
    use crate::config::{AuthMethod, Host};

    fn listed_session(
        target_host: &str,
        target_port: u16,
        target_user: &str,
    ) -> ListedProxySession {
        let now = Utc::now();
        ListedProxySession {
            session_id: Uuid::new_v4(),
            target_host: target_host.to_string(),
            target_port,
            target_user: target_user.to_string(),
            created_at: now,
            updated_at: now,
            last_output_at: Some(now),
            preview: b"shell output".to_vec(),
            preview_truncated: false,
        }
    }

    fn ssh_host(name: &str, hostname: &str, port: u16, username: &str) -> Host {
        let now = Utc::now();
        Host {
            id: Uuid::new_v4(),
            name: name.to_string(),
            hostname: hostname.to_string(),
            port,
            username: username.to_string(),
            protocol: Protocol::Ssh,
            vnc_port: None,
            vnc_password_id: None,
            vnc_via_ssh_host_id: None,
            allow_cleartext_vnc: false,
            auth: AuthMethod::Agent,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            hub_routing: HubRouting::Hub,
            jump_host_id: None,
            group_id: None,
            notes: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        }
    }

    #[test]
    fn session_card_uses_matching_host_name() {
        let host = ssh_host("Hermes", "192.0.2.206", 22, "john");
        let hosts = HostsConfig {
            hosts: vec![host.clone()],
            groups: Vec::new(),
        };
        let session = listed_session("192.0.2.206", 22, "john");

        let card = ProxySessionCard::from_listed(session, &hosts);

        assert_eq!(card.display_name, "Hermes");
        assert_eq!(card.host_id, Some(host.id));
    }

    #[test]
    fn session_card_falls_back_to_target_label_without_host_match() {
        let hosts = HostsConfig::default();
        let session = listed_session("192.0.2.206", 22, "john");

        let card = ProxySessionCard::from_listed(session, &hosts);

        assert_eq!(card.display_name, "john@192.0.2.206:22");
        assert_eq!(card.host_id, None);
    }

    #[test]
    fn session_card_preserves_preview_for_resume_seed() {
        let hosts = HostsConfig::default();
        let session = listed_session("192.0.2.206", 22, "john");

        let card = ProxySessionCard::from_listed(session, &hosts);

        assert_eq!(card.preview, b"shell output");
    }

    #[test]
    fn set_sessions_reuses_unchanged_preview_terminal() {
        let hosts = HostsConfig::default();
        let session = listed_session("192.0.2.206", 22, "john");
        let mut state = ProxySessionsState::new();

        assert!(state.set_sessions(vec![session.clone()], &hosts));
        let terminal_before = state.sessions[0].terminal.term();

        assert!(!state.set_sessions(vec![session], &hosts));
        let terminal_after = state.sessions[0].terminal.term();

        assert!(Arc::ptr_eq(&terminal_before, &terminal_after));
    }

    #[test]
    fn set_sessions_rebuilds_preview_when_output_changes() {
        let hosts = HostsConfig::default();
        let session = listed_session("192.0.2.206", 22, "john");
        let mut state = ProxySessionsState::new();

        assert!(state.set_sessions(vec![session.clone()], &hosts));
        let terminal_before = state.sessions[0].terminal.term();

        let mut changed_session = session;
        changed_session.last_output_at = changed_session
            .last_output_at
            .map(|time| time + chrono::Duration::seconds(1));
        changed_session.updated_at += chrono::Duration::seconds(1);
        changed_session.preview = b"new output".to_vec();

        assert!(state.set_sessions(vec![changed_session], &hosts));
        let terminal_after = state.sessions[0].terminal.term();

        assert!(!Arc::ptr_eq(&terminal_before, &terminal_after));
    }

    #[test]
    fn set_sessions_updates_display_name_without_rebuilding_preview() {
        let mut host = ssh_host("Hermes", "192.0.2.206", 22, "john");
        let mut hosts = HostsConfig {
            hosts: vec![host.clone()],
            groups: Vec::new(),
        };
        let session = listed_session("192.0.2.206", 22, "john");
        let mut state = ProxySessionsState::new();

        assert!(state.set_sessions(vec![session.clone()], &hosts));
        let terminal_before = state.sessions[0].terminal.term();

        host.name = "Hermes Renamed".to_string();
        hosts.hosts = vec![host];
        assert!(state.set_sessions(vec![session], &hosts));
        let terminal_after = state.sessions[0].terminal.term();

        assert_eq!(state.sessions[0].display_name, "Hermes Renamed");
        assert!(Arc::ptr_eq(&terminal_before, &terminal_after));
    }
}
