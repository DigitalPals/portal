//! Theme system for Portal SSH client
//!
//! Supports 5 pre-configured themes:
//! - Portal Default (dark navy blue)
//! - Catppuccin Latte (light)
//! - Catppuccin Frappé (dark, muted)
//! - Catppuccin Macchiato (dark, medium)
//! - Catppuccin Mocha (dark, rich)

use iced::Color;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Available theme identifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeId {
    #[default]
    PortalDefault,
    CatppuccinLatte,
    CatppuccinFrappe,
    CatppuccinMacchiato,
    CatppuccinMocha,
}

impl ThemeId {
    /// Get all available themes
    pub const fn all() -> &'static [ThemeId] {
        &[
            ThemeId::PortalDefault,
            ThemeId::CatppuccinLatte,
            ThemeId::CatppuccinFrappe,
            ThemeId::CatppuccinMacchiato,
            ThemeId::CatppuccinMocha,
        ]
    }

    /// Display name for UI
    pub const fn display_name(&self) -> &'static str {
        match self {
            ThemeId::PortalDefault => "Portal Default",
            ThemeId::CatppuccinLatte => "Catppuccin Latte",
            ThemeId::CatppuccinFrappe => "Catppuccin Frappé",
            ThemeId::CatppuccinMacchiato => "Catppuccin Macchiato",
            ThemeId::CatppuccinMocha => "Catppuccin Mocha",
        }
    }

    /// Whether this is a dark theme (for Iced theme selection)
    pub const fn is_dark(&self) -> bool {
        !matches!(self, ThemeId::CatppuccinLatte)
    }
}

impl fmt::Display for ThemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// Terminal color palette (16 ANSI colors + fg/bg/cursor)
#[derive(Clone, Copy)]
pub struct TerminalColors {
    pub foreground: Color,
    pub background: Color,
    pub cursor: Color,
    /// Standard ANSI colors (16 colors: 0-7 normal, 8-15 bright)
    pub ansi: [Color; 16],
}

/// Complete theme including UI and terminal colors
#[derive(Clone, Copy)]
pub struct Theme {
    // UI colors
    pub background: Color,
    pub surface: Color,
    pub sidebar: Color,
    pub tab_bar: Color,
    pub accent: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub border: Color,
    pub hover: Color,
    pub selected: Color,
    pub focus_ring: Color,
    // Terminal colors
    pub terminal: TerminalColors,
}

impl Theme {
    /// Portal Default theme - Dark navy blue
    pub fn portal_default() -> Self {
        Self {
            background: Color::from_rgb8(0x1D, 0x20, 0x33),
            surface: Color::from_rgb8(0x28, 0x2B, 0x3D),
            sidebar: Color::from_rgb8(0x28, 0x2B, 0x3D),
            tab_bar: Color::from_rgb8(0x14, 0x17, 0x29),
            accent: Color::from_rgb8(0x32, 0x36, 0x4A),
            text_primary: Color::from_rgb8(0xe8, 0xe8, 0xe8),
            text_secondary: Color::from_rgb8(0x9a, 0xa0, 0xb0),
            text_muted: Color::from_rgb8(0x5B, 0x5F, 0x74),
            border: Color::from_rgb8(0x3a, 0x40, 0x55),
            hover: Color::from_rgb8(0x35, 0x3d, 0x50),
            selected: Color::from_rgb8(0x2a, 0x4a, 0x6d),
            focus_ring: Color::from_rgb8(0x58, 0x9c, 0xff),
            terminal: TerminalColors {
                foreground: Color::from_rgb8(0xe6, 0xe6, 0xe6),
                background: Color::from_rgb8(0x1e, 0x1e, 0x2e), // Catppuccin Mocha base
                cursor: Color::from_rgb8(0xe6, 0xe6, 0xe6),
                ansi: [
                    // Normal colors (0-7)
                    Color::from_rgb8(0x00, 0x00, 0x00), // Black
                    Color::from_rgb8(0xcc, 0x00, 0x00), // Red
                    Color::from_rgb8(0x00, 0xcc, 0x00), // Green
                    Color::from_rgb8(0xcc, 0xcc, 0x00), // Yellow
                    Color::from_rgb8(0x5c, 0x8a, 0xff), // Blue (#5C8AFF)
                    Color::from_rgb8(0xcc, 0x00, 0xcc), // Magenta
                    Color::from_rgb8(0x00, 0xcc, 0xcc), // Cyan
                    Color::from_rgb8(0xc0, 0xc0, 0xc0), // White
                    // Bright colors (8-15)
                    Color::from_rgb8(0x80, 0x80, 0x80), // Bright Black
                    Color::from_rgb8(0xff, 0x00, 0x00), // Bright Red
                    Color::from_rgb8(0x00, 0xff, 0x00), // Bright Green
                    Color::from_rgb8(0xff, 0xff, 0x00), // Bright Yellow
                    Color::from_rgb8(0x7a, 0xa2, 0xff), // Bright Blue (#7AA2FF)
                    Color::from_rgb8(0xff, 0x00, 0xff), // Bright Magenta
                    Color::from_rgb8(0x00, 0xff, 0xff), // Bright Cyan
                    Color::from_rgb8(0xff, 0xff, 0xff), // Bright White
                ],
            },
        }
    }

    /// Catppuccin Latte - Light pastel theme
    pub fn catppuccin_latte() -> Self {
        Self {
            background: Color::from_rgb8(0xef, 0xf1, 0xf5), // Base
            surface: Color::from_rgb8(0xe6, 0xe9, 0xef),    // Mantle
            sidebar: Color::from_rgb8(0xdc, 0xe0, 0xe8),    // Crust
            tab_bar: Color::from_rgb8(0xdc, 0xe0, 0xe8),    // Crust
            accent: Color::from_rgb8(0x1e, 0x66, 0xf5),     // Blue
            text_primary: Color::from_rgb8(0x4c, 0x4f, 0x69), // Text
            text_secondary: Color::from_rgb8(0x5c, 0x5f, 0x77), // Subtext1
            text_muted: Color::from_rgb8(0x6c, 0x6f, 0x85), // Subtext0
            border: Color::from_rgb8(0xcc, 0xd0, 0xda),     // Surface0
            hover: Color::from_rgb8(0xbc, 0xc0, 0xcc),      // Surface1
            selected: Color::from_rgb8(0xac, 0xb0, 0xbe),   // Surface2
            focus_ring: Color::from_rgb8(0x1e, 0x66, 0xf5), // Blue
            terminal: TerminalColors {
                foreground: Color::from_rgb8(0x4c, 0x4f, 0x69), // Text
                background: Color::from_rgb8(0xef, 0xf1, 0xf5), // Base
                cursor: Color::from_rgb8(0xdc, 0x8a, 0x78),     // Rosewater
                ansi: [
                    // Normal colors (0-7)
                    Color::from_rgb8(0x5c, 0x5f, 0x77), // Black (Subtext1)
                    Color::from_rgb8(0xd2, 0x0f, 0x39), // Red
                    Color::from_rgb8(0x40, 0xa0, 0x2b), // Green
                    Color::from_rgb8(0xdf, 0x8e, 0x1d), // Yellow
                    Color::from_rgb8(0x1e, 0x66, 0xf5), // Blue
                    Color::from_rgb8(0x88, 0x39, 0xef), // Magenta (Mauve)
                    Color::from_rgb8(0x17, 0x92, 0x99), // Cyan (Teal)
                    Color::from_rgb8(0xbc, 0xc0, 0xcc), // White (Surface1)
                    // Bright colors (8-15)
                    Color::from_rgb8(0x6c, 0x6f, 0x85), // Bright Black (Subtext0)
                    Color::from_rgb8(0xd2, 0x0f, 0x39), // Bright Red
                    Color::from_rgb8(0x40, 0xa0, 0x2b), // Bright Green
                    Color::from_rgb8(0xdf, 0x8e, 0x1d), // Bright Yellow
                    Color::from_rgb8(0x1e, 0x66, 0xf5), // Bright Blue
                    Color::from_rgb8(0x88, 0x39, 0xef), // Bright Magenta
                    Color::from_rgb8(0x17, 0x92, 0x99), // Bright Cyan
                    Color::from_rgb8(0x4c, 0x4f, 0x69), // Bright White (Text)
                ],
            },
        }
    }

    /// Catppuccin Frappé - Mid-tone muted dark theme
    pub fn catppuccin_frappe() -> Self {
        Self {
            background: Color::from_rgb8(0x30, 0x34, 0x46), // Base
            surface: Color::from_rgb8(0x29, 0x2c, 0x3c),    // Mantle
            sidebar: Color::from_rgb8(0x23, 0x26, 0x34),    // Crust
            tab_bar: Color::from_rgb8(0x23, 0x26, 0x34),    // Crust
            accent: Color::from_rgb8(0xba, 0xbb, 0xf1),     // Lavender
            text_primary: Color::from_rgb8(0xc6, 0xd0, 0xf5), // Text
            text_secondary: Color::from_rgb8(0xb5, 0xbf, 0xe2), // Subtext1
            text_muted: Color::from_rgb8(0xa5, 0xad, 0xce), // Subtext0
            border: Color::from_rgb8(0x41, 0x45, 0x59),     // Surface0
            hover: Color::from_rgb8(0x51, 0x57, 0x6d),      // Surface1
            selected: Color::from_rgb8(0x62, 0x68, 0x80),   // Surface2
            focus_ring: Color::from_rgb8(0xba, 0xbb, 0xf1), // Lavender
            terminal: TerminalColors {
                foreground: Color::from_rgb8(0xc6, 0xd0, 0xf5), // Text
                background: Color::from_rgb8(0x30, 0x34, 0x46), // Base
                cursor: Color::from_rgb8(0xf2, 0xd5, 0xcf),     // Rosewater
                ansi: [
                    // Normal colors (0-7)
                    Color::from_rgb8(0x51, 0x57, 0x6d), // Black (Surface1)
                    Color::from_rgb8(0xe7, 0x82, 0x84), // Red
                    Color::from_rgb8(0xa6, 0xd1, 0x89), // Green
                    Color::from_rgb8(0xe5, 0xc8, 0x90), // Yellow
                    Color::from_rgb8(0x8c, 0xaa, 0xee), // Blue
                    Color::from_rgb8(0xca, 0x9e, 0xe6), // Magenta (Mauve)
                    Color::from_rgb8(0x81, 0xc8, 0xbe), // Cyan (Teal)
                    Color::from_rgb8(0xb5, 0xbf, 0xe2), // White (Subtext1)
                    // Bright colors (8-15)
                    Color::from_rgb8(0x62, 0x68, 0x80), // Bright Black (Surface2)
                    Color::from_rgb8(0xe7, 0x82, 0x84), // Bright Red
                    Color::from_rgb8(0xa6, 0xd1, 0x89), // Bright Green
                    Color::from_rgb8(0xe5, 0xc8, 0x90), // Bright Yellow
                    Color::from_rgb8(0x8c, 0xaa, 0xee), // Bright Blue
                    Color::from_rgb8(0xca, 0x9e, 0xe6), // Bright Magenta
                    Color::from_rgb8(0x81, 0xc8, 0xbe), // Bright Cyan
                    Color::from_rgb8(0xc6, 0xd0, 0xf5), // Bright White (Text)
                ],
            },
        }
    }

    /// Catppuccin Macchiato - Slightly lighter dark theme
    pub fn catppuccin_macchiato() -> Self {
        Self {
            background: Color::from_rgb8(0x24, 0x27, 0x3a), // Base
            surface: Color::from_rgb8(0x1e, 0x20, 0x30),    // Mantle
            sidebar: Color::from_rgb8(0x18, 0x19, 0x26),    // Crust
            tab_bar: Color::from_rgb8(0x18, 0x19, 0x26),    // Crust
            accent: Color::from_rgb8(0xc6, 0xa0, 0xf6),     // Mauve
            text_primary: Color::from_rgb8(0xca, 0xd3, 0xf5), // Text
            text_secondary: Color::from_rgb8(0xb8, 0xc0, 0xe0), // Subtext1
            text_muted: Color::from_rgb8(0xa5, 0xad, 0xcb), // Subtext0
            border: Color::from_rgb8(0x36, 0x3a, 0x4f),     // Surface0
            hover: Color::from_rgb8(0x49, 0x4d, 0x64),      // Surface1
            selected: Color::from_rgb8(0x5b, 0x60, 0x78),   // Surface2
            focus_ring: Color::from_rgb8(0xc6, 0xa0, 0xf6), // Mauve
            terminal: TerminalColors {
                foreground: Color::from_rgb8(0xca, 0xd3, 0xf5), // Text
                background: Color::from_rgb8(0x24, 0x27, 0x3a), // Base
                cursor: Color::from_rgb8(0xf4, 0xdb, 0xd6),     // Rosewater
                ansi: [
                    // Normal colors (0-7)
                    Color::from_rgb8(0x49, 0x4d, 0x64), // Black (Surface1)
                    Color::from_rgb8(0xed, 0x87, 0x96), // Red
                    Color::from_rgb8(0xa6, 0xda, 0x95), // Green
                    Color::from_rgb8(0xee, 0xd4, 0x9f), // Yellow
                    Color::from_rgb8(0x8a, 0xad, 0xf4), // Blue
                    Color::from_rgb8(0xc6, 0xa0, 0xf6), // Magenta (Mauve)
                    Color::from_rgb8(0x8b, 0xd5, 0xca), // Cyan (Teal)
                    Color::from_rgb8(0xb8, 0xc0, 0xe0), // White (Subtext1)
                    // Bright colors (8-15)
                    Color::from_rgb8(0x5b, 0x60, 0x78), // Bright Black (Surface2)
                    Color::from_rgb8(0xed, 0x87, 0x96), // Bright Red
                    Color::from_rgb8(0xa6, 0xda, 0x95), // Bright Green
                    Color::from_rgb8(0xee, 0xd4, 0x9f), // Bright Yellow
                    Color::from_rgb8(0x8a, 0xad, 0xf4), // Bright Blue
                    Color::from_rgb8(0xc6, 0xa0, 0xf6), // Bright Magenta
                    Color::from_rgb8(0x8b, 0xd5, 0xca), // Bright Cyan
                    Color::from_rgb8(0xca, 0xd3, 0xf5), // Bright White (Text)
                ],
            },
        }
    }

    /// Catppuccin Mocha - Deep, rich dark theme
    pub fn catppuccin_mocha() -> Self {
        Self {
            background: Color::from_rgb8(0x1e, 0x1e, 0x2e), // Base
            surface: Color::from_rgb8(0x18, 0x18, 0x25),    // Mantle
            sidebar: Color::from_rgb8(0x11, 0x11, 0x1b),    // Crust
            tab_bar: Color::from_rgb8(0x11, 0x11, 0x1b),    // Crust
            accent: Color::from_rgb8(0xfa, 0xb3, 0x87),     // Peach
            text_primary: Color::from_rgb8(0xcd, 0xd6, 0xf4), // Text
            text_secondary: Color::from_rgb8(0xba, 0xc2, 0xde), // Subtext1
            text_muted: Color::from_rgb8(0xa6, 0xad, 0xc8), // Subtext0
            border: Color::from_rgb8(0x31, 0x32, 0x44),     // Surface0
            hover: Color::from_rgb8(0x45, 0x47, 0x5a),      // Surface1
            selected: Color::from_rgb8(0x58, 0x5b, 0x70),   // Surface2
            focus_ring: Color::from_rgb8(0xfa, 0xb3, 0x87), // Peach
            terminal: TerminalColors {
                foreground: Color::from_rgb8(0xcd, 0xd6, 0xf4), // Text
                background: Color::from_rgb8(0x1e, 0x1e, 0x2e), // Base
                cursor: Color::from_rgb8(0xf5, 0xe0, 0xdc),     // Rosewater
                ansi: [
                    // Normal colors (0-7)
                    Color::from_rgb8(0x45, 0x47, 0x5a), // Black (Surface1)
                    Color::from_rgb8(0xf3, 0x8b, 0xa8), // Red
                    Color::from_rgb8(0xa6, 0xe3, 0xa1), // Green
                    Color::from_rgb8(0xf9, 0xe2, 0xaf), // Yellow
                    Color::from_rgb8(0x89, 0xb4, 0xfa), // Blue
                    Color::from_rgb8(0xcb, 0xa6, 0xf7), // Magenta (Mauve)
                    Color::from_rgb8(0x94, 0xe2, 0xd5), // Cyan (Teal)
                    Color::from_rgb8(0xba, 0xc2, 0xde), // White (Subtext1)
                    // Bright colors (8-15)
                    Color::from_rgb8(0x58, 0x5b, 0x70), // Bright Black (Surface2)
                    Color::from_rgb8(0xf3, 0x8b, 0xa8), // Bright Red
                    Color::from_rgb8(0xa6, 0xe3, 0xa1), // Bright Green
                    Color::from_rgb8(0xf9, 0xe2, 0xaf), // Bright Yellow
                    Color::from_rgb8(0x89, 0xb4, 0xfa), // Bright Blue
                    Color::from_rgb8(0xcb, 0xa6, 0xf7), // Bright Magenta
                    Color::from_rgb8(0x94, 0xe2, 0xd5), // Bright Cyan
                    Color::from_rgb8(0xcd, 0xd6, 0xf4), // Bright White (Text)
                ],
            },
        }
    }
}

/// Get theme by ID
pub fn get_theme(id: ThemeId) -> Theme {
    match id {
        ThemeId::PortalDefault => Theme::portal_default(),
        ThemeId::CatppuccinLatte => Theme::catppuccin_latte(),
        ThemeId::CatppuccinFrappe => Theme::catppuccin_frappe(),
        ThemeId::CatppuccinMacchiato => Theme::catppuccin_macchiato(),
        ThemeId::CatppuccinMocha => Theme::catppuccin_mocha(),
    }
}

// Layout constants

/// Sidebar width when expanded
pub const SIDEBAR_WIDTH: f32 = 200.0;

/// Sidebar width when collapsed (icons only)
pub const SIDEBAR_WIDTH_COLLAPSED: f32 = 60.0;

/// Border radius for UI elements
pub const BORDER_RADIUS: f32 = 8.0;

/// Border radius for cards
pub const CARD_BORDER_RADIUS: f32 = 12.0;

/// Minimum card width for responsive grid
pub const MIN_CARD_WIDTH: f32 = 260.0;

/// Fixed card height for consistent tile heights
pub const CARD_HEIGHT: f32 = 72.0;

/// Grid spacing between cards
pub const GRID_SPACING: f32 = 12.0;

/// Grid horizontal padding (left + right)
pub const GRID_PADDING: f32 = 48.0;

/// Width of the snippet results panel
pub const RESULTS_PANEL_WIDTH: f32 = 500.0;

/// Minimum card width for snippet grid (wider due to more content)
pub const MIN_SNIPPET_CARD_WIDTH: f32 = 380.0;

// Status colors for execution results
/// Success status color (green)
pub const STATUS_SUCCESS: Color = Color::from_rgb(
    0x40 as f32 / 255.0,
    0xa0 as f32 / 255.0,
    0x2b as f32 / 255.0,
);
/// Success status color darker variant (for hover states)
pub const STATUS_SUCCESS_DARK: Color = Color::from_rgb(
    0x30 as f32 / 255.0,
    0x90 as f32 / 255.0,
    0x20 as f32 / 255.0,
);
/// Failure status color (red)
pub const STATUS_FAILURE: Color = Color::from_rgb(
    0xd2 as f32 / 255.0,
    0x0f as f32 / 255.0,
    0x39 as f32 / 255.0,
);
/// Partial success status color (orange)
pub const STATUS_PARTIAL: Color = Color::from_rgb(
    0xd2 as f32 / 255.0,
    0x8f as f32 / 255.0,
    0x39 as f32 / 255.0,
);

// Typography constants - Base font sizes for consistent UI text
// These are the base sizes before UI scaling is applied

/// Page headers (Settings, About page titles)
pub const FONT_SIZE_PAGE_TITLE: f32 = 30.8;

/// Dialog titles
pub const FONT_SIZE_DIALOG_TITLE: f32 = 22.0;

/// Empty state headings (large centered messages)
pub const FONT_SIZE_HEADING: f32 = 19.8;

/// Section headers (Groups, Hosts, Snippets, History, Results)
pub const FONT_SIZE_SECTION: f32 = 14.3;

/// Body text, primary readable content
pub const FONT_SIZE_BODY: f32 = 14.3;

/// Small button text, menu items, inline buttons
pub const FONT_SIZE_BUTTON_SMALL: f32 = 14.3;

/// Secondary info, descriptions, form field labels
pub const FONT_SIZE_LABEL: f32 = 9.9;

/// Metadata, timestamps, status bar, keyboard shortcuts
pub const FONT_SIZE_CAPTION: f32 = 13.2;

/// Very small text (badges, fingerprints, tiny labels)
pub const FONT_SIZE_SMALL: f32 = 12.1;

/// Monospace code display (ASCII logos)
pub const FONT_SIZE_MONO_TINY: f32 = 11.0;

/// Scaled font sizes for UI elements.
///
/// This struct holds all font sizes after applying the UI scale factor.
/// Use `ScaledFonts::new(scale)` to create an instance with the appropriate scaling.
#[derive(Debug, Clone, Copy)]
pub struct ScaledFonts {
    /// Page headers (Settings, About page titles)
    pub page_title: f32,
    /// Dialog titles
    pub dialog_title: f32,
    /// Empty state headings (large centered messages)
    pub heading: f32,
    /// Section headers (Groups, Hosts, Snippets, History, Results)
    pub section: f32,
    /// Body text, primary readable content
    pub body: f32,
    /// Small button text, menu items, inline buttons
    pub button_small: f32,
    /// Secondary info, descriptions, form field labels
    pub label: f32,
    /// Metadata, timestamps, status bar, keyboard shortcuts
    pub caption: f32,
    /// Very small text (badges, fingerprints, tiny labels)
    pub small: f32,
    /// Monospace code display (ASCII logos)
    pub mono_tiny: f32,
}

impl ScaledFonts {
    /// Create scaled fonts with the given scale factor.
    ///
    /// Scale factor should be between 0.8 and 1.5 (80% to 150%).
    /// Values are rounded to produce clean pixel sizes.
    pub fn new(scale: f32) -> Self {
        Self {
            page_title: (FONT_SIZE_PAGE_TITLE * scale).round(),
            dialog_title: (FONT_SIZE_DIALOG_TITLE * scale).round(),
            heading: (FONT_SIZE_HEADING * scale).round(),
            section: (FONT_SIZE_SECTION * scale).round(),
            body: (FONT_SIZE_BODY * scale).round(),
            button_small: (FONT_SIZE_BUTTON_SMALL * scale).round(),
            label: (FONT_SIZE_LABEL * scale).round(),
            caption: (FONT_SIZE_CAPTION * scale).round(),
            small: (FONT_SIZE_SMALL * scale).round(),
            mono_tiny: (FONT_SIZE_MONO_TINY * scale).round(),
        }
    }
}
