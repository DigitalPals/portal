//! Color conversion utilities for terminal rendering

use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use iced::Color;

/// Standard ANSI color palette (dark theme)
pub const ANSI_COLORS: [Color; 16] = [
    // Normal colors (0-7)
    Color::from_rgb(0.0, 0.0, 0.0),           // Black
    Color::from_rgb(0.8, 0.0, 0.0),           // Red
    Color::from_rgb(0.0, 0.8, 0.0),           // Green
    Color::from_rgb(0.8, 0.8, 0.0),           // Yellow
    Color::from_rgb(0.0, 0.0, 0.8),           // Blue
    Color::from_rgb(0.8, 0.0, 0.8),           // Magenta
    Color::from_rgb(0.0, 0.8, 0.8),           // Cyan
    Color::from_rgb(0.75, 0.75, 0.75),        // White
    // Bright colors (8-15)
    Color::from_rgb(0.5, 0.5, 0.5),           // Bright Black (Gray)
    Color::from_rgb(1.0, 0.0, 0.0),           // Bright Red
    Color::from_rgb(0.0, 1.0, 0.0),           // Bright Green
    Color::from_rgb(1.0, 1.0, 0.0),           // Bright Yellow
    Color::from_rgb(0.0, 0.0, 1.0),           // Bright Blue
    Color::from_rgb(1.0, 0.0, 1.0),           // Bright Magenta
    Color::from_rgb(0.0, 1.0, 1.0),           // Bright Cyan
    Color::from_rgb(1.0, 1.0, 1.0),           // Bright White
];

/// Default foreground color
pub const DEFAULT_FG: Color = Color::from_rgb(0.9, 0.9, 0.9);

/// Default background color
pub const DEFAULT_BG: Color = Color::from_rgb(0.1, 0.1, 0.1);

/// Convert an alacritty ANSI color to an iced Color
pub fn ansi_to_iced(color: AnsiColor) -> Color {
    match color {
        AnsiColor::Named(named) => named_to_iced(named),
        AnsiColor::Spec(rgb) => Color::from_rgb8(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(idx) => indexed_to_iced(idx),
    }
}

/// Convert a named color to iced Color
fn named_to_iced(named: NamedColor) -> Color {
    match named {
        NamedColor::Black => ANSI_COLORS[0],
        NamedColor::Red => ANSI_COLORS[1],
        NamedColor::Green => ANSI_COLORS[2],
        NamedColor::Yellow => ANSI_COLORS[3],
        NamedColor::Blue => ANSI_COLORS[4],
        NamedColor::Magenta => ANSI_COLORS[5],
        NamedColor::Cyan => ANSI_COLORS[6],
        NamedColor::White => ANSI_COLORS[7],
        NamedColor::BrightBlack => ANSI_COLORS[8],
        NamedColor::BrightRed => ANSI_COLORS[9],
        NamedColor::BrightGreen => ANSI_COLORS[10],
        NamedColor::BrightYellow => ANSI_COLORS[11],
        NamedColor::BrightBlue => ANSI_COLORS[12],
        NamedColor::BrightMagenta => ANSI_COLORS[13],
        NamedColor::BrightCyan => ANSI_COLORS[14],
        NamedColor::BrightWhite => ANSI_COLORS[15],
        NamedColor::Foreground => DEFAULT_FG,
        NamedColor::Background => DEFAULT_BG,
        NamedColor::Cursor => DEFAULT_FG,
        NamedColor::BrightForeground => Color::from_rgb(1.0, 1.0, 1.0),
        NamedColor::DimForeground => dim_color(DEFAULT_FG),
        // Dim variants - slightly darker versions
        NamedColor::DimBlack => dim_color(ANSI_COLORS[0]),
        NamedColor::DimRed => dim_color(ANSI_COLORS[1]),
        NamedColor::DimGreen => dim_color(ANSI_COLORS[2]),
        NamedColor::DimYellow => dim_color(ANSI_COLORS[3]),
        NamedColor::DimBlue => dim_color(ANSI_COLORS[4]),
        NamedColor::DimMagenta => dim_color(ANSI_COLORS[5]),
        NamedColor::DimCyan => dim_color(ANSI_COLORS[6]),
        NamedColor::DimWhite => dim_color(ANSI_COLORS[7]),
    }
}

/// Convert a 256-color indexed color to iced Color
fn indexed_to_iced(idx: u8) -> Color {
    if idx < 16 {
        // Standard ANSI colors
        ANSI_COLORS[idx as usize]
    } else if idx < 232 {
        // 216 color cube (6x6x6)
        let idx = idx - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        Color::from_rgb(
            if r == 0 { 0.0 } else { (r as f32 * 40.0 + 55.0) / 255.0 },
            if g == 0 { 0.0 } else { (g as f32 * 40.0 + 55.0) / 255.0 },
            if b == 0 { 0.0 } else { (b as f32 * 40.0 + 55.0) / 255.0 },
        )
    } else {
        // Grayscale ramp (24 shades)
        let gray = (idx - 232) as f32 * 10.0 + 8.0;
        let v = gray / 255.0;
        Color::from_rgb(v, v, v)
    }
}

/// Create a dimmed version of a color
fn dim_color(color: Color) -> Color {
    Color::from_rgba(
        color.r * 0.66,
        color.g * 0.66,
        color.b * 0.66,
        color.a,
    )
}
