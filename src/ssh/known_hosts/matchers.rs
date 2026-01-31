use data_encoding::BASE64_MIME;
use hmac::{Hmac, Mac};
use sha1::Sha1;

pub(crate) fn host_matches(host_port: &str, host: &str, host_field: &str) -> bool {
    let mut matched = false;

    for raw_entry in host_field.split(',') {
        let entry = raw_entry.trim();
        if entry.is_empty() {
            continue;
        }

        let (negated, pattern) = entry
            .strip_prefix('!')
            .map(|p| (true, p))
            .unwrap_or((false, entry));

        let is_match = match_host_pattern(host_port, host, pattern);
        if negated {
            if is_match {
                return false;
            }
            continue;
        }

        if is_match {
            matched = true;
        }
    }

    matched
}

fn match_host_pattern(host_port: &str, host: &str, pattern: &str) -> bool {
    if pattern.starts_with("|1|") {
        return match_hashed_host(host_port, pattern);
    }

    if pattern.contains('*') || pattern.contains('?') {
        return glob_match(pattern, host) || glob_match(pattern, host_port);
    }

    pattern == host || pattern == host_port
}

fn match_hashed_host(host_port: &str, pattern: &str) -> bool {
    let mut parts = pattern.split('|').skip(2);
    let Some(salt) = parts.next() else {
        return false;
    };
    let Some(hash) = parts.next() else {
        return false;
    };

    let Ok(salt) = BASE64_MIME.decode(salt.as_bytes()) else {
        return false;
    };
    let Ok(hash) = BASE64_MIME.decode(hash.as_bytes()) else {
        return false;
    };

    let Ok(mut hmac) = Hmac::<Sha1>::new_from_slice(&salt) else {
        return false;
    };
    hmac.update(host_port.as_bytes());
    hmac.verify_slice(&hash).is_ok()
}

pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    let (mut p_idx, mut t_idx) = (0usize, 0usize);
    let mut star_idx = None;
    let mut match_idx = 0usize;
    let p_bytes = pattern.as_bytes();
    let t_bytes = text.as_bytes();

    while t_idx < t_bytes.len() {
        if p_idx < p_bytes.len() && (p_bytes[p_idx] == b'?' || p_bytes[p_idx] == t_bytes[t_idx]) {
            p_idx += 1;
            t_idx += 1;
            continue;
        }

        if p_idx < p_bytes.len() && p_bytes[p_idx] == b'*' {
            star_idx = Some(p_idx);
            match_idx = t_idx;
            p_idx += 1;
            continue;
        }

        if let Some(star_pos) = star_idx {
            p_idx = star_pos + 1;
            match_idx += 1;
            t_idx = match_idx;
            continue;
        }

        return false;
    }

    while p_idx < p_bytes.len() && p_bytes[p_idx] == b'*' {
        p_idx += 1;
    }

    p_idx == p_bytes.len()
}
