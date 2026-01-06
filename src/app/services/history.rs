use uuid::Uuid;

use crate::config::HistoryConfig;

pub fn mark_entry_disconnected(history: &mut HistoryConfig, entry_id: Uuid) -> bool {
    if let Some(entry) = history.find_entry_mut(entry_id) {
        entry.disconnected_at = Some(chrono::Utc::now());
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HistoryEntry, SessionType};

    #[test]
    fn mark_entry_disconnected_updates_entry() {
        let mut history = HistoryConfig::default();
        let entry = HistoryEntry::new(
            Uuid::new_v4(),
            "Host".to_string(),
            "host.example".to_string(),
            "user".to_string(),
            SessionType::Sftp,
        );
        let entry_id = entry.id;
        history.add_entry(entry);

        assert!(mark_entry_disconnected(&mut history, entry_id));
        assert!(history.find_entry(entry_id).unwrap().disconnected_at.is_some());
    }

    #[test]
    fn mark_entry_disconnected_returns_false_for_missing() {
        let mut history = HistoryConfig::default();
        assert!(!mark_entry_disconnected(&mut history, Uuid::new_v4()));
    }
}
