//! Domain managers for Portal application state
//!
//! These managers encapsulate related state and operations,
//! reducing the complexity of the main Portal struct.

mod dialog_manager;
mod file_viewer_manager;
pub mod session_manager;
mod sftp_manager;

pub use dialog_manager::{ActiveDialog, DialogManager};
pub use file_viewer_manager::FileViewerManager;
pub use session_manager::{ActiveSession, SessionBackend, SessionManager};
pub use sftp_manager::SftpManager;
