//! SFTP manager for file transfer sessions
//!
//! Manages SFTP connections, dual-pane browser state, and history tracking.

use std::collections::HashMap;
use uuid::Uuid;

use crate::message::SessionId;
use crate::sftp::SharedSftpSession;
use crate::views::sftp::{DualPaneSftpState, PaneId, PaneSource};

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

    /// Remove a tab by ID and collect any unique remote session IDs it used
    pub fn remove_tab_and_collect_sessions(&mut self, id: SessionId) -> Vec<SessionId> {
        let Some(state) = self.tabs.remove(&id) else {
            return Vec::new();
        };

        let mut ids = Vec::new();
        for pane in [&state.left_pane, &state.right_pane] {
            if let PaneSource::Remote { session_id, .. } = &pane.source {
                if !ids.contains(session_id) {
                    ids.push(*session_id);
                }
            }
        }

        ids
    }

    /// Check if a tab exists
    pub fn contains_tab(&self, id: SessionId) -> bool {
        self.tabs.contains_key(&id)
    }

    /// Get first tab ID (for keyboard navigation)
    pub fn first_tab_id(&self) -> Option<SessionId> {
        self.tabs.keys().next().copied()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::sftp::{DualPaneSftpState, PaneSource};

    // ---- Basic manager tests ----

    #[test]
    fn new_creates_empty_manager() {
        let manager = SftpManager::new();

        assert!(manager.tabs.is_empty());
        assert!(manager.connections.is_empty());
        assert!(manager.history_entries.is_empty());
        assert!(manager.pending_connection.is_none());
    }

    #[test]
    fn default_creates_empty_manager() {
        let manager = SftpManager::default();

        assert!(manager.tabs.is_empty());
        assert!(manager.connections.is_empty());
    }

    // ---- Tab operations tests ----

    #[test]
    fn insert_and_get_tab() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let state = DualPaneSftpState::new(tab_id);

        manager.insert_tab(tab_id, state);

        assert!(manager.get_tab(tab_id).is_some());
        assert_eq!(manager.get_tab(tab_id).unwrap().tab_id, tab_id);
    }

    #[test]
    fn get_tab_returns_none_for_missing() {
        let manager = SftpManager::new();
        let missing_id = Uuid::new_v4();

        assert!(manager.get_tab(missing_id).is_none());
    }

    #[test]
    fn get_tab_mut_allows_modification() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let state = DualPaneSftpState::new(tab_id);

        manager.insert_tab(tab_id, state);

        if let Some(tab) = manager.get_tab_mut(tab_id) {
            tab.left_pane.source = PaneSource::Remote {
                session_id,
                host_name: "modified".to_string(),
            };
        }

        let tab = manager.get_tab(tab_id).unwrap();
        assert_eq!(tab.left_pane.source.session_id(), Some(session_id));
    }

    #[test]
    fn contains_tab_returns_true_for_existing() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();

        manager.insert_tab(tab_id, DualPaneSftpState::new(tab_id));

        assert!(manager.contains_tab(tab_id));
    }

    #[test]
    fn contains_tab_returns_false_for_missing() {
        let manager = SftpManager::new();

        assert!(!manager.contains_tab(Uuid::new_v4()));
    }

    #[test]
    fn first_tab_id_returns_none_when_empty() {
        let manager = SftpManager::new();

        assert!(manager.first_tab_id().is_none());
    }

    #[test]
    fn first_tab_id_returns_some_when_tabs_exist() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();

        manager.insert_tab(tab_id, DualPaneSftpState::new(tab_id));

        assert!(manager.first_tab_id().is_some());
    }

    #[test]
    fn tab_values_mut_iterates_all_tabs() {
        let mut manager = SftpManager::new();
        let tab1 = Uuid::new_v4();
        let tab2 = Uuid::new_v4();

        manager.insert_tab(tab1, DualPaneSftpState::new(tab1));
        manager.insert_tab(tab2, DualPaneSftpState::new(tab2));

        let count = manager.tab_values_mut().count();

        assert_eq!(count, 2);
    }

    #[test]
    fn remove_tab_and_collect_sessions_empty_when_missing() {
        let mut manager = SftpManager::new();
        let missing_id = Uuid::new_v4();

        let sessions = manager.remove_tab_and_collect_sessions(missing_id);

        assert!(sessions.is_empty());
    }

    #[test]
    fn remove_tab_and_collect_sessions_returns_unique_ids() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let mut state = DualPaneSftpState::new(tab_id);
        let session_id = Uuid::new_v4();

        state.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "alpha".to_string(),
        };
        state.right_pane.source = PaneSource::Remote {
            session_id,
            host_name: "alpha".to_string(),
        };

        manager.insert_tab(tab_id, state);

        let sessions = manager.remove_tab_and_collect_sessions(tab_id);

        assert_eq!(sessions, vec![session_id]);
        assert!(manager.get_tab(tab_id).is_none());
    }

    #[test]
    fn remove_tab_and_collect_sessions_keeps_both_panes() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let mut state = DualPaneSftpState::new(tab_id);
        let left_id = Uuid::new_v4();
        let right_id = Uuid::new_v4();

        state.left_pane.source = PaneSource::Remote {
            session_id: left_id,
            host_name: "left".to_string(),
        };
        state.right_pane.source = PaneSource::Remote {
            session_id: right_id,
            host_name: "right".to_string(),
        };

        manager.insert_tab(tab_id, state);

        let sessions = manager.remove_tab_and_collect_sessions(tab_id);

        assert_eq!(sessions, vec![left_id, right_id]);
    }

    #[test]
    fn remove_tab_with_local_panes_returns_empty() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let state = DualPaneSftpState::new(tab_id); // Both panes default to Local

        manager.insert_tab(tab_id, state);

        let sessions = manager.remove_tab_and_collect_sessions(tab_id);

        assert!(sessions.is_empty());
    }

    #[test]
    fn remove_tab_with_mixed_panes() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let mut state = DualPaneSftpState::new(tab_id);
        let session_id = Uuid::new_v4();

        // Left pane remote, right pane local
        state.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };
        // right_pane stays Local (default)

        manager.insert_tab(tab_id, state);

        let sessions = manager.remove_tab_and_collect_sessions(tab_id);

        assert_eq!(sessions, vec![session_id]);
    }

    // ---- Connection pool tests ----

    #[test]
    fn insert_and_get_connection() {
        let manager = SftpManager::new();
        let session_id = Uuid::new_v4();

        // We can't create a real SharedSftpSession without a server,
        // but we can test the HashMap operations work correctly
        assert!(manager.get_connection(session_id).is_none());
    }

    #[test]
    fn remove_connection_returns_none_for_missing() {
        let mut manager = SftpManager::new();

        assert!(manager.remove_connection(Uuid::new_v4()).is_none());
    }

    // ---- Connection in use tests (critical for cleanup) ----

    #[test]
    fn is_connection_in_use_returns_false_when_no_tabs() {
        let manager = SftpManager::new();

        assert!(!manager.is_connection_in_use(Uuid::new_v4()));
    }

    #[test]
    fn is_connection_in_use_returns_false_for_local_panes() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        // Tab with both panes local
        manager.insert_tab(tab_id, DualPaneSftpState::new(tab_id));

        assert!(!manager.is_connection_in_use(session_id));
    }

    #[test]
    fn is_connection_in_use_returns_true_for_left_pane() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let mut state = DualPaneSftpState::new(tab_id);

        state.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };

        manager.insert_tab(tab_id, state);

        assert!(manager.is_connection_in_use(session_id));
    }

    #[test]
    fn is_connection_in_use_returns_true_for_right_pane() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let mut state = DualPaneSftpState::new(tab_id);

        state.right_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };

        manager.insert_tab(tab_id, state);

        assert!(manager.is_connection_in_use(session_id));
    }

    #[test]
    fn is_connection_in_use_returns_false_for_different_session() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let other_session_id = Uuid::new_v4();
        let mut state = DualPaneSftpState::new(tab_id);

        state.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };

        manager.insert_tab(tab_id, state);

        assert!(!manager.is_connection_in_use(other_session_id));
    }

    #[test]
    fn is_connection_in_use_checks_all_tabs() {
        let mut manager = SftpManager::new();
        let tab1 = Uuid::new_v4();
        let tab2 = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        // Tab1 has local panes
        manager.insert_tab(tab1, DualPaneSftpState::new(tab1));

        // Tab2 has remote pane
        let mut state2 = DualPaneSftpState::new(tab2);
        state2.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };
        manager.insert_tab(tab2, state2);

        assert!(manager.is_connection_in_use(session_id));
    }

    #[test]
    fn connection_not_in_use_after_tab_removed() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let mut state = DualPaneSftpState::new(tab_id);

        state.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };

        manager.insert_tab(tab_id, state);
        assert!(manager.is_connection_in_use(session_id));

        manager.remove_tab_and_collect_sessions(tab_id);
        assert!(!manager.is_connection_in_use(session_id));
    }

    // ---- Connection lifecycle scenarios ----

    #[test]
    fn connection_still_in_use_when_other_tab_uses_it() {
        let mut manager = SftpManager::new();
        let tab1 = Uuid::new_v4();
        let tab2 = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        // Both tabs use the same session
        let mut state1 = DualPaneSftpState::new(tab1);
        state1.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };
        manager.insert_tab(tab1, state1);

        let mut state2 = DualPaneSftpState::new(tab2);
        state2.right_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };
        manager.insert_tab(tab2, state2);

        // Remove first tab
        manager.remove_tab_and_collect_sessions(tab1);

        // Connection should still be in use by tab2
        assert!(manager.is_connection_in_use(session_id));
    }

    #[test]
    fn connection_not_in_use_after_all_tabs_removed() {
        let mut manager = SftpManager::new();
        let tab1 = Uuid::new_v4();
        let tab2 = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        // Both tabs use the same session
        let mut state1 = DualPaneSftpState::new(tab1);
        state1.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };
        manager.insert_tab(tab1, state1);

        let mut state2 = DualPaneSftpState::new(tab2);
        state2.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };
        manager.insert_tab(tab2, state2);

        // Remove both tabs
        manager.remove_tab_and_collect_sessions(tab1);
        manager.remove_tab_and_collect_sessions(tab2);

        // Connection should no longer be in use
        assert!(!manager.is_connection_in_use(session_id));
    }

    #[test]
    fn multiple_connections_tracked_independently() {
        let mut manager = SftpManager::new();
        let tab1 = Uuid::new_v4();
        let tab2 = Uuid::new_v4();
        let session_a = Uuid::new_v4();
        let session_b = Uuid::new_v4();

        // Tab1 uses session_a
        let mut state1 = DualPaneSftpState::new(tab1);
        state1.left_pane.source = PaneSource::Remote {
            session_id: session_a,
            host_name: "server-a".to_string(),
        };
        manager.insert_tab(tab1, state1);

        // Tab2 uses session_b
        let mut state2 = DualPaneSftpState::new(tab2);
        state2.left_pane.source = PaneSource::Remote {
            session_id: session_b,
            host_name: "server-b".to_string(),
        };
        manager.insert_tab(tab2, state2);

        // Remove tab1
        manager.remove_tab_and_collect_sessions(tab1);

        // session_a should not be in use, session_b should still be in use
        assert!(!manager.is_connection_in_use(session_a));
        assert!(manager.is_connection_in_use(session_b));
    }

    // ---- History entry tests ----

    #[test]
    fn insert_and_remove_history_entry() {
        let mut manager = SftpManager::new();
        let session_id = Uuid::new_v4();
        let entry_id = Uuid::new_v4();

        manager.insert_history_entry(session_id, entry_id);

        let removed = manager.remove_history_entry(session_id);
        assert_eq!(removed, Some(entry_id));
    }

    #[test]
    fn remove_history_entry_returns_none_for_missing() {
        let mut manager = SftpManager::new();

        assert!(manager.remove_history_entry(Uuid::new_v4()).is_none());
    }

    #[test]
    fn history_entries_are_independent() {
        let mut manager = SftpManager::new();
        let session1 = Uuid::new_v4();
        let session2 = Uuid::new_v4();
        let entry1 = Uuid::new_v4();
        let entry2 = Uuid::new_v4();

        manager.insert_history_entry(session1, entry1);
        manager.insert_history_entry(session2, entry2);

        assert_eq!(manager.remove_history_entry(session1), Some(entry1));
        assert_eq!(manager.remove_history_entry(session2), Some(entry2));
    }

    // ---- Pending connection tests ----

    #[test]
    fn set_and_clear_pending_connection() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let host_id = Uuid::new_v4();

        assert!(manager.pending_connection.is_none());

        manager.set_pending_connection(Some((tab_id, PaneId::Left, host_id)));
        assert!(manager.pending_connection.is_some());
        assert_eq!(
            manager.pending_connection,
            Some((tab_id, PaneId::Left, host_id))
        );

        manager.clear_pending_connection();
        assert!(manager.pending_connection.is_none());
    }

    #[test]
    fn set_pending_connection_replaces_existing() {
        let mut manager = SftpManager::new();
        let tab1 = Uuid::new_v4();
        let tab2 = Uuid::new_v4();
        let host_id = Uuid::new_v4();

        manager.set_pending_connection(Some((tab1, PaneId::Left, host_id)));
        manager.set_pending_connection(Some((tab2, PaneId::Right, host_id)));

        assert_eq!(
            manager.pending_connection,
            Some((tab2, PaneId::Right, host_id))
        );
    }

    #[test]
    fn set_pending_connection_to_none_clears() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let host_id = Uuid::new_v4();

        manager.set_pending_connection(Some((tab_id, PaneId::Left, host_id)));
        manager.set_pending_connection(None);

        assert!(manager.pending_connection.is_none());
    }

    // ---- Concurrent/multi-pane scenario tests ----

    #[test]
    fn both_panes_same_session_counted_once() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let mut state = DualPaneSftpState::new(tab_id);

        // Both panes connected to same host
        state.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };
        state.right_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };

        manager.insert_tab(tab_id, state);

        // Session should be in use
        assert!(manager.is_connection_in_use(session_id));

        // Remove tab - should get deduplicated session list
        let sessions = manager.remove_tab_and_collect_sessions(tab_id);
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0], session_id);
    }

    #[test]
    fn cross_tab_connection_sharing() {
        let mut manager = SftpManager::new();
        let tab1 = Uuid::new_v4();
        let tab2 = Uuid::new_v4();
        let tab3 = Uuid::new_v4();
        let session_shared = Uuid::new_v4();
        let session_unique = Uuid::new_v4();

        // Tab1: left=shared, right=local
        let mut state1 = DualPaneSftpState::new(tab1);
        state1.left_pane.source = PaneSource::Remote {
            session_id: session_shared,
            host_name: "shared-server".to_string(),
        };
        manager.insert_tab(tab1, state1);

        // Tab2: left=local, right=shared
        let mut state2 = DualPaneSftpState::new(tab2);
        state2.right_pane.source = PaneSource::Remote {
            session_id: session_shared,
            host_name: "shared-server".to_string(),
        };
        manager.insert_tab(tab2, state2);

        // Tab3: left=unique, right=local
        let mut state3 = DualPaneSftpState::new(tab3);
        state3.left_pane.source = PaneSource::Remote {
            session_id: session_unique,
            host_name: "unique-server".to_string(),
        };
        manager.insert_tab(tab3, state3);

        // All sessions in use
        assert!(manager.is_connection_in_use(session_shared));
        assert!(manager.is_connection_in_use(session_unique));

        // Close tab1 - shared still in use by tab2
        manager.remove_tab_and_collect_sessions(tab1);
        assert!(manager.is_connection_in_use(session_shared));
        assert!(manager.is_connection_in_use(session_unique));

        // Close tab2 - shared no longer in use
        manager.remove_tab_and_collect_sessions(tab2);
        assert!(!manager.is_connection_in_use(session_shared));
        assert!(manager.is_connection_in_use(session_unique));

        // Close tab3 - unique no longer in use
        manager.remove_tab_and_collect_sessions(tab3);
        assert!(!manager.is_connection_in_use(session_unique));
    }

    #[test]
    fn pane_source_change_affects_connection_tracking() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let session_old = Uuid::new_v4();
        let session_new = Uuid::new_v4();

        // Initial state: left pane uses session_old
        let mut state = DualPaneSftpState::new(tab_id);
        state.left_pane.source = PaneSource::Remote {
            session_id: session_old,
            host_name: "old-server".to_string(),
        };
        manager.insert_tab(tab_id, state);

        assert!(manager.is_connection_in_use(session_old));
        assert!(!manager.is_connection_in_use(session_new));

        // Change pane source to new session
        if let Some(tab) = manager.get_tab_mut(tab_id) {
            tab.left_pane.source = PaneSource::Remote {
                session_id: session_new,
                host_name: "new-server".to_string(),
            };
        }

        // Now session_new is in use, session_old is not
        assert!(!manager.is_connection_in_use(session_old));
        assert!(manager.is_connection_in_use(session_new));
    }

    #[test]
    fn switch_pane_to_local_releases_connection() {
        let mut manager = SftpManager::new();
        let tab_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();

        // Initial state: left pane is remote
        let mut state = DualPaneSftpState::new(tab_id);
        state.left_pane.source = PaneSource::Remote {
            session_id,
            host_name: "server".to_string(),
        };
        manager.insert_tab(tab_id, state);

        assert!(manager.is_connection_in_use(session_id));

        // Switch pane to local
        if let Some(tab) = manager.get_tab_mut(tab_id) {
            tab.left_pane.source = PaneSource::Local;
        }

        assert!(!manager.is_connection_in_use(session_id));
    }
}
