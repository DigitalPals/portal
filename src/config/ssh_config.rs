use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use chrono::Utc;
use uuid::Uuid;

use crate::config::hosts::{HubRouting, default_username};
use crate::config::paths::{expand_tilde, ssh_dir};
use crate::config::{AuthMethod, Host, Protocol};
use crate::error::ConfigError;
use crate::fs_utils;

const SSH_CONFIG_MAX_BYTES: u64 = 1024 * 1024;

#[derive(Default, Debug, Clone)]
struct HostBlock {
    patterns: Vec<String>,
    hostname: Option<String>,
    user: Option<String>,
    port: Option<u16>,
    identity_file: Option<Option<String>>,
    /// ProxyJump directive: `Some(None)` for "none", `Some(Some(spec))` for a
    /// (possibly comma-separated) jump spec.
    proxy_jump: Option<Option<String>>,
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

    let content = match read_ssh_config_file(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ConfigError::ReadFile {
                path: path.clone(),
                source: error,
            });
        }
    };

    Ok(parse_ssh_config(&content))
}

fn read_ssh_config_file(path: &Path) -> std::io::Result<String> {
    fs_utils::read_regular_file_follow_symlink_to_string_limited(
        path,
        SSH_CONFIG_MAX_BYTES,
        "SSH config",
    )
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
            "proxyjump" => {
                if current.proxy_jump.is_none()
                    && let Some(value) = args.first()
                {
                    if value.eq_ignore_ascii_case("none") {
                        current.proxy_jump = Some(None);
                    } else {
                        current.proxy_jump = Some(Some(value.to_string()));
                    }
                }
            }
            "proxycommand" => {
                // ProxyCommand cannot be represented as a jump host chain.
                tracing::debug!("Skipping unsupported ProxyCommand directive during SSH import");
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
    let mut jump_specs: Vec<(Uuid, String)> = Vec::new();

    for block in blocks {
        for pattern in &block.patterns {
            if should_skip_pattern(pattern) {
                continue;
            }
            if hosts.iter().any(|host: &Host| host.name == *pattern) {
                continue;
            }
            let (host, proxy_jump) = resolve_host(pattern, blocks);
            if let Some(spec) = proxy_jump {
                jump_specs.push((host.id, spec));
            }
            hosts.push(host);
        }
    }

    link_proxy_jumps(&mut hosts, &jump_specs);

    hosts
}

fn resolve_host(alias: &str, blocks: &[HostBlock]) -> (Host, Option<String>) {
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
        if resolved.proxy_jump.is_none() {
            resolved.proxy_jump = block.proxy_jump.clone();
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
            vault_key_id: None,
        },
        Some(None) | None => AuthMethod::Agent,
    };

    let proxy_jump = resolved.proxy_jump.flatten();

    let now = Utc::now();
    let host = Host {
        id: Uuid::new_v4(),
        name: alias.to_string(),
        hostname,
        port,
        username,
        protocol: Protocol::Ssh,
        vnc_port: None,
        vnc_password_id: None,
        auth,
        agent_forwarding: false,
        port_forwards: Vec::new(),
        hub_routing: HubRouting::Auto,
        jump_host_id: None,
        group_id: None,
        notes: None,
        tags: Vec::new(),
        created_at: now,
        updated_at: now,
        detected_os: None,
        last_connected: None,
    };

    (host, proxy_jump)
}

/// A parsed `[user@]host[:port]` ProxyJump hop spec.
#[derive(Debug, PartialEq, Eq)]
struct JumpSpec {
    user: Option<String>,
    host: String,
    port: Option<u16>,
}

fn parse_jump_spec(spec: &str) -> Option<JumpSpec> {
    let spec = spec.trim();
    if spec.is_empty() {
        return None;
    }

    let (user, rest) = match spec.split_once('@') {
        Some((user, rest)) if !user.is_empty() => (Some(user.to_string()), rest),
        Some((_, rest)) => (None, rest),
        None => (None, spec),
    };

    // Bracketed IPv6: [::1]:2222
    if let Some(stripped) = rest.strip_prefix('[') {
        let (host, tail) = stripped.split_once(']')?;
        let port = tail
            .strip_prefix(':')
            .and_then(|p| p.parse::<u16>().ok().filter(|p| *p > 0));
        return Some(JumpSpec {
            user,
            host: host.to_string(),
            port,
        });
    }

    // host:port — only when there is exactly one colon (a bare IPv6 address
    // contains several and is treated as a plain host).
    if rest.matches(':').count() == 1
        && let Some((host, port)) = rest.split_once(':')
        && let Ok(port) = port.parse::<u16>()
        && port > 0
    {
        return Some(JumpSpec {
            user,
            host: host.to_string(),
            port: Some(port),
        });
    }

    Some(JumpSpec {
        user,
        host: rest.to_string(),
        port: None,
    })
}

/// Link ProxyJump chains: for each host with a jump spec, find or synthesize
/// the hop hosts and wire `jump_host_id` (each hop links to the previous one).
fn link_proxy_jumps(hosts: &mut Vec<Host>, jump_specs: &[(Uuid, String)]) {
    for (target_id, spec) in jump_specs {
        let mut prev: Option<Uuid> = None;
        let mut valid = true;

        for hop_spec in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let Some(hop_id) = find_or_synthesize_jump_host(hosts, hop_spec) else {
                tracing::debug!("Skipping unparsable ProxyJump hop '{}'", hop_spec);
                valid = false;
                break;
            };
            if hop_id == *target_id {
                tracing::debug!("Ignoring self-referential ProxyJump for host {}", target_id);
                valid = false;
                break;
            }

            if let Some(prev_id) = prev
                && prev_id != hop_id
                && let Some(hop) = hosts.iter_mut().find(|h| h.id == hop_id)
                && hop.jump_host_id.is_none()
            {
                hop.jump_host_id = Some(prev_id);
            }
            prev = Some(hop_id);
        }

        if valid
            && let Some(prev_id) = prev
            && let Some(target) = hosts.iter_mut().find(|h| h.id == *target_id)
        {
            target.jump_host_id = Some(prev_id);
        }
    }
}

/// Match a jump hop spec against known hosts (by alias, then by endpoint) or
/// synthesize a minimal agent-auth SSH host for it.
fn find_or_synthesize_jump_host(hosts: &mut Vec<Host>, spec: &str) -> Option<Uuid> {
    // Alias match first (e.g. `ProxyJump bastion` referring to `Host bastion`).
    if let Some(host) = hosts.iter().find(|h| h.name == spec) {
        return Some(host.id);
    }

    let parsed = parse_jump_spec(spec)?;
    let port = parsed.port.unwrap_or(22);

    // Endpoint match: hostname/port and, when given, the username.
    if let Some(host) = hosts.iter().find(|h| {
        h.hostname.eq_ignore_ascii_case(&parsed.host)
            && h.port == port
            && parsed
                .user
                .as_ref()
                .is_none_or(|user| h.effective_username().eq_ignore_ascii_case(user))
    }) {
        return Some(host.id);
    }

    // Synthesize a minimal SSH host entry (default auth = agent).
    let now = Utc::now();
    let host = Host {
        id: Uuid::new_v4(),
        name: spec.to_string(),
        hostname: parsed.host,
        port,
        username: parsed.user.unwrap_or_default(),
        protocol: Protocol::Ssh,
        vnc_port: None,
        vnc_password_id: None,
        auth: AuthMethod::Agent,
        agent_forwarding: false,
        port_forwards: Vec::new(),
        hub_routing: HubRouting::Auto,
        jump_host_id: None,
        group_id: None,
        notes: None,
        tags: Vec::new(),
        created_at: now,
        updated_at: now,
        detected_os: None,
        last_connected: None,
    };
    let id = host.id;
    hosts.push(host);
    Some(id)
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
    use std::io::ErrorKind;

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
            AuthMethod::PublicKey { key_path, .. } => {
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
        if let AuthMethod::PublicKey { key_path, .. } = &hosts[0].auth {
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
        if let AuthMethod::PublicKey { key_path, .. } = &hosts[0].auth {
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
        if let AuthMethod::PublicKey { key_path, .. } = &hosts[0].auth {
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

    #[test]
    fn proxy_jump_alias_links_to_existing_host() {
        let content = r#"
            Host bastion
              HostName bastion.internal
              User jump

            Host target
              HostName target.internal
              ProxyJump bastion
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 2);
        let bastion = hosts.iter().find(|h| h.name == "bastion").unwrap();
        let target = hosts.iter().find(|h| h.name == "target").unwrap();
        assert_eq!(target.jump_host_id, Some(bastion.id));
        assert_eq!(bastion.jump_host_id, None);
    }

    #[test]
    fn proxy_jump_user_host_port_synthesizes_host() {
        let content = r#"
            Host target
              HostName target.internal
              ProxyJump admin@10.0.0.1:2222
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 2);
        let target = hosts.iter().find(|h| h.name == "target").unwrap();
        let bastion = hosts
            .iter()
            .find(|h| h.name == "admin@10.0.0.1:2222")
            .unwrap();
        assert_eq!(target.jump_host_id, Some(bastion.id));
        assert_eq!(bastion.hostname, "10.0.0.1");
        assert_eq!(bastion.port, 2222);
        assert_eq!(bastion.username, "admin");
        assert!(matches!(bastion.auth, AuthMethod::Agent));
        assert_eq!(bastion.protocol, Protocol::Ssh);
    }

    #[test]
    fn proxy_jump_multi_hop_builds_chain() {
        let content = r#"
            Host outer
              HostName outer.internal

            Host inner
              HostName inner.internal

            Host target
              HostName target.internal
              ProxyJump outer,inner
        "#;
        let hosts = parse_ssh_config(content);

        let outer = hosts.iter().find(|h| h.name == "outer").unwrap();
        let inner = hosts.iter().find(|h| h.name == "inner").unwrap();
        let target = hosts.iter().find(|h| h.name == "target").unwrap();

        // OpenSSH semantics: connect via outer first, then inner, then target.
        assert_eq!(outer.jump_host_id, None);
        assert_eq!(inner.jump_host_id, Some(outer.id));
        assert_eq!(target.jump_host_id, Some(inner.id));
    }

    #[test]
    fn proxy_jump_multi_hop_synthesizes_missing_hops() {
        let content = r#"
            Host target
              HostName target.internal
              ProxyJump one.example.com,two.example.com
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 3);
        let one = hosts.iter().find(|h| h.hostname == "one.example.com").unwrap();
        let two = hosts.iter().find(|h| h.hostname == "two.example.com").unwrap();
        let target = hosts.iter().find(|h| h.name == "target").unwrap();
        assert_eq!(one.jump_host_id, None);
        assert_eq!(two.jump_host_id, Some(one.id));
        assert_eq!(target.jump_host_id, Some(two.id));
    }

    #[test]
    fn proxy_jump_none_leaves_host_direct() {
        let content = r#"
            Host target
              HostName target.internal
              ProxyJump none
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].jump_host_id, None);
    }

    #[test]
    fn proxy_command_is_ignored() {
        let content = r#"
            Host target
              HostName target.internal
              ProxyCommand ssh -W %h:%p bastion
        "#;
        let hosts = parse_ssh_config(content);

        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].jump_host_id, None);
    }

    #[test]
    fn proxy_jump_keeps_first_value_like_openssh() {
        let content = r#"
            Host bastion
              HostName bastion.internal

            Host other
              HostName other.internal

            Host target
              HostName target.internal
              ProxyJump bastion
              ProxyJump other
        "#;
        let hosts = parse_ssh_config(content);

        let bastion = hosts.iter().find(|h| h.name == "bastion").unwrap();
        let target = hosts.iter().find(|h| h.name == "target").unwrap();
        assert_eq!(target.jump_host_id, Some(bastion.id));
    }

    #[test]
    fn jump_spec_parsing_variants() {
        assert_eq!(
            parse_jump_spec("bastion"),
            Some(JumpSpec {
                user: None,
                host: "bastion".to_string(),
                port: None
            })
        );
        assert_eq!(
            parse_jump_spec("admin@bastion"),
            Some(JumpSpec {
                user: Some("admin".to_string()),
                host: "bastion".to_string(),
                port: None
            })
        );
        assert_eq!(
            parse_jump_spec("bastion:2222"),
            Some(JumpSpec {
                user: None,
                host: "bastion".to_string(),
                port: Some(2222)
            })
        );
        assert_eq!(
            parse_jump_spec("admin@[::1]:2200"),
            Some(JumpSpec {
                user: Some("admin".to_string()),
                host: "::1".to_string(),
                port: Some(2200)
            })
        );
        // Bare IPv6 address without brackets is a plain host.
        assert_eq!(
            parse_jump_spec("fe80::1"),
            Some(JumpSpec {
                user: None,
                host: "fe80::1".to_string(),
                port: None
            })
        );
        assert_eq!(parse_jump_spec(""), None);
    }

    #[test]
    fn read_ssh_config_file_reads_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        std::fs::write(&path, "Host api\n  HostName api.internal\n").unwrap();

        let content = read_ssh_config_file(&path).unwrap();

        assert_eq!(content, "Host api\n  HostName api.internal\n");
    }

    #[test]
    fn read_ssh_config_file_reports_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");

        let error = read_ssh_config_file(&path).expect_err("missing file should be reported");

        assert_eq!(error.kind(), ErrorKind::NotFound);
    }

    #[test]
    fn read_ssh_config_file_rejects_oversized_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let data = vec![b'a'; SSH_CONFIG_MAX_BYTES as usize + 1];
        std::fs::write(&path, data).unwrap();

        let error = read_ssh_config_file(&path).expect_err("oversized config should be rejected");

        assert_eq!(error.kind(), ErrorKind::FileTooLarge);
        assert!(error.to_string().contains("too large"));
    }

    #[test]
    fn read_ssh_config_file_rejects_directory() {
        let dir = tempfile::tempdir().unwrap();

        let error = read_ssh_config_file(dir.path()).expect_err("directory should be rejected");

        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(error.to_string().contains("not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn read_ssh_config_file_allows_symlinked_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("real_config");
        let link = dir.path().join("config");
        std::fs::write(&target, "Host api\n  HostName api.internal\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let content = read_ssh_config_file(&link).unwrap();

        assert_eq!(content, "Host api\n  HostName api.internal\n");
    }

    #[cfg(unix)]
    #[test]
    fn read_ssh_config_file_rejects_socket() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();

        let error = read_ssh_config_file(&path).expect_err("socket should be rejected");

        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        assert!(error.to_string().contains("not a regular file"));
    }
}
