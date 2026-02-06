//! SSH client module for Portal
//!
//! Provides SSH connection, authentication, and session management.

pub mod auth;
pub mod client;
pub mod connection_pool;
pub mod handler;
pub mod host_key_verification;
pub mod key_install;
pub mod known_hosts;
pub mod os_detect;
pub mod passphrase_cache;
pub mod reconnect;
pub mod session;

pub use client::SshClient;
pub use connection_pool::{SshConnection, SshConnectionKey, SshConnectionPool};
pub use key_install::install_ssh_key;
pub use passphrase_cache::PassphraseCache;
pub use session::SshSession;

use std::sync::{Arc, OnceLock};

use host_key_verification::HostKeyVerificationRequest;

/// Events emitted by the SSH layer
#[derive(Debug)]
pub enum SshEvent {
    /// SSH connection established
    Connected,
    /// Data received from remote
    Data(Vec<u8>),
    /// Connection closed
    /// `clean` is true if the shell exited normally (exit status 0), false for unexpected drops
    Disconnected { clean: bool },
    /// Host key verification required
    HostKeyVerification(Box<HostKeyVerificationRequest>),
}

static SSH_CONNECTION_POOL: OnceLock<Arc<SshConnectionPool>> = OnceLock::new();

pub fn shared_connection_pool() -> Arc<SshConnectionPool> {
    SSH_CONNECTION_POOL
        .get_or_init(|| Arc::new(SshConnectionPool::new()))
        .clone()
}
