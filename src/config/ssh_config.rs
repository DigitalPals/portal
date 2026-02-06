use std::path::PathBuf;

use chrono::Utc;
use uuid::Uuid;

use crate::config::paths::{expand_tilde, ssh_dir};
use crate::config::{AuthMethod, Host, Protocol};
use crate::error::ConfigError;

#[derive(Default, Debug, Clone)]
struct HostBlock {
    patterns: Vec<String>,
    hostname: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<PathBuf>,
}

pub fn load_hosts_from_ssh_config() -> Result<Vec<Host>, ConfigError> {
    let path = ssh_dir()
        .map(|dir| dir.join("config"))
        .ok_or_else(|| ConfigError::ReadFile {
            path: PathBuf::from("~/.ssh/config"),
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine SSH config path",
            ),
        })?;

    if !path.exists() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&path).map_err(|e| ConfigError::ReadFile {
        path: path.clone(),
        source: e,
    })?;

    Ok(parse_ssh_config(&content))
}

pub fn parse_ssh_config(content: &str) -> Vec<Host> {
    let mut hosts = Vec::new();
    let mut current = HostBlock::default();
    let mut in_match_block = false;

    for raw_line in content.lines() {
        let line = strip_comments(raw_line);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let tokens = split_tokens(line);
        if tokens.is_empty() {
            continue;
        }

        let key = tokens[0].to_ascii_lowercase();
        if key == "match" {
            // Ignore Match blocks for imports.
            flush_block(&mut current, &mut hosts);
            in_match_block = true;
            continue;
        }

        if key == "host" {
            if in_match_block {
                in_match_block = false;
            }
            flush_block(&mut current, &mut hosts);
            current = HostBlock::default();
            current.patterns = tokens[1..].iter().map(|s| s.to_string()).collect();
            continue;
        }

        if in_match_block || current.patterns.is_empty() {
            continue;
        }

        match key.as_str() {
            "hostname" => {
                if let Some(value) = tokens.get(1) {
                    current.hostname = Some(value.to_string());
                }
            }
            "user" => {
                if let Some(value) = tokens.get(1) {
                    current.user = Some(value.to_string());
                }
            }
            "port" => {
                if let Some(value) = tokens.get(1) {
                    if let Ok(port) = value.parse::<u16>() {
                        current.port = Some(port);
                    }
                }
            }
            "identityfile" => {
                if let Some(value) = tokens.get(1) {
                    current.identity_file = Some(expand_identity_path(value));
                }
            }
            _ => {}
        }
    }

    flush_block(&mut current, &mut hosts);
    hosts
}

fn flush_block(current: &mut HostBlock, hosts: &mut Vec<Host>) {
    if current.patterns.is_empty() {
        return;
    }

    for pattern in &current.patterns {
        if should_skip_pattern(pattern) {
            continue;
        }

        let hostname = current
            .hostname
            .as_ref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| pattern.to_string());
        let username = current
            .user
            .as_ref()
            .map(|s| s.to_string())
            .unwrap_or_else(default_user);
        let port = current.port.unwrap_or(22);

        let auth = match &current.identity_file {
            Some(path) => AuthMethod::PublicKey {
                key_path: Some(path.clone()),
            },
            None => AuthMethod::Agent,
        };

        let now = Utc::now();
        hosts.push(Host {
            id: Uuid::new_v4(),
            name: pattern.to_string(),
            hostname,
            port,
            username,
            protocol: Protocol::Ssh,
            vnc_port: None,
            auth,
            group_id: None,
            notes: None,
            tags: Vec::new(),
            created_at: now,
            updated_at: now,
            detected_os: None,
            last_connected: None,
        });
    }
}

fn should_skip_pattern(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return true;
    }

    trimmed.starts_with('!')
        || trimmed.contains('*')
        || trimmed.contains('?')
        || trimmed.contains('[')
        || trimmed.contains(']')
}

fn default_user() -> String {
    std::env::var("USER").unwrap_or_else(|_| "root".to_string())
}

fn expand_identity_path(raw: &str) -> PathBuf {
    let cleaned = raw.trim_matches('"');
    let expanded = expand_tilde(cleaned);
    if expanded.is_absolute() {
        return expanded;
    }

    if let Some(dir) = ssh_dir() {
        return dir.join(expanded);
    }

    expanded
}

fn strip_comments(line: &str) -> String {
    let mut result = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
                result.push(ch);
            }
            '#' if !in_quotes => break,
            _ => result.push(ch),
        }
    }
    result
}

fn split_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;

    while let Some(ch) = chars.next() {
        match ch {
            '"' => {
                in_quotes = !in_quotes;
            }
            '\\' => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_host() {
        let content = r#"
            Host my-server
              HostName 10.0.0.5
              User alice
              Port 2222
              IdentityFile ~/.ssh/id_ed25519
        "#;

        let hosts = parse_ssh_config(content);
        assert_eq!(hosts.len(), 1);
        let host = &hosts[0];
        assert_eq!(host.name, "my-server");
        assert_eq!(host.hostname, "10.0.0.5");
        assert_eq!(host.username, "alice");
        assert_eq!(host.port, 2222);
        match &host.auth {
            AuthMethod::PublicKey { key_path } => {
                assert!(key_path.is_some());
            }
            _ => panic!("expected public key auth"),
        }
    }

    #[test]
    fn parse_missing_hostname_defaults_to_alias() {
        let content = "Host example\n  User bob\n";
        let hosts = parse_ssh_config(content);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "example");
    }

    #[test]
    fn parse_defaults_port_and_user() {
        let content = "Host example";
        let hosts = parse_ssh_config(content);
        let expected_user = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].port, 22);
        assert_eq!(hosts[0].username, expected_user);
    }

    #[test]
    fn parse_skips_wildcards_and_patterns() {
        let content = r#"
            Host *
              User root
            Host web*
              HostName web.example.com
            Host api
              HostName api.example.com
        "#;
        let hosts = parse_ssh_config(content);
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "api");
    }

    #[test]
    fn parse_multiple_hosts_in_block() {
        let content = r#"
            Host alpha beta
              HostName 192.168.1.10
        "#;
        let hosts = parse_ssh_config(content);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].hostname, "192.168.1.10");
        assert_eq!(hosts[1].hostname, "192.168.1.10");
    }

    #[test]
    fn identity_file_relative_expands_with_ssh_dir() {
        let content = "Host test\n  IdentityFile keys/id_rsa\n";
        let hosts = parse_ssh_config(content);
        assert_eq!(hosts.len(), 1);
        if let AuthMethod::PublicKey { key_path } = &hosts[0].auth {
            let key_path = key_path.as_ref().expect("missing key path");
            if let Some(dir) = ssh_dir() {
                assert_eq!(key_path, &dir.join("keys/id_rsa"));
            }
        } else {
            panic!("expected public key auth");
        }
    }
}
