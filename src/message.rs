use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::mpsc;
use uuid::Uuid;

use crate::config::Host;
use crate::sftp::{FileEntry, SharedSftpSession};
use crate::ssh::{SshEvent, SshSession};

/// Session ID type alias
pub type SessionId = Uuid;

/// Application messages for the Elm-style update loop
#[derive(Debug, Clone)]
pub enum Message {
    // Host management
    HostSelected(Uuid),
    HostConnect(Uuid),
    HostAdd,
    HostEdit(Uuid),
    HostSave(Host),
    HostDelete(Uuid),

    // Dialog
    DialogOpen(DialogType),
    DialogClose,
    DialogSubmit,
    DialogFieldChanged(String, String),

    // Terminal / Session
    TerminalInput(SessionId, Vec<u8>),
    TerminalOutput(SessionId, Vec<u8>),
    SessionCreated(SessionId),
    SessionClosed(SessionId),

    // SSH connection
    SshConnected {
        session_id: SessionId,
        host_name: String,
        ssh_session: Arc<SshSession>,
        event_rx: EventReceiver,
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
    SftpError(SessionId, String),

    // SFTP dialog actions
    SftpMkdirNameChanged(String),
    SftpMkdirSubmit,
    SftpMkdirResult(SessionId, Result<PathBuf, String>),
    SftpDeleteConfirm,
    SftpDeleteResult(SessionId, Result<PathBuf, String>),
    SftpDownloadComplete(SessionId, Result<PathBuf, String>),
    SftpUploadComplete(SessionId, Result<(), String>),

    // UI navigation
    SearchChanged(String),
    FolderToggle(Uuid),
    ToggleTerminalDemo,

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
    SnippetFieldChanged(String, String),
    SnippetEditCancel,
    SnippetSave,

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

/// Types of dialogs that can be opened
#[derive(Debug, Clone)]
pub enum DialogType {
    AddHost,
    EditHost(Uuid),
    SftpMkdir(SessionId, PathBuf),
    SftpDeleteConfirm(SessionId, PathBuf, bool), // session_id, path, is_directory
}
