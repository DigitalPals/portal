use iced::{Font, alignment};

use crate::fonts::{NOTO_COLOR_EMOJI, SYMBOLS_NERD_FONT_MONO, TerminalFont};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CellTextPresentation {
    pub font: Font,
    pub size_scale: f32,
    pub y_offset_ratio: f32,
    pub anchor_y_ratio: f32,
    pub align_y: alignment::Vertical,
}

/// Build the full renderable content of a terminal cell, preserving any
/// zero-width codepoints stored alongside the base scalar.
pub fn build_cell_content(character: char, zerowidth: Option<&[char]>) -> String {
    let extra = zerowidth.map_or(0, <[char]>::len);
    let mut content = String::with_capacity(1 + extra);
    content.push(character);

    if let Some(zerowidth) = zerowidth {
        for c in zerowidth {
            content.push(*c);
        }
    }

    content
}

/// Pick the best bundled font for a terminal cell.
///
/// Terminal cells are rendered independently, so we choose a deterministic font
/// per cell instead of relying on incidental fallback ordering inside the
/// shared font database.
pub fn presentation_for_cell_content(
    primary: TerminalFont,
    content: &str,
    bold: bool,
    italic: bool,
) -> CellTextPresentation {
    let uses_soft_powerline = uses_soft_powerline_font_adjustment(content);
    let font = if uses_powerline_font(content) {
        primary.variant(bold, italic)
    } else if uses_symbols_font(content) {
        SYMBOLS_NERD_FONT_MONO
    } else if uses_emoji_font(content) {
        NOTO_COLOR_EMOJI
    } else {
        primary.variant(bold, italic)
    };

    let (size_scale, y_offset_ratio, anchor_y_ratio, align_y) = if uses_soft_powerline {
        // Let the text renderer place soft Powerline glyphs from its measured
        // bounds instead of shrinking them into the cell with ad hoc offsets.
        (
            1.0,
            0.0,
            0.5,
            alignment::Vertical::Center,
        )
    } else {
        (
            1.0,
            0.0,
            0.0,
            alignment::Vertical::Top,
        )
    };

    CellTextPresentation {
        font,
        size_scale,
        y_offset_ratio,
        anchor_y_ratio,
        align_y,
    }
}

fn uses_symbols_font(content: &str) -> bool {
    content.chars().any(is_private_use)
}

fn uses_powerline_font(content: &str) -> bool {
    content
        .chars()
        .all(|c| matches!(c, '\u{E0B0}' | '\u{E0B2}' | '\u{E0B4}' | '\u{E0B6}' | ' '))
}

fn uses_soft_powerline_font_adjustment(content: &str) -> bool {
    content.chars().all(|c| matches!(c, '\u{E0B4}' | '\u{E0B6}' | ' '))
}

fn uses_emoji_font(content: &str) -> bool {
    content.contains('\u{200D}')
        || content.contains('\u{FE0F}')
        || content.chars().any(is_extended_emoji_scalar)
}

fn is_private_use(c: char) -> bool {
    matches!(
        c as u32,
        0xE000..=0xF8FF | 0xF0000..=0xFFFFD | 0x100000..=0x10FFFD
    )
}

fn is_extended_emoji_scalar(c: char) -> bool {
    matches!(
        c as u32,
        0x1F000..=0x1FAFF | 0x1FC00..=0x1FFFD
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fonts::{
        HACK_NERD,
        JETBRAINS_MONO_NERD,
        NOTO_COLOR_EMOJI,
        SYMBOLS_NERD_FONT_MONO,
    };
    use iced::font::{Style, Weight};

    #[test]
    fn build_cell_content_preserves_zero_width_codepoints() {
        let content = build_cell_content('e', Some(&['\u{0301}', '\u{FE0F}']));

        assert_eq!(content, "e\u{0301}\u{FE0F}");
    }

    #[test]
    fn powerline_symbols_use_symbols_nerd_font() {
        let presentation =
            presentation_for_cell_content(TerminalFont::JetBrainsMono, "\u{E0B4}", false, false);

        assert_eq!(presentation.font.family, JETBRAINS_MONO_NERD.family);
    }

    #[test]
    fn soft_powerline_falls_back_to_metric_capable_font() {
        let presentation =
            presentation_for_cell_content(TerminalFont::Hack, "\u{E0B4}", false, false);

        assert_eq!(presentation.font.family, HACK_NERD.family);
    }

    #[test]
    fn non_powerline_private_use_symbols_use_symbols_font() {
        let presentation =
            presentation_for_cell_content(TerminalFont::JetBrainsMono, "\u{F0256}", false, false);

        assert_eq!(presentation.font, SYMBOLS_NERD_FONT_MONO);
    }

    #[test]
    fn emoji_sequences_use_emoji_font() {
        let presentation =
            presentation_for_cell_content(TerminalFont::Hack, "🐕\u{200D}🦺", false, false);

        assert_eq!(presentation.font, NOTO_COLOR_EMOJI);
    }

    #[test]
    fn plain_text_uses_primary_font_variant() {
        let presentation =
            presentation_for_cell_content(TerminalFont::Hack, "portal", true, true);

        assert_eq!(presentation.font.family, HACK_NERD.family);
        assert_eq!(presentation.font.weight, Weight::Bold);
        assert_eq!(presentation.font.style, Style::Italic);
    }

    #[test]
    fn jetbrains_regular_text_stays_on_primary_font() {
        let presentation =
            presentation_for_cell_content(TerminalFont::JetBrainsMono, "git", false, false);

        assert_eq!(presentation.font, JETBRAINS_MONO_NERD);
    }

    #[test]
    fn soft_powerline_glyphs_use_renderer_alignment_instead_of_scaling() {
        let presentation =
            presentation_for_cell_content(TerminalFont::JetBrainsMono, "\u{E0B4}", false, false);

        assert_eq!(presentation.size_scale, 1.0);
        assert_eq!(presentation.y_offset_ratio, 0.0);
        assert_eq!(presentation.anchor_y_ratio, 0.5);
        assert_eq!(presentation.align_y, alignment::Vertical::Center);
    }
}
