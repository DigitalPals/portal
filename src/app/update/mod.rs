//! Update handler modules for the Portal application
//!
//! This module contains the update handlers for each message category,
//! breaking down the monolithic update() function into focused handlers.

mod session;
mod sftp;
mod file_viewer;
mod dialog;
mod tab;
mod host;
mod history;
mod snippet;
mod ui;

pub use session::handle_session;
pub use sftp::handle_sftp;
pub use file_viewer::handle_file_viewer;
pub use dialog::handle_dialog;
pub use tab::handle_tab;
pub use host::handle_host;
pub use history::handle_history;
pub use snippet::handle_snippet;
pub use ui::handle_ui;
