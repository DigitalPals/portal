//! Settings page view (full page, not dialog)

use iced::widget::{
    Column, Row, Space, button, column, container, mouse_area, row, scrollable, slider, text,
};
use iced::{Alignment, Element, Fill, Length};

use crate::fonts::TerminalFont;
use crate::message::{Message, UiMessage};
use crate::theme::{BORDER_RADIUS, CARD_BORDER_RADIUS, ScaledFonts, Theme, ThemeId, get_theme};

pub struct SettingsPageContext {
    pub current_theme: ThemeId,
    pub terminal_font_size: f32,
    pub terminal_font: TerminalFont,
    pub snippet_history_enabled: bool,
    pub snippet_store_command: bool,
    pub snippet_store_output: bool,
    pub snippet_redact_output: bool,
    pub session_logging_enabled: bool,
    /// Credential cache timeout in seconds (0 = disabled)
    pub credential_timeout: u64,
    /// Effective UI scale (user override or system default)
    pub ui_scale: f32,
    /// System-detected UI scale
    pub system_ui_scale: f32,
    /// Whether user has overridden the UI scale
    pub has_ui_scale_override: bool,
}

/// Build the settings page view
pub fn settings_page_view(
    context: SettingsPageContext,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let header = text("Settings")
        .size(fonts.page_title)
        .color(theme.text_primary);

    // === Appearance Section ===
    let appearance_section = settings_section(
        "Appearance",
        theme,
        fonts,
        vec![
            theme_tiles_row(context.current_theme, theme, fonts),
            ui_scale_setting(
                context.ui_scale,
                context.system_ui_scale,
                context.has_ui_scale_override,
                theme,
                fonts,
            ),
        ],
    );

    // === Terminal Section ===
    let terminal_section = settings_section(
        "Terminal",
        theme,
        fonts,
        vec![
            font_selector_setting(context.terminal_font, theme, fonts),
            font_size_setting(context.terminal_font_size, theme, fonts),
            toggle_setting(
                "Enable session logging",
                "Save terminal output to a log file per session",
                context.session_logging_enabled,
                |value| Message::Ui(UiMessage::SessionLoggingEnabled(value)),
                theme,
                fonts,
            ),
        ],
    );

    // === Security Section ===
    let security_section = settings_section(
        "Security",
        theme,
        fonts,
        vec![credential_timeout_setting(context.credential_timeout, theme, fonts)],
    );

    // === Snippet History Section ===
    let snippet_history_section = settings_section(
        "Snippet History",
        theme,
        fonts,
        vec![
            toggle_setting(
                "Enable snippet history",
                "Save snippet execution history to disk",
                context.snippet_history_enabled,
                |value| Message::Ui(UiMessage::SnippetHistoryEnabled(value)),
                theme,
                fonts,
            ),
            toggle_setting(
                "Store commands",
                "Persist executed command text in history entries",
                context.snippet_store_command,
                |value| Message::Ui(UiMessage::SnippetHistoryStoreCommand(value)),
                theme,
                fonts,
            ),
            toggle_setting(
                "Store output",
                "Persist stdout/stderr from snippet executions",
                context.snippet_store_output,
                |value| Message::Ui(UiMessage::SnippetHistoryStoreOutput(value)),
                theme,
                fonts,
            ),
            toggle_setting(
                "Redact sensitive values",
                "Redact common secrets in stored commands and output",
                context.snippet_redact_output,
                |value| Message::Ui(UiMessage::SnippetHistoryRedactOutput(value)),
                theme,
                fonts,
            ),
        ],
    );

    let content = column![
        header,
        Space::new().height(24),
        appearance_section,
        Space::new().height(16),
        terminal_section,
        Space::new().height(16),
        security_section,
        Space::new().height(16),
        snippet_history_section,
    ]
    .padding(32)
    .max_width(700);

    let scrollable_content = scrollable(content).height(Fill).width(Fill);

    container(scrollable_content)
        .width(Fill)
        .height(Fill)
        .style(move |_| container::Style {
            background: Some(theme.background.into()),
            ..Default::default()
        })
        .into()
}

/// Create a settings section with title and items
fn settings_section<'a>(
    title: &'static str,
    theme: Theme,
    fonts: ScaledFonts,
    items: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut section = Column::new().spacing(8);

    // Section title
    section = section.push(text(title).size(fonts.label).color(theme.text_muted));

    // Section card with items
    let mut card_content = Column::new().spacing(16).padding(20);
    for item in items {
        card_content = card_content.push(item);
    }

    let card = container(card_content)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                radius: CARD_BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    section.push(card).into()
}

/// Theme selector with visual tile previews
fn theme_tiles_row(
    current: ThemeId,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let tiles: Vec<Element<'static, Message>> = ThemeId::all()
        .iter()
        .map(|&theme_id| theme_tile(theme_id, theme_id == current, theme, fonts))
        .collect();

    let tiles_row = Row::from_vec(tiles).spacing(12);

    // Wrap in scrollable for narrow screens
    let scrollable_tiles = scrollable(tiles_row)
        .direction(scrollable::Direction::Horizontal(
            scrollable::Scrollbar::new().width(0).scroller_width(0),
        ))
        .width(Fill);

    column![scrollable_tiles,].spacing(0).into()
}

/// Individual theme tile with mini app preview
fn theme_tile(
    tile_theme_id: ThemeId,
    is_selected: bool,
    current_theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let preview_theme = get_theme(tile_theme_id);

    // Mini app preview
    let preview = mini_app_preview(preview_theme);

    // Short theme name for the tile
    let short_name = match tile_theme_id {
        ThemeId::PortalDefault => "Default",
        ThemeId::CatppuccinLatte => "Latte",
        ThemeId::CatppuccinFrappe => "Frappé",
        ThemeId::CatppuccinMacchiato => "Macchiato",
        ThemeId::CatppuccinMocha => "Mocha",
    };

    let name = text(short_name).size(fonts.small).color(if is_selected {
        current_theme.accent
    } else {
        current_theme.text_secondary
    });

    let border_width = if is_selected { 2.0 } else { 1.0 };
    let border_color = if is_selected {
        current_theme.accent
    } else {
        current_theme.border
    };

    let tile_content = column![preview, Space::new().height(6), name,]
        .align_x(Alignment::Center)
        .spacing(0);

    let tile_container = container(tile_content)
        .padding(6)
        .style(move |_| container::Style {
            background: None, // Transparent - let preview colors show through
            border: iced::Border {
                radius: BORDER_RADIUS.into(),
                width: border_width,
                color: border_color,
            },
            ..Default::default()
        });

    mouse_area(tile_container)
        .on_press(Message::Ui(UiMessage::ThemeChange(tile_theme_id)))
        .into()
}

/// Mini app preview showing sidebar, main area, and accent elements
fn mini_app_preview(preview_theme: Theme) -> Element<'static, Message> {
    // Sidebar strip
    let sidebar = container(Space::new().width(14).height(48)).style(move |_| container::Style {
        background: Some(preview_theme.sidebar.into()),
        ..Default::default()
    });

    // Terminal-like lines in main area
    let line = |width: u32| {
        container(Space::new().width(width).height(3u32)).style(move |_| container::Style {
            background: Some(preview_theme.text_muted.into()),
            border: iced::Border {
                radius: 1.5.into(),
                ..Default::default()
            },
            ..Default::default()
        })
    };

    // Accent button element
    let accent_button =
        container(Space::new().width(20).height(5)).style(move |_| container::Style {
            background: Some(preview_theme.accent.into()),
            border: iced::Border {
                radius: 2.0.into(),
                ..Default::default()
            },
            ..Default::default()
        });

    let main_content = column![
        Space::new().height(5),
        line(40),
        Space::new().height(3),
        line(28),
        Space::new().height(3),
        line(34),
        Space::new().height(6),
        accent_button,
    ]
    .padding([4, 6]);

    let main_area = container(main_content)
        .width(66)
        .height(48)
        .style(move |_| container::Style {
            background: Some(preview_theme.background.into()),
            ..Default::default()
        });

    // Combine sidebar and main area
    let preview_content = row![sidebar, main_area].spacing(0);

    container(preview_content)
        .style(move |_| container::Style {
            background: Some(preview_theme.surface.into()),
            border: iced::Border {
                radius: 4.0.into(),
                color: preview_theme.border,
                width: 1.0,
            },
            ..Default::default()
        })
        .into()
}

/// Font selector with tile previews
fn font_selector_setting(
    current_font: TerminalFont,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("Font").size(fonts.body).color(theme.text_primary);

    let description = text("Terminal font family")
        .size(fonts.label)
        .color(theme.text_muted);

    // Create font tiles
    let tiles: Vec<Element<'static, Message>> = TerminalFont::all()
        .iter()
        .map(|&font| font_tile(font, font == current_font, theme, fonts))
        .collect();

    let tiles_row = Row::from_vec(tiles).spacing(12);

    column![
        label,
        Space::new().height(4),
        description,
        Space::new().height(12),
        tiles_row,
    ]
    .spacing(0)
    .into()
}

/// Individual font tile showing font preview
fn font_tile(
    font: TerminalFont,
    is_selected: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let iced_font = font.to_iced_font();

    // Preview text showing the font
    let preview = text("Aa")
        .size(20)
        .font(iced_font)
        .color(theme.text_primary);

    let name = text(font.display_name())
        .size(fonts.small)
        .color(if is_selected {
            theme.accent
        } else {
            theme.text_secondary
        });

    let border_width = if is_selected { 2.0 } else { 1.0 };
    let border_color = if is_selected {
        theme.accent
    } else {
        theme.border
    };

    let tile_content = column![preview, Space::new().height(6), name,]
        .align_x(Alignment::Center)
        .spacing(0);

    let tile_container =
        container(tile_content)
            .padding([12, 20])
            .style(move |_| container::Style {
                background: Some(theme.background.into()),
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    width: border_width,
                    color: border_color,
                },
                ..Default::default()
            });

    mouse_area(tile_container)
        .on_press(Message::Ui(UiMessage::FontChange(font)))
        .into()
}

/// Font size slider setting
fn font_size_setting(
    current_size: f32,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("Font Size").size(fonts.body).color(theme.text_primary);

    let description = text("Terminal text size")
        .size(fonts.label)
        .color(theme.text_muted);

    let slider_widget = slider(6.0..=20.0, current_size, |v| {
        Message::Ui(UiMessage::FontSizeChange(v))
    })
    .step(1.0)
    .width(140);

    let value_text = text(format!("{}px", current_size as u32))
        .size(fonts.body)
        .color(theme.text_secondary);

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}

/// UI scale slider setting
fn ui_scale_setting(
    current_scale: f32,
    system_scale: f32,
    has_override: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("UI Scale").size(fonts.body).color(theme.text_primary);

    // Show system default in description when not overridden
    let description_text = if has_override {
        format!("System default: {}%", (system_scale * 100.0).round() as u32)
    } else {
        "Scale all interface text (except terminal)".to_string()
    };
    let description = text(description_text)
        .size(fonts.label)
        .color(theme.text_muted);

    // Slider from 80% to 150% with 5% steps
    let slider_widget = slider(0.8..=1.5, current_scale, |v| {
        // Round to nearest 0.05 (5%)
        let rounded = (v * 20.0).round() / 20.0;
        Message::Ui(UiMessage::UiScaleChange(rounded))
    })
    .step(0.05)
    .width(140);

    let value_text = text(format!("{}%", (current_scale * 100.0).round() as u32))
        .size(fonts.body)
        .color(theme.text_secondary);

    // Reset button (only visible when override is set)
    let reset_button: Element<'static, Message> = if has_override {
        button(
            container(text("Reset").size(fonts.label).color(theme.text_secondary))
                .padding([4, 8])
                .align_x(Alignment::Center)
                .align_y(Alignment::Center),
        )
        .padding(0)
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered => Some(theme.hover.into()),
                _ => None,
            };
            iced::widget::button::Style {
                background,
                border: iced::Border {
                    radius: BORDER_RADIUS.into(),
                    width: 1.0,
                    color: theme.border,
                },
                ..Default::default()
            }
        })
        .on_press(Message::Ui(UiMessage::UiScaleReset))
        .into()
    } else {
        Space::new().width(0).into()
    };

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
            Space::new().width(8),
            reset_button,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}

fn toggle_setting<F>(
    label: &'static str,
    description: &'static str,
    enabled: bool,
    on_toggle: F,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message>
where
    F: Fn(bool) -> Message + 'static,
{
    let label_text = text(label).size(fonts.body).color(theme.text_primary);

    let description_text = text(description).size(fonts.label).color(theme.text_muted);

    let toggle_label = if enabled { "On" } else { "Off" };
    let toggle_color = if enabled {
        theme.accent
    } else {
        theme.text_muted
    };

    let toggle_button = button(
        container(text(toggle_label).size(fonts.label).color(toggle_color))
            .padding([6, 12])
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .padding(0)
    .style(move |_theme, _status| iced::widget::button::Style {
        background: Some(theme.surface.into()),
        border: iced::Border {
            radius: BORDER_RADIUS.into(),
            width: 1.0,
            color: theme.border,
        },
        ..Default::default()
    })
    .on_press(on_toggle(!enabled));

    column![
        row![
            column![label_text, Space::new().height(4), description_text].spacing(0),
            Space::new().width(Length::Fill),
            toggle_button,
        ]
        .align_y(Alignment::Center),
    ]
    .spacing(0)
    .into()
}

fn format_timeout_seconds(seconds: u64) -> String {
    if seconds == 0 {
        return "Off".to_string();
    }

    if seconds % 3600 == 0 {
        let hours = seconds / 3600;
        return if hours == 1 {
            "1 hour".to_string()
        } else {
            format!("{} hours", hours)
        };
    }

    if seconds % 60 == 0 {
        let minutes = seconds / 60;
        return if minutes == 1 {
            "1 min".to_string()
        } else {
            format!("{} min", minutes)
        };
    }

    format!("{} sec", seconds)
}

fn credential_timeout_setting(
    timeout_seconds: u64,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'static, Message> {
    let label = text("Credential Timeout")
        .size(fonts.body)
        .color(theme.text_primary);

    let description = text("Cache SSH credentials in memory for this long (0 disables caching)")
        .size(fonts.label)
        .color(theme.text_muted);

    let current = timeout_seconds.min(3600) as f32;
    let slider_widget = slider(0.0..=3600.0, current, |v| {
        // Keep changes stable and predictable by snapping to 30s increments.
        let snapped = ((v / 30.0).round() * 30.0).clamp(0.0, 3600.0);
        Message::Ui(UiMessage::CredentialTimeoutChange(snapped as u64))
    })
    .step(30.0)
    .width(140);

    let value_text = text(format_timeout_seconds(timeout_seconds.min(3600)))
        .size(fonts.body)
        .color(theme.text_secondary);

    column![
        row![
            label,
            Space::new().width(Length::Fill),
            slider_widget,
            Space::new().width(12),
            value_text,
        ]
        .align_y(Alignment::Center),
        Space::new().height(4),
        description,
    ]
    .spacing(0)
    .into()
}
