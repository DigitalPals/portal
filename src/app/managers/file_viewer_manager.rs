//! File viewer manager for tracking open file viewers

use std::collections::HashMap;

use crate::message::SessionId;
use crate::views::file_viewer::FileViewerState;

/// Manager for file viewer instances
#[derive(Debug, Default)]
pub struct FileViewerManager {
    viewers: HashMap<SessionId, FileViewerState>,
}

impl FileViewerManager {
    /// Create a new file viewer manager
    pub fn new() -> Self {
        Self {
            viewers: HashMap::new(),
        }
    }

    /// Get a reference to a file viewer by ID
    pub fn get(&self, viewer_id: SessionId) -> Option<&FileViewerState> {
        self.viewers.get(&viewer_id)
    }

    /// Get a mutable reference to a file viewer by ID
    pub fn get_mut(&mut self, viewer_id: SessionId) -> Option<&mut FileViewerState> {
        self.viewers.get_mut(&viewer_id)
    }

    /// Insert a new file viewer
    pub fn insert(&mut self, state: FileViewerState) {
        self.viewers.insert(state.viewer_id, state);
    }

    /// Remove a file viewer by ID
    pub fn remove(&mut self, viewer_id: SessionId) -> Option<FileViewerState> {
        self.viewers.remove(&viewer_id)
    }

    /// Check if a viewer exists
    pub fn contains(&self, viewer_id: SessionId) -> bool {
        self.viewers.contains_key(&viewer_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::views::file_viewer::{FileSource, FileType, FileViewerState};
    use std::path::PathBuf;
    use uuid::Uuid;

    #[test]
    fn insert_get_remove_viewer() {
        let mut manager = FileViewerManager::new();
        let viewer_id = Uuid::new_v4();
        let state = FileViewerState::new(
            viewer_id,
            "notes.txt".to_string(),
            FileSource::Local {
                path: PathBuf::from("notes.txt"),
            },
            FileType::Text { language: None },
        );

        manager.insert(state);

        assert!(manager.contains(viewer_id));
        assert!(manager.get(viewer_id).is_some());
        assert!(manager.remove(viewer_id).is_some());
        assert!(!manager.contains(viewer_id));
    }
}
