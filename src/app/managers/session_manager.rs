//! Session manager for SSH sessions
//!
//! Manages the lifecycle of SSH terminal sessions, including
//! tracking active sessions, their terminals, and status messages.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::message::SessionId;
use crate::ssh::SshSession;
use crate::views::terminal_view::TerminalSession;

/// Active SSH session with its terminal
pub struct ActiveSession {
    pub ssh_session: Arc<SshSession>,
    pub terminal: TerminalSession,
    pub session_start: Instant,
    pub host_name: String,
    pub history_entry_id: Uuid,
    /// Transient status message (message, shown_at) - auto-expires after 3 seconds
    pub status_message: Option<(String, Instant)>,
}

/// Manages SSH terminal sessions
pub struct SessionManager {
    sessions: HashMap<SessionId, ActiveSession>,
}

impl SessionManager {
    /// Create a new empty session manager
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    /// Get a reference to a session by ID
    pub fn get(&self, id: SessionId) -> Option<&ActiveSession> {
        self.sessions.get(&id)
    }

    /// Get a mutable reference to a session by ID
    pub fn get_mut(&mut self, id: SessionId) -> Option<&mut ActiveSession> {
        self.sessions.get_mut(&id)
    }

    /// Insert a new session
    pub fn insert(&mut self, id: SessionId, session: ActiveSession) {
        self.sessions.insert(id, session);
    }

    /// Remove a session by ID, returning it if it existed
    pub fn remove(&mut self, id: SessionId) -> Option<ActiveSession> {
        self.sessions.remove(&id)
    }

    /// Check if a session exists
    pub fn contains(&self, id: SessionId) -> bool {
        self.sessions.contains_key(&id)
    }

    /// Check if there are any active sessions
    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    /// Get the number of active sessions
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Iterate over all sessions
    #[allow(dead_code)]
    pub fn iter(&self) -> impl Iterator<Item = (&SessionId, &ActiveSession)> {
        self.sessions.iter()
    }

    /// Iterate over all sessions mutably
    #[allow(dead_code)]
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&SessionId, &mut ActiveSession)> {
        self.sessions.iter_mut()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
