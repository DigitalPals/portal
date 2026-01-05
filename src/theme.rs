use iced::Color;

/// Dark theme colors based on the UI specification
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
    /// Dark theme (default)
    pub fn dark() -> Self {
        Self {
            background: Color::from_rgb8(0x1a, 0x1a, 0x1a),    // #1a1a1a
            surface: Color::from_rgb8(0x25, 0x25, 0x26),       // #252526
            sidebar: Color::from_rgb8(0x1e, 0x1e, 0x1e),       // #1e1e1e
            accent: Color::from_rgb8(0x00, 0x78, 0xd4),        // #0078d4
            text_primary: Color::from_rgb8(0xe0, 0xe0, 0xe0), // #e0e0e0
            text_secondary: Color::from_rgb8(0xa0, 0xa0, 0xa0), // #a0a0a0
            text_muted: Color::from_rgb8(0x70, 0x70, 0x70),    // #707070
            border: Color::from_rgb8(0x3c, 0x3c, 0x3c),        // #3c3c3c
            hover: Color::from_rgb8(0x2a, 0x2a, 0x2a),         // #2a2a2a
            selected: Color::from_rgb8(0x09, 0x45, 0x71),      // #094571
        }
    }

    /// Light theme
    #[allow(dead_code)]
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

/// Global theme instance (dark by default)
pub static THEME: std::sync::LazyLock<Theme> = std::sync::LazyLock::new(Theme::dark);

/// Sidebar width in pixels
pub const SIDEBAR_WIDTH: f32 = 220.0;

/// Border radius for UI elements
pub const BORDER_RADIUS: f32 = 4.0;
