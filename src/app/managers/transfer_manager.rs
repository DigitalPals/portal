//! SFTP transfer manager
//!
//! Tracks long-running copy/upload/download operations independently from pane
//! loading state so users can see progress, failures, and request cancellation.

use std::collections::VecDeque;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::message::SessionId;
use crate::views::sftp::PaneId;

const MAX_FINISHED_TRANSFERS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDirection {
    LocalToLocal,
    LocalToRemote,
    RemoteToLocal,
    RemoteToRemote,
}

impl TransferDirection {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalToLocal => "Local copy",
            Self::LocalToRemote => "Upload",
            Self::RemoteToLocal => "Download",
            Self::RemoteToRemote => "Remote copy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferStatus {
    Queued,
    Running,
    Cancelling,
    Completed,
    Failed(String),
    Cancelled,
}

impl TransferStatus {
    pub fn is_finished(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed(_) | Self::Cancelled)
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Queued => "Queued",
            Self::Running => "Running",
            Self::Cancelling => "Cancelling",
            Self::Completed => "Completed",
            Self::Failed(_) => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransferItem {
    pub id: Uuid,
    pub tab_id: SessionId,
    pub target_pane: PaneId,
    pub direction: TransferDirection,
    pub label: String,
    pub current_item: Option<String>,
    pub completed_files: usize,
    pub total_files: usize,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
    pub status: TransferStatus,
    pub started_at: Instant,
    pub finished_at: Option<Instant>,
    cancel_requested: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
pub struct TransferItemInit {
    pub id: Uuid,
    pub tab_id: SessionId,
    pub target_pane: PaneId,
    pub direction: TransferDirection,
    pub label: String,
    pub total_files: usize,
    pub total_bytes: Option<u64>,
    pub cancel_requested: Arc<AtomicBool>,
}

impl TransferItem {
    pub fn new(init: TransferItemInit) -> Self {
        Self {
            id: init.id,
            tab_id: init.tab_id,
            target_pane: init.target_pane,
            direction: init.direction,
            label: init.label,
            current_item: None,
            completed_files: 0,
            total_files: init.total_files,
            completed_bytes: 0,
            total_bytes: init.total_bytes,
            status: TransferStatus::Queued,
            started_at: Instant::now(),
            finished_at: None,
            cancel_requested: init.cancel_requested,
        }
    }

    pub fn progress_fraction(&self) -> Option<f32> {
        if let Some(total_bytes) = self.total_bytes
            && total_bytes > 0
        {
            return Some((self.completed_bytes as f32 / total_bytes as f32).clamp(0.0, 1.0));
        }

        if self.total_files > 0 {
            return Some((self.completed_files as f32 / self.total_files as f32).clamp(0.0, 1.0));
        }

        None
    }

    pub fn elapsed(&self) -> Duration {
        self.finished_at.unwrap_or_else(Instant::now) - self.started_at
    }

    pub fn cancel(&mut self) {
        if !self.status.is_finished() {
            self.status = TransferStatus::Cancelling;
            self.cancel_requested.store(true, Ordering::Relaxed);
        }
    }
}

#[derive(Debug, Clone)]
pub struct TransferProgress {
    pub transfer_id: Uuid,
    pub current_item: Option<String>,
    pub completed_files: usize,
    pub total_files: usize,
    pub completed_bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Default)]
pub struct TransferManager {
    transfers: VecDeque<TransferItem>,
}

impl TransferManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, mut transfer: TransferItem) {
        transfer.status = TransferStatus::Running;
        self.transfers.push_front(transfer);
        self.prune_finished();
    }

    pub fn progress(&mut self, progress: TransferProgress) {
        if let Some(transfer) = self
            .transfers
            .iter_mut()
            .find(|transfer| transfer.id == progress.transfer_id)
            && matches!(
                transfer.status,
                TransferStatus::Running | TransferStatus::Queued
            )
        {
            transfer.current_item = progress.current_item;
            transfer.completed_files = progress.completed_files;
            transfer.total_files = progress.total_files;
            transfer.completed_bytes = progress.completed_bytes;
            transfer.total_bytes = progress.total_bytes;
        }
    }

    pub fn finish(&mut self, id: Uuid, status: TransferStatus) -> Option<TransferItem> {
        let transfer = self
            .transfers
            .iter_mut()
            .find(|transfer| transfer.id == id)?;
        transfer.status = status;
        transfer.current_item = None;
        transfer.finished_at = Some(Instant::now());
        let clone = transfer.clone();
        self.prune_finished();
        Some(clone)
    }

    pub fn cancel(&mut self, id: Uuid) -> bool {
        let Some(transfer) = self.transfers.iter_mut().find(|transfer| transfer.id == id) else {
            return false;
        };
        transfer.cancel();
        true
    }

    pub fn cancel_for_tab(&mut self, tab_id: SessionId) {
        for transfer in &mut self.transfers {
            if transfer.tab_id == tab_id {
                transfer.cancel();
            }
        }
    }

    pub fn clear_finished(&mut self) {
        self.transfers
            .retain(|transfer| !transfer.status.is_finished());
    }

    pub fn for_tab(&self, tab_id: SessionId) -> Vec<TransferItem> {
        self.transfers
            .iter()
            .filter(|transfer| transfer.tab_id == tab_id)
            .cloned()
            .collect()
    }

    pub fn any_active_for_tab(&self, tab_id: SessionId) -> bool {
        self.transfers
            .iter()
            .any(|transfer| transfer.tab_id == tab_id && !transfer.status.is_finished())
    }

    fn prune_finished(&mut self) {
        let mut finished_seen = 0usize;
        self.transfers.retain(|transfer| {
            if transfer.status.is_finished() {
                finished_seen += 1;
                finished_seen <= MAX_FINISHED_TRANSFERS
            } else {
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_progress_prefers_bytes_when_available() {
        let transfer = TransferItem::new(TransferItemInit {
            id: Uuid::new_v4(),
            tab_id: Uuid::new_v4(),
            target_pane: PaneId::Left,
            direction: TransferDirection::LocalToRemote,
            label: "Upload".to_string(),
            total_files: 10,
            total_bytes: Some(100),
            cancel_requested: Arc::new(AtomicBool::new(false)),
        });
        let mut transfer = transfer;
        transfer.completed_files = 9;
        transfer.completed_bytes = 25;

        assert_eq!(transfer.progress_fraction(), Some(0.25));
    }

    #[test]
    fn transfer_cancel_sets_token_and_status() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut transfer = TransferItem::new(TransferItemInit {
            id: Uuid::new_v4(),
            tab_id: Uuid::new_v4(),
            target_pane: PaneId::Right,
            direction: TransferDirection::RemoteToLocal,
            label: "Download".to_string(),
            total_files: 1,
            total_bytes: None,
            cancel_requested: cancel.clone(),
        });

        transfer.cancel();

        assert_eq!(transfer.status, TransferStatus::Cancelling);
        assert!(cancel.load(Ordering::Relaxed));
    }
}
