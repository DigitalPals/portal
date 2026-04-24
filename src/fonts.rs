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

    /// Convert to Iced Font (regular weight, normal style)
    pub const fn to_iced_font(self) -> Font {
        match self {
            TerminalFont::JetBrainsMono => JETBRAINS_MONO_NERD,
            TerminalFont::Hack => HACK_NERD,
        }
    }

    /// Raw bytes for the bundled regular face.
    pub const fn bytes(self) -> &'static [u8] {
        match self {
            TerminalFont::JetBrainsMono => JETBRAINS_MONO_NERD_BYTES,
            TerminalFont::Hack => HACK_NERD_BYTES,
        }
    }

    /// Resolve the font used for terminal cells.
    ///
    /// Portal currently bundles only the regular Nerd Font faces. Returning the
    /// regular face for styled cells keeps Private Use Area glyph coverage
    /// stable instead of letting the renderer fall back to an unrelated system
    /// bold/italic face that may not contain prompt symbols.
    pub fn variant(self, _bold: bool, _italic: bool) -> Font {
        self.to_iced_font()
    }

    /// Legacy fallback line height ratio.
    ///
    /// New terminal rendering should use measured `TerminalMetrics` instead.
    pub const fn line_height_ratio(self) -> f32 {
        match self {
            // JetBrains Mono: ascent=1020 descent=300 gap=0 unitsPerEm=1000 → natural 1.32×.
            // 1.35 adds ~0.4 px at 13 px to accommodate Nerd Font icons whose patched
            // glyphs can reach slightly above the base font's ascender.
            TerminalFont::JetBrainsMono => 1.35,
            // Hack: ascent=1901 descent=-483 gap=0  → 2384/2048
            TerminalFont::Hack => 1.1641,
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

/// JetBrains Mono Nerd Font (regular)
pub const JETBRAINS_MONO_NERD: Font = Font::with_name("JetBrainsMono Nerd Font");

/// Hack Nerd Font (regular)
pub const HACK_NERD: Font = Font::with_name("Hack Nerd Font");

/// Raw font bytes — Inter
pub const INTER_BYTES: &[u8] = include_bytes!("../assets/fonts/Inter-Regular.ttf");

/// Raw font bytes — JetBrains Mono Nerd Font
pub const JETBRAINS_MONO_NERD_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Regular.ttf");

/// Raw font bytes — Hack Nerd Font
pub const HACK_NERD_BYTES: &[u8] = include_bytes!("../assets/fonts/HackNerdFont-Regular.ttf");
