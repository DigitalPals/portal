//! Domain managers for Portal application state
//!
//! These managers encapsulate related state and operations,
//! reducing the complexity of the main Portal struct.

pub mod session_manager;
mod sftp_manager;
mod dialog_manager;
mod file_viewer_manager;

pub use session_manager::{ActiveSession, SessionManager};
pub use sftp_manager::SftpManager;
pub use dialog_manager::{ActiveDialog, DialogManager};
pub use file_viewer_manager::FileViewerManager;
