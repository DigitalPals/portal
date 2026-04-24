//! Context menu for terminal tabs

use iced::widget::{Column, Space, button, container, text};
use iced::{Color, Element, Length, Padding};
use iced::{Fill, Point};
use uuid::Uuid;

use crate::message::{Message, TabContextMenuAction, TabMessage};
use crate::theme::{ScaledFonts, Theme};
use crate::widgets::mouse_area;

const CONTEXT_MENU_WIDTH: f32 = 220.0;
const ESTIMATED_MENU_HEIGHT: f32 = 96.0;

/// State for the terminal tab context menu
#[derive(Debug, Clone)]
pub struct TabContextMenuState {
    pub visible: bool,
    pub position: Point,
    pub target_tab: Option<Uuid>,
}

impl Default for TabContextMenuState {
    fn default() -> Self {
        Self {
            visible: false,
            position: Point::ORIGIN,
            target_tab: None,
        }
    }
}

impl TabContextMenuState {
    pub fn show(&mut self, tab_id: Uuid, x: f32, y: f32) {
        self.visible = true;
        self.position = Point::new(x, y);
        self.target_tab = Some(tab_id);
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.target_tab = None;
    }
}

fn context_menu_item<'a>(
    label: &'static str,
    action: TabContextMenuAction,
    tab_id: Uuid,
    enabled: bool,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let text_color = if enabled {
        theme.text_primary
    } else {
        theme.text_muted
    };

    let content = container(text(label).size(fonts.body).color(text_color))
        .padding(Padding::new(8.0).left(12.0))
        .width(Length::Fill);

    let mut btn = button(content)
        .style(move |_theme, status| {
            let background = match status {
                iced::widget::button::Status::Hovered if enabled => Some(theme.hover.into()),
                _ => None,
            };
            iced::widget::button::Style {
                background,
                text_color,
                ..Default::default()
            }
        })
        .padding(0);
    if enabled {
        btn = btn.on_press(Message::Tab(TabMessage::ContextMenuAction(tab_id, action)));
    }

    btn.into()
}

/// Build the context menu overlay for terminal tabs
pub fn tab_context_menu_overlay(
    state: &TabContextMenuState,
    theme: Theme,
    fonts: ScaledFonts,
    window_size: iced::Size,
    has_log_file: bool,
    has_log_dir: bool,
) -> Element<'_, Message> {
    if !state.visible {
        return Space::new().into();
    }

    let Some(tab_id) = state.target_tab else {
        return Space::new().into();
    };

    let items: Vec<Element<'_, Message>> = vec![
        context_menu_item(
            "Open Log File",
            TabContextMenuAction::OpenLogFile,
            tab_id,
            has_log_file,
            theme,
            fonts,
        ),
        context_menu_item(
            "Open Log Directory",
            TabContextMenuAction::OpenLogDirectory,
            tab_id,
            has_log_dir,
            theme,
            fonts,
        ),
    ];

    let menu = container(Column::with_children(items).spacing(4))
        .padding(8)
        .width(Length::Fixed(CONTEXT_MENU_WIDTH))
        .style(move |_| container::Style {
            background: Some(theme.surface.into()),
            border: iced::Border {
                color: theme.border,
                width: 1.0,
                radius: 12.0.into(),
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.15),
                offset: iced::Vector::new(0.0, 4.0),
                blur_radius: 16.0,
            },
            ..Default::default()
        });
    let menu = mouse_area(menu).capture_all_events(true);

    let pos = state.position;
    let mut x = pos.x;
    let mut y = pos.y;

    if x + CONTEXT_MENU_WIDTH > window_size.width {
        x = (window_size.width - CONTEXT_MENU_WIDTH).max(0.0);
    }

    if y + ESTIMATED_MENU_HEIGHT > window_size.height {
        y = (window_size.height - ESTIMATED_MENU_HEIGHT).max(0.0);
    }

    let background = mouse_area(
        container(Space::new().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill),
    )
    .on_press(Message::Tab(TabMessage::HideContextMenu));

    let positioned_menu = container(menu).padding(Padding::new(0.0).top(y).left(x));

    iced::widget::stack![background, positioned_menu].into()
}
