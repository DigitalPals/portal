use iced::advanced::renderer::{self, Quad};
use iced::{Background, Border, Color, Rectangle, Shadow};

/// Render block element characters as rectangles.
/// This bypasses font rendering for pixel-perfect block graphics.
/// Returns true if the character was rendered, false if it should use text rendering.
pub fn render_block_element<Renderer: renderer::Renderer>(
    renderer: &mut Renderer,
    c: char,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fg_color: Color,
) -> bool {
    let half_w = width / 2.0;
    let half_h = height / 2.0;

    // Helper to draw a rectangle
    let draw_rect = |renderer: &mut Renderer, rect: Rectangle| {
        renderer.fill_quad(
            Quad {
                bounds: rect,
                border: Border::default(),
                shadow: Shadow::default(),
                snap: true,
            },
            Background::Color(fg_color),
        );
    };

    match c {
        // Full block
        '█' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width,
                    height,
                },
            );
            true
        }
        // Upper half block
        '▀' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width,
                    height: half_h,
                },
            );
            true
        }
        // Lower half block
        '▄' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + half_h,
                    width,
                    height: half_h,
                },
            );
            true
        }
        // Left half block
        '▌' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: half_w,
                    height,
                },
            );
            true
        }
        // Right half block
        '▐' => {
            draw_rect(
                renderer,
                Rectangle {
                    x: x + half_w,
                    y,
                    width: half_w,
                    height,
                },
            );
            true
        }
        // Quadrant upper left and upper right and lower left (▛)
        '▛' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width,
                    height: half_h,
                },
            ); // top full
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + half_h,
                    width: half_w,
                    height: half_h,
                },
            ); // bottom left
            true
        }
        // Quadrant upper left and upper right and lower right (▜)
        '▜' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width,
                    height: half_h,
                },
            ); // top full
            draw_rect(
                renderer,
                Rectangle {
                    x: x + half_w,
                    y: y + half_h,
                    width: half_w,
                    height: half_h,
                },
            ); // bottom right
            true
        }
        // Quadrant upper left and lower left and lower right (▙)
        '▙' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: half_w,
                    height: half_h,
                },
            ); // top left
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + half_h,
                    width,
                    height: half_h,
                },
            ); // bottom full
            true
        }
        // Quadrant upper right and lower left and lower right (▟)
        '▟' => {
            draw_rect(
                renderer,
                Rectangle {
                    x: x + half_w,
                    y,
                    width: half_w,
                    height: half_h,
                },
            ); // top right
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + half_h,
                    width,
                    height: half_h,
                },
            ); // bottom full
            true
        }
        // Quadrant upper left (▘)
        '▘' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: half_w,
                    height: half_h,
                },
            );
            true
        }
        // Quadrant upper right (▝)
        '▝' => {
            draw_rect(
                renderer,
                Rectangle {
                    x: x + half_w,
                    y,
                    width: half_w,
                    height: half_h,
                },
            );
            true
        }
        // Quadrant lower left (▖)
        '▖' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + half_h,
                    width: half_w,
                    height: half_h,
                },
            );
            true
        }
        // Quadrant lower right (▗)
        '▗' => {
            draw_rect(
                renderer,
                Rectangle {
                    x: x + half_w,
                    y: y + half_h,
                    width: half_w,
                    height: half_h,
                },
            );
            true
        }
        // Quadrant upper left and lower right (▚)
        '▚' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: half_w,
                    height: half_h,
                },
            );
            draw_rect(
                renderer,
                Rectangle {
                    x: x + half_w,
                    y: y + half_h,
                    width: half_w,
                    height: half_h,
                },
            );
            true
        }
        // Quadrant upper right and lower left (▞)
        '▞' => {
            draw_rect(
                renderer,
                Rectangle {
                    x: x + half_w,
                    y,
                    width: half_w,
                    height: half_h,
                },
            );
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + half_h,
                    width: half_w,
                    height: half_h,
                },
            );
            true
        }
        // Upper one eighth block
        '▔' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width,
                    height: height / 8.0,
                },
            );
            true
        }
        // Lower one eighth block
        '▁' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + height * 7.0 / 8.0,
                    width,
                    height: height / 8.0,
                },
            );
            true
        }
        // Lower one quarter block
        '▂' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + height * 3.0 / 4.0,
                    width,
                    height: height / 4.0,
                },
            );
            true
        }
        // Lower three eighths block
        '▃' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + height * 5.0 / 8.0,
                    width,
                    height: height * 3.0 / 8.0,
                },
            );
            true
        }
        // Lower five eighths block
        '▅' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + height * 3.0 / 8.0,
                    width,
                    height: height * 5.0 / 8.0,
                },
            );
            true
        }
        // Lower three quarters block
        '▆' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + height / 4.0,
                    width,
                    height: height * 3.0 / 4.0,
                },
            );
            true
        }
        // Lower seven eighths block
        '▇' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y: y + height / 8.0,
                    width,
                    height: height * 7.0 / 8.0,
                },
            );
            true
        }
        // Left seven eighths block
        '▉' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: width * 7.0 / 8.0,
                    height,
                },
            );
            true
        }
        // Left three quarters block
        '▊' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: width * 3.0 / 4.0,
                    height,
                },
            );
            true
        }
        // Left five eighths block
        '▋' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: width * 5.0 / 8.0,
                    height,
                },
            );
            true
        }
        // Left three eighths block
        '▍' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: width * 3.0 / 8.0,
                    height,
                },
            );
            true
        }
        // Left one quarter block
        '▎' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: width / 4.0,
                    height,
                },
            );
            true
        }
        // Left one eighth block
        '▏' => {
            draw_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: width / 8.0,
                    height,
                },
            );
            true
        }
        // Right one eighth block
        '▕' => {
            draw_rect(
                renderer,
                Rectangle {
                    x: x + width * 7.0 / 8.0,
                    y,
                    width: width / 8.0,
                    height,
                },
            );
            true
        }
        // Shade characters - use text rendering for these
        '░' | '▒' | '▓' => false,
        // Not a block element we handle
        _ => false,
    }
}
