//! SFTP client module for Portal
//!
//! Provides SFTP file browsing and transfer capabilities.

pub mod client;
pub mod session;
pub mod types;

pub use client::SftpClient;
pub use session::SharedSftpSession;
pub use types::{FileEntry, FileIcon, SortOrder, format_size, is_safe_sftp_entry_name};
