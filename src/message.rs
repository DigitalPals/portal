use std::path::PathBuf;
use std::sync::Arc;

use uuid::Uuid;

use crate::config::DetectedOs;
use crate::sftp::{FileEntry, SharedSftpSession};
use crate::ssh::host_key_verification::HostKeyVerificationRequest;
use crate::ssh::SshSession;
use crate::views::sftp::{ContextMenuAction, PaneId, PaneSource, PermissionBit};

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
pub enum Message {
    // Host management
    HostConnect(Uuid),
    HostAdd,
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

    // Dual-pane SFTP browser
    DualSftpOpen,                                                    // Open dual-pane SFTP tab
    DualSftpPaneSourceChanged(SessionId, PaneId, PaneSource),        // Dropdown changed
    DualSftpPaneNavigate(SessionId, PaneId, PathBuf),               // Navigate to path
    DualSftpPaneNavigateUp(SessionId, PaneId),                      // Go to parent directory
    DualSftpPaneRefresh(SessionId, PaneId),                         // Refresh current directory
    DualSftpPaneSelect(SessionId, PaneId, usize),                   // Select file by index
    DualSftpPaneListResult(SessionId, PaneId, Result<Vec<FileEntry>, String>), // Directory listing result
    DualSftpConnectHost(SessionId, PaneId, Uuid),                   // Connect pane to remote host
    DualSftpConnected {                                              // Connection succeeded for pane
        tab_id: SessionId,
        pane_id: PaneId,
        sftp_session_id: SessionId,
        host_id: Uuid,
        host_name: String,
        sftp_session: SharedSftpSession,
    },

    // Context menu
    DualSftpShowContextMenu(SessionId, PaneId, f32, f32, Option<usize>), // Show context menu at position, optionally selecting item
    DualSftpHideContextMenu(SessionId),                              // Hide context menu
    DualSftpContextMenuAction(SessionId, ContextMenuAction),         // Execute context menu action

    // SFTP dialogs (New Folder, Rename, Delete, Permissions)
    DualSftpDialogInputChanged(SessionId, String),                   // Dialog input text changed
    DualSftpDialogCancel(SessionId),                                 // Cancel/close dialog
    DualSftpDialogSubmit(SessionId),                                 // Submit dialog action
    DualSftpNewFolderResult(SessionId, PaneId, Result<(), String>),  // Result of folder creation
    DualSftpRenameResult(SessionId, PaneId, Result<(), String>),     // Result of rename operation
    DualSftpDeleteResult(SessionId, PaneId, Result<usize, String>),  // Result of delete (count deleted)
    DualSftpPermissionToggle(SessionId, PermissionBit, bool),        // Toggle a permission checkbox
    DualSftpPermissionsResult(SessionId, PaneId, Result<(), String>), // Result of chmod operation

    // SFTP file transfer (Copy to Target)
    DualSftpCopyToTarget(SessionId),                                 // Start copying selected files to target pane
    DualSftpCopyResult(SessionId, PaneId, Result<usize, String>),    // Result of copy (count copied, target pane)

    // Open With result
    DualSftpOpenWithResult(Result<(), String>),                      // Result of open with command

    // UI navigation
    SearchChanged(String),
    FolderToggle(Uuid),

    // Sidebar navigation
    SidebarItemSelect(SidebarMenuItem),
    SidebarToggleCollapse,

    // History
    HistoryClear,
    HistoryReconnect(Uuid),

    // Keyboard shortcuts
    KeyboardEvent(iced::keyboard::Key, iced::keyboard::Modifiers),

    SettingsThemeToggle(bool),
    SettingsFontSizeChange(f32),

    // Snippets
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

    // Session duration and SSH key installation
    SessionDurationTick,                                   // Timer tick for duration updates
    InstallSshKey(SessionId),                              // User pressed Ctrl+Shift+K
    InstallSshKeyResult(SessionId, Result<bool, String>),  // bool = was_newly_installed

    // Placeholder for future messages
    Noop,
}

/// Wrapper for host key verification request that implements Clone (by wrapping in Option)
/// The oneshot::Sender inside is not Clone, so we use a wrapper to allow cloning.
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
