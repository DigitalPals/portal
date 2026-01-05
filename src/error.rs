use std::path::PathBuf;
use thiserror::Error;

/// Top-level application error
#[derive(Error, Debug)]
pub enum PortalError {
    #[error("Configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("SSH error: {0}")]
    Ssh(#[from] SshError),

    #[error("SFTP error: {0}")]
    Sftp(#[from] SftpError),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Configuration-related errors
#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Failed to read config file '{path}': {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("Failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("Failed to write config file '{path}': {source}")]
    WriteFile {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Host not found: {0}")]
    HostNotFound(uuid::Uuid),

    #[error("Group not found: {0}")]
    GroupNotFound(uuid::Uuid),

    #[error("Snippet not found: {0}")]
    SnippetNotFound(uuid::Uuid),

    #[error("Invalid host data: {0}")]
    Validation(String),

    #[error("Failed to create config directory: {0}")]
    CreateDir(std::io::Error),
}

/// SSH-related errors
#[derive(Error, Debug)]
pub enum SshError {
    #[error("Connection failed to {host}:{port}: {reason}")]
    ConnectionFailed {
        host: String,
        port: u16,
        reason: String,
    },

    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    #[error("Key file error: {0}")]
    KeyFile(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("Timeout connecting to {0}")]
    Timeout(String),

    #[error("SSH agent error: {0}")]
    Agent(String),

    #[error("Host key verification failed: {0}")]
    HostKeyVerification(String),

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Disconnected")]
    Disconnected,

    #[error("russh error: {0}")]
    Russh(String),
}

impl From<russh::Error> for SshError {
    fn from(err: russh::Error) -> Self {
        SshError::Russh(err.to_string())
    }
}

/// SFTP-related errors
#[derive(Error, Debug)]
pub enum SftpError {
    #[error("SFTP connection failed: {0}")]
    ConnectionFailed(String),

    #[error("File operation failed: {0}")]
    FileOperation(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("File not found: {0}")]
    NotFound(String),

    #[error("Transfer failed: {0}")]
    Transfer(String),

    #[error("Local I/O error: {0}")]
    LocalIo(String),

    #[error("Remote error: {0}")]
    Remote(String),

    #[error("SFTP protocol error: {0}")]
    Protocol(String),
}

/// Result type alias for convenience
pub type Result<T> = std::result::Result<T, PortalError>;
