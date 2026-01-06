//! Session manager for terminal sessions
//!
//! Manages the lifecycle of terminal sessions (both SSH and local),
//! including tracking active sessions, their terminals, and status messages.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use uuid::Uuid;

use crate::local::LocalSession;
use crate::message::SessionId;
use crate::ssh::SshSession;
use crate::views::terminal_view::TerminalSession;

/// Backend type for a terminal session
pub enum SessionBackend {
    /// SSH connection to a remote host
    Ssh(Arc<SshSession>),
    /// Local PTY session
    Local(Arc<LocalSession>),
}

/// Active terminal session with its backend
pub struct ActiveSession {
    pub backend: SessionBackend,
    pub terminal: TerminalSession,
    pub session_start: Instant,
    pub host_name: String,
    pub history_entry_id: Uuid,
    /// Transient status message (message, shown_at) - auto-expires after 3 seconds
    pub status_message: Option<(String, Instant)>,
    /// Buffered output to process in small chunks for UI responsiveness
    pub pending_output: VecDeque<Vec<u8>>,
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

    /// Check if any session has pending output to process
    pub fn has_pending_output(&self) -> bool {
        self.sessions
            .values()
            .any(|session| !session.pending_output.is_empty())
    }

    /// Get mutable iterator over all sessions
    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut ActiveSession> {
        self.sessions.values_mut()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
