//! SSH integration tests
//!
//! These tests require Docker to run a test SSH server.
//! The server is automatically started when tests run.
//!
//! ## Running the tests
//!
//! ```bash
//! # Generate test keys (only needed once)
//! cd tests/docker/test_keys && ./generate_keys.sh
//!
//! # Run the tests (Docker containers start automatically)
//! cargo test --test ssh_integration
//!
//! # Cleanup (optional - containers are reused)
//! cd tests/docker && docker-compose down -v
//! ```

#[macro_use]
pub mod fixtures;

mod auth_tests;
mod connection_tests;
mod host_key_tests;
mod multiplexing_tests;
mod port_forward_tests;
