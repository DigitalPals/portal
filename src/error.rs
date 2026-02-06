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

    #[error("Key file requires passphrase: {0}")]
    KeyFilePassphraseRequired(PathBuf),

    #[error("Key file passphrase is invalid: {0}")]
    KeyFilePassphraseInvalid(PathBuf),

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

    #[error("Key file requires passphrase: {0}")]
    KeyFilePassphraseRequired(PathBuf),

    #[error("Key file passphrase is invalid: {0}")]
    KeyFilePassphraseInvalid(PathBuf),

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

#[cfg(test)]
mod tests {
    use super::*;

    // ---- ConfigError Display tests ----

    #[test]
    fn config_error_read_file_display() {
        let err = ConfigError::ReadFile {
            path: PathBuf::from("/home/user/.config/portal/hosts.toml"),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        };
        let msg = err.to_string();
        assert!(msg.contains("/home/user/.config/portal/hosts.toml"));
        assert!(msg.contains("file not found"));
    }

    #[test]
    fn config_error_write_file_display() {
        let err = ConfigError::WriteFile {
            path: PathBuf::from("/tmp/test.toml"),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "permission denied"),
        };
        let msg = err.to_string();
        assert!(msg.contains("/tmp/test.toml"));
        assert!(msg.contains("permission denied"));
    }

    #[test]
    fn config_error_host_not_found_display() {
        let id = uuid::Uuid::new_v4();
        let err = ConfigError::HostNotFound(id);
        assert!(err.to_string().contains(&id.to_string()));
    }

    #[test]
    fn config_error_snippet_not_found_display() {
        let id = uuid::Uuid::new_v4();
        let err = ConfigError::SnippetNotFound(id);
        assert!(err.to_string().contains(&id.to_string()));
    }

    // ---- SshError Display tests ----

    #[test]
    fn ssh_error_connection_failed_display() {
        let err = SshError::ConnectionFailed {
            host: "example.com".to_string(),
            port: 22,
            reason: "network unreachable".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("example.com"));
        assert!(msg.contains("22"));
        assert!(msg.contains("network unreachable"));
    }

    #[test]
    fn ssh_error_auth_failed_display() {
        let err = SshError::AuthenticationFailed("invalid key".to_string());
        assert!(err.to_string().contains("invalid key"));
    }

    #[test]
    fn ssh_error_timeout_display() {
        let err = SshError::Timeout("server.example.com".to_string());
        assert!(err.to_string().contains("server.example.com"));
    }

    #[test]
    fn ssh_error_key_file_passphrase_required() {
        let err = SshError::KeyFilePassphraseRequired(PathBuf::from("/home/user/.ssh/id_rsa"));
        assert!(err.to_string().contains("/home/user/.ssh/id_rsa"));
    }

    #[test]
    fn ssh_error_key_file_passphrase_invalid() {
        let err = SshError::KeyFilePassphraseInvalid(PathBuf::from("/home/user/.ssh/id_ed25519"));
        assert!(err.to_string().contains("/home/user/.ssh/id_ed25519"));
    }

    // ---- SftpError Display tests ----

    #[test]
    fn sftp_error_connection_failed_display() {
        let err = SftpError::ConnectionFailed("handshake timeout".to_string());
        assert!(err.to_string().contains("handshake timeout"));
    }

    #[test]
    fn sftp_error_file_operation_display() {
        let err = SftpError::FileOperation("failed to create directory".to_string());
        assert!(err.to_string().contains("failed to create directory"));
    }

    #[test]
    fn sftp_error_transfer_display() {
        let err = SftpError::Transfer("connection reset".to_string());
        assert!(err.to_string().contains("connection reset"));
    }

    // ---- LocalError Display tests ----

    #[test]
    fn local_error_pty_creation_display() {
        let err = LocalError::PtyCreation("no available PTY".to_string());
        assert!(err.to_string().contains("no available PTY"));
    }

    #[test]
    fn local_error_spawn_failed_display() {
        let err = LocalError::SpawnFailed("shell not found".to_string());
        assert!(err.to_string().contains("shell not found"));
    }
}
