//! Ghostty-style symbol and Nerd Font glyph constraints.
//!
//! Ghostty uses a generated table from Nerd Font patcher metadata plus a
//! fallback for symbol-like codepoints. Portal keeps the same constraint model
//! and covers the common Nerd Font/Powerline ranges used by prompts.

use super::glyph_constraints::{
    ConstraintAlign, ConstraintHeight, ConstraintSize, GlyphConstraint,
};

pub fn constraint_for(c: char) -> Option<GlyphConstraint> {
    let cp = c as u32;

    let icon = GlyphConstraint {
        size: ConstraintSize::FitCoverOne,
        height: ConstraintHeight::Icon,
        align_horizontal: ConstraintAlign::CenterOne,
        align_vertical: ConstraintAlign::CenterOne,
        ..GlyphConstraint::NONE
    };

    let centered_icon = GlyphConstraint {
        size: ConstraintSize::Cover,
        height: ConstraintHeight::Icon,
        max_constraint_width: 1,
        align_horizontal: ConstraintAlign::CenterOne,
        align_vertical: ConstraintAlign::CenterOne,
        pad_left: 0.05,
        pad_right: 0.05,
        pad_top: 0.05,
        pad_bottom: 0.05,
        ..GlyphConstraint::NONE
    };

    match cp {
        0x2630 => Some(centered_icon),
        0x276c..=0x2771 => Some(GlyphConstraint {
            size: ConstraintSize::Cover,
            max_constraint_width: 1,
            align_horizontal: ConstraintAlign::CenterOne,
            align_vertical: ConstraintAlign::CenterOne,
            pad_top: 0.15,
            pad_bottom: 0.15,
            ..GlyphConstraint::NONE
        }),
        0xe0a0..=0xe0a3 | 0xe0cf => Some(icon),
        0xe0b0..=0xe0d7 => Some(GlyphConstraint {
            size: ConstraintSize::Stretch,
            align_horizontal: if matches!(
                cp,
                0xe0b2 | 0xe0b3 | 0xe0b6 | 0xe0b7 | 0xe0ba | 0xe0bb | 0xe0be | 0xe0bf
            ) {
                ConstraintAlign::End
            } else {
                ConstraintAlign::Start
            },
            align_vertical: ConstraintAlign::CenterOne,
            max_constraint_width: 1,
            max_xy_ratio: Some(0.7),
            ..GlyphConstraint::NONE
        }),
        0xe300..=0xe3ff
        | 0xe5fa..=0xe6b7
        | 0xe700..=0xe8ef
        | 0xea60..=0xebeb
        | 0xed00..=0xefce
        | 0xf000..=0xf8ff
        | 0xf0001..=0xf1af0 => Some(icon),
        _ if is_symbol(c) => Some(GlyphConstraint {
            size: ConstraintSize::Fit,
            ..GlyphConstraint::NONE
        }),
        _ => None,
    }
}

pub fn constraint_width(row: &[char], x: usize, grid_width: u8) -> u8 {
    if grid_width > 1 {
        return grid_width;
    }

    let Some(&c) = row.get(x) else {
        return 1;
    };

    if !is_symbol(c) {
        return grid_width.max(1);
    }

    if x + 1 >= row.len() {
        return 1;
    }

    if x > 0 {
        let previous = row[x - 1];
        if is_symbol(previous) && !is_graphics_element(previous) {
            return 1;
        }
    }

    if is_constraint_space(row[x + 1]) {
        2
    } else {
        1
    }
}

pub fn is_symbol(c: char) -> bool {
    let cp = c as u32;
    is_private_use(cp)
        || matches!(
            cp,
            0x2190..=0x21ff
                | 0x2300..=0x23ff
                | 0x2460..=0x24ff
                | 0x25a0..=0x25ff
                | 0x2600..=0x27bf
                | 0x1f300..=0x1f5ff
                | 0x1f600..=0x1f64f
                | 0x1f680..=0x1f6ff
                | 0x1f900..=0x1f9ff
        )
}

pub fn is_graphics_element(c: char) -> bool {
    let cp = c as u32;
    matches!(
        cp,
        0x2500..=0x257f | 0x2580..=0x259f | 0x1fb00..=0x1fbff | 0x1cc00..=0x1cebf | 0xe0b0..=0xe0d7
    )
}

fn is_private_use(cp: u32) -> bool {
    matches!(cp, 0xe000..=0xf8ff | 0xf0000..=0xffffd | 0x100000..=0x10fffd)
}

fn is_constraint_space(c: char) -> bool {
    matches!(c, '\0' | ' ' | '\u{2002}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_constraint_width_matches_ghostty_cases() {
        assert_eq!(constraint_width(&['\u{e8ef}', '\0'], 0, 1), 2);
        assert_eq!(constraint_width(&['\u{e8ef}', 'z'], 0, 1), 1);
        assert_eq!(constraint_width(&['\u{e8ef}', ' ', 'z'], 0, 1), 2);
        assert_eq!(constraint_width(&['\u{e8ef}', '\u{e8ee}'], 1, 1), 1);
        assert_eq!(constraint_width(&['\u{e8ef}'], 0, 1), 1);
    }

    #[test]
    fn graphics_elements_are_excluded_from_previous_symbol_rule() {
        assert!(is_graphics_element('\u{e0b0}'));
        assert!(constraint_for('\u{e8ef}').is_some());
    }
}
