//! Font system for Portal SSH client

use iced::Font;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Available terminal fonts
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum TerminalFont {
    #[default]
    JetBrainsMono,
    Hack,
}

impl TerminalFont {
    /// Get all available fonts
    pub const fn all() -> &'static [TerminalFont] {
        &[TerminalFont::JetBrainsMono, TerminalFont::Hack]
    }

    /// Display name for UI
    pub const fn display_name(&self) -> &'static str {
        match self {
            TerminalFont::JetBrainsMono => "JetBrains Mono",
            TerminalFont::Hack => "Hack",
        }
    }

    /// Convert to Iced Font
    pub const fn to_iced_font(self) -> Font {
        match self {
            TerminalFont::JetBrainsMono => JETBRAINS_MONO_NERD,
            TerminalFont::Hack => HACK_NERD,
        }
    }
}

impl fmt::Display for TerminalFont {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Inter font for UI
pub const INTER: Font = Font::with_name("Inter");

/// JetBrains Mono Nerd Font
pub const JETBRAINS_MONO_NERD: Font = Font::with_name("JetBrainsMono Nerd Font");

/// Hack Nerd Font
pub const HACK_NERD: Font = Font::with_name("Hack Nerd Font");

/// Raw font bytes for loading at startup - Inter
pub const INTER_BYTES: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");

/// Raw font bytes for loading at startup - JetBrains Mono
pub const JETBRAINS_MONO_NERD_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");

/// Raw font bytes for loading at startup - Hack
pub const HACK_NERD_BYTES: &[u8] = include_bytes!("../assets/fonts/HackNerdFont-Regular.ttf");
