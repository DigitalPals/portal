use std::borrow::Cow;
use std::path::PathBuf;

use russh::keys::{self, PublicKey};

use crate::error::SshError;

use super::matchers;

#[derive(Default)]
pub(crate) struct HostKeyScan {
    pub(crate) keys: Vec<PublicKey>,
    pub(crate) revoked_keys: Vec<PublicKey>,
    pub(crate) line_numbers: Vec<usize>,
}

pub(crate) fn scan_known_hosts_path(
    host: &str,
    port: u16,
    path: &PathBuf,
) -> Result<HostKeyScan, SshError> {
    if !path.exists() {
        return Ok(HostKeyScan::default());
    }

    let content = std::fs::read_to_string(path).map_err(|e| {
        SshError::HostKeyVerification(format!(
            "Failed to read known_hosts {}: {}",
            path.display(),
            e
        ))
    })?;

    let host_port = if port == 22 {
        Cow::Borrowed(host)
    } else {
        Cow::Owned(format!("[{}]:{}", host, port))
    };

    let mut scan = HostKeyScan::default();

    for (index, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let (marker, rest) = if let Some(stripped) = trimmed.strip_prefix('@') {
            match stripped.split_once(' ') {
                Some((marker, rest)) => (Some(marker), rest.trim_start()),
                None => continue,
            }
        } else {
            (None, trimmed)
        };

        let mut parts = rest.split_whitespace();
        let Some(hosts_field) = parts.next() else {
            continue;
        };
        let Some(_key_type) = parts.next() else {
            continue;
        };
        let Some(key_data) = parts.next() else {
            continue;
        };

        if !matchers::host_matches(host_port.as_ref(), host, hosts_field) {
            continue;
        }

        let key = match keys::parse_public_key_base64(key_data) {
            Ok(key) => key,
            Err(e) => {
                tracing::debug!(
                    "Failed to parse known_hosts key in {} line {}: {}",
                    path.display(),
                    index + 1,
                    e
                );
                continue;
            }
        };

        match marker {
            Some("revoked") => {
                scan.revoked_keys.push(key);
            }
            Some("cert-authority") => {
                continue;
            }
            Some(_) => {
                continue;
            }
            None => {
                scan.keys.push(key);
                scan.line_numbers.push(index + 1);
            }
        }
    }

    Ok(scan)
}
