use iced::advanced::renderer::{self, Quad};
use iced::{Background, Border, Color, Rectangle, Shadow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stroke {
    Light,
    Heavy,
    Double,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoxGlyph {
    up: bool,
    right: bool,
    down: bool,
    left: bool,
    stroke: Stroke,
}

/// Render terminal graphics characters as rectangles.
/// This bypasses font rendering for pixel-perfect block and box drawing.
/// Returns true if the character was rendered, false if it should use text rendering.
pub fn render_terminal_graphic<Renderer: renderer::Renderer>(
    renderer: &mut Renderer,
    c: char,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fg_color: Color,
) -> bool {
    if let Some(rects) = block_element_rects(c, x, y, width, height) {
        draw_rects(renderer, &rects, fg_color);
        return true;
    }

    if let Some(rects) = box_drawing_rects(c, x, y, width, height) {
        draw_rects(renderer, &rects, fg_color);
        return true;
    }

    false
}

fn draw_rects<Renderer: renderer::Renderer>(
    renderer: &mut Renderer,
    rects: &[Rectangle],
    fg_color: Color,
) {
    for rect in rects {
        renderer.fill_quad(
            Quad {
                bounds: *rect,
                border: Border::default(),
                shadow: Shadow::default(),
                snap: true,
            },
            Background::Color(fg_color),
        );
    }
}

fn block_element_rects(c: char, x: f32, y: f32, width: f32, height: f32) -> Option<Vec<Rectangle>> {
    let half_w = width / 2.0;
    let half_h = height / 2.0;

    let rects = match c {
        // Full block
        '█' => vec![Rectangle {
            x,
            y,
            width,
            height,
        }],
        // Upper half block
        '▀' => vec![Rectangle {
            x,
            y,
            width,
            height: half_h,
        }],
        // Lower half block
        '▄' => vec![Rectangle {
            x,
            y: y + half_h,
            width,
            height: half_h,
        }],
        // Left half block
        '▌' => vec![Rectangle {
            x,
            y,
            width: half_w,
            height,
        }],
        // Right half block
        '▐' => vec![Rectangle {
            x: x + half_w,
            y,
            width: half_w,
            height,
        }],
        // Quadrant upper left and upper right and lower left (▛)
        '▛' => vec![
            Rectangle {
                x,
                y,
                width,
                height: half_h,
            },
            Rectangle {
                x,
                y: y + half_h,
                width: half_w,
                height: half_h,
            },
        ],
        // Quadrant upper left and upper right and lower right (▜)
        '▜' => vec![
            Rectangle {
                x,
                y,
                width,
                height: half_h,
            },
            Rectangle {
                x: x + half_w,
                y: y + half_h,
                width: half_w,
                height: half_h,
            },
        ],
        // Quadrant upper left and lower left and lower right (▙)
        '▙' => vec![
            Rectangle {
                x,
                y,
                width: half_w,
                height: half_h,
            },
            Rectangle {
                x,
                y: y + half_h,
                width,
                height: half_h,
            },
        ],
        // Quadrant upper right and lower left and lower right (▟)
        '▟' => vec![
            Rectangle {
                x: x + half_w,
                y,
                width: half_w,
                height: half_h,
            },
            Rectangle {
                x,
                y: y + half_h,
                width,
                height: half_h,
            },
        ],
        // Quadrant upper left (▘)
        '▘' => vec![Rectangle {
            x,
            y,
            width: half_w,
            height: half_h,
        }],
        // Quadrant upper right (▝)
        '▝' => vec![Rectangle {
            x: x + half_w,
            y,
            width: half_w,
            height: half_h,
        }],
        // Quadrant lower left (▖)
        '▖' => vec![Rectangle {
            x,
            y: y + half_h,
            width: half_w,
            height: half_h,
        }],
        // Quadrant lower right (▗)
        '▗' => vec![Rectangle {
            x: x + half_w,
            y: y + half_h,
            width: half_w,
            height: half_h,
        }],
        // Quadrant upper left and lower right (▚)
        '▚' => vec![
            Rectangle {
                x,
                y,
                width: half_w,
                height: half_h,
            },
            Rectangle {
                x: x + half_w,
                y: y + half_h,
                width: half_w,
                height: half_h,
            },
        ],
        // Quadrant upper right and lower left (▞)
        '▞' => vec![
            Rectangle {
                x: x + half_w,
                y,
                width: half_w,
                height: half_h,
            },
            Rectangle {
                x,
                y: y + half_h,
                width: half_w,
                height: half_h,
            },
        ],
        // Upper one eighth block
        '▔' => vec![Rectangle {
            x,
            y,
            width,
            height: height / 8.0,
        }],
        // Lower one eighth block
        '▁' => vec![Rectangle {
            x,
            y: y + height * 7.0 / 8.0,
            width,
            height: height / 8.0,
        }],
        // Lower one quarter block
        '▂' => vec![Rectangle {
            x,
            y: y + height * 3.0 / 4.0,
            width,
            height: height / 4.0,
        }],
        // Lower three eighths block
        '▃' => vec![Rectangle {
            x,
            y: y + height * 5.0 / 8.0,
            width,
            height: height * 3.0 / 8.0,
        }],
        // Lower five eighths block
        '▅' => vec![Rectangle {
            x,
            y: y + height * 3.0 / 8.0,
            width,
            height: height * 5.0 / 8.0,
        }],
        // Lower three quarters block
        '▆' => vec![Rectangle {
            x,
            y: y + height / 4.0,
            width,
            height: height * 3.0 / 4.0,
        }],
        // Lower seven eighths block
        '▇' => vec![Rectangle {
            x,
            y: y + height / 8.0,
            width,
            height: height * 7.0 / 8.0,
        }],
        // Left seven eighths block
        '▉' => vec![Rectangle {
            x,
            y,
            width: width * 7.0 / 8.0,
            height,
        }],
        // Left three quarters block
        '▊' => vec![Rectangle {
            x,
            y,
            width: width * 3.0 / 4.0,
            height,
        }],
        // Left five eighths block
        '▋' => vec![Rectangle {
            x,
            y,
            width: width * 5.0 / 8.0,
            height,
        }],
        // Left three eighths block
        '▍' => vec![Rectangle {
            x,
            y,
            width: width * 3.0 / 8.0,
            height,
        }],
        // Left one quarter block
        '▎' => vec![Rectangle {
            x,
            y,
            width: width / 4.0,
            height,
        }],
        // Left one eighth block
        '▏' => vec![Rectangle {
            x,
            y,
            width: width / 8.0,
            height,
        }],
        // Right one eighth block
        '▕' => vec![Rectangle {
            x: x + width * 7.0 / 8.0,
            y,
            width: width / 8.0,
            height,
        }],
        // Shade characters - use text rendering for these
        '░' | '▒' | '▓' => return None,
        // Not a block element we handle
        _ => return None,
    };

    Some(rects)
}

fn box_glyph(c: char) -> Option<BoxGlyph> {
    let light = Stroke::Light;
    let heavy = Stroke::Heavy;
    let double = Stroke::Double;

    let glyph = match c {
        '─' | '╌' => BoxGlyph {
            up: false,
            right: true,
            down: false,
            left: true,
            stroke: light,
        },
        '│' | '╎' => BoxGlyph {
            up: true,
            right: false,
            down: true,
            left: false,
            stroke: light,
        },
        '╴' => BoxGlyph {
            up: false,
            right: false,
            down: false,
            left: true,
            stroke: light,
        },
        '╶' => BoxGlyph {
            up: false,
            right: true,
            down: false,
            left: false,
            stroke: light,
        },
        '╵' => BoxGlyph {
            up: true,
            right: false,
            down: false,
            left: false,
            stroke: light,
        },
        '╷' => BoxGlyph {
            up: false,
            right: false,
            down: true,
            left: false,
            stroke: light,
        },
        '┌' | '╭' => BoxGlyph {
            up: false,
            right: true,
            down: true,
            left: false,
            stroke: light,
        },
        '┐' | '╮' => BoxGlyph {
            up: false,
            right: false,
            down: true,
            left: true,
            stroke: light,
        },
        '└' | '╰' => BoxGlyph {
            up: true,
            right: true,
            down: false,
            left: false,
            stroke: light,
        },
        '┘' | '╯' => BoxGlyph {
            up: true,
            right: false,
            down: false,
            left: true,
            stroke: light,
        },
        '├' => BoxGlyph {
            up: true,
            right: true,
            down: true,
            left: false,
            stroke: light,
        },
        '┤' => BoxGlyph {
            up: true,
            right: false,
            down: true,
            left: true,
            stroke: light,
        },
        '┬' => BoxGlyph {
            up: false,
            right: true,
            down: true,
            left: true,
            stroke: light,
        },
        '┴' => BoxGlyph {
            up: true,
            right: true,
            down: false,
            left: true,
            stroke: light,
        },
        '┼' => BoxGlyph {
            up: true,
            right: true,
            down: true,
            left: true,
            stroke: light,
        },
        '━' => BoxGlyph {
            up: false,
            right: true,
            down: false,
            left: true,
            stroke: heavy,
        },
        '┃' => BoxGlyph {
            up: true,
            right: false,
            down: true,
            left: false,
            stroke: heavy,
        },
        '╸' => BoxGlyph {
            up: false,
            right: false,
            down: false,
            left: true,
            stroke: heavy,
        },
        '╺' => BoxGlyph {
            up: false,
            right: true,
            down: false,
            left: false,
            stroke: heavy,
        },
        '╹' => BoxGlyph {
            up: true,
            right: false,
            down: false,
            left: false,
            stroke: heavy,
        },
        '╻' => BoxGlyph {
            up: false,
            right: false,
            down: true,
            left: false,
            stroke: heavy,
        },
        '┏' => BoxGlyph {
            up: false,
            right: true,
            down: true,
            left: false,
            stroke: heavy,
        },
        '┓' => BoxGlyph {
            up: false,
            right: false,
            down: true,
            left: true,
            stroke: heavy,
        },
        '┗' => BoxGlyph {
            up: true,
            right: true,
            down: false,
            left: false,
            stroke: heavy,
        },
        '┛' => BoxGlyph {
            up: true,
            right: false,
            down: false,
            left: true,
            stroke: heavy,
        },
        '┣' => BoxGlyph {
            up: true,
            right: true,
            down: true,
            left: false,
            stroke: heavy,
        },
        '┫' => BoxGlyph {
            up: true,
            right: false,
            down: true,
            left: true,
            stroke: heavy,
        },
        '┳' => BoxGlyph {
            up: false,
            right: true,
            down: true,
            left: true,
            stroke: heavy,
        },
        '┻' => BoxGlyph {
            up: true,
            right: true,
            down: false,
            left: true,
            stroke: heavy,
        },
        '╋' => BoxGlyph {
            up: true,
            right: true,
            down: true,
            left: true,
            stroke: heavy,
        },
        '═' => BoxGlyph {
            up: false,
            right: true,
            down: false,
            left: true,
            stroke: double,
        },
        '║' => BoxGlyph {
            up: true,
            right: false,
            down: true,
            left: false,
            stroke: double,
        },
        '╔' => BoxGlyph {
            up: false,
            right: true,
            down: true,
            left: false,
            stroke: double,
        },
        '╗' => BoxGlyph {
            up: false,
            right: false,
            down: true,
            left: true,
            stroke: double,
        },
        '╚' => BoxGlyph {
            up: true,
            right: true,
            down: false,
            left: false,
            stroke: double,
        },
        '╝' => BoxGlyph {
            up: true,
            right: false,
            down: false,
            left: true,
            stroke: double,
        },
        '╠' => BoxGlyph {
            up: true,
            right: true,
            down: true,
            left: false,
            stroke: double,
        },
        '╣' => BoxGlyph {
            up: true,
            right: false,
            down: true,
            left: true,
            stroke: double,
        },
        '╦' => BoxGlyph {
            up: false,
            right: true,
            down: true,
            left: true,
            stroke: double,
        },
        '╩' => BoxGlyph {
            up: true,
            right: true,
            down: false,
            left: true,
            stroke: double,
        },
        '╬' => BoxGlyph {
            up: true,
            right: true,
            down: true,
            left: true,
            stroke: double,
        },
        _ => return None,
    };

    Some(glyph)
}

fn box_drawing_rects(c: char, x: f32, y: f32, width: f32, height: f32) -> Option<Vec<Rectangle>> {
    let glyph = box_glyph(c)?;

    match glyph.stroke {
        Stroke::Light | Stroke::Heavy => Some(single_stroke_rects(
            glyph,
            x,
            y,
            width,
            height,
            stroke_width(glyph.stroke, width, height),
        )),
        Stroke::Double => Some(double_stroke_rects(glyph, x, y, width, height)),
    }
}

fn stroke_width(stroke: Stroke, width: f32, height: f32) -> f32 {
    let base = (width.min(height) / 8.0).max(1.0);

    match stroke {
        Stroke::Light | Stroke::Double => base,
        Stroke::Heavy => (base * 2.0).min(width.min(height) / 3.0).max(base + 1.0),
    }
}

fn single_stroke_rects(
    glyph: BoxGlyph,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    stroke: f32,
) -> Vec<Rectangle> {
    let center_x = x + width / 2.0;
    let center_y = y + height / 2.0;
    let half = stroke / 2.0;
    let mut rects = Vec::with_capacity(4);

    if glyph.up {
        rects.push(Rectangle {
            x: center_x - half,
            y,
            width: stroke,
            height: height / 2.0 + half,
        });
    }

    if glyph.down {
        rects.push(Rectangle {
            x: center_x - half,
            y: center_y - half,
            width: stroke,
            height: height / 2.0 + half,
        });
    }

    if glyph.left {
        rects.push(Rectangle {
            x,
            y: center_y - half,
            width: width / 2.0 + half,
            height: stroke,
        });
    }

    if glyph.right {
        rects.push(Rectangle {
            x: center_x - half,
            y: center_y - half,
            width: width / 2.0 + half,
            height: stroke,
        });
    }

    rects
}

fn double_stroke_rects(glyph: BoxGlyph, x: f32, y: f32, width: f32, height: f32) -> Vec<Rectangle> {
    let stroke = stroke_width(Stroke::Double, width, height);
    let gap = stroke.max(width.min(height) / 8.0);
    let center_x = x + width / 2.0;
    let center_y = y + height / 2.0;
    let vertical_left = center_x - gap / 2.0 - stroke;
    let vertical_right = center_x + gap / 2.0;
    let horizontal_top = center_y - gap / 2.0 - stroke;
    let horizontal_bottom = center_y + gap / 2.0;
    let mut rects = Vec::with_capacity(8);

    for line_x in [vertical_left, vertical_right] {
        if glyph.up {
            rects.push(Rectangle {
                x: line_x,
                y,
                width: stroke,
                height: height / 2.0 + gap / 2.0,
            });
        }
        if glyph.down {
            rects.push(Rectangle {
                x: line_x,
                y: center_y - gap / 2.0,
                width: stroke,
                height: height / 2.0 + gap / 2.0,
            });
        }
    }

    for line_y in [horizontal_top, horizontal_bottom] {
        if glyph.left {
            rects.push(Rectangle {
                x,
                y: line_y,
                width: width / 2.0 + gap / 2.0,
                height: stroke,
            });
        }
        if glyph.right {
            rects.push(Rectangle {
                x: center_x - gap / 2.0,
                y: line_y,
                width: width / 2.0 + gap / 2.0,
                height: stroke,
            });
        }
    }

    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {expected}, got {actual}"
        );
    }

    #[test]
    fn vertical_light_line_spans_full_cell_height() {
        let rects = box_drawing_rects('│', 10.0, 20.0, 8.0, 16.0).expect("box glyph");

        assert_eq!(rects.len(), 2);
        assert_close(rects[0].y, 20.0);
        assert_close(rects[0].height, 8.5);
        assert_close(rects[1].y, 27.5);
        assert_close(rects[1].height, 8.5);
        assert_close(rects[1].y + rects[1].height, 36.0);
    }

    #[test]
    fn horizontal_light_line_spans_full_cell_width() {
        let rects = box_drawing_rects('─', 10.0, 20.0, 8.0, 16.0).expect("box glyph");

        assert_eq!(rects.len(), 2);
        assert_close(rects[0].x, 10.0);
        assert_close(rects[0].width, 4.5);
        assert_close(rects[1].x, 13.5);
        assert_close(rects[1].width, 4.5);
        assert_close(rects[1].x + rects[1].width, 18.0);
    }

    #[test]
    fn corner_connects_to_right_and_down_edges() {
        let rects = box_drawing_rects('┌', 10.0, 20.0, 8.0, 16.0).expect("box glyph");

        assert_eq!(rects.len(), 2);
        assert_close(rects[0].y + rects[0].height, 36.0);
        assert_close(rects[1].x + rects[1].width, 18.0);
    }

    #[test]
    fn double_vertical_line_draws_two_full_height_strokes() {
        let rects = box_drawing_rects('║', 10.0, 20.0, 8.0, 16.0).expect("box glyph");

        assert_eq!(rects.len(), 4);
        assert_close(rects[0].y, 20.0);
        assert_close(rects[1].y + rects[1].height, 36.0);
        assert_close(rects[2].y, 20.0);
        assert_close(rects[3].y + rects[3].height, 36.0);
    }

    #[test]
    fn block_elements_still_render_as_rects() {
        let rects = block_element_rects('█', 10.0, 20.0, 8.0, 16.0).expect("block glyph");

        assert_eq!(
            rects,
            vec![Rectangle {
                x: 10.0,
                y: 20.0,
                width: 8.0,
                height: 16.0,
            }]
        );
    }

    #[test]
    fn unsupported_characters_fall_back_to_text() {
        assert!(block_element_rects('A', 0.0, 0.0, 8.0, 16.0).is_none());
        assert!(box_drawing_rects('A', 0.0, 0.0, 8.0, 16.0).is_none());
    }
}
