//! SFTP type definitions
//!
//! This module contains all type definitions for the SFTP dual-pane browser.

use iced::Point;
use serde::{Deserialize, Serialize};

use crate::message::SessionId;

/// Identifies which pane an action targets
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneId {
    Left,
    Right,
}

/// Source of files for a pane - either local filesystem or remote SFTP
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneSource {
    Local,
    Remote {
        session_id: SessionId,
        host_name: String,
    },
}

impl PaneSource {
    pub fn display_name(&self) -> &str {
        match self {
            PaneSource::Local => "Local",
            PaneSource::Remote { host_name, .. } => host_name,
        }
    }

    /// Get the session ID if this is a remote source
    pub fn session_id(&self) -> Option<SessionId> {
        match self {
            PaneSource::Local => None,
            PaneSource::Remote { session_id, .. } => Some(*session_id),
        }
    }
}

/// Context menu action types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuAction {
    Open,
    CopyToTarget,
    Rename,
    Delete,
    Refresh,
    NewFolder,
    EditPermissions,
}

/// State for the context menu
#[derive(Debug, Clone)]
pub struct ContextMenuState {
    pub visible: bool,
    pub position: Point,
    pub target_pane: PaneId,
}

impl Default for ContextMenuState {
    fn default() -> Self {
        Self {
            visible: false,
            position: Point::ORIGIN,
            target_pane: PaneId::Left,
        }
    }
}

/// Type of SFTP dialog currently open
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SftpDialogType {
    NewFolder,
    Rename {
        original_name: String,
    },
    Delete {
        entries: Vec<(String, std::path::PathBuf, bool)>,
    }, // (name, path, is_dir)
    EditPermissions {
        name: String,
        path: std::path::PathBuf,
        permissions: PermissionBits,
    },
}

/// Individual permission bit identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionBit {
    OwnerRead,
    OwnerWrite,
    OwnerExecute,
    GroupRead,
    GroupWrite,
    GroupExecute,
    OtherRead,
    OtherWrite,
    OtherExecute,
}

/// Unix permission bits for a file or directory
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PermissionBits {
    pub owner_read: bool,
    pub owner_write: bool,
    pub owner_execute: bool,
    pub group_read: bool,
    pub group_write: bool,
    pub group_execute: bool,
    pub other_read: bool,
    pub other_write: bool,
    pub other_execute: bool,
}

impl PermissionBits {
    /// Create from a Unix mode (e.g., 0o755)
    pub fn from_mode(mode: u32) -> Self {
        Self {
            owner_read: mode & 0o400 != 0,
            owner_write: mode & 0o200 != 0,
            owner_execute: mode & 0o100 != 0,
            group_read: mode & 0o040 != 0,
            group_write: mode & 0o020 != 0,
            group_execute: mode & 0o010 != 0,
            other_read: mode & 0o004 != 0,
            other_write: mode & 0o002 != 0,
            other_execute: mode & 0o001 != 0,
        }
    }

    /// Convert to Unix mode
    pub fn to_mode(self) -> u32 {
        let mut mode = 0u32;
        if self.owner_read {
            mode |= 0o400;
        }
        if self.owner_write {
            mode |= 0o200;
        }
        if self.owner_execute {
            mode |= 0o100;
        }
        if self.group_read {
            mode |= 0o040;
        }
        if self.group_write {
            mode |= 0o020;
        }
        if self.group_execute {
            mode |= 0o010;
        }
        if self.other_read {
            mode |= 0o004;
        }
        if self.other_write {
            mode |= 0o002;
        }
        if self.other_execute {
            mode |= 0o001;
        }
        mode
    }

    /// Format as octal string (e.g., "755")
    pub fn as_octal_string(&self) -> String {
        format!("{:03o}", self.to_mode())
    }
}

impl Default for PermissionBits {
    fn default() -> Self {
        // Default to 644 (rw-r--r--)
        Self::from_mode(0o644)
    }
}

/// Column identifiers for the SFTP file list
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SftpColumn {
    Name,
    DateModified,
    Size,
    Kind,
}

/// Minimum column width in pixels
pub const MIN_COLUMN_WIDTH: f32 = 60.0;

/// Column width configuration for SFTP file lists (stored as pixel widths)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnWidths {
    pub name: f32,
    pub date_modified: f32,
    pub size: f32,
    pub kind: f32,
}

impl Default for ColumnWidths {
    fn default() -> Self {
        Self {
            name: 200.0,
            date_modified: 150.0,
            size: 80.0,
            kind: 120.0,
        }
    }
}

impl ColumnWidths {
    /// Get the pixel width for a column
    pub fn get(&self, column: SftpColumn) -> f32 {
        match column {
            SftpColumn::Name => self.name,
            SftpColumn::DateModified => self.date_modified,
            SftpColumn::Size => self.size,
            SftpColumn::Kind => self.kind,
        }
    }

    /// Set the width for a column, enforcing minimum width
    pub fn set(&mut self, column: SftpColumn, width: f32) {
        let width = width.max(MIN_COLUMN_WIDTH);
        match column {
            SftpColumn::Name => self.name = width,
            SftpColumn::DateModified => self.date_modified = width,
            SftpColumn::Size => self.size = width,
            SftpColumn::Kind => self.kind = width,
        }
    }

    /// Get the total width of all columns
    pub fn total_width(&self) -> f32 {
        self.name + self.date_modified + self.size + self.kind
    }
}
