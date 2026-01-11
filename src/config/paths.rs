use directories::ProjectDirs;
use std::path::PathBuf;

/// Get the configuration directory path
/// Creates the directory if it doesn't exist
pub fn config_dir() -> Option<PathBuf> {
    ProjectDirs::from("com", "portal", "portal")
        .map(|proj_dirs| proj_dirs.config_dir().to_path_buf())
}

/// Get the path to the hosts config file
pub fn hosts_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("hosts.toml"))
}

/// Get the path to the known_hosts file
pub fn known_hosts_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("known_hosts"))
}

/// Get the path to the user's SSH known_hosts file
pub fn ssh_known_hosts_file() -> Option<PathBuf> {
    ssh_dir().map(|dir| dir.join("known_hosts"))
}

/// Get the path to the snippets config file
pub fn snippets_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("snippets.toml"))
}

/// Get the path to the history config file
pub fn history_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("history.toml"))
}

/// Get the path to the settings config file
pub fn settings_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("settings.toml"))
}

/// Get the path to the snippet execution history file
pub fn snippet_history_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("snippet_history.toml"))
}

/// Ensure the config directory exists with proper permissions
pub fn ensure_config_dir() -> std::io::Result<PathBuf> {
    let dir = config_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine config directory",
        )
    })?;

    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
        // Set restrictive permissions on Unix (owner-only access)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }

    Ok(dir)
}

/// Expand tilde in path (e.g., ~/.ssh/id_rsa -> /home/user/.ssh/id_rsa)
pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs_home() {
            return home.join(stripped);
        }
    }
    PathBuf::from(path)
}

/// Get the user's home directory
fn dirs_home() -> Option<PathBuf> {
    // Try directories crate first, fall back to HOME env var
    directories::BaseDirs::new()
        .map(|dirs| dirs.home_dir().to_path_buf())
        .or_else(|| std::env::var("HOME").ok().map(PathBuf::from))
}

/// Get the default SSH directory
pub fn ssh_dir() -> Option<PathBuf> {
    dirs_home().map(|home| home.join(".ssh"))
}

/// Get the default SSH identity files to try
pub fn default_identity_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Some(ssh_dir) = ssh_dir() {
        files.push(ssh_dir.join("id_ed25519"));
        files.push(ssh_dir.join("id_rsa"));
        files.push(ssh_dir.join("id_ecdsa"));
    }
    files
}

/// Get the log directory path
pub fn log_dir() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var("PORTAL_LOG_DIR") {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        return Some(PathBuf::from(trimmed));
    }

    config_dir().map(|d| d.join("logs"))
}

/// Ensure the log directory exists with proper permissions
pub fn ensure_log_dir() -> std::io::Result<PathBuf> {
    if std::env::var_os("PORTAL_LOG_DIR").is_none() {
        // First ensure parent config dir exists
        ensure_config_dir()?;
    }

    let dir = log_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine log directory",
        )
    })?;

    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }

    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde_with_tilde() {
        let path = expand_tilde("~/test/file.txt");
        // Should not start with ~ after expansion
        assert!(!path.to_string_lossy().starts_with("~/"));
        // Should end with the relative path
        assert!(path.to_string_lossy().ends_with("test/file.txt"));
    }

    #[test]
    fn test_expand_tilde_without_tilde() {
        let path = expand_tilde("/absolute/path");
        assert_eq!(path, PathBuf::from("/absolute/path"));
    }

    #[test]
    fn test_expand_tilde_relative_path() {
        let path = expand_tilde("relative/path");
        assert_eq!(path, PathBuf::from("relative/path"));
    }

    #[test]
    fn test_config_dir_returns_some() {
        // config_dir should return Some on most systems
        let dir = config_dir();
        assert!(dir.is_some());
    }

    #[test]
    fn test_log_dir_is_under_config_dir() {
        let config = config_dir();
        let log = log_dir();

        if let (Some(config_path), Some(log_path)) = (config, log) {
            assert!(log_path.starts_with(&config_path));
            assert!(log_path.ends_with("logs"));
        }
    }

    #[test]
    fn test_hosts_file_ends_with_toml() {
        let path = hosts_file();
        assert!(path.is_some());
        assert!(path.unwrap().to_string_lossy().ends_with("hosts.toml"));
    }

    #[test]
    fn test_default_identity_files_not_empty() {
        let files = default_identity_files();
        // Should have at least some identity files if home dir exists
        if ssh_dir().is_some() {
            assert!(!files.is_empty());
            assert!(files.iter().any(|f| f.ends_with("id_ed25519")));
        }
    }
}
