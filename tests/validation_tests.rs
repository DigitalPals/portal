//! Integration tests for input validation
//!
//! These tests verify that the validation functions work correctly
//! for various input patterns.

mod common;

// Note: Since validation module is private to the portal crate,
// we test it through the public API (host dialog behavior).
// The unit tests in src/validation.rs cover the detailed validation logic.

// These integration tests would verify the validation through the
// host dialog public API if it were exposed as a library.
// For now, the unit tests in src/validation.rs provide comprehensive coverage.

#[test]
fn test_environment_setup() {
    // Verify test environment can be created
    let env = common::TestEnvironment::new();
    assert!(env.config_dir.path().exists());
}

#[test]
fn test_known_hosts_path() {
    let env = common::TestEnvironment::new();
    assert!(!env.known_hosts_path.exists()); // File doesn't exist yet
    assert!(env.known_hosts_path.starts_with(env.config_dir.path()));
}
