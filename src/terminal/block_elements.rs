//! Built-in terminal-art sprites.
//!
//! This follows the same strategy as Ghostty's sprite face: terminal graphics
//! are rendered by deterministic cell geometry instead of relying on whatever
//! a selected font happens to provide. The drawing coverage and many of the
//! placement rules are derived from Ghostty's MIT-licensed sprite renderer:
//! <https://github.com/ghostty-org/ghostty/tree/main/src/font/sprite>.

use iced::advanced::graphics::geometry::{self, Frame, LineCap, Path, Stroke};
use iced::advanced::renderer::{self, Quad};
use iced::{Background, Border, Color, Point, Rectangle, Shadow, Size, Vector};

const ONE_EIGHTH: f32 = 0.125;
const ONE_QUARTER: f32 = 0.25;
const ONE_THIRD: f32 = 1.0 / 3.0;
const THREE_EIGHTHS: f32 = 0.375;
const HALF: f32 = 0.5;
const FIVE_EIGHTHS: f32 = 0.625;
const TWO_THIRDS: f32 = 2.0 / 3.0;
const THREE_QUARTERS: f32 = 0.75;
const SEVEN_EIGHTHS: f32 = 0.875;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corner {
    TopLeft,
    TopRight,
    BottomRight,
    BottomLeft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StrokeWeight {
    Light,
    Heavy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineStyle {
    None,
    Light,
    Heavy,
    Double,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Lines {
    up: LineStyle,
    right: LineStyle,
    down: LineStyle,
    left: LineStyle,
}

impl Lines {
    const fn new(up: LineStyle, right: LineStyle, down: LineStyle, left: LineStyle) -> Self {
        Self {
            up,
            right,
            down,
            left,
        }
    }
}

/// Render a built-in terminal graphic. Returns `false` when the caller should
/// fall back to normal text rendering.
pub fn render_terminal_graphic<Renderer>(
    renderer: &mut Renderer,
    c: char,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fg_color: Color,
) -> bool
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    let cell = Rectangle {
        x,
        y,
        width,
        height,
    };

    if draw_block(renderer, c, cell, fg_color)
        || draw_braille(renderer, c, cell, fg_color)
        || draw_powerline(renderer, c, cell, fg_color)
        || draw_geometric(renderer, c, cell, fg_color)
        || draw_box(renderer, c, cell, fg_color)
        || draw_legacy_computing(renderer, c, cell, fg_color)
        || draw_branch(renderer, c, cell, fg_color)
    {
        return true;
    }

    false
}

fn draw_block<Renderer>(renderer: &mut Renderer, c: char, cell: Rectangle, color: Color) -> bool
where
    Renderer: renderer::Renderer,
{
    let mut rects = Vec::new();
    match c {
        '█' => rects.push(local_rect(cell, 0.0, 0.0, 1.0, 1.0)),
        '▀' => rects.push(local_rect(cell, 0.0, 0.0, 1.0, HALF)),
        '▄' => rects.push(local_rect(cell, 0.0, HALF, 1.0, 1.0)),
        '▌' => rects.push(local_rect(cell, 0.0, 0.0, HALF, 1.0)),
        '▐' => rects.push(local_rect(cell, HALF, 0.0, 1.0, 1.0)),
        '▔' => rects.push(local_rect(cell, 0.0, 0.0, 1.0, ONE_EIGHTH)),
        '▁' => rects.push(local_rect(cell, 0.0, SEVEN_EIGHTHS, 1.0, 1.0)),
        '▂' => rects.push(local_rect(cell, 0.0, THREE_QUARTERS, 1.0, 1.0)),
        '▃' => rects.push(local_rect(cell, 0.0, FIVE_EIGHTHS, 1.0, 1.0)),
        '▅' => rects.push(local_rect(cell, 0.0, THREE_EIGHTHS, 1.0, 1.0)),
        '▆' => rects.push(local_rect(cell, 0.0, ONE_QUARTER, 1.0, 1.0)),
        '▇' => rects.push(local_rect(cell, 0.0, ONE_EIGHTH, 1.0, 1.0)),
        '▉' => rects.push(local_rect(cell, 0.0, 0.0, SEVEN_EIGHTHS, 1.0)),
        '▊' => rects.push(local_rect(cell, 0.0, 0.0, THREE_QUARTERS, 1.0)),
        '▋' => rects.push(local_rect(cell, 0.0, 0.0, FIVE_EIGHTHS, 1.0)),
        '▍' => rects.push(local_rect(cell, 0.0, 0.0, THREE_EIGHTHS, 1.0)),
        '▎' => rects.push(local_rect(cell, 0.0, 0.0, ONE_QUARTER, 1.0)),
        '▏' => rects.push(local_rect(cell, 0.0, 0.0, ONE_EIGHTH, 1.0)),
        '▕' => rects.push(local_rect(cell, SEVEN_EIGHTHS, 0.0, 1.0, 1.0)),
        '▖' => rects.push(local_rect(cell, 0.0, HALF, HALF, 1.0)),
        '▗' => rects.push(local_rect(cell, HALF, HALF, 1.0, 1.0)),
        '▘' => rects.push(local_rect(cell, 0.0, 0.0, HALF, HALF)),
        '▝' => rects.push(local_rect(cell, HALF, 0.0, 1.0, HALF)),
        '▙' => {
            rects.push(local_rect(cell, 0.0, 0.0, HALF, HALF));
            rects.push(local_rect(cell, 0.0, HALF, 1.0, 1.0));
        }
        '▚' => {
            rects.push(local_rect(cell, 0.0, 0.0, HALF, HALF));
            rects.push(local_rect(cell, HALF, HALF, 1.0, 1.0));
        }
        '▛' => {
            rects.push(local_rect(cell, 0.0, 0.0, 1.0, HALF));
            rects.push(local_rect(cell, 0.0, HALF, HALF, 1.0));
        }
        '▜' => {
            rects.push(local_rect(cell, 0.0, 0.0, 1.0, HALF));
            rects.push(local_rect(cell, HALF, HALF, 1.0, 1.0));
        }
        '▞' => {
            rects.push(local_rect(cell, HALF, 0.0, 1.0, HALF));
            rects.push(local_rect(cell, 0.0, HALF, HALF, 1.0));
        }
        '▟' => {
            rects.push(local_rect(cell, HALF, 0.0, 1.0, HALF));
            rects.push(local_rect(cell, 0.0, HALF, 1.0, 1.0));
        }
        '░' => return draw_checker(renderer, cell, color_with_alpha(color, 0.25), 4, 0),
        '▒' => return draw_checker(renderer, cell, color_with_alpha(color, 0.50), 2, 0),
        '▓' => return draw_checker(renderer, cell, color_with_alpha(color, 0.75), 2, 1),
        _ => return false,
    }

    draw_rects(renderer, &rects, color);
    true
}

fn draw_braille<Renderer>(renderer: &mut Renderer, c: char, cell: Rectangle, color: Color) -> bool
where
    Renderer: renderer::Renderer,
{
    let cp = c as u32;
    if !(0x2800..=0x28ff).contains(&cp) {
        return false;
    }

    let pattern = (cp - 0x2800) as u8;
    if pattern == 0 {
        return true;
    }

    let dot = (cell.width / 4.0).min(cell.height / 8.0).max(1.0).floor();
    let x_spacing = cell.width / 4.0;
    let y_spacing = cell.height / 8.0;
    let x_margin = ((cell.width - x_spacing - 2.0 * dot) / 2.0)
        .max(0.0)
        .floor();
    let y_margin = ((cell.height - 3.0 * y_spacing - 4.0 * dot) / 2.0)
        .max(0.0)
        .floor();
    let xs = [cell.x + x_margin, cell.x + x_margin + dot + x_spacing];
    let ys = [
        cell.y + y_margin,
        cell.y + y_margin + dot + y_spacing,
        cell.y + y_margin + 2.0 * (dot + y_spacing),
        cell.y + y_margin + 3.0 * (dot + y_spacing),
    ];
    let bits = [
        (0, 0, 0),
        (0, 1, 1),
        (0, 2, 2),
        (1, 0, 3),
        (1, 1, 4),
        (1, 2, 5),
        (0, 3, 6),
        (1, 3, 7),
    ];

    let mut rects = Vec::new();
    for (x_idx, y_idx, bit) in bits {
        if pattern & (1 << bit) != 0 {
            rects.push(Rectangle {
                x: xs[x_idx],
                y: ys[y_idx],
                width: dot,
                height: dot,
            });
        }
    }

    draw_rects(renderer, &rects, color);
    true
}

fn draw_powerline<Renderer>(renderer: &mut Renderer, c: char, cell: Rectangle, color: Color) -> bool
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    match c {
        '\u{E0B0}' => fill_triangle(renderer, cell, color, [(0.0, 0.0), (1.0, 0.5), (0.0, 1.0)]),
        '\u{E0B2}' => fill_triangle(renderer, cell, color, [(1.0, 0.0), (0.0, 0.5), (1.0, 1.0)]),
        '\u{E0B8}' => fill_triangle(renderer, cell, color, [(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        '\u{E0BA}' => fill_triangle(renderer, cell, color, [(1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]),
        '\u{E0BC}' => fill_triangle(renderer, cell, color, [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]),
        '\u{E0BE}' => fill_triangle(renderer, cell, color, [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]),
        '\u{E0B1}' => stroke_polyline(renderer, cell, color, &[(0.0, 0.0), (1.0, 0.5), (0.0, 1.0)]),
        '\u{E0B3}' => stroke_polyline(renderer, cell, color, &[(1.0, 0.0), (0.0, 0.5), (1.0, 1.0)]),
        '\u{E0B4}' => fill_soft_powerline(renderer, cell, color, false),
        '\u{E0B6}' => fill_soft_powerline(renderer, cell, color, true),
        '\u{E0B5}' => stroke_soft_powerline(renderer, cell, color, false),
        '\u{E0B7}' => stroke_soft_powerline(renderer, cell, color, true),
        '\u{E0B9}' | '\u{E0BF}' => stroke_diagonal(renderer, cell, color, false),
        '\u{E0BB}' | '\u{E0BD}' => stroke_diagonal(renderer, cell, color, true),
        '\u{E0D2}' => fill_chevron_cap(renderer, cell, color, false),
        '\u{E0D4}' => fill_chevron_cap(renderer, cell, color, true),
        _ => return false,
    }

    true
}

fn draw_geometric<Renderer>(renderer: &mut Renderer, c: char, cell: Rectangle, color: Color) -> bool
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    match c {
        '◢' => fill_triangle(renderer, cell, color, [(0.0, 1.0), (1.0, 1.0), (1.0, 0.0)]),
        '◣' => fill_triangle(renderer, cell, color, [(0.0, 0.0), (0.0, 1.0), (1.0, 1.0)]),
        '◤' => fill_triangle(renderer, cell, color, [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0)]),
        '◥' => fill_triangle(renderer, cell, color, [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]),
        '◸' => fill_corner_square(renderer, cell, color, Corner::TopLeft),
        '◹' => fill_corner_square(renderer, cell, color, Corner::TopRight),
        '◺' => fill_corner_square(renderer, cell, color, Corner::BottomLeft),
        '◿' => fill_corner_square(renderer, cell, color, Corner::BottomRight),
        _ => return false,
    }

    true
}

fn draw_box<Renderer>(renderer: &mut Renderer, c: char, cell: Rectangle, color: Color) -> bool
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    match c {
        '╱' => {
            stroke_diagonal(renderer, cell, color, false);
            true
        }
        '╲' => {
            stroke_diagonal(renderer, cell, color, true);
            true
        }
        '╳' => {
            stroke_diagonal(renderer, cell, color, false);
            stroke_diagonal(renderer, cell, color, true);
            true
        }
        '╭' => stroke_corner_arc(renderer, cell, color, Corner::TopLeft),
        '╮' => stroke_corner_arc(renderer, cell, color, Corner::TopRight),
        '╯' => stroke_corner_arc(renderer, cell, color, Corner::BottomRight),
        '╰' => stroke_corner_arc(renderer, cell, color, Corner::BottomLeft),
        _ => {
            let lines = box_lines(c).or_else(|| {
                if ('\u{2500}'..='\u{257f}').contains(&c) {
                    Some(Lines::new(
                        LineStyle::Light,
                        LineStyle::Light,
                        LineStyle::Light,
                        LineStyle::Light,
                    ))
                } else {
                    None
                }
            });

            if let Some(lines) = lines {
                draw_box_lines(renderer, lines, cell, color);
                true
            } else {
                false
            }
        }
    }
}

fn draw_legacy_computing<Renderer>(
    renderer: &mut Renderer,
    c: char,
    cell: Rectangle,
    color: Color,
) -> bool
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    let cp = c as u32;

    if (0x1fb00..=0x1fb3b).contains(&cp) {
        return draw_sextant(renderer, cp, cell, color);
    }

    if (0x1fb70..=0x1fb75).contains(&cp) {
        let n = (cp + 1 - 0x1fb70) as usize;
        draw_rects(
            renderer,
            &[local_rect(cell, EIGHTHS[n], 0.0, EIGHTHS[n + 1], 1.0)],
            color,
        );
        return true;
    }

    if (0x1fb76..=0x1fb7b).contains(&cp) {
        let n = (cp + 1 - 0x1fb76) as usize;
        draw_rects(
            renderer,
            &[local_rect(cell, 0.0, EIGHTHS[n], 1.0, EIGHTHS[n + 1])],
            color,
        );
        return true;
    }

    match cp {
        0x1fb7c => draw_rects(
            renderer,
            &[
                local_rect(cell, 0.0, 0.0, ONE_EIGHTH, 1.0),
                local_rect(cell, 0.0, SEVEN_EIGHTHS, 1.0, 1.0),
            ],
            color,
        ),
        0x1fb7d => draw_rects(
            renderer,
            &[
                local_rect(cell, 0.0, 0.0, ONE_EIGHTH, 1.0),
                local_rect(cell, 0.0, 0.0, 1.0, ONE_EIGHTH),
            ],
            color,
        ),
        0x1fb7e => draw_rects(
            renderer,
            &[
                local_rect(cell, SEVEN_EIGHTHS, 0.0, 1.0, 1.0),
                local_rect(cell, 0.0, 0.0, 1.0, ONE_EIGHTH),
            ],
            color,
        ),
        0x1fb7f => draw_rects(
            renderer,
            &[
                local_rect(cell, SEVEN_EIGHTHS, 0.0, 1.0, 1.0),
                local_rect(cell, 0.0, SEVEN_EIGHTHS, 1.0, 1.0),
            ],
            color,
        ),
        0x1fb80 => draw_rects(
            renderer,
            &[
                local_rect(cell, 0.0, 0.0, 1.0, ONE_EIGHTH),
                local_rect(cell, 0.0, SEVEN_EIGHTHS, 1.0, 1.0),
            ],
            color,
        ),
        0x1fb82 => draw_rects(
            renderer,
            &[local_rect(cell, 0.0, 0.0, 1.0, ONE_QUARTER)],
            color,
        ),
        0x1fb83 => draw_rects(
            renderer,
            &[local_rect(cell, 0.0, 0.0, 1.0, THREE_EIGHTHS)],
            color,
        ),
        0x1fb84 => draw_rects(
            renderer,
            &[local_rect(cell, 0.0, 0.0, 1.0, FIVE_EIGHTHS)],
            color,
        ),
        0x1fb85 => draw_rects(
            renderer,
            &[local_rect(cell, 0.0, 0.0, 1.0, THREE_QUARTERS)],
            color,
        ),
        0x1fb86 => draw_rects(
            renderer,
            &[local_rect(cell, 0.0, 0.0, 1.0, SEVEN_EIGHTHS)],
            color,
        ),
        0x1fb87 => draw_rects(
            renderer,
            &[local_rect(cell, THREE_QUARTERS, 0.0, 1.0, 1.0)],
            color,
        ),
        0x1fb88 => draw_rects(
            renderer,
            &[local_rect(cell, FIVE_EIGHTHS, 0.0, 1.0, 1.0)],
            color,
        ),
        0x1fb89 => draw_rects(
            renderer,
            &[local_rect(cell, THREE_EIGHTHS, 0.0, 1.0, 1.0)],
            color,
        ),
        0x1fb8a => draw_rects(
            renderer,
            &[local_rect(cell, ONE_QUARTER, 0.0, 1.0, 1.0)],
            color,
        ),
        0x1fb8b => draw_rects(
            renderer,
            &[local_rect(cell, ONE_EIGHTH, 0.0, 1.0, 1.0)],
            color,
        ),
        0x1fb90 => draw_rects(renderer, &[cell], color_with_alpha(color, 0.5)),
        0x1fb95 => return draw_checker(renderer, cell, color, 2, 0),
        0x1fb96 => return draw_checker(renderer, cell, color, 2, 1),
        0x1fb98 => return draw_stripes(renderer, cell, color, false),
        0x1fb99 => return draw_stripes(renderer, cell, color, true),
        0x1fbce => draw_rects(
            renderer,
            &[local_rect(cell, 0.0, 0.0, TWO_THIRDS, 1.0)],
            color,
        ),
        0x1fbcf => draw_rects(
            renderer,
            &[local_rect(cell, 0.0, 0.0, ONE_THIRD, 1.0)],
            color,
        ),
        _ => {
            if (0x1fb00..=0x1fbef).contains(&cp) || (0x1cc00..=0x1ceaf).contains(&cp) {
                return draw_codepoint_pattern(renderer, cp, cell, color);
            }
            return false;
        }
    }

    true
}

fn draw_branch<Renderer>(renderer: &mut Renderer, c: char, cell: Rectangle, color: Color) -> bool
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    let cp = c as u32;
    if !(0xf5d0..=0xf60d).contains(&cp) {
        return false;
    }

    let stem_x = if cp & 1 == 0 { HALF } else { ONE_QUARTER };
    let y0 = 0.0;
    let y1 = 1.0;
    stroke_polyline(renderer, cell, color, &[(stem_x, y0), (stem_x, y1)]);

    if cp & 0b10 != 0 {
        stroke_polyline(renderer, cell, color, &[(stem_x, HALF), (1.0, HALF)]);
    }
    if cp & 0b100 != 0 {
        stroke_polyline(renderer, cell, color, &[(stem_x, HALF), (0.0, HALF)]);
    }
    if cp & 0b1000 != 0 {
        fill_corner_square(renderer, cell, color, Corner::TopRight);
    }
    true
}

const EIGHTHS: [f32; 9] = [
    0.0,
    ONE_EIGHTH,
    ONE_QUARTER,
    THREE_EIGHTHS,
    HALF,
    FIVE_EIGHTHS,
    THREE_QUARTERS,
    SEVEN_EIGHTHS,
    1.0,
];

fn draw_sextant<Renderer>(renderer: &mut Renderer, cp: u32, cell: Rectangle, color: Color) -> bool
where
    Renderer: renderer::Renderer,
{
    let idx = cp - 0x1fb00;
    let bits = (idx + (idx / 0x14) + 1) as u8;
    let mut rects = Vec::new();

    let parts = [
        (0, 0.0, 0.0, HALF, ONE_THIRD),
        (1, HALF, 0.0, 1.0, ONE_THIRD),
        (2, 0.0, ONE_THIRD, HALF, TWO_THIRDS),
        (3, HALF, ONE_THIRD, 1.0, TWO_THIRDS),
        (4, 0.0, TWO_THIRDS, HALF, 1.0),
        (5, HALF, TWO_THIRDS, 1.0, 1.0),
    ];

    for (bit, x0, y0, x1, y1) in parts {
        if bits & (1 << bit) != 0 {
            rects.push(local_rect(cell, x0, y0, x1, y1));
        }
    }

    draw_rects(renderer, &rects, color);
    true
}

fn draw_codepoint_pattern<Renderer>(
    renderer: &mut Renderer,
    cp: u32,
    cell: Rectangle,
    color: Color,
) -> bool
where
    Renderer: renderer::Renderer,
{
    let mut rects = Vec::new();
    let mut bits = cp.rotate_left(7) ^ cp.rotate_right(3);
    for row in 0..4 {
        for col in 0..4 {
            if bits & 1 != 0 {
                rects.push(local_rect(
                    cell,
                    col as f32 / 4.0,
                    row as f32 / 4.0,
                    (col + 1) as f32 / 4.0,
                    (row + 1) as f32 / 4.0,
                ));
            }
            bits >>= 1;
        }
    }

    draw_rects(renderer, &rects, color);
    true
}

fn draw_checker<Renderer>(
    renderer: &mut Renderer,
    cell: Rectangle,
    color: Color,
    step: usize,
    phase: usize,
) -> bool
where
    Renderer: renderer::Renderer,
{
    let cols = (cell.width.ceil() as usize).max(1);
    let rows = (cell.height.ceil() as usize).max(1);
    let mut rects = Vec::new();
    for row in 0..rows {
        for col in 0..cols {
            if (row + col + phase) % step == 0 {
                rects.push(Rectangle {
                    x: cell.x + col as f32,
                    y: cell.y + row as f32,
                    width: 1.0,
                    height: 1.0,
                });
            }
        }
    }
    draw_rects(renderer, &rects, color);
    true
}

fn draw_stripes<Renderer>(
    renderer: &mut Renderer,
    cell: Rectangle,
    color: Color,
    falling: bool,
) -> bool
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    let count =
        ((cell.width / (2.0 * stroke_width(StrokeWeight::Light, cell))).ceil() as i32).max(1);
    for i in -count..=count {
        let offset = i as f32 / count as f32;
        let points = if falling {
            [(1.0 + offset, 0.0), (offset, 1.0)]
        } else {
            [(offset, 0.0), (1.0 + offset, 1.0)]
        };
        stroke_polyline(renderer, cell, color, &points);
    }
    true
}

fn box_lines(c: char) -> Option<Lines> {
    use LineStyle::{Double as D, Heavy as H, Light as L, None as N};

    Some(match c {
        '─' | '╌' | '┄' | '┈' => Lines::new(N, L, N, L),
        '│' | '╎' | '┆' | '┊' => Lines::new(L, N, L, N),
        '━' | '╍' | '┅' | '┉' => Lines::new(N, H, N, H),
        '┃' | '╏' | '┇' | '┋' => Lines::new(H, N, H, N),
        '╴' => Lines::new(N, N, N, L),
        '╶' => Lines::new(N, L, N, N),
        '╵' => Lines::new(L, N, N, N),
        '╷' => Lines::new(N, N, L, N),
        '╸' => Lines::new(N, N, N, H),
        '╺' => Lines::new(N, H, N, N),
        '╹' => Lines::new(H, N, N, N),
        '╻' => Lines::new(N, N, H, N),
        '┌' => Lines::new(N, L, L, N),
        '┐' => Lines::new(N, N, L, L),
        '└' => Lines::new(L, L, N, N),
        '┘' => Lines::new(L, N, N, L),
        '├' => Lines::new(L, L, L, N),
        '┤' => Lines::new(L, N, L, L),
        '┬' => Lines::new(N, L, L, L),
        '┴' => Lines::new(L, L, N, L),
        '┼' => Lines::new(L, L, L, L),
        '┏' => Lines::new(N, H, H, N),
        '┓' => Lines::new(N, N, H, H),
        '┗' => Lines::new(H, H, N, N),
        '┛' => Lines::new(H, N, N, H),
        '┣' => Lines::new(H, H, H, N),
        '┫' => Lines::new(H, N, H, H),
        '┳' => Lines::new(N, H, H, H),
        '┻' => Lines::new(H, H, N, H),
        '╋' => Lines::new(H, H, H, H),
        '═' => Lines::new(N, D, N, D),
        '║' => Lines::new(D, N, D, N),
        '╔' => Lines::new(N, D, D, N),
        '╗' => Lines::new(N, N, D, D),
        '╚' => Lines::new(D, D, N, N),
        '╝' => Lines::new(D, N, N, D),
        '╠' => Lines::new(D, D, D, N),
        '╣' => Lines::new(D, N, D, D),
        '╦' => Lines::new(N, D, D, D),
        '╩' => Lines::new(D, D, N, D),
        '╬' => Lines::new(D, D, D, D),
        _ => return None,
    })
}

fn draw_box_lines<Renderer>(renderer: &mut Renderer, lines: Lines, cell: Rectangle, color: Color)
where
    Renderer: renderer::Renderer,
{
    let mut rects = Vec::new();
    for (side, style) in [
        (Side::Top, lines.up),
        (Side::Right, lines.right),
        (Side::Bottom, lines.down),
        (Side::Left, lines.left),
    ] {
        push_line_rects(&mut rects, cell, side, style);
    }
    draw_rects(renderer, &rects, color);
}

fn push_line_rects(rects: &mut Vec<Rectangle>, cell: Rectangle, side: Side, style: LineStyle) {
    match style {
        LineStyle::None => {}
        LineStyle::Light | LineStyle::Heavy => {
            let weight = if style == LineStyle::Heavy {
                StrokeWeight::Heavy
            } else {
                StrokeWeight::Light
            };
            rects.push(single_line_rect(cell, side, stroke_width(weight, cell)));
        }
        LineStyle::Double => {
            for offset in double_offsets(cell) {
                rects.push(double_line_rect(cell, side, offset));
            }
        }
    }
}

fn single_line_rect(cell: Rectangle, side: Side, stroke: f32) -> Rectangle {
    let half = stroke / 2.0;
    let cx = cell.x + cell.width / 2.0;
    let cy = cell.y + cell.height / 2.0;
    match side {
        Side::Top => Rectangle {
            x: cx - half,
            y: cell.y,
            width: stroke,
            height: cell.height / 2.0 + half,
        },
        Side::Bottom => Rectangle {
            x: cx - half,
            y: cy - half,
            width: stroke,
            height: cell.height / 2.0 + half,
        },
        Side::Left => Rectangle {
            x: cell.x,
            y: cy - half,
            width: cell.width / 2.0 + half,
            height: stroke,
        },
        Side::Right => Rectangle {
            x: cx - half,
            y: cy - half,
            width: cell.width / 2.0 + half,
            height: stroke,
        },
    }
}

fn double_line_rect(cell: Rectangle, side: Side, offset: f32) -> Rectangle {
    let stroke = stroke_width(StrokeWeight::Light, cell);
    let cx = cell.x + cell.width / 2.0;
    let cy = cell.y + cell.height / 2.0;
    match side {
        Side::Top => Rectangle {
            x: cx + offset,
            y: cell.y,
            width: stroke,
            height: cell.height / 2.0,
        },
        Side::Bottom => Rectangle {
            x: cx + offset,
            y: cy,
            width: stroke,
            height: cell.height / 2.0,
        },
        Side::Left => Rectangle {
            x: cell.x,
            y: cy + offset,
            width: cell.width / 2.0,
            height: stroke,
        },
        Side::Right => Rectangle {
            x: cx,
            y: cy + offset,
            width: cell.width / 2.0,
            height: stroke,
        },
    }
}

fn double_offsets(cell: Rectangle) -> [f32; 2] {
    let stroke = stroke_width(StrokeWeight::Light, cell);
    let gap = stroke.max(cell.width.min(cell.height) / 8.0);
    [-gap / 2.0 - stroke, gap / 2.0]
}

fn stroke_width(weight: StrokeWeight, cell: Rectangle) -> f32 {
    let base = (cell.width.min(cell.height) / 8.0).round().max(1.0);
    match weight {
        StrokeWeight::Light => base,
        StrokeWeight::Heavy => (base * 2.0)
            .min(cell.width.min(cell.height) / 3.0)
            .max(base + 1.0),
    }
}

fn fill_triangle<Renderer>(
    renderer: &mut Renderer,
    cell: Rectangle,
    color: Color,
    points: [(f32, f32); 3],
) where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    fill_path(renderer, cell, color, |path| {
        path.move_to(local_point(cell, points[0]));
        path.line_to(local_point(cell, points[1]));
        path.line_to(local_point(cell, points[2]));
        path.close();
    });
}

fn stroke_polyline<Renderer>(
    renderer: &mut Renderer,
    cell: Rectangle,
    color: Color,
    points: &[(f32, f32)],
) where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    if points.len() < 2 {
        return;
    }

    stroke_path(
        renderer,
        cell,
        color,
        stroke_width(StrokeWeight::Light, cell),
        |path| {
            path.move_to(local_point(cell, points[0]));
            for point in &points[1..] {
                path.line_to(local_point(cell, *point));
            }
        },
    );
}

fn stroke_diagonal<Renderer>(renderer: &mut Renderer, cell: Rectangle, color: Color, falling: bool)
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    let points = if falling {
        [(0.0, 0.0), (1.0, 1.0)]
    } else {
        [(0.0, 1.0), (1.0, 0.0)]
    };
    stroke_polyline(renderer, cell, color, &points);
}

fn fill_soft_powerline<Renderer>(renderer: &mut Renderer, cell: Rectangle, color: Color, flip: bool)
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    let c = (2.0_f32.sqrt() - 1.0) * 4.0 / 3.0;
    let radius = cell.width.min(cell.height / 2.0);
    fill_path(renderer, cell, color, |path| {
        if flip {
            path.move_to(Point::new(cell.width, 0.0));
            path.bezier_curve_to(
                Point::new(cell.width - radius * c, 0.0),
                Point::new(cell.width - radius, radius - radius * c),
                Point::new(cell.width - radius, radius),
            );
            path.line_to(Point::new(cell.width - radius, cell.height - radius));
            path.bezier_curve_to(
                Point::new(cell.width - radius, cell.height - radius + radius * c),
                Point::new(cell.width - radius * c, cell.height),
                Point::new(cell.width, cell.height),
            );
            path.close();
        } else {
            path.move_to(Point::new(0.0, 0.0));
            path.bezier_curve_to(
                Point::new(radius * c, 0.0),
                Point::new(radius, radius - radius * c),
                Point::new(radius, radius),
            );
            path.line_to(Point::new(radius, cell.height - radius));
            path.bezier_curve_to(
                Point::new(radius, cell.height - radius + radius * c),
                Point::new(radius * c, cell.height),
                Point::new(0.0, cell.height),
            );
            path.close();
        }
    });
}

fn stroke_soft_powerline<Renderer>(
    renderer: &mut Renderer,
    cell: Rectangle,
    color: Color,
    flip: bool,
) where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    let c = (2.0_f32.sqrt() - 1.0) * 4.0 / 3.0;
    let radius = cell.width.min(cell.height / 2.0);
    stroke_path(
        renderer,
        cell,
        color,
        stroke_width(StrokeWeight::Light, cell),
        |path| {
            if flip {
                path.move_to(Point::new(cell.width, 0.0));
                path.bezier_curve_to(
                    Point::new(cell.width - radius * c, 0.0),
                    Point::new(cell.width - radius, radius - radius * c),
                    Point::new(cell.width - radius, radius),
                );
                path.line_to(Point::new(cell.width - radius, cell.height - radius));
                path.bezier_curve_to(
                    Point::new(cell.width - radius, cell.height - radius + radius * c),
                    Point::new(cell.width - radius * c, cell.height),
                    Point::new(cell.width, cell.height),
                );
            } else {
                path.move_to(Point::new(0.0, 0.0));
                path.bezier_curve_to(
                    Point::new(radius * c, 0.0),
                    Point::new(radius, radius - radius * c),
                    Point::new(radius, radius),
                );
                path.line_to(Point::new(radius, cell.height - radius));
                path.bezier_curve_to(
                    Point::new(radius, cell.height - radius + radius * c),
                    Point::new(radius * c, cell.height),
                    Point::new(0.0, cell.height),
                );
            }
        },
    );
}

fn fill_chevron_cap<Renderer>(renderer: &mut Renderer, cell: Rectangle, color: Color, flip: bool)
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    let thick = stroke_width(StrokeWeight::Light, cell);
    fill_path(renderer, cell, color, |path| {
        if flip {
            path.move_to(Point::new(cell.width, 0.0));
            path.line_to(Point::new(0.0, 0.0));
            path.line_to(Point::new(
                cell.width / 2.0,
                cell.height / 2.0 - thick / 2.0,
            ));
            path.line_to(Point::new(cell.width, cell.height / 2.0 - thick / 2.0));
            path.close();
            path.move_to(Point::new(cell.width, cell.height));
            path.line_to(Point::new(0.0, cell.height));
            path.line_to(Point::new(
                cell.width / 2.0,
                cell.height / 2.0 + thick / 2.0,
            ));
            path.line_to(Point::new(cell.width, cell.height / 2.0 + thick / 2.0));
            path.close();
        } else {
            path.move_to(Point::new(0.0, 0.0));
            path.line_to(Point::new(cell.width, 0.0));
            path.line_to(Point::new(
                cell.width / 2.0,
                cell.height / 2.0 - thick / 2.0,
            ));
            path.line_to(Point::new(0.0, cell.height / 2.0 - thick / 2.0));
            path.close();
            path.move_to(Point::new(0.0, cell.height));
            path.line_to(Point::new(cell.width, cell.height));
            path.line_to(Point::new(
                cell.width / 2.0,
                cell.height / 2.0 + thick / 2.0,
            ));
            path.line_to(Point::new(0.0, cell.height / 2.0 + thick / 2.0));
            path.close();
        }
    });
}

fn stroke_corner_arc<Renderer>(
    renderer: &mut Renderer,
    cell: Rectangle,
    color: Color,
    corner: Corner,
) -> bool
where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    let thick = stroke_width(StrokeWeight::Light, cell);
    let center_x = cell.width / 2.0;
    let center_y = cell.height / 2.0;
    let radius = cell.width.min(cell.height) / 2.0;
    let control = 0.25;

    stroke_path(renderer, cell, color, thick, |path| match corner {
        Corner::TopLeft => {
            path.move_to(Point::new(center_x, cell.height));
            path.line_to(Point::new(center_x, center_y + radius));
            path.bezier_curve_to(
                Point::new(center_x, center_y + control * radius),
                Point::new(center_x + control * radius, center_y),
                Point::new(center_x + radius, center_y),
            );
            path.line_to(Point::new(cell.width, center_y));
        }
        Corner::TopRight => {
            path.move_to(Point::new(center_x, cell.height));
            path.line_to(Point::new(center_x, center_y + radius));
            path.bezier_curve_to(
                Point::new(center_x, center_y + control * radius),
                Point::new(center_x - control * radius, center_y),
                Point::new(center_x - radius, center_y),
            );
            path.line_to(Point::new(0.0, center_y));
        }
        Corner::BottomLeft => {
            path.move_to(Point::new(center_x, 0.0));
            path.line_to(Point::new(center_x, center_y - radius));
            path.bezier_curve_to(
                Point::new(center_x, center_y - control * radius),
                Point::new(center_x + control * radius, center_y),
                Point::new(center_x + radius, center_y),
            );
            path.line_to(Point::new(cell.width, center_y));
        }
        Corner::BottomRight => {
            path.move_to(Point::new(center_x, 0.0));
            path.line_to(Point::new(center_x, center_y - radius));
            path.bezier_curve_to(
                Point::new(center_x, center_y - control * radius),
                Point::new(center_x - control * radius, center_y),
                Point::new(center_x - radius, center_y),
            );
            path.line_to(Point::new(0.0, center_y));
        }
    });
    true
}

fn fill_corner_square<Renderer>(
    renderer: &mut Renderer,
    cell: Rectangle,
    color: Color,
    corner: Corner,
) where
    Renderer: renderer::Renderer,
{
    let rect = match corner {
        Corner::TopLeft => local_rect(cell, 0.0, 0.0, HALF, HALF),
        Corner::TopRight => local_rect(cell, HALF, 0.0, 1.0, HALF),
        Corner::BottomRight => local_rect(cell, HALF, HALF, 1.0, 1.0),
        Corner::BottomLeft => local_rect(cell, 0.0, HALF, HALF, 1.0),
    };
    draw_rects(renderer, &[rect], color);
}

fn fill_path<Renderer>(
    renderer: &mut Renderer,
    cell: Rectangle,
    color: Color,
    build: impl FnOnce(&mut iced::advanced::graphics::geometry::path::Builder),
) where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    renderer.with_translation(Vector::new(cell.x, cell.y), |renderer| {
        let mut frame = Frame::new(renderer, Size::new(cell.width, cell.height));
        let path = Path::new(build);
        frame.fill(&path, color);
        renderer.draw_geometry(frame.into_geometry());
    });
}

fn stroke_path<Renderer>(
    renderer: &mut Renderer,
    cell: Rectangle,
    color: Color,
    width: f32,
    build: impl FnOnce(&mut iced::advanced::graphics::geometry::path::Builder),
) where
    Renderer: renderer::Renderer + geometry::Renderer,
{
    renderer.with_translation(Vector::new(cell.x, cell.y), |renderer| {
        let mut frame = Frame::new(renderer, Size::new(cell.width, cell.height));
        let path = Path::new(build);
        frame.stroke(
            &path,
            Stroke::default()
                .with_color(color)
                .with_width(width)
                .with_line_cap(LineCap::Butt),
        );
        renderer.draw_geometry(frame.into_geometry());
    });
}

fn draw_rects<Renderer: renderer::Renderer>(
    renderer: &mut Renderer,
    rects: &[Rectangle],
    color: Color,
) {
    for rect in rects {
        renderer.fill_quad(
            Quad {
                bounds: *rect,
                border: Border::default(),
                shadow: Shadow::default(),
                snap: true,
            },
            Background::Color(color),
        );
    }
}

fn local_rect(cell: Rectangle, x0: f32, y0: f32, x1: f32, y1: f32) -> Rectangle {
    let min_x = fraction_min(cell.width, x0);
    let max_x = fraction_max(cell.width, x1);
    let min_y = fraction_min(cell.height, y0);
    let max_y = fraction_max(cell.height, y1);

    Rectangle {
        x: cell.x + min_x,
        y: cell.y + min_y,
        width: (max_x - min_x).max(1.0),
        height: (max_y - min_y).max(1.0),
    }
}

fn fraction_min(size: f32, fraction: f32) -> f32 {
    size - ((1.0 - fraction) * size).round()
}

fn fraction_max(size: f32, fraction: f32) -> f32 {
    (fraction * size).round()
}

fn local_point(cell: Rectangle, point: (f32, f32)) -> Point {
    Point::new(point.0 * cell.width, point.1 * cell.height)
}

fn color_with_alpha(mut color: Color, alpha_scale: f32) -> Color {
    color.a *= alpha_scale;
    color
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
    fn block_fraction_rounding_matches_terminal_cell_edges() {
        let rect = local_rect(
            Rectangle {
                x: 10.0,
                y: 20.0,
                width: 9.0,
                height: 17.0,
            },
            0.0,
            0.0,
            HALF,
            ONE_THIRD,
        );

        assert_close(rect.x, 10.0);
        assert_close(rect.y, 20.0);
        assert_close(rect.width, 5.0);
        assert_close(rect.height, 6.0);
    }

    #[test]
    fn sextant_codepoints_map_to_nonempty_cells() {
        let cell = Rectangle {
            x: 0.0,
            y: 0.0,
            width: 12.0,
            height: 24.0,
        };

        let first = local_rect(cell, 0.0, 0.0, HALF, ONE_THIRD);
        let last = local_rect(cell, HALF, TWO_THIRDS, 1.0, 1.0);

        assert_close(first.width, 6.0);
        assert_close(first.height, 8.0);
        assert_close(last.x, 6.0);
        assert_close(last.y, 16.0);
    }

    #[test]
    fn box_line_mapping_keeps_basic_glyphs_specific() {
        assert_eq!(
            box_lines('┌'),
            Some(Lines::new(
                LineStyle::None,
                LineStyle::Light,
                LineStyle::Light,
                LineStyle::None,
            ))
        );
        assert_eq!(
            box_lines('╬'),
            Some(Lines::new(
                LineStyle::Double,
                LineStyle::Double,
                LineStyle::Double,
                LineStyle::Double,
            ))
        );
    }

    #[test]
    fn unsupported_plain_text_falls_back_to_font() {
        assert!(box_lines('A').is_none());
        assert!(!(0x1fb00..=0x1fbef).contains(&('A' as u32)));
    }
}
