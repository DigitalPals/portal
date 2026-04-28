use chrono::{DateTime, Utc};
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

    pub fn set_sessions(&mut self, sessions: Vec<ListedProxySession>, hosts: &HostsConfig) {
        self.sessions = sessions
            .into_iter()
            .map(|session| ProxySessionCard::from_listed(session, hosts))
            .collect();
        self.loading = false;
        self.error = None;
        if self
            .kill_requested
            .is_some_and(|session_id| self.get(session_id).is_none())
        {
            self.kill_requested = None;
        }
        self.last_loaded_at = Some(Utc::now());
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
            preview_truncated: session.preview_truncated,
            terminal,
        }
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

    use super::*;
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
            auth: AuthMethod::Agent,
            agent_forwarding: false,
            port_forwards: Vec::new(),
            portal_hub_enabled: true,
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
}
