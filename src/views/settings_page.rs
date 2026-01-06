//! Settings page view (full page, not dialog)

use iced::widget::{
    column, container, mouse_area, row, scrollable, slider, text, Column, Row, Space,
};
use iced::{Alignment, Element, Fill, Length};

use crate::message::{Message, UiMessage};
use crate::theme::{get_theme, Theme, ThemeId, BORDER_RADIUS, CARD_BORDER_RADIUS};

/// Build the settings page view
pub fn settings_page_view(
    current_theme: ThemeId,
    terminal_font_size: f32,
    theme: Theme,
) -> Element<'static, Message> {
    let header = text("Settings").size(28).color(theme.text_primary);

    // === Appearance Section ===
    let appearance_section = settings_section(
        "Appearance",
        theme,
        vec![theme_tiles_row(current_theme, theme)],
    );

    // === Terminal Section ===
    let terminal_section = settings_section(
        "Terminal",
        theme,
        vec![font_size_setting(terminal_font_size, theme)],
    );

    // === About Section ===
    let about_section = settings_section("About", theme, vec![about_content(theme)]);

    let content = column![
        header,
        Space::new().height(24),
        appearance_section,
        Space::new().height(16),
        terminal_section,
        Space::new().height(16),
        about_section,
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
    items: Vec<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut section = Column::new().spacing(8);

    // Section title
    section = section.push(
        text(title)
            .size(12)
            .color(theme.text_muted),
    );

    // Section card with items
    let mut card_content = Column::new().spacing(16).padding(20);
    for item in items {
        card_content = card_content.push(item);
    }

    let card = container(card_content).width(Fill).style(move |_| {
        container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                radius: CARD_BORDER_RADIUS.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    });

    section.push(card).into()
}

/// Theme selector with visual tile previews
fn theme_tiles_row(current: ThemeId, theme: Theme) -> Element<'static, Message> {
    let tiles: Vec<Element<'static, Message>> = ThemeId::all()
        .iter()
        .map(|&theme_id| theme_tile(theme_id, theme_id == current, theme))
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
fn theme_tile(tile_theme_id: ThemeId, is_selected: bool, current_theme: Theme) -> Element<'static, Message> {
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

    let name = text(short_name)
        .size(11)
        .color(if is_selected {
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
    let accent_button = container(Space::new().width(20).height(5)).style(move |_| container::Style {
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

/// Font size slider setting
fn font_size_setting(current_size: f32, theme: Theme) -> Element<'static, Message> {
    let label = text("Font Size").size(14).color(theme.text_primary);

    let description = text("Terminal text size")
        .size(12)
        .color(theme.text_muted);

    let slider_widget = slider(6.0..=20.0, current_size, |v| {
        Message::Ui(UiMessage::FontSizeChange(v))
    })
    .step(1.0)
    .width(140);

    let value_text = text(format!("{}px", current_size as u32))
        .size(14)
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

/// About section content
fn about_content(theme: Theme) -> Element<'static, Message> {
    column![
        text(format!(
            "Portal SSH Client v{}",
            env!("CARGO_PKG_VERSION")
        ))
        .size(14)
        .color(theme.text_primary),
        Space::new().height(4),
        text("A modern SSH client built with Rust and Iced")
            .size(12)
            .color(theme.text_muted),
    ]
    .spacing(0)
    .into()
}

