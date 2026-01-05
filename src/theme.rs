use iced::Color;

/// Dark theme colors based on the UI specification
#[derive(Clone, Copy)]
pub struct Theme {
    pub background: Color,
    pub surface: Color,
    pub sidebar: Color,
    pub accent: Color,
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub border: Color,
    pub hover: Color,
    pub selected: Color,
}

impl Theme {
    /// Dark theme (default) - Royal TSX-style navy blue-gray
    pub fn dark() -> Self {
        Self {
            background: Color::from_rgb8(0x1e, 0x22, 0x33),    // #1e2233 - dark navy blue
            surface: Color::from_rgb8(0x2a, 0x31, 0x42),       // #2a3142 - slate blue-gray
            sidebar: Color::from_rgb8(0x1a, 0x1d, 0x2b),       // #1a1d2b - darker navy
            accent: Color::from_rgb8(0x00, 0x78, 0xd4),        // #0078d4 - bright blue
            text_primary: Color::from_rgb8(0xe8, 0xe8, 0xe8),  // #e8e8e8 - bright white
            text_secondary: Color::from_rgb8(0x9a, 0xa0, 0xb0), // #9aa0b0 - blue-gray text
            text_muted: Color::from_rgb8(0x6a, 0x70, 0x80),    // #6a7080 - muted blue-gray
            border: Color::from_rgb8(0x3a, 0x40, 0x55),        // #3a4055 - navy border
            hover: Color::from_rgb8(0x35, 0x3d, 0x50),         // #353d50 - hover blue-gray
            selected: Color::from_rgb8(0x2a, 0x4a, 0x6d),      // #2a4a6d - selected blue
        }
    }

    /// Light theme
    pub fn light() -> Self {
        Self {
            background: Color::from_rgb8(0xff, 0xff, 0xff),    // #ffffff
            surface: Color::from_rgb8(0xf3, 0xf3, 0xf3),       // #f3f3f3
            sidebar: Color::from_rgb8(0xf0, 0xf0, 0xf0),       // #f0f0f0
            accent: Color::from_rgb8(0x00, 0x78, 0xd4),        // #0078d4
            text_primary: Color::from_rgb8(0x1a, 0x1a, 0x1a),  // #1a1a1a
            text_secondary: Color::from_rgb8(0x50, 0x50, 0x50), // #505050
            text_muted: Color::from_rgb8(0x90, 0x90, 0x90),    // #909090
            border: Color::from_rgb8(0xd0, 0xd0, 0xd0),        // #d0d0d0
            hover: Color::from_rgb8(0xe8, 0xe8, 0xe8),         // #e8e8e8
            selected: Color::from_rgb8(0xcc, 0xe4, 0xf7),      // #cce4f7
        }
    }
}

/// Select theme based on preference.
pub fn theme_for(dark_mode: bool) -> Theme {
    if dark_mode {
        Theme::dark()
    } else {
        Theme::light()
    }
}

/// Sidebar width when expanded
pub const SIDEBAR_WIDTH: f32 = 200.0;

/// Sidebar width when collapsed (icons only)
pub const SIDEBAR_WIDTH_COLLAPSED: f32 = 60.0;

/// Border radius for UI elements
pub const BORDER_RADIUS: f32 = 8.0;

/// Border radius for cards
pub const CARD_BORDER_RADIUS: f32 = 12.0;

/// Minimum card width for responsive grid
pub const MIN_CARD_WIDTH: f32 = 300.0;

/// Fixed card height for consistent tile heights
pub const CARD_HEIGHT: f32 = 80.0;

/// Grid spacing between cards
pub const GRID_SPACING: f32 = 16.0;

/// Grid horizontal padding (left + right)
pub const GRID_PADDING: f32 = 48.0;

/// Threshold for auto-collapsing sidebar
pub const SIDEBAR_AUTO_COLLAPSE_THRESHOLD: f32 = 900.0;
