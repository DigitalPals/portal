//! Update handler modules for the Portal application
//!
//! This module contains the update handlers for each message category,
//! breaking down the monolithic update() function into focused handlers.

mod dialog;
mod file_viewer;
mod history;
mod host;
mod session;
mod sftp;
mod snippet;
mod tab;
mod ui;
mod vnc;

pub use dialog::handle_dialog;
pub use file_viewer::handle_file_viewer;
pub use history::handle_history;
pub use host::handle_host;
pub use session::handle_session;
pub use sftp::handle_sftp;
pub use snippet::handle_snippet;
pub use tab::handle_tab;
pub use ui::handle_ui;
pub use vnc::handle_vnc;
