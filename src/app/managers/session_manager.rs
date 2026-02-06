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
use crate::message::{QualityLevel, VncScreen};
use crate::ssh::SshSession;
use crate::views::terminal_view::TerminalSession;
use crate::vnc::VncSession;

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
    pub host_id: Option<Uuid>,
    pub history_entry_id: Uuid,
    /// Transient status message (message, shown_at) - auto-expires after 3 seconds
    pub status_message: Option<(String, Instant)>,
    /// Number of reconnect attempts made for this session
    pub reconnect_attempts: u32,
    /// Next scheduled reconnect attempt time (if any)
    pub reconnect_next_attempt: Option<Instant>,
    /// Buffered output to process in small chunks for UI responsiveness
    pub pending_output: VecDeque<Vec<u8>>,
}

/// Active VNC session
pub struct VncActiveSession {
    pub session: Arc<VncSession>,
    pub host_name: String,
    pub session_start: Instant,
    /// Frame counter for FPS calculation
    pub frame_count: u32,
    /// Last time FPS was calculated
    pub fps_last_check: Instant,
    /// Current estimated FPS
    pub current_fps: f32,
    /// Whether fullscreen mode is active
    pub fullscreen: bool,
    /// Whether keyboard passthrough is active (all keys go to VNC)
    pub keyboard_passthrough: bool,
    /// Current adaptive quality level
    pub quality_level: QualityLevel,
    /// Discovered remote monitors
    pub monitors: Vec<VncScreen>,
    /// Currently selected monitor (None = full desktop)
    pub selected_monitor: Option<usize>,
    /// History entry ID for marking disconnection
    pub history_entry_id: uuid::Uuid,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::terminal_view::TerminalSession;

    /// Helper to create a test ActiveSession
    fn create_test_session(host_name: &str) -> ActiveSession {
        let (terminal, _rx) = TerminalSession::new(host_name);
        ActiveSession {
            backend: SessionBackend::Local(Arc::new(LocalSession::new_test_stub())),
            terminal,
            session_start: Instant::now(),
            host_name: host_name.to_string(),
            host_id: None,
            history_entry_id: Uuid::new_v4(),
            status_message: None,
            reconnect_attempts: 0,
            reconnect_next_attempt: None,
            pending_output: VecDeque::new(),
        }
    }

    // ---- Basic manager tests ----

    #[test]
    fn test_new_manager_is_empty() {
        let manager = SessionManager::new();
        assert!(manager.is_empty());
    }

    #[test]
    fn test_default_creates_empty_manager() {
        let manager = SessionManager::default();
        assert!(manager.is_empty());
        assert!(!manager.has_pending_output());
    }

    // ---- Session CRUD tests ----

    #[test]
    fn test_insert_and_get_session() {
        let mut manager = SessionManager::new();
        let session_id = Uuid::new_v4();
        let session = create_test_session("test-host");

        manager.insert(session_id, session);

        assert!(!manager.is_empty());
        assert!(manager.contains(session_id));
        assert!(manager.get(session_id).is_some());
        assert_eq!(manager.get(session_id).unwrap().host_name, "test-host");
    }

    #[test]
    fn test_get_returns_none_for_unknown_id() {
        let manager = SessionManager::new();
        let random_id = Uuid::new_v4();
        assert!(manager.get(random_id).is_none());
    }

    #[test]
    fn test_get_mut_allows_modification() {
        let mut manager = SessionManager::new();
        let session_id = Uuid::new_v4();
        let session = create_test_session("original");

        manager.insert(session_id, session);

        if let Some(session) = manager.get_mut(session_id) {
            session.host_name = "modified".to_string();
        }

        assert_eq!(manager.get(session_id).unwrap().host_name, "modified");
    }

    #[test]
    fn test_contains_returns_false_for_unknown_id() {
        let manager = SessionManager::new();
        let random_id = Uuid::new_v4();
        assert!(!manager.contains(random_id));
    }

    #[test]
    fn test_contains_returns_true_for_existing() {
        let mut manager = SessionManager::new();
        let session_id = Uuid::new_v4();

        manager.insert(session_id, create_test_session("test"));

        assert!(manager.contains(session_id));
    }

    #[test]
    fn test_remove_returns_session() {
        let mut manager = SessionManager::new();
        let session_id = Uuid::new_v4();

        manager.insert(session_id, create_test_session("removable"));

        let removed = manager.remove(session_id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().host_name, "removable");
        assert!(!manager.contains(session_id));
    }

    #[test]
    fn test_remove_returns_none_for_unknown() {
        let mut manager = SessionManager::new();

        assert!(manager.remove(Uuid::new_v4()).is_none());
    }

    #[test]
    fn test_is_empty_after_remove() {
        let mut manager = SessionManager::new();
        let session_id = Uuid::new_v4();

        manager.insert(session_id, create_test_session("temporary"));
        assert!(!manager.is_empty());

        manager.remove(session_id);
        assert!(manager.is_empty());
    }

    // ---- Pending output tests ----

    #[test]
    fn test_has_pending_output_false_when_empty() {
        let manager = SessionManager::new();
        assert!(!manager.has_pending_output());
    }

    #[test]
    fn test_has_pending_output_false_with_empty_buffers() {
        let mut manager = SessionManager::new();
        let session_id = Uuid::new_v4();

        // Session with empty pending_output
        manager.insert(session_id, create_test_session("test"));

        assert!(!manager.has_pending_output());
    }

    #[test]
    fn test_has_pending_output_true_with_data() {
        let mut manager = SessionManager::new();
        let session_id = Uuid::new_v4();
        let mut session = create_test_session("test");

        session.pending_output.push_back(vec![1, 2, 3]);

        manager.insert(session_id, session);

        assert!(manager.has_pending_output());
    }

    #[test]
    fn test_has_pending_output_checks_all_sessions() {
        let mut manager = SessionManager::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        // First session with no pending output
        manager.insert(id1, create_test_session("empty"));

        // Second session with pending output
        let mut session2 = create_test_session("has-data");
        session2.pending_output.push_back(vec![1, 2, 3]);
        manager.insert(id2, session2);

        assert!(manager.has_pending_output());
    }

    #[test]
    fn test_pending_output_cleared_after_processing() {
        let mut manager = SessionManager::new();
        let session_id = Uuid::new_v4();
        let mut session = create_test_session("test");

        session.pending_output.push_back(vec![1, 2, 3]);
        manager.insert(session_id, session);

        assert!(manager.has_pending_output());

        // Simulate processing output
        if let Some(session) = manager.get_mut(session_id) {
            session.pending_output.clear();
        }

        assert!(!manager.has_pending_output());
    }

    // ---- values_mut iterator tests ----

    #[test]
    fn test_values_mut_iterates_all_sessions() {
        let mut manager = SessionManager::new();

        manager.insert(Uuid::new_v4(), create_test_session("host1"));
        manager.insert(Uuid::new_v4(), create_test_session("host2"));
        manager.insert(Uuid::new_v4(), create_test_session("host3"));

        let count = manager.values_mut().count();
        assert_eq!(count, 3);
    }

    #[test]
    fn test_values_mut_allows_modification() {
        let mut manager = SessionManager::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        manager.insert(id1, create_test_session("host1"));
        manager.insert(id2, create_test_session("host2"));

        // Add pending output to all sessions
        for session in manager.values_mut() {
            session.pending_output.push_back(vec![42]);
        }

        // Both sessions should now have pending output
        assert!(manager.get(id1).unwrap().pending_output.len() == 1);
        assert!(manager.get(id2).unwrap().pending_output.len() == 1);
    }

    // ---- Multiple sessions tests ----

    #[test]
    fn test_multiple_sessions_independent() {
        let mut manager = SessionManager::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        manager.insert(id1, create_test_session("host1"));
        manager.insert(id2, create_test_session("host2"));

        // Modify only first session
        if let Some(session) = manager.get_mut(id1) {
            session.host_name = "modified".to_string();
        }

        // Second session should be unchanged
        assert_eq!(manager.get(id1).unwrap().host_name, "modified");
        assert_eq!(manager.get(id2).unwrap().host_name, "host2");
    }

    #[test]
    fn test_remove_one_keeps_others() {
        let mut manager = SessionManager::new();
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        manager.insert(id1, create_test_session("host1"));
        manager.insert(id2, create_test_session("host2"));

        manager.remove(id1);

        assert!(!manager.contains(id1));
        assert!(manager.contains(id2));
        assert!(!manager.is_empty());
    }

    // ---- Status message tests ----

    #[test]
    fn test_status_message_can_be_set() {
        let mut manager = SessionManager::new();
        let session_id = Uuid::new_v4();

        manager.insert(session_id, create_test_session("test"));

        if let Some(session) = manager.get_mut(session_id) {
            session.status_message = Some(("Installing key...".to_string(), Instant::now()));
        }

        let session = manager.get(session_id).unwrap();
        assert!(session.status_message.is_some());
        assert_eq!(
            session.status_message.as_ref().unwrap().0,
            "Installing key..."
        );
    }

    #[test]
    fn test_status_message_can_be_cleared() {
        let mut manager = SessionManager::new();
        let session_id = Uuid::new_v4();
        let mut session = create_test_session("test");
        session.status_message = Some(("message".to_string(), Instant::now()));

        manager.insert(session_id, session);

        if let Some(session) = manager.get_mut(session_id) {
            session.status_message = None;
        }

        assert!(manager.get(session_id).unwrap().status_message.is_none());
    }
}
