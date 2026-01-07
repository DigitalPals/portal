//! Input validation for user-provided configuration values.
//!
//! Provides validation functions for hostnames, ports, usernames, and other
//! user inputs to ensure they conform to expected formats before use.

use std::net::IpAddr;

use regex::Regex;
use std::sync::LazyLock;

/// Validation error with field context.
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub field: String,
    pub message: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}

impl std::error::Error for ValidationError {}

// Pre-compiled regex patterns for validation
static DNS_LABEL_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?$").unwrap());

static USERNAME_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_-]{0,31}$").unwrap());

/// Validate a hostname (DNS name or IP address).
///
/// Accepts:
/// - IPv4 addresses (e.g., "192.168.1.1")
/// - IPv6 addresses (e.g., "::1", "2001:db8::1")
/// - DNS hostnames (RFC 1123 compliant)
///
/// # Errors
///
/// Returns `ValidationError` if the hostname is empty, too long, or malformed.
pub fn validate_hostname(hostname: &str) -> Result<(), ValidationError> {
    let hostname = hostname.trim();

    if hostname.is_empty() {
        return Err(ValidationError {
            field: "hostname".to_string(),
            message: "Hostname is required".to_string(),
        });
    }

    // Check total length (DNS max is 253 characters)
    if hostname.len() > 253 {
        return Err(ValidationError {
            field: "hostname".to_string(),
            message: "Hostname exceeds maximum length of 253 characters".to_string(),
        });
    }

    // Try parsing as IP address first
    if hostname.parse::<IpAddr>().is_ok() {
        return Ok(());
    }

    // Validate as DNS hostname (RFC 1123)
    validate_dns_hostname(hostname)
}

/// Validate a DNS hostname according to RFC 1123.
fn validate_dns_hostname(hostname: &str) -> Result<(), ValidationError> {
    // Split into labels and validate each
    let labels: Vec<&str> = hostname.split('.').collect();

    if labels.is_empty() {
        return Err(ValidationError {
            field: "hostname".to_string(),
            message: "Invalid hostname format".to_string(),
        });
    }

    for label in labels {
        // Each label must be 1-63 characters
        if label.is_empty() || label.len() > 63 {
            return Err(ValidationError {
                field: "hostname".to_string(),
                message: "Hostname labels must be 1-63 characters".to_string(),
            });
        }

        // Labels must match the pattern: alphanumeric, may contain hyphens but not at start/end
        if !DNS_LABEL_REGEX.is_match(label) {
            return Err(ValidationError {
                field: "hostname".to_string(),
                message: format!(
                    "Invalid hostname label '{}': must start and end with alphanumeric, may contain hyphens",
                    label
                ),
            });
        }
    }

    Ok(())
}

/// Validate a port number string and parse it.
///
/// # Errors
///
/// Returns `ValidationError` if the port is not a valid number in range 1-65535.
pub fn validate_port(port_str: &str) -> Result<u16, ValidationError> {
    let port_str = port_str.trim();

    if port_str.is_empty() {
        return Err(ValidationError {
            field: "port".to_string(),
            message: "Port is required".to_string(),
        });
    }

    match port_str.parse::<u16>() {
        Ok(port) if port >= 1 => Ok(port),
        Ok(_) => Err(ValidationError {
            field: "port".to_string(),
            message: "Port must be between 1 and 65535".to_string(),
        }),
        Err(_) => Err(ValidationError {
            field: "port".to_string(),
            message: format!("Invalid port number: '{}'", port_str),
        }),
    }
}

/// Validate a username for SSH connections.
///
/// Accepts empty usernames (will default to current user).
/// Non-empty usernames must follow POSIX conventions:
/// - Start with a letter or underscore
/// - Contain only alphanumeric, underscore, or hyphen
/// - Maximum 32 characters
///
/// # Errors
///
/// Returns `ValidationError` if the username format is invalid.
pub fn validate_username(username: &str) -> Result<(), ValidationError> {
    let username = username.trim();

    // Empty username is allowed (will default to current user)
    if username.is_empty() {
        return Ok(());
    }

    // Check length
    if username.len() > 32 {
        return Err(ValidationError {
            field: "username".to_string(),
            message: "Username exceeds maximum length of 32 characters".to_string(),
        });
    }

    // Validate format (POSIX-like)
    if !USERNAME_REGEX.is_match(username) {
        return Err(ValidationError {
            field: "username".to_string(),
            message: "Username must start with letter or underscore, and contain only alphanumeric, underscore, or hyphen".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- Hostname validation tests ----

    #[test]
    fn hostname_valid_ipv4() {
        assert!(validate_hostname("192.168.1.1").is_ok());
        assert!(validate_hostname("10.0.0.1").is_ok());
        assert!(validate_hostname("127.0.0.1").is_ok());
        assert!(validate_hostname("255.255.255.255").is_ok());
    }

    #[test]
    fn hostname_valid_ipv6() {
        assert!(validate_hostname("::1").is_ok());
        assert!(validate_hostname("2001:db8::1").is_ok());
        assert!(validate_hostname("fe80::1").is_ok());
        assert!(validate_hostname("::ffff:192.168.1.1").is_ok());
    }

    #[test]
    fn hostname_valid_dns() {
        assert!(validate_hostname("example.com").is_ok());
        assert!(validate_hostname("sub.example.com").is_ok());
        assert!(validate_hostname("my-host").is_ok());
        assert!(validate_hostname("server1").is_ok());
        assert!(validate_hostname("a").is_ok());
        assert!(validate_hostname("a1").is_ok());
        assert!(validate_hostname("test-server-01.internal.example.com").is_ok());
    }

    #[test]
    fn hostname_invalid_empty() {
        assert!(validate_hostname("").is_err());
        assert!(validate_hostname("   ").is_err());
    }

    #[test]
    fn hostname_invalid_format() {
        assert!(validate_hostname("-invalid").is_err());
        assert!(validate_hostname("invalid-").is_err());
        assert!(validate_hostname("invalid..host").is_err());
        assert!(validate_hostname(".invalid").is_err());
        assert!(validate_hostname("invalid.").is_err());
    }

    #[test]
    fn hostname_invalid_characters() {
        assert!(validate_hostname("invalid_host").is_err()); // underscore not allowed in DNS
        assert!(validate_hostname("invalid host").is_err()); // space not allowed
        assert!(validate_hostname("invalid@host").is_err()); // @ not allowed
    }

    // ---- Port validation tests ----

    #[test]
    fn port_valid() {
        assert_eq!(validate_port("22").unwrap(), 22);
        assert_eq!(validate_port("1").unwrap(), 1);
        assert_eq!(validate_port("65535").unwrap(), 65535);
        assert_eq!(validate_port("443").unwrap(), 443);
        assert_eq!(validate_port("8080").unwrap(), 8080);
    }

    #[test]
    fn port_valid_with_whitespace() {
        assert_eq!(validate_port("  22  ").unwrap(), 22);
    }

    #[test]
    fn port_invalid_zero() {
        assert!(validate_port("0").is_err());
    }

    #[test]
    fn port_invalid_out_of_range() {
        assert!(validate_port("65536").is_err());
        assert!(validate_port("100000").is_err());
    }

    #[test]
    fn port_invalid_not_number() {
        assert!(validate_port("abc").is_err());
        assert!(validate_port("22a").is_err());
        assert!(validate_port("-1").is_err());
    }

    #[test]
    fn port_invalid_empty() {
        assert!(validate_port("").is_err());
        assert!(validate_port("   ").is_err());
    }

    // ---- Username validation tests ----

    #[test]
    fn username_valid() {
        assert!(validate_username("root").is_ok());
        assert!(validate_username("john").is_ok());
        assert!(validate_username("_system").is_ok());
        assert!(validate_username("user-name").is_ok());
        assert!(validate_username("user_name").is_ok());
        assert!(validate_username("User123").is_ok());
        assert!(validate_username("a").is_ok());
    }

    #[test]
    fn username_valid_empty() {
        // Empty is allowed (defaults to current user)
        assert!(validate_username("").is_ok());
        assert!(validate_username("   ").is_ok());
    }

    #[test]
    fn username_invalid_start() {
        assert!(validate_username("123user").is_err());
        assert!(validate_username("-user").is_err());
        assert!(validate_username("1abc").is_err());
    }

    #[test]
    fn username_invalid_characters() {
        assert!(validate_username("user@host").is_err());
        assert!(validate_username("user name").is_err());
        assert!(validate_username("user.name").is_err());
    }

    #[test]
    fn username_invalid_too_long() {
        let long_name = "a".repeat(33);
        assert!(validate_username(&long_name).is_err());
    }
}
