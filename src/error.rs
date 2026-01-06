use std::path::PathBuf;
use thiserror::Error;

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

    #[error("Snippet not found: {0}")]
    SnippetNotFound(uuid::Uuid),

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

    #[error("Channel error: {0}")]
    Channel(String),

    #[error("Timeout connecting to {0}")]
    Timeout(String),

    #[error("SSH agent error: {0}")]
    Agent(String),

    #[error("Host key verification failed: {0}")]
    HostKeyVerification(String),

    #[error("russh error: {0}")]
    Russh(String),

    #[error("SSH key installation failed: {0}")]
    KeyInstall(String),
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

    #[error("Transfer failed: {0}")]
    Transfer(String),

    #[error("Local I/O error: {0}")]
    LocalIo(String),
}

/// Local terminal errors
#[derive(Error, Debug)]
pub enum LocalError {
    #[error("Failed to create PTY: {0}")]
    PtyCreation(String),

    #[error("Failed to spawn shell: {0}")]
    SpawnFailed(String),

    #[error("PTY I/O error: {0}")]
    Io(String),
}
