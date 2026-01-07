use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::text_editor;
use uuid::Uuid;

use crate::config::DetectedOs;
use crate::local::LocalSession;
use crate::sftp::{FileEntry, SharedSftpSession};
use crate::ssh::SshSession;
use crate::ssh::host_key_verification::HostKeyVerificationRequest;
use crate::terminal::backend::TerminalEvent;
use crate::theme::ThemeId;
use crate::views::file_viewer::ViewerContent;
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
    About,
}

#[derive(Debug, Clone)]
pub enum HostDialogField {
    Name,
    Hostname,
    Port,
    Username,
    AuthMethod,
    KeyPath,
    Tags,
    Notes,
}

#[derive(Debug, Clone)]
pub enum SnippetField {
    Name,
    Command,
    Description,
}

// ============================================================================
// Nested Message Enums
// ============================================================================

/// Terminal session-related messages
#[derive(Debug, Clone)]
pub enum SessionMessage {
    /// SSH connection established
    Connected {
        session_id: SessionId,
        host_name: String,
        ssh_session: Arc<SshSession>,
        host_id: Uuid,
        detected_os: Option<DetectedOs>,
    },
    /// Local terminal session established
    LocalConnected {
        session_id: SessionId,
        local_session: Arc<LocalSession>,
    },
    /// Data received from terminal (SSH or local)
    Data(SessionId, Vec<u8>),
    /// Process buffered terminal output in time-sliced chunks
    ProcessOutputTick,
    /// Terminal session disconnected
    Disconnected(SessionId),
    /// Session error occurred
    Error(String),
    /// Terminal input from user
    Input(SessionId, Vec<u8>),
    /// Terminal resize event
    Resize(SessionId, u16, u16),
    /// Terminal backend event (title/bell/clipboard/exit)
    TerminalEvent(SessionId, TerminalEvent),
    /// Clipboard content read for terminal
    ClipboardLoaded(SessionId, Option<String>),
    /// Timer tick for session duration updates
    DurationTick,
    /// User pressed Ctrl+Shift+K to install SSH key
    InstallKey(SessionId),
    /// Result of SSH key installation (bool = was_newly_installed)
    InstallKeyResult(SessionId, Result<bool, String>),
}

/// SFTP browser messages
#[derive(Debug, Clone)]
pub enum SftpMessage {
    /// Open dual-pane SFTP browser tab
    Open,
    /// Pane source dropdown changed
    PaneSourceChanged(SessionId, PaneId, PaneSource),
    /// Navigate to path in pane
    PaneNavigate(SessionId, PaneId, PathBuf),
    /// Navigate to parent directory
    PaneNavigateUp(SessionId, PaneId),
    /// Refresh current directory
    PaneRefresh(SessionId, PaneId),
    /// Select file by index
    PaneSelect(SessionId, PaneId, usize),
    /// Directory listing result
    PaneListResult(SessionId, PaneId, Result<Vec<FileEntry>, String>),
    /// Connect pane to remote host
    ConnectHost(SessionId, PaneId, Uuid),
    /// SFTP connection succeeded for pane
    Connected {
        tab_id: SessionId,
        pane_id: PaneId,
        sftp_session_id: SessionId,
        host_id: Uuid,
        host_name: String,
        sftp_session: SharedSftpSession,
    },
    /// Show context menu at position
    ShowContextMenu(SessionId, PaneId, f32, f32, Option<usize>),
    /// Hide context menu
    HideContextMenu(SessionId),
    /// Execute context menu action
    ContextMenuAction(SessionId, ContextMenuAction),
    /// Dialog input text changed
    DialogInputChanged(SessionId, String),
    /// Cancel/close SFTP dialog
    DialogCancel(SessionId),
    /// Submit SFTP dialog action
    DialogSubmit(SessionId),
    /// Result of folder creation
    NewFolderResult(SessionId, PaneId, Result<(), String>),
    /// Result of rename operation
    RenameResult(SessionId, PaneId, Result<(), String>),
    /// Result of delete operation (count deleted)
    DeleteResult(SessionId, PaneId, Result<usize, String>),
    /// Toggle a permission checkbox
    PermissionToggle(SessionId, PermissionBit, bool),
    /// Result of chmod operation
    PermissionsResult(SessionId, PaneId, Result<(), String>),
    /// Start copying selected files to target pane
    CopyToTarget(SessionId),
    /// Result of copy operation (count copied, target pane)
    CopyResult(SessionId, PaneId, Result<usize, String>),
    /// Toggle hidden files visibility
    ToggleShowHidden(SessionId, PaneId),
    /// Toggle actions menu visibility
    ToggleActionsMenu(SessionId, PaneId),
    /// Filter text changed
    FilterChanged(SessionId, PaneId, String),
    /// Navigate to specific breadcrumb path segment
    PaneBreadcrumbNavigate(SessionId, PaneId, PathBuf),
}

/// Dialog-related messages
#[derive(Debug, Clone)]
pub enum DialogMessage {
    /// Close any open dialog
    Close,
    /// Submit host dialog
    Submit,
    /// Host dialog field changed
    FieldChanged(HostDialogField, String),
    /// Host key verification request received
    HostKeyVerification(VerificationRequestWrapper),
    /// User accepted host key
    HostKeyAccept,
    /// User rejected host key
    HostKeyReject,
}

/// Tab management messages
#[derive(Debug, Clone)]
pub enum TabMessage {
    /// Select a tab
    Select(Uuid),
    /// Close a tab
    Close(Uuid),
    /// Open new tab (go to host grid)
    New,
    /// Track which tab is being hovered (for showing close button)
    Hover(Option<Uuid>),
}

/// Host management messages
#[derive(Debug, Clone)]
pub enum HostMessage {
    /// Connect to a host by ID
    Connect(Uuid),
    /// Open add host dialog
    Add,
    /// Open edit host dialog for existing host
    Edit(Uuid),
    /// Track which host is being hovered (for showing edit button)
    Hover(Option<Uuid>),
    /// Quick connect using search query
    QuickConnect,
    /// Open local terminal (stubbed)
    LocalTerminal,
}

/// History management messages
#[derive(Debug, Clone)]
pub enum HistoryMessage {
    /// Clear all history
    Clear,
    /// Reconnect to a history entry
    Reconnect(Uuid),
}

/// Result of executing a snippet command on a single host
#[derive(Debug, Clone)]
pub struct HostExecutionResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Snippet management messages
#[derive(Debug, Clone)]
pub enum SnippetMessage {
    /// Select a snippet (show results panel)
    Select(Uuid),
    /// Create new snippet
    New,
    /// Edit existing snippet
    Edit(Uuid),
    /// Delete a snippet
    Delete(Uuid),
    /// Snippet field changed
    FieldChanged(SnippetField, String),
    /// Cancel snippet edit
    EditCancel,
    /// Save snippet changes
    Save,

    // Page navigation
    /// Search query changed on snippets page
    SearchChanged(String),
    /// Track which snippet is being hovered
    Hover(Option<Uuid>),

    // Host association (during edit)
    /// Toggle host selection in edit form
    ToggleHost(Uuid, bool),

    // Execution
    /// Run snippet on associated hosts
    Run(Uuid),
    /// Single host execution result received
    HostResult {
        snippet_id: Uuid,
        host_id: Uuid,
        result: Result<HostExecutionResult, String>,
        duration_ms: u64,
    },

    // Results panel
    /// Deselect snippet (close results panel)
    Deselect,
    /// Toggle expand/collapse of host result output
    ToggleResultExpand(Uuid, Uuid),
    /// Clear results for a snippet
    ClearResults(Uuid),
    /// View a historical execution entry
    ViewHistoryEntry(Uuid),
    /// Return to current results from history view
    ViewCurrentResults,
}

/// File viewer messages
#[derive(Debug, Clone)]
pub enum FileViewerMessage {
    /// File content loaded successfully
    ContentLoaded {
        viewer_id: SessionId,
        content: ViewerContent,
    },
    /// Error loading file content
    LoadError(SessionId, String),
    /// Text content changed via editor
    TextChanged(SessionId, text_editor::Action),
    /// Save current content
    Save(SessionId),
    /// Save operation completed
    SaveResult(SessionId, Result<(), String>),
    /// PDF page navigation
    PdfPageChange(SessionId, usize),
    /// Render a PDF page on demand
    PdfRenderPage(SessionId, usize),
    /// PDF page rendered
    PdfPageRendered(SessionId, usize, Result<Vec<u8>, String>),
    /// Toggle markdown preview mode
    MarkdownTogglePreview(SessionId),
    /// Image zoom level changed
    ImageZoom(SessionId, f32),
}

/// UI state messages
#[derive(Debug, Clone)]
pub enum UiMessage {
    /// Search query changed
    SearchChanged(String),
    /// Toggle folder collapsed state
    FolderToggle(Uuid),
    /// Sidebar item selected
    SidebarItemSelect(SidebarMenuItem),
    /// Toggle sidebar collapsed state
    SidebarToggleCollapse,
    /// Theme changed
    ThemeChange(ThemeId),
    /// Terminal font changed
    FontChange(crate::fonts::TerminalFont),
    /// Terminal font size changed
    FontSizeChange(f32),
    /// Window resized
    WindowResized(iced::Size),
    /// Dismiss toast notification
    ToastDismiss(Uuid),
    /// Toast timer tick
    ToastTick,
    /// Keyboard event
    KeyboardEvent(iced::keyboard::Key, iced::keyboard::Modifiers),
}

// ============================================================================
// Main Message Enum
// ============================================================================

/// Application messages for the Elm-style update loop
#[derive(Debug, Clone)]
pub enum Message {
    /// SSH session messages
    Session(SessionMessage),
    /// SFTP browser messages
    Sftp(SftpMessage),
    /// File viewer messages
    FileViewer(FileViewerMessage),
    /// Dialog messages
    Dialog(DialogMessage),
    /// Tab management messages
    Tab(TabMessage),
    /// Host management messages
    Host(HostMessage),
    /// History messages
    History(HistoryMessage),
    /// Snippet messages
    Snippet(SnippetMessage),
    /// UI state messages
    Ui(UiMessage),
    /// No-op placeholder
    Noop,
}

// ============================================================================
// Convenience From implementations
// ============================================================================

impl From<SessionMessage> for Message {
    fn from(msg: SessionMessage) -> Self {
        Message::Session(msg)
    }
}

impl From<SftpMessage> for Message {
    fn from(msg: SftpMessage) -> Self {
        Message::Sftp(msg)
    }
}

impl From<DialogMessage> for Message {
    fn from(msg: DialogMessage) -> Self {
        Message::Dialog(msg)
    }
}

impl From<TabMessage> for Message {
    fn from(msg: TabMessage) -> Self {
        Message::Tab(msg)
    }
}

impl From<HostMessage> for Message {
    fn from(msg: HostMessage) -> Self {
        Message::Host(msg)
    }
}

impl From<HistoryMessage> for Message {
    fn from(msg: HistoryMessage) -> Self {
        Message::History(msg)
    }
}

impl From<SnippetMessage> for Message {
    fn from(msg: SnippetMessage) -> Self {
        Message::Snippet(msg)
    }
}

impl From<UiMessage> for Message {
    fn from(msg: UiMessage) -> Self {
        Message::Ui(msg)
    }
}

impl From<FileViewerMessage> for Message {
    fn from(msg: FileViewerMessage) -> Self {
        Message::FileViewer(msg)
    }
}

// ============================================================================
// Wrapper Types
// ============================================================================

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
