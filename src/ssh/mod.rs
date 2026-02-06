//! SSH client module for Portal
//!
//! Provides SSH connection, authentication, and session management.

pub mod auth;
pub mod client;
pub mod handler;
pub mod host_key_verification;
pub mod key_install;
pub mod known_hosts;
pub mod os_detect;
pub mod reconnect;
pub mod session;

pub use client::SshClient;
pub use key_install::install_ssh_key;
pub use session::SshSession;

use host_key_verification::HostKeyVerificationRequest;

/// Events emitted by the SSH layer
#[derive(Debug)]
pub enum SshEvent {
    /// SSH connection established
    Connected,
    /// Data received from remote
    Data(Vec<u8>),
    /// Connection closed
    Disconnected,
    /// Host key verification required
    HostKeyVerification(Box<HostKeyVerificationRequest>),
}
