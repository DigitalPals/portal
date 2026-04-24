//! Ghostty-style terminal font metrics.

use iced::Rectangle;
use ttf_parser::{Face, GlyphId};

use crate::fonts::TerminalFont;

/// Horizontal padding before the terminal grid starts.
pub const TERMINAL_PADDING_LEFT: f32 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceMetrics {
    pub px_per_em: f32,
    pub cell_width: f32,
    pub ascent: f32,
    pub descent: f32,
    pub line_gap: f32,
    pub underline_position: Option<f32>,
    pub underline_thickness: Option<f32>,
    pub strikethrough_position: Option<f32>,
    pub strikethrough_thickness: Option<f32>,
    pub cap_height: Option<f32>,
    pub ex_height: Option<f32>,
    pub ascii_height: Option<f32>,
    pub ic_width: Option<f32>,
}

impl FaceMetrics {
    fn line_height(self) -> f32 {
        self.ascent - self.descent + self.line_gap
    }

    fn cap_height(self) -> f32 {
        self.cap_height
            .filter(|value| *value > 0.0)
            .unwrap_or(0.75 * self.ascent)
    }

    fn ex_height(self) -> f32 {
        self.ex_height
            .filter(|value| *value > 0.0)
            .unwrap_or(0.75 * self.cap_height())
    }

    fn underline_thickness(self) -> f32 {
        self.underline_thickness
            .filter(|value| *value > 0.0)
            .unwrap_or(0.15 * self.ex_height())
    }

    fn strikethrough_thickness(self) -> f32 {
        self.strikethrough_thickness
            .filter(|value| *value > 0.0)
            .unwrap_or_else(|| self.underline_thickness())
    }

    fn underline_position(self) -> f32 {
        self.underline_position
            .unwrap_or_else(|| -self.underline_thickness())
    }

    fn strikethrough_position(self) -> f32 {
        self.strikethrough_position
            .unwrap_or_else(|| (self.ex_height() + self.strikethrough_thickness()) * 0.5)
    }
}

/// Complete terminal grid metrics used by layout, sprites, cursor, and glyph placement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalMetrics {
    pub cell_width: f32,
    pub cell_height: f32,
    pub cell_baseline: f32,
    pub underline_position: f32,
    pub underline_thickness: f32,
    pub strikethrough_position: f32,
    pub strikethrough_thickness: f32,
    pub overline_position: f32,
    pub overline_thickness: f32,
    pub box_thickness: f32,
    pub cursor_thickness: f32,
    pub cursor_height: f32,
    pub icon_height: f32,
    pub icon_height_single: f32,
    pub face_width: f32,
    pub face_height: f32,
    pub face_y: f32,
}

impl TerminalMetrics {
    pub fn for_font(font: TerminalFont, font_size: f32) -> Self {
        measured_face_metrics(font, font_size)
            .map(Self::from_face_metrics)
            .unwrap_or_else(|| Self::fallback(font_size, font.line_height_ratio()))
    }

    pub fn columns_for_bounds(self, bounds: Rectangle) -> usize {
        ((bounds.width - TERMINAL_PADDING_LEFT).max(0.0) / self.cell_width) as usize
    }

    pub fn rows_for_bounds(self, bounds: Rectangle) -> usize {
        (bounds.height / self.cell_height) as usize
    }

    fn from_face_metrics(face: FaceMetrics) -> Self {
        let face_width = face.cell_width;
        let face_height = face.line_height();
        let cell_width = face_width.round().max(1.0);
        let cell_height = face_height.round().max(1.0);

        let half_line_gap = face.line_gap / 2.0;
        let face_baseline = half_line_gap - face.descent;
        let cell_baseline = (face_baseline - (cell_height - face_height) / 2.0)
            .round()
            .max(1.0);
        let face_y = cell_baseline - face_baseline;
        let top_to_baseline = cell_height - cell_baseline;

        let underline_thickness = face.underline_thickness().ceil().max(1.0);
        let strikethrough_thickness = face.strikethrough_thickness().ceil().max(1.0);
        let underline_position = (top_to_baseline - face.underline_position()).round();
        let strikethrough_position = (top_to_baseline - face.strikethrough_position()).round();
        let cap_height = face.cap_height();

        let mut metrics = Self {
            cell_width,
            cell_height,
            cell_baseline,
            underline_position,
            underline_thickness,
            strikethrough_position,
            strikethrough_thickness,
            overline_position: 0.0,
            overline_thickness: underline_thickness,
            box_thickness: underline_thickness,
            cursor_thickness: 1.0,
            cursor_height: cell_height,
            icon_height: face_height.max(1.0),
            icon_height_single: ((2.0 * cap_height + face_height) / 3.0).max(1.0),
            face_width: face_width.max(1.0),
            face_height: face_height.max(1.0),
            face_y,
        };
        metrics.clamp();
        metrics
    }

    fn fallback(font_size: f32, line_height_ratio: f32) -> Self {
        let face = FaceMetrics {
            px_per_em: font_size,
            cell_width: font_size * 0.6,
            ascent: font_size,
            descent: -(font_size * (line_height_ratio - 1.0)).max(0.0),
            line_gap: 0.0,
            underline_position: None,
            underline_thickness: None,
            strikethrough_position: None,
            strikethrough_thickness: None,
            cap_height: None,
            ex_height: None,
            ascii_height: None,
            ic_width: None,
        };
        Self::from_face_metrics(face)
    }

    fn clamp(&mut self) {
        self.cell_width = self.cell_width.max(1.0);
        self.cell_height = self.cell_height.max(1.0);
        self.underline_thickness = self.underline_thickness.max(1.0);
        self.strikethrough_thickness = self.strikethrough_thickness.max(1.0);
        self.overline_thickness = self.overline_thickness.max(1.0);
        self.box_thickness = self.box_thickness.max(1.0);
        self.cursor_thickness = self.cursor_thickness.max(1.0);
        self.cursor_height = self.cursor_height.max(1.0);
        self.icon_height = self.icon_height.max(1.0);
        self.icon_height_single = self.icon_height_single.max(1.0);
        self.face_width = self.face_width.max(1.0);
        self.face_height = self.face_height.max(1.0);
    }
}

fn measured_face_metrics(font: TerminalFont, font_size: f32) -> Option<FaceMetrics> {
    let face = Face::parse(font.bytes(), 0).ok()?;
    let units_per_em = face.units_per_em() as f32;
    let px_per_unit = font_size / units_per_em;

    let (ascent, descent, line_gap) = vertical_metrics(&face, px_per_unit);
    let (cell_width, ascii_height) = ascii_measurements(&face, px_per_unit);
    let (cap_height, ex_height) = letter_heights(&face, px_per_unit, ascent);

    Some(FaceMetrics {
        px_per_em: font_size,
        cell_width,
        ascent,
        descent,
        line_gap,
        underline_position: face.underline_metrics().and_then(|metrics| {
            if metrics.thickness == 0 && metrics.position == 0 {
                None
            } else {
                Some(metrics.position as f32 * px_per_unit)
            }
        }),
        underline_thickness: face.underline_metrics().and_then(|metrics| {
            (metrics.thickness != 0).then_some(metrics.thickness as f32 * px_per_unit)
        }),
        strikethrough_position: face
            .strikeout_metrics()
            .map(|metrics| metrics.position as f32 * px_per_unit),
        strikethrough_thickness: face.strikeout_metrics().and_then(|metrics| {
            (metrics.thickness != 0).then_some(metrics.thickness as f32 * px_per_unit)
        }),
        cap_height,
        ex_height,
        ascii_height,
        ic_width: measured_advance(&face, '水', px_per_unit),
    })
}

fn vertical_metrics(face: &Face<'_>, px_per_unit: f32) -> (f32, f32, f32) {
    let hhea = face.tables().hhea;
    if let Some(os2) = face.tables().os2 {
        if os2.use_typographic_metrics() {
            return (
                os2.typographic_ascender() as f32 * px_per_unit,
                os2.typographic_descender() as f32 * px_per_unit,
                os2.typographic_line_gap() as f32 * px_per_unit,
            );
        }
    }

    if hhea.ascender != 0 || hhea.descender != 0 {
        return (
            hhea.ascender as f32 * px_per_unit,
            hhea.descender as f32 * px_per_unit,
            hhea.line_gap as f32 * px_per_unit,
        );
    }

    if let Some(os2) = face.tables().os2 {
        if os2.typographic_ascender() != 0 || os2.typographic_descender() != 0 {
            return (
                os2.typographic_ascender() as f32 * px_per_unit,
                os2.typographic_descender() as f32 * px_per_unit,
                os2.typographic_line_gap() as f32 * px_per_unit,
            );
        }

        return (
            os2.windows_ascender() as f32 * px_per_unit,
            os2.windows_descender() as f32 * px_per_unit,
            0.0,
        );
    }

    (
        face.ascender() as f32 * px_per_unit,
        face.descender() as f32 * px_per_unit,
        face.line_gap() as f32 * px_per_unit,
    )
}

fn ascii_measurements(face: &Face<'_>, px_per_unit: f32) -> (f32, Option<f32>) {
    let mut max_advance = 0.0_f32;
    let mut top = 0.0_f32;
    let mut bottom = 0.0_f32;

    for c in ' '..='~' {
        if let Some(glyph) = face.glyph_index(c) {
            if let Some(advance) = face.glyph_hor_advance(glyph) {
                max_advance = max_advance.max(advance as f32 * px_per_unit);
            }
            if let Some(rect) = face.glyph_bounding_box(glyph) {
                top = top.max(rect.y_max as f32 * px_per_unit);
                bottom = bottom.min(rect.y_min as f32 * px_per_unit);
            }
        }
    }

    if max_advance <= 0.0 {
        max_advance = face
            .glyph_index('M')
            .and_then(|glyph| face.glyph_hor_advance(glyph))
            .map(|value| value as f32 * px_per_unit)
            .unwrap_or(1.0);
    }

    let height = (top - bottom > 0.0).then_some(top - bottom);
    (max_advance, height)
}

fn letter_heights(face: &Face<'_>, px_per_unit: f32, ascent: f32) -> (Option<f32>, Option<f32>) {
    let cap_height = face
        .capital_height()
        .filter(|value| *value > 0)
        .map(|value| value as f32 * px_per_unit)
        .or_else(|| measured_bbox_height(face, GlyphId(face.glyph_index('H')?.0), px_per_unit));

    let ex_height = face
        .x_height()
        .filter(|value| *value > 0)
        .map(|value| value as f32 * px_per_unit)
        .or_else(|| measured_bbox_height(face, GlyphId(face.glyph_index('x')?.0), px_per_unit));

    (cap_height.or(Some(0.75 * ascent)), ex_height)
}

fn measured_bbox_height(face: &Face<'_>, glyph: GlyphId, px_per_unit: f32) -> Option<f32> {
    face.glyph_bounding_box(glyph)
        .map(|rect| rect.height() as f32 * px_per_unit)
        .filter(|height| *height > 0.0)
}

fn measured_advance(face: &Face<'_>, c: char, px_per_unit: f32) -> Option<f32> {
    let glyph = face.glyph_index(c)?;
    let advance = face.glyph_hor_advance(glyph)? as f32 * px_per_unit;
    let width = face
        .glyph_bounding_box(glyph)
        .map(|rect| rect.width() as f32 * px_per_unit)
        .unwrap_or(0.0);

    (width <= advance).then_some(advance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_round_to_integer_cells() {
        let metrics = TerminalMetrics::for_font(TerminalFont::JetBrainsMono, 13.0);

        assert_eq!(metrics.cell_width.fract(), 0.0);
        assert_eq!(metrics.cell_height.fract(), 0.0);
        assert!(metrics.cell_width >= 1.0);
        assert!(metrics.cell_height >= 1.0);
    }

    #[test]
    fn bundled_fonts_have_measured_metrics() {
        for font in TerminalFont::all() {
            let metrics = TerminalMetrics::for_font(*font, 13.0);

            assert!(metrics.face_width > 1.0);
            assert!(metrics.face_height > 1.0);
            assert!(metrics.cell_baseline > 1.0);
            assert!(metrics.box_thickness >= 1.0);
            assert!(metrics.icon_height_single >= 1.0);
        }
    }
}
