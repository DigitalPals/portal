//! SFTP manager for file transfer sessions
//!
//! Manages SFTP connections, dual-pane browser state, and history tracking.

use std::collections::HashMap;
use uuid::Uuid;

use crate::message::SessionId;
use crate::sftp::SharedSftpSession;
use crate::views::sftp::{DualPaneSftpState, PaneId};

/// Manages SFTP connections and dual-pane browser tabs
pub struct SftpManager {
    /// Dual-pane SFTP browser tab states
    tabs: HashMap<SessionId, DualPaneSftpState>,
    /// Shared SFTP connections pool (can be used by multiple panes)
    connections: HashMap<SessionId, SharedSftpSession>,
    /// History entry IDs for SFTP sessions
    history_entries: HashMap<SessionId, Uuid>,
    /// Pending dual-pane SFTP connection (tab_id, pane_id, host_id)
    /// Used to track which pane is waiting for connection after host key verification
    pending_connection: Option<(SessionId, PaneId, Uuid)>,
}

impl SftpManager {
    /// Create a new empty SFTP manager
    pub fn new() -> Self {
        Self {
            tabs: HashMap::new(),
            connections: HashMap::new(),
            history_entries: HashMap::new(),
            pending_connection: None,
        }
    }

    // ---- Tab operations ----

    /// Get a reference to a tab's state by ID
    pub fn get_tab(&self, id: SessionId) -> Option<&DualPaneSftpState> {
        self.tabs.get(&id)
    }

    /// Get a mutable reference to a tab's state by ID
    pub fn get_tab_mut(&mut self, id: SessionId) -> Option<&mut DualPaneSftpState> {
        self.tabs.get_mut(&id)
    }

    /// Insert a new tab
    pub fn insert_tab(&mut self, id: SessionId, state: DualPaneSftpState) {
        self.tabs.insert(id, state);
    }

    /// Remove a tab by ID
    pub fn remove_tab(&mut self, id: SessionId) -> Option<DualPaneSftpState> {
        self.tabs.remove(&id)
    }

    /// Check if a tab exists
    pub fn contains_tab(&self, id: SessionId) -> bool {
        self.tabs.contains_key(&id)
    }

    /// Get first tab ID (for keyboard navigation)
    pub fn first_tab_id(&self) -> Option<SessionId> {
        self.tabs.keys().next().copied()
    }

    /// Iterate over all tabs
    #[allow(dead_code)]
    pub fn tabs(&self) -> impl Iterator<Item = (&SessionId, &DualPaneSftpState)> {
        self.tabs.iter()
    }

    /// Iterate over all tabs mutably
    pub fn tabs_mut(&mut self) -> impl Iterator<Item = (&SessionId, &mut DualPaneSftpState)> {
        self.tabs.iter_mut()
    }

    /// Get all tab values mutably
    pub fn tab_values_mut(&mut self) -> impl Iterator<Item = &mut DualPaneSftpState> {
        self.tabs.values_mut()
    }

    // ---- Connection operations ----

    /// Get a reference to an SFTP connection by session ID
    pub fn get_connection(&self, id: SessionId) -> Option<&SharedSftpSession> {
        self.connections.get(&id)
    }

    /// Get a clone of an SFTP connection by session ID
    pub fn get_connection_cloned(&self, id: SessionId) -> Option<SharedSftpSession> {
        self.connections.get(&id).cloned()
    }

    /// Insert a new SFTP connection
    pub fn insert_connection(&mut self, id: SessionId, session: SharedSftpSession) {
        self.connections.insert(id, session);
    }

    /// Remove an SFTP connection by ID
    pub fn remove_connection(&mut self, id: SessionId) -> Option<SharedSftpSession> {
        self.connections.remove(&id)
    }

    /// Check if a connection is still used by any tab
    pub fn is_connection_in_use(&self, session_id: SessionId) -> bool {
        self.tabs.values().any(|state| {
            state.left_pane.source.session_id() == Some(session_id)
                || state.right_pane.source.session_id() == Some(session_id)
        })
    }

    // ---- History entry operations ----

    /// Insert a history entry for an SFTP session
    pub fn insert_history_entry(&mut self, session_id: SessionId, entry_id: Uuid) {
        self.history_entries.insert(session_id, entry_id);
    }

    /// Remove and return a history entry for an SFTP session
    pub fn remove_history_entry(&mut self, session_id: SessionId) -> Option<Uuid> {
        self.history_entries.remove(&session_id)
    }

    // ---- Pending connection operations ----

    /// Get the pending connection info
    pub fn pending_connection(&self) -> Option<(SessionId, PaneId, Uuid)> {
        self.pending_connection
    }

    /// Set the pending connection info
    pub fn set_pending_connection(&mut self, info: Option<(SessionId, PaneId, Uuid)>) {
        self.pending_connection = info;
    }

    /// Clear the pending connection
    pub fn clear_pending_connection(&mut self) {
        self.pending_connection = None;
    }
}

impl Default for SftpManager {
    fn default() -> Self {
        Self::new()
    }
}
