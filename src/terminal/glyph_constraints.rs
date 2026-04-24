//! Ghostty-style glyph constraint rules.

#![allow(dead_code)]

use super::metrics::TerminalMetrics;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphSize {
    pub width: f32,
    pub height: f32,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintSize {
    None,
    Fit,
    Cover,
    FitCoverOne,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintAlign {
    None,
    Start,
    End,
    Center,
    CenterOne,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConstraintHeight {
    Cell,
    Icon,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphConstraint {
    pub size: ConstraintSize,
    pub align_vertical: ConstraintAlign,
    pub align_horizontal: ConstraintAlign,
    pub pad_top: f32,
    pub pad_left: f32,
    pub pad_right: f32,
    pub pad_bottom: f32,
    pub relative_width: f32,
    pub relative_height: f32,
    pub relative_x: f32,
    pub relative_y: f32,
    pub max_xy_ratio: Option<f32>,
    pub max_constraint_width: u8,
    pub height: ConstraintHeight,
}

impl GlyphConstraint {
    pub const NONE: Self = Self {
        size: ConstraintSize::None,
        align_vertical: ConstraintAlign::None,
        align_horizontal: ConstraintAlign::None,
        pad_top: 0.0,
        pad_left: 0.0,
        pad_right: 0.0,
        pad_bottom: 0.0,
        relative_width: 1.0,
        relative_height: 1.0,
        relative_x: 0.0,
        relative_y: 0.0,
        max_xy_ratio: None,
        max_constraint_width: 2,
        height: ConstraintHeight::Cell,
    };

    pub fn does_anything(self) -> bool {
        self.size != ConstraintSize::None
            || self.align_horizontal != ConstraintAlign::None
            || self.align_vertical != ConstraintAlign::None
    }

    pub fn constrain(
        self,
        glyph: GlyphSize,
        metrics: TerminalMetrics,
        constraint_width: u8,
    ) -> GlyphSize {
        if !self.does_anything() {
            return glyph;
        }

        if self.size == ConstraintSize::Stretch {
            let mut grid_metrics = metrics;
            grid_metrics.face_width = grid_metrics.cell_width;
            grid_metrics.face_height = grid_metrics.cell_height;
            grid_metrics.face_y = 0.0;

            let mut grid_constraint = self;
            grid_constraint.pad_bottom = grid_constraint.pad_bottom.max(0.0);
            grid_constraint.pad_top = grid_constraint.pad_top.max(0.0);
            grid_constraint.pad_left = grid_constraint.pad_left.max(0.0);
            grid_constraint.pad_right = grid_constraint.pad_right.max(0.0);

            return grid_constraint.constrain_inner(glyph, grid_metrics, constraint_width);
        }

        self.constrain_inner(glyph, metrics, constraint_width)
    }

    fn constrain_inner(
        self,
        glyph: GlyphSize,
        metrics: TerminalMetrics,
        constraint_width: u8,
    ) -> GlyphSize {
        let min_constraint_width = if self.size == ConstraintSize::Stretch
            && metrics.face_width > 0.9 * metrics.face_height
        {
            1
        } else {
            self.max_constraint_width.min(constraint_width.max(1))
        };

        let mut group = GlyphSize {
            width: glyph.width / self.relative_width,
            height: glyph.height / self.relative_height,
            x: glyph.x - (glyph.width / self.relative_width * self.relative_x),
            y: glyph.y - (glyph.height / self.relative_height * self.relative_y),
        };

        let (width_factor, height_factor) =
            self.scale_factors(group, metrics, min_constraint_width);
        let center_x = group.x + group.width / 2.0;
        let center_y = group.y + group.height / 2.0;
        group.width *= width_factor;
        group.height *= height_factor;
        group.x = center_x - group.width / 2.0;
        group.y = center_y - group.height / 2.0;

        group.y = self.aligned_y(group, metrics);
        group.x = self.aligned_x(group, metrics, min_constraint_width);

        GlyphSize {
            width: width_factor * glyph.width,
            height: height_factor * glyph.height,
            x: group.x + group.width * self.relative_x,
            y: group.y + group.height * self.relative_y,
        }
    }

    fn scale_factors(
        self,
        group: GlyphSize,
        metrics: TerminalMetrics,
        min_constraint_width: u8,
    ) -> (f32, f32) {
        if self.size == ConstraintSize::None {
            return (1.0, 1.0);
        }

        let multi_cell = min_constraint_width > 1;
        let pad_width_factor = min_constraint_width as f32 - (self.pad_left + self.pad_right);
        let pad_height_factor = 1.0 - (self.pad_bottom + self.pad_top);
        let target_width = pad_width_factor * metrics.face_width;
        let target_height = pad_height_factor
            * match self.height {
                ConstraintHeight::Cell => metrics.face_height,
                ConstraintHeight::Icon if multi_cell => metrics.icon_height,
                ConstraintHeight::Icon => metrics.icon_height_single,
            };

        let mut width_factor = target_width / group.width;
        let mut height_factor = target_height / group.height;

        match self.size {
            ConstraintSize::None => unreachable!(),
            ConstraintSize::Fit => {
                height_factor = 1.0_f32.min(width_factor).min(height_factor);
                width_factor = height_factor;
            }
            ConstraintSize::Cover => {
                height_factor = width_factor.min(height_factor);
                width_factor = height_factor;
            }
            ConstraintSize::FitCoverOne => {
                height_factor = width_factor.min(height_factor);
                if multi_cell && height_factor > 1.0 {
                    let (_, single_height_factor) = self.scale_factors(group, metrics, 1);
                    height_factor = 1.0_f32.max(single_height_factor);
                }
                width_factor = height_factor;
            }
            ConstraintSize::Stretch => {}
        }

        if let Some(ratio) = self.max_xy_ratio {
            if group.width * width_factor > group.height * height_factor * ratio {
                width_factor = group.height * height_factor * ratio / group.width;
            }
        }

        (width_factor, height_factor)
    }

    fn aligned_y(self, group: GlyphSize, metrics: TerminalMetrics) -> f32 {
        if self.size == ConstraintSize::None && self.align_vertical == ConstraintAlign::None {
            return group.y;
        }

        let pad_bottom_dy = self.pad_bottom * metrics.face_height;
        let pad_top_dy = self.pad_top * metrics.face_height;
        let start_y = metrics.face_y + pad_bottom_dy;
        let end_y = metrics.face_y + (metrics.face_height - group.height - pad_top_dy);
        let center_y = (start_y + end_y) / 2.0;

        match self.align_vertical {
            ConstraintAlign::None => {
                if end_y < start_y {
                    center_y
                } else {
                    start_y.max(group.y.min(end_y))
                }
            }
            ConstraintAlign::Start => start_y,
            ConstraintAlign::End => end_y,
            ConstraintAlign::Center | ConstraintAlign::CenterOne => center_y,
        }
    }

    fn aligned_x(
        self,
        group: GlyphSize,
        metrics: TerminalMetrics,
        min_constraint_width: u8,
    ) -> f32 {
        if self.size == ConstraintSize::None && self.align_horizontal == ConstraintAlign::None {
            return group.x;
        }

        let full_face_span =
            metrics.face_width + ((min_constraint_width - 1) as f32 * metrics.cell_width);
        let pad_left_dx = self.pad_left * metrics.face_width;
        let pad_right_dx = self.pad_right * metrics.face_width;
        let start_x = pad_left_dx;
        let end_x = full_face_span - group.width - pad_right_dx;

        match self.align_horizontal {
            ConstraintAlign::None => start_x.max(group.x.min(end_x)),
            ConstraintAlign::Start => start_x,
            ConstraintAlign::End => start_x.max(end_x),
            ConstraintAlign::Center => start_x.max((start_x + end_x) / 2.0),
            ConstraintAlign::CenterOne => {
                let end1_x = metrics.face_width - group.width - pad_right_dx;
                start_x.max((start_x + end1_x) / 2.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics() -> TerminalMetrics {
        TerminalMetrics {
            cell_width: 10.0,
            cell_height: 22.0,
            cell_baseline: 5.0,
            underline_position: 19.0,
            underline_thickness: 1.0,
            strikethrough_position: 12.0,
            strikethrough_thickness: 1.0,
            overline_position: 0.0,
            overline_thickness: 1.0,
            box_thickness: 1.0,
            cursor_thickness: 1.0,
            cursor_height: 22.0,
            icon_height: 21.12,
            icon_height_single: 44.48 / 3.0,
            face_width: 9.6,
            face_height: 21.12,
            face_y: 0.2,
        }
    }

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.01,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn unconstrained_ascii_is_unchanged() {
        let glyph = GlyphSize {
            width: 6.784,
            height: 15.28,
            x: 1.408,
            y: 4.84,
        };

        assert_eq!(GlyphConstraint::NONE.constrain(glyph, metrics(), 1), glyph);
        assert_eq!(GlyphConstraint::NONE.constrain(glyph, metrics(), 2), glyph);
    }

    #[test]
    fn fit_constraint_matches_ghostty_symbol_example() {
        let constraint = GlyphConstraint {
            size: ConstraintSize::Fit,
            ..GlyphConstraint::NONE
        };
        let glyph = GlyphSize {
            width: 10.272,
            height: 10.272,
            x: 2.864,
            y: 5.304,
        };

        let constrained = constraint.constrain(glyph, metrics(), 1);

        assert_close(constrained.width, 9.6);
        assert_close(constrained.height, 9.6);
        assert_close(constrained.x, 0.0);
        assert_close(constrained.y, 5.64);
    }
}
