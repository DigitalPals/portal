//! Font system for Portal SSH client

use iced::Font;
use iced::font::{Style, Weight};
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

    /// Resolve the correct font variant for a given bold/italic combination.
    pub fn variant(self, bold: bool, italic: bool) -> Font {
        let base = self.to_iced_font();
        Font {
            weight: if bold { Weight::Bold } else { Weight::Normal },
            style: if italic { Style::Italic } else { Style::Normal },
            ..base
        }
    }

    /// Natural line height as a multiple of the font's em size, derived from
    /// the hhea table (ascent + |descent| + lineGap) / unitsPerEm.
    /// Cell height is set to this value so box-drawing glyphs fill the cell
    /// exactly and connect across line boundaries.
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
pub const JETBRAINS_MONO_NERD_BOLD_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Bold.ttf");
pub const JETBRAINS_MONO_NERD_ITALIC_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-Italic.ttf");
pub const JETBRAINS_MONO_NERD_BOLD_ITALIC_BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMonoNerdFont-BoldItalic.ttf");

/// Raw font bytes — Hack Nerd Font
pub const HACK_NERD_BYTES: &[u8] = include_bytes!("../assets/fonts/HackNerdFont-Regular.ttf");
pub const HACK_NERD_BOLD_BYTES: &[u8] = include_bytes!("../assets/fonts/HackNerdFont-Bold.ttf");
pub const HACK_NERD_ITALIC_BYTES: &[u8] =
    include_bytes!("../assets/fonts/HackNerdFont-Italic.ttf");
pub const HACK_NERD_BOLD_ITALIC_BYTES: &[u8] =
    include_bytes!("../assets/fonts/HackNerdFont-BoldItalic.ttf");
