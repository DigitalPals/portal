//! Portal SSH Client library
//!
//! This module exposes the core functionality for use in integration tests
//! and the main binary.

#![allow(clippy::uninlined_format_args)]

// Public modules for integration testing
pub mod config;
pub mod error;
pub mod ssh;
pub mod validation;

// Public modules for the binary
pub mod app;
pub mod fonts;
pub mod keybindings;
pub mod platform;

// Internal modules
pub(crate) mod fs_utils;
pub(crate) mod hub;
pub(crate) mod icons;
pub(crate) mod local;
pub(crate) mod local_fs;
pub mod logging;
pub(crate) mod message;
pub(crate) mod proxy;
pub(crate) mod security_log;
pub mod sftp;
pub(crate) mod terminal;
pub(crate) mod theme;
pub(crate) mod views;
pub(crate) mod vnc;
pub(crate) mod widgets;

#[cfg(test)]
pub(crate) mod contract_test_support {
    use std::path::PathBuf;

    use serde_json::Value;

    fn contract_dir() -> PathBuf {
        std::env::var_os("PORTAL_HUB_CONTRACT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../portal-hub/contracts/portal-hub/v2")
            })
    }

    pub(crate) fn assert_portal_hub_contract(schema_name: &str, instance: &Value) {
        let path = contract_dir().join(format!("{schema_name}.schema.json"));
        if !path.exists() {
            eprintln!(
                "Skipping Portal Hub contract check; set PORTAL_HUB_CONTRACT_DIR or place schemas at {}",
                path.display()
            );
            return;
        }

        let schema: Value = serde_json::from_str(
            &std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display())),
        )
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", path.display()));
        let validator = jsonschema::validator_for(&schema)
            .unwrap_or_else(|error| panic!("failed to compile {}: {error}", path.display()));

        if let Err(error) = validator.validate(instance) {
            panic!("{schema_name} contract validation failed: {error}\n{instance:#}");
        }
    }
}
