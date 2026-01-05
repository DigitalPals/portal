//! Terminal widget module using alacritty_terminal
//!
//! This module provides a custom iced widget for terminal emulation.

pub mod backend;
mod colors;
pub mod widget;

pub use backend::TerminalBackend;
