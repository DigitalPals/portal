//! Domain managers for Portal application state
//!
//! These managers encapsulate related state and operations,
//! reducing the complexity of the main Portal struct.

mod dialog_manager;
mod file_viewer_manager;
pub mod session_manager;
mod sftp_manager;
mod snippet_execution_manager;

pub use dialog_manager::{ActiveDialog, DialogManager};
pub use file_viewer_manager::FileViewerManager;
pub use session_manager::{ActiveSession, SessionBackend, SessionManager, VncActiveSession};
pub use sftp_manager::SftpManager;
pub use snippet_execution_manager::{
    ExecutionStatus, HostResult, SnippetExecution, SnippetExecutionManager,
};
