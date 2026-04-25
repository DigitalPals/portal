use std::path::PathBuf;

use chrono::Utc;
use uuid::Uuid;

use crate::config::hosts::default_username;
use crate::config::paths::{expand_tilde, ssh_dir};
use crate::config::{AuthMethod, Host, Protocol};
use crate::error::ConfigError;

#[derive(Default, Debug, Clone)]
struct HostBlock {
    patterns: Vec<String>,
    hostname: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<Option<String>>,
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
    let mut blocks = Vec::new();
    let mut current = HostBlock {
        patterns: vec!["*".to_string()],
        ..HostBlock::default()
    };
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

        let (key, args) = directive_parts(&tokens);
        if key == "match" {
            // Ignore Match blocks for imports.
            flush_block(&mut current, &mut blocks);
            in_match_block = true;
            continue;
        }

        if key == "host" {
            if in_match_block {
                in_match_block = false;
            }
            flush_block(&mut current, &mut blocks);
            current = HostBlock::default();
            current.patterns = args;
            continue;
        }

        if in_match_block || current.patterns.is_empty() {
            continue;
        }

        match key.as_str() {
            "hostname" => {
                if current.hostname.is_none()
                    && let Some(value) = args.first()
                {
                    current.hostname = Some(value.to_string());
                }
            }
            "user" => {
                if current.user.is_none()
                    && let Some(value) = args.first()
                {
                    current.user = Some(value.to_string());
                }
            }
            "port" => {
                if current.port.is_none()
                    && let Some(value) = args.first()
                    && let Ok(port) = value.parse::<u16>()
                    && port > 0
                {
                    current.port = Some(port);
                }
            }
            "identityfile" => {
                if current.identity_file.is_none()
                    && let Some(value) = args.first()
                {
                    if value.eq_ignore_ascii_case("none") {
                        current.identity_file = Some(None);
                    } else {
                        current.identity_file = Some(Some(value.to_string()));
                    }
                }
            }
            _ => {}
        }
    }

    flush_block(&mut current, &mut blocks);
    build_hosts_from_blocks(&blocks)
}

fn directive_parts(tokens: &[String]) -> (String, Vec<String>) {
    let mut key = tokens[0].as_str();
    let mut args = Vec::new();

    if let Some((left, right)) = key.split_once('=') {
        key = left;
        if !right.is_empty() {
            args.push(right.to_string());
        }
        args.extend(tokens.iter().skip(1).cloned());
    } else if let Some(stripped) = key.strip_suffix('=') {
        key = stripped;
        args.extend(tokens.iter().skip(1).cloned());
    } else if tokens.get(1).is_some_and(|token| token == "=") {
        args.extend(tokens.iter().skip(2).cloned());
    } else if let Some(rest) = tokens.get(1).and_then(|token| token.strip_prefix('=')) {
        if !rest.is_empty() {
            args.push(rest.to_string());
        }
        args.extend(tokens.iter().skip(2).cloned());
    } else {
        args.extend(tokens.iter().skip(1).cloned());
    }

    (key.to_ascii_lowercase(), args)
}

fn flush_block(current: &mut HostBlock, blocks: &mut Vec<HostBlock>) {
    if current.patterns.is_empty() {
        return;
    }

    blocks.push(std::mem::take(current));
}

fn build_hosts_from_blocks(blocks: &[HostBlock]) -> Vec<Host> {
    let mut hosts = Vec::new();

    for block in blocks {
        for pattern in &block.patterns {
            if should_skip_pattern(pattern) {
                continue;
            }
            if hosts.iter().any(|host: &Host| host.name == *pattern) {
                continue;
            }
            hosts.push(resolve_host(pattern, blocks));
        }
    }

    hosts
}

fn resolve_host(alias: &str, blocks: &[HostBlock]) -> Host {
    let mut resolved = HostBlock::default();

    for block in blocks {
        if !host_block_matches(&block.patterns, alias) {
            continue;
        }
        if resolved.hostname.is_none() {
            resolved.hostname = block.hostname.clone();
        }
        if resolved.user.is_none() {
            resolved.user = block.user.clone();
        }
        if resolved.port.is_none() {
            resolved.port = block.port;
        }
        if resolved.identity_file.is_none() {
            resolved.identity_file = block.identity_file.clone();
        }
    }

    let port = resolved.port.unwrap_or(22);
    let username = resolved.user.unwrap_or_else(default_user);
    let hostname = resolved
        .hostname
        .map(|value| expand_ssh_tokens(&value, alias, alias, &username, port))
        .unwrap_or_else(|| alias.to_string());

    let auth = match resolved.identity_file {
        Some(Some(path)) => AuthMethod::PublicKey {
            key_path: Some(expand_identity_path(&expand_ssh_tokens(
                &path, alias, &hostname, &username, port,
            ))),
        },
        Some(None) | None => AuthMethod::Agent,
    };

    let now = Utc::now();
    Host {
        id: Uuid::new_v4(),
        name: alias.to_string(),
        hostname,
        port,
        username,
        protocol: Protocol::Ssh,
        vnc_port: None,
        auth,
        agent_forwarding: false,
        port_forwards: Vec::new(),
        portal_proxy_enabled: false,
        group_id: None,
        notes: None,
        tags: Vec::new(),
        created_at: now,
        updated_at: now,
        detected_os: None,
        last_connected: None,
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

fn host_block_matches(patterns: &[String], alias: &str) -> bool {
    let mut matched = false;
    for pattern in patterns {
        let pattern = pattern.trim();
        if pattern.is_empty() {
            continue;
        }
        if let Some(negated) = pattern.strip_prefix('!') {
            if pattern_matches(negated, alias) {
                return false;
            }
            continue;
        }
        if pattern_matches(pattern, alias) {
            matched = true;
        }
    }
    matched
}

fn pattern_matches(pattern: &str, value: &str) -> bool {
    fn inner(pattern: &[u8], value: &[u8]) -> bool {
        match pattern.split_first() {
            None => value.is_empty(),
            Some((&b'*', rest)) => {
                inner(rest, value) || (!value.is_empty() && inner(pattern, &value[1..]))
            }
            Some((&b'?', rest)) => !value.is_empty() && inner(rest, &value[1..]),
            Some((&expected, rest)) => value.split_first().is_some_and(|(&actual, value_rest)| {
                expected.eq_ignore_ascii_case(&actual) && inner(rest, value_rest)
            }),
        }
    }

    inner(pattern.as_bytes(), value.as_bytes())
}

fn expand_ssh_tokens(raw: &str, alias: &str, hostname: &str, username: &str, port: u16) -> String {
    let mut expanded = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            expanded.push(ch);
            continue;
        }

        match chars.next() {
            Some('%') => expanded.push('%'),
            Some('h') => expanded.push_str(hostname),
            Some('n') => expanded.push_str(alias),
            Some('r') => expanded.push_str(username),
            Some('p') => expanded.push_str(&port.to_string()),
            Some(other) => {
                expanded.push('%');
                expanded.push(other);
            }
            None => expanded.push('%'),
        }
    }
    expanded
}

fn default_user() -> String {
    default_username()
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
    for ch in line.chars() {
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

    #[test]
    fn identity_file_none_uses_agent_auth() {
        let content = "Host test\n  IdentityFile none\n";
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert!(matches!(hosts[0].auth, AuthMethod::Agent));
    }

    #[test]
    fn parse_accepts_equals_separator() {
        let content = r#"
            Host=eq-host
              HostName=10.0.0.6
              User = alice
              Port = 2200
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].name, "eq-host");
        assert_eq!(hosts[0].hostname, "10.0.0.6");
        assert_eq!(hosts[0].username, "alice");
        assert_eq!(hosts[0].port, 2200);
    }

    #[test]
    fn parse_keeps_first_value_like_openssh() {
        let content = r#"
            Host first-value
              HostName 10.0.0.7
              HostName 10.0.0.8
              User alice
              User bob
              Port 2201
              Port 2202
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "10.0.0.7");
        assert_eq!(hosts[0].username, "alice");
        assert_eq!(hosts[0].port, 2201);
    }

    #[test]
    fn parse_identity_file_none_is_a_first_value() {
        let content = r#"
            Host no-key
              IdentityFile none
              IdentityFile ~/.ssh/id_ed25519
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert!(matches!(hosts[0].auth, AuthMethod::Agent));
    }

    #[test]
    fn parse_applies_host_star_defaults_after_specific_block() {
        let content = r#"
            Host api
              HostName api.internal

            Host *
              User deploy
              Port 2200
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "api.internal");
        assert_eq!(hosts[0].username, "deploy");
        assert_eq!(hosts[0].port, 2200);
    }

    #[test]
    fn parse_applies_global_defaults_before_first_host() {
        let content = r#"
            User admin
            Port 2222

            Host db
              HostName db.internal
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].username, "admin");
        assert_eq!(hosts[0].port, 2222);
    }

    #[test]
    fn parse_honors_first_matching_value_order() {
        let content = r#"
            Host *
              User default-user

            Host app
              User app-user
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].username, "default-user");
    }

    #[test]
    fn parse_expands_hostname_tokens() {
        let content = r#"
            Host api
              User deploy
              Port 2200
              HostName %h-%r-%p.example.com
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "api-deploy-2200.example.com");
    }

    #[test]
    fn parse_expands_identity_file_tokens() {
        let content = r#"
            Host api
              User deploy
              IdentityFile keys/%h-%r.pem
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        if let AuthMethod::PublicKey { key_path } = &hosts[0].auth {
            let key_path = key_path.as_ref().expect("missing key path");
            if let Some(dir) = ssh_dir() {
                assert_eq!(key_path, &dir.join("keys/api-deploy.pem"));
            }
        } else {
            panic!("expected public key auth");
        }
    }

    #[test]
    fn parse_expands_identity_hostname_token_to_resolved_hostname() {
        let content = r#"
            Host api
              HostName api.internal
              IdentityFile keys/%h-%n.pem
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        if let AuthMethod::PublicKey { key_path } = &hosts[0].auth {
            let key_path = key_path.as_ref().expect("missing key path");
            if let Some(dir) = ssh_dir() {
                assert_eq!(key_path, &dir.join("keys/api.internal-api.pem"));
            }
        } else {
            panic!("expected public key auth");
        }
    }

    #[test]
    fn parse_ignores_invalid_zero_port() {
        let content = r#"
            Host zero-port
              Port 0
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].port, 22);
    }

    #[test]
    fn parse_uses_wildcard_defaults_with_negated_exceptions() {
        let content = r#"
            Host api db

            Host !db *
              User deploy
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 2);
        let api = hosts.iter().find(|host| host.name == "api").unwrap();
        let db = hosts.iter().find(|host| host.name == "db").unwrap();
        assert_eq!(api.username, "deploy");
        assert_ne!(db.username, "deploy");
    }
}
