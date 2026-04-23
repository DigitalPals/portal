//! Font system for Portal SSH client

use iced::Font;
use iced::font::{Style, Weight};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::OnceLock;
use ttf_parser::OutlineBuilder;

#[derive(Debug, Clone, Copy)]
struct TerminalFontMetrics {
    cell_width_ratio: f32,
    line_height_ratio: f32,
    ascender_ratio: f32,
    soft_powerline_ink_bounds: Option<GlyphInkBounds>,
}

#[derive(Debug, Clone, Copy)]
struct GlyphInkBounds {
    top_ratio: f32,
    height_ratio: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct GlyphCellPlacement {
    pub size_scale: f32,
    pub y_offset_ratio: f32,
}

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

    /// Natural cell width as a multiple of the font em size, derived from the
    /// font's horizontal advance for monospaced glyphs.
    pub fn cell_width_ratio(self) -> f32 {
        self.metrics().cell_width_ratio
    }

    /// Natural line height as a multiple of the font's em size, derived from
    /// the hhea table (ascent + |descent| + lineGap) / unitsPerEm.
    /// Cell height is set to the font's natural metrics instead of being
    /// inflated for patched glyph overhangs.
    pub fn line_height_ratio(self) -> f32 {
        self.metrics().line_height_ratio
    }

    /// Metric-based placement for soft Powerline separators (`` / ``).
    ///
    /// These glyphs are drawn from the font path, but their ink box is often
    /// taller and higher than regular text. We derive a cell-relative size and
    /// vertical placement from the font's actual glyph bounding box instead of
    /// hand-tuning constants.
    pub fn soft_powerline_placement(self) -> Option<GlyphCellPlacement> {
        placement_for_cell_height(self.metrics(), self.line_height_ratio())
    }

    pub fn supports_soft_powerline(self) -> bool {
        self.metrics().soft_powerline_ink_bounds.is_some()
    }

    fn metrics(self) -> &'static TerminalFontMetrics {
        match self {
            TerminalFont::JetBrainsMono => {
                static METRICS: OnceLock<TerminalFontMetrics> = OnceLock::new();
                METRICS.get_or_init(|| {
                    parse_font_metrics(
                        JETBRAINS_MONO_NERD_BYTES,
                        TerminalFontMetrics {
                            cell_width_ratio: 0.6,
                            line_height_ratio: 1.32,
                            ascender_ratio: 1.0,
                            soft_powerline_ink_bounds: None,
                        },
                    )
                })
            },
            TerminalFont::Hack => {
                static METRICS: OnceLock<TerminalFontMetrics> = OnceLock::new();
                METRICS.get_or_init(|| {
                    parse_font_metrics(
                        HACK_NERD_BYTES,
                        TerminalFontMetrics {
                            cell_width_ratio: 0.6,
                            line_height_ratio: 1.1641,
                            ascender_ratio: 1.0,
                            soft_powerline_ink_bounds: None,
                        },
                    )
                })
            },
        }
    }
}

pub fn soft_powerline_fallback_placement(cell_line_height_ratio: f32) -> Option<GlyphCellPlacement> {
    static METRICS: OnceLock<TerminalFontMetrics> = OnceLock::new();
    let metrics = METRICS.get_or_init(|| {
        parse_font_metrics(
            SYMBOLS_NERD_FONT_MONO_BYTES,
            TerminalFontMetrics {
                cell_width_ratio: 0.6,
                line_height_ratio: 1.0,
                ascender_ratio: 1.0,
                soft_powerline_ink_bounds: None,
            },
        )
    });

    placement_for_cell_height(metrics, cell_line_height_ratio)
}

fn placement_for_cell_height(
    metrics: &TerminalFontMetrics,
    cell_line_height_ratio: f32,
) -> Option<GlyphCellPlacement> {
    let ink = metrics.soft_powerline_ink_bounds?;

    // Leave a small amount of headroom so rasterization does not kiss the
    // cell edge at small sizes.
    let size_scale = ((cell_line_height_ratio * 0.98) / ink.height_ratio).min(1.0);
    let ink_height_in_cell = (ink.height_ratio * size_scale) / cell_line_height_ratio;
    let natural_top_in_cell = (ink.top_ratio * size_scale) / cell_line_height_ratio;
    let centered_top_in_cell = (1.0 - ink_height_in_cell) / 2.0;

    Some(GlyphCellPlacement {
        size_scale,
        y_offset_ratio: centered_top_in_cell - natural_top_in_cell,
    })
}

fn parse_font_metrics(bytes: &[u8], fallback: TerminalFontMetrics) -> TerminalFontMetrics {
    let face = match ttf_parser::Face::parse(bytes, 0) {
        Ok(face) => face,
        Err(_) => return fallback,
    };

    let units_per_em = face.units_per_em() as f32;
    if units_per_em <= 0.0 {
        return fallback;
    }

    let width_ratio = ['m', '0', ' ']
        .into_iter()
        .find_map(|c| {
            face.glyph_index(c)
                .and_then(|glyph| face.glyph_hor_advance(glyph))
                .map(|advance| advance as f32 / units_per_em)
        })
        .filter(|ratio| ratio.is_finite() && *ratio > 0.0)
        .unwrap_or(fallback.cell_width_ratio);

    let ascender_ratio = face.ascender() as f32 / units_per_em;
    let line_height_ratio =
        (face.ascender() as f32 - face.descender() as f32 + face.line_gap() as f32) / units_per_em;
    let soft_powerline_ink_bounds = ['\u{E0B4}', '\u{E0B6}'].into_iter().find_map(|c| {
        let glyph = face.glyph_index(c)?;
        let bbox = face
            .glyph_bounding_box(glyph)
            .or_else(|| outline_bounding_box(&face, glyph))?;
        let top_ratio = (face.ascender() as f32 - bbox.y_max as f32) / units_per_em;
        let height_ratio = (bbox.y_max as f32 - bbox.y_min as f32) / units_per_em;

        (top_ratio.is_finite() && top_ratio >= 0.0 && height_ratio.is_finite() && height_ratio > 0.0)
            .then_some(GlyphInkBounds {
                top_ratio,
                height_ratio,
            })
    });

    TerminalFontMetrics {
        cell_width_ratio: width_ratio,
        line_height_ratio: if line_height_ratio.is_finite() && line_height_ratio > 0.0 {
            line_height_ratio
        } else {
            fallback.line_height_ratio
        },
        ascender_ratio: if ascender_ratio.is_finite() && ascender_ratio > 0.0 {
            ascender_ratio
        } else {
            fallback.ascender_ratio
        },
        soft_powerline_ink_bounds: soft_powerline_ink_bounds.or(fallback.soft_powerline_ink_bounds),
    }
}

fn outline_bounding_box(face: &ttf_parser::Face<'_>, glyph: ttf_parser::GlyphId) -> Option<ttf_parser::Rect> {
    let mut builder = OutlineBoundsBuilder::default();
    face.outline_glyph(glyph, &mut builder)?;
    builder.finish()
}

#[derive(Default)]
struct OutlineBoundsBuilder {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
    has_points: bool,
}

impl OutlineBoundsBuilder {
    fn include(&mut self, x: f32, y: f32) {
        if self.has_points {
            self.min_x = self.min_x.min(x);
            self.min_y = self.min_y.min(y);
            self.max_x = self.max_x.max(x);
            self.max_y = self.max_y.max(y);
        } else {
            self.min_x = x;
            self.min_y = y;
            self.max_x = x;
            self.max_y = y;
            self.has_points = true;
        }
    }

    fn finish(self) -> Option<ttf_parser::Rect> {
        self.has_points.then(|| ttf_parser::Rect {
            x_min: self.min_x.floor() as i16,
            y_min: self.min_y.floor() as i16,
            x_max: self.max_x.ceil() as i16,
            y_max: self.max_y.ceil() as i16,
        })
    }
}

impl OutlineBuilder for OutlineBoundsBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.include(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.include(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.include(x1, y1);
        self.include(x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.include(x1, y1);
        self.include(x2, y2);
        self.include(x, y);
    }

    fn close(&mut self) {}
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

/// Symbols Nerd Font Mono (regular)
pub const SYMBOLS_NERD_FONT_MONO: Font = Font::with_name("Symbols Nerd Font Mono");

/// Noto Color Emoji (regular)
pub const NOTO_COLOR_EMOJI: Font = Font::with_name("Noto Color Emoji");

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

/// Raw font bytes — Symbols Nerd Font Mono
pub const SYMBOLS_NERD_FONT_MONO_BYTES: &[u8] =
    include_bytes!("../assets/fonts/SymbolsNerdFontMono-Regular.ttf");

/// Raw font bytes — Noto Color Emoji
pub const NOTO_COLOR_EMOJI_BYTES: &[u8] =
    include_bytes!("../assets/fonts/NotoColorEmoji.ttf");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_terminal_fonts_expose_stable_metrics() {
        for font in TerminalFont::all() {
            let width = font.cell_width_ratio();
            let height = font.line_height_ratio();

            assert!(width.is_finite() && width > 0.4 && width < 0.8, "{font} width={width}");
            assert!(height.is_finite() && height > 1.0 && height < 1.5, "{font} height={height}");
        }
    }

}
