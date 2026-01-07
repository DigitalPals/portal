//! Portal SSH Client library
//!
//! This module exposes the core functionality for use in integration tests
//! and the main binary.

// Public modules for integration testing
pub mod config;
pub mod error;
pub mod ssh;
pub mod validation;

// Public modules for the binary
pub mod app;
pub mod fonts;

// Internal modules
pub(crate) mod fs_utils;
pub(crate) mod icons;
pub(crate) mod local;
pub(crate) mod local_fs;
pub(crate) mod message;
pub(crate) mod security_log;
pub(crate) mod sftp;
pub(crate) mod terminal;
pub(crate) mod theme;
pub(crate) mod views;
pub(crate) mod widgets;
