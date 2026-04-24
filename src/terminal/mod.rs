//! Terminal widget module using alacritty_terminal
//!
//! This module provides a custom iced widget for terminal emulation.

pub mod backend;
mod block_elements;
mod colors;
pub mod glyph_constraints;
pub mod logger;
pub mod metrics;
pub mod widget;

pub use backend::TerminalBackend;
