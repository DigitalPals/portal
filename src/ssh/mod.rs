//! SSH client module for Portal
//!
//! Provides SSH connection, authentication, and session management.

pub mod auth;
pub mod client;
pub mod handler;
pub mod host_key_verification;
pub mod known_hosts;
pub mod session;

pub use client::SshClient;
pub use host_key_verification::{
    HostKeyInfo, HostKeyVerificationRequest, HostKeyVerificationResponse,
};
pub use known_hosts::{HostKeyStatus, KnownHostsManager};
pub use session::SshSession;

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
    /// Error occurred
    Error(String),
}
