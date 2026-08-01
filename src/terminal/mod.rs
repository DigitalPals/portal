//! Terminal widget module using alacritty_terminal
//!
//! This module provides a custom iced widget for terminal emulation.

pub mod backend;
mod block_elements;
mod colors;
pub mod glyph_constraints;
pub mod links;
pub mod logger;
pub mod metrics;
mod nerd_font_attributes;
pub mod search;
pub mod widget;

pub use backend::TerminalBackend;

/// Compatibility capability understood by Kimi Code and other terminals that
/// emit ConEmu-style OSC 9;4 progress reports. Portal implements those reports,
/// so advertising the capability is accurate even though it is not ConEmu.
pub const OSC_PROGRESS_ENV: (&str, &str) = ("ConEmuANSI", "ON");
