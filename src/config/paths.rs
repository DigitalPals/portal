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

/// Ensure the config directory exists
pub fn ensure_config_dir() -> std::io::Result<PathBuf> {
    let dir = config_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine config directory",
        )
    })?;

    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
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
