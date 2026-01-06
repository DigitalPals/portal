//! Color conversion utilities for terminal rendering

use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor};
use iced::Color;

use crate::theme::TerminalColors;

/// Standard ANSI color palette (dark theme) - kept for backward compatibility
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

/// Default foreground color - kept for backward compatibility
pub const DEFAULT_FG: Color = Color::from_rgb(0.9, 0.9, 0.9);

/// Default background color - kept for backward compatibility
pub const DEFAULT_BG: Color = Color::from_rgb(0.1, 0.1, 0.1);

/// Convert an alacritty ANSI color to an iced Color using themed colors
pub fn ansi_to_iced_themed(color: AnsiColor, colors: &TerminalColors) -> Color {
    match color {
        AnsiColor::Named(named) => named_to_iced_themed(named, colors),
        AnsiColor::Spec(rgb) => Color::from_rgb8(rgb.r, rgb.g, rgb.b),
        AnsiColor::Indexed(idx) => indexed_to_iced_themed(idx, colors),
    }
}

/// Convert a named color to iced Color using themed colors
fn named_to_iced_themed(named: NamedColor, colors: &TerminalColors) -> Color {
    match named {
        NamedColor::Black => colors.ansi[0],
        NamedColor::Red => colors.ansi[1],
        NamedColor::Green => colors.ansi[2],
        NamedColor::Yellow => colors.ansi[3],
        NamedColor::Blue => colors.ansi[4],
        NamedColor::Magenta => colors.ansi[5],
        NamedColor::Cyan => colors.ansi[6],
        NamedColor::White => colors.ansi[7],
        NamedColor::BrightBlack => colors.ansi[8],
        NamedColor::BrightRed => colors.ansi[9],
        NamedColor::BrightGreen => colors.ansi[10],
        NamedColor::BrightYellow => colors.ansi[11],
        NamedColor::BrightBlue => colors.ansi[12],
        NamedColor::BrightMagenta => colors.ansi[13],
        NamedColor::BrightCyan => colors.ansi[14],
        NamedColor::BrightWhite => colors.ansi[15],
        NamedColor::Foreground => colors.foreground,
        NamedColor::Background => colors.background,
        NamedColor::Cursor => colors.cursor,
        NamedColor::BrightForeground => colors.foreground,
        NamedColor::DimForeground => dim_color(colors.foreground),
        // Dim variants - slightly darker versions
        NamedColor::DimBlack => dim_color(colors.ansi[0]),
        NamedColor::DimRed => dim_color(colors.ansi[1]),
        NamedColor::DimGreen => dim_color(colors.ansi[2]),
        NamedColor::DimYellow => dim_color(colors.ansi[3]),
        NamedColor::DimBlue => dim_color(colors.ansi[4]),
        NamedColor::DimMagenta => dim_color(colors.ansi[5]),
        NamedColor::DimCyan => dim_color(colors.ansi[6]),
        NamedColor::DimWhite => dim_color(colors.ansi[7]),
    }
}

/// Convert a 256-color indexed color to iced Color using themed colors for 0-15
fn indexed_to_iced_themed(idx: u8, colors: &TerminalColors) -> Color {
    if idx < 16 {
        // Standard ANSI colors from theme
        colors.ansi[idx as usize]
    } else if idx < 232 {
        // 216 color cube (6x6x6) - same calculation as non-themed
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
        // Grayscale ramp (24 shades) - same calculation as non-themed
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
