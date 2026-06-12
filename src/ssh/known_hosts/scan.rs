use std::borrow::Cow;
use std::path::Path;

use russh::keys::{self, PublicKey};

use super::matchers;

#[derive(Default)]
pub(crate) struct HostKeyScan {
    pub(crate) keys: Vec<PublicKey>,
    pub(crate) revoked_keys: Vec<PublicKey>,
    pub(crate) line_numbers: Vec<usize>,
}

pub(crate) fn scan_known_hosts_content(
    host: &str,
    port: u16,
    path: &Path,
    content: &str,
) -> HostKeyScan {
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
            let mut marker_parts = stripped.splitn(2, char::is_whitespace);
            let Some(marker) = marker_parts.next() else {
                continue;
            };
            let Some(rest) = marker_parts.next() else {
                continue;
            };
            (Some(marker), rest.trim_start())
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

    scan
}
