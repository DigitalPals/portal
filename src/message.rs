use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::DetectedOs;
use crate::sftp::{FileEntry, SharedSftpSession};
use crate::ssh::host_key_verification::HostKeyVerificationRequest;
use crate::ssh::{SshEvent, SshSession};
use crate::views::sftp_view::{PaneId, PaneSource};

/// Session ID type alias
pub type SessionId = Uuid;

/// Sidebar menu item selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidebarMenuItem {
    #[default]
    Hosts,
    Sftp,
    Snippets,
    History,
    Settings,
}

#[derive(Debug, Clone)]
pub enum HostDialogField {
    Name,
    Hostname,
    Port,
    Username,
    AuthMethod,
    KeyPath,
    GroupId,
    Tags,
    Notes,
}

#[derive(Debug, Clone)]
pub enum SnippetField {
    Name,
    Command,
    Description,
}

/// Application messages for the Elm-style update loop
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Message {
    // Host management
    HostSelected(Uuid),
    HostConnect(Uuid),
    HostAdd,
    HostEdit(Uuid),
    QuickConnect,   // Parse search query as user@host:port and connect
    LocalTerminal,  // Open local terminal (stubbed for now)

    // Dialog
    DialogClose,
    DialogSubmit,
    DialogFieldChanged(HostDialogField, String),

    // Terminal / Session
    TerminalInput(SessionId, Vec<u8>),
    TerminalResize(SessionId, u16, u16),

    // SSH connection
    SshConnected {
        session_id: SessionId,
        host_name: String,
        ssh_session: Arc<SshSession>,
        event_rx: EventReceiver,
        host_id: uuid::Uuid,
        detected_os: Option<DetectedOs>,
    },
    SshData(SessionId, Vec<u8>),
    SshDisconnected(SessionId),
    SshError(String),

    // Tab management
    TabSelect(Uuid),
    TabClose(Uuid),
    TabNew,

    // SFTP browser
    SftpOpen(Uuid),  // Open SFTP browser for a host
    SftpConnected {
        session_id: SessionId,
        host_name: String,
        sftp_session: SharedSftpSession,
    },
    SftpNavigate(SessionId, PathBuf),
    SftpNavigateUp(SessionId),
    SftpRefresh(SessionId),
    SftpSelect(SessionId, usize),
    SftpListResult(SessionId, Result<Vec<FileEntry>, String>),
    SftpDownload(SessionId, PathBuf),
    SftpUpload(SessionId),
    SftpMkdir(SessionId),
    SftpDelete(SessionId, PathBuf),

    // SFTP dialog actions
    SftpMkdirNameChanged(String),
    SftpMkdirSubmit,
    SftpMkdirResult(SessionId, Result<PathBuf, String>),
    SftpDeleteConfirm,
    SftpDeleteResult(SessionId, Result<PathBuf, String>),
    SftpDownloadComplete(SessionId, Result<PathBuf, String>),
    SftpUploadComplete(SessionId, Result<(), String>),

    // Dual-pane SFTP browser
    DualSftpOpen,                                                    // Open dual-pane SFTP tab
    DualSftpPaneSourceChanged(SessionId, PaneId, PaneSource),        // Dropdown changed
    DualSftpPaneNavigate(SessionId, PaneId, PathBuf),               // Navigate to path
    DualSftpPaneNavigateUp(SessionId, PaneId),                      // Go to parent directory
    DualSftpPaneRefresh(SessionId, PaneId),                         // Refresh current directory
    DualSftpPaneSelect(SessionId, PaneId, usize),                   // Select file by index
    DualSftpPaneListResult(SessionId, PaneId, Result<Vec<FileEntry>, String>), // Directory listing result
    DualSftpPaneFocus(SessionId, PaneId),                           // Set active pane
    DualSftpConnectHost(SessionId, PaneId, Uuid),                   // Connect pane to remote host
    DualSftpConnected {                                              // Connection succeeded for pane
        tab_id: SessionId,
        pane_id: PaneId,
        sftp_session_id: SessionId,
        host_name: String,
        sftp_session: SharedSftpSession,
    },

    // UI navigation
    SearchChanged(String),
    FolderToggle(Uuid),
    ToggleTerminalDemo,

    // Sidebar navigation
    SidebarItemSelect(SidebarMenuItem),
    SidebarToggleCollapse,

    // OS Detection
    OsDetectionResult(Uuid, Result<DetectedOs, String>),

    // History
    HistoryClear,
    HistoryReconnect(Uuid),

    // Keyboard shortcuts
    KeyboardEvent(iced::keyboard::Key, iced::keyboard::Modifiers),

    // Settings
    SettingsOpen,
    SettingsThemeToggle(bool),

    // Snippets
    SnippetsOpen,
    SnippetSelect(Uuid),
    SnippetNew,
    SnippetEdit(Uuid),
    SnippetDelete(Uuid),
    SnippetInsert(Uuid),
    SnippetFieldChanged(SnippetField, String),
    SnippetEditCancel,
    SnippetSave,

    // Host key verification
    HostKeyVerification(VerificationRequestWrapper),
    HostKeyVerificationAccept,
    HostKeyVerificationReject,

    // Window resize
    WindowResized(iced::Size),

    // Toast notifications
    ToastDismiss(Uuid),  // User clicked X to dismiss
    ToastTick,           // Timer tick for auto-dismiss cleanup

    // Placeholder for future messages
    Noop,
}

/// Wrapper for event receiver that implements Clone (by wrapping in Option)
/// This allows it to be used in iced Messages which require Clone
#[derive(Debug)]
pub struct EventReceiver(pub Option<mpsc::UnboundedReceiver<SshEvent>>);

impl Clone for EventReceiver {
    fn clone(&self) -> Self {
        // Cloning returns None - only the original message has the receiver
        EventReceiver(None)
    }
}

/// Wrapper for host key verification request that implements Clone (by wrapping in Option)
/// The oneshot::Sender inside is not Clone, so we use the same pattern as EventReceiver
pub struct VerificationRequestWrapper(pub Option<Box<HostKeyVerificationRequest>>);

impl Clone for VerificationRequestWrapper {
    fn clone(&self) -> Self {
        // Cloning returns None - only the original message has the request
        VerificationRequestWrapper(None)
    }
}

impl std::fmt::Debug for VerificationRequestWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerificationRequestWrapper")
            .field("has_request", &self.0.is_some())
            .finish()
    }
}
