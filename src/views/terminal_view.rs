//! Terminal view component
//!
//! This wraps the terminal widget with session management.

use std::sync::Arc;
use std::time::Instant;

use iced::widget::{button, column, container, row, stack, text, text_input};
use iced::{Alignment, Color, Element, Fill};
use parking_lot::Mutex;
use uuid::Uuid;

use crate::config::settings::TerminalMetricAdjustments;
use crate::fonts::TerminalFont;
use crate::icons::{icon_with_color, ui};
use crate::keybindings::KeybindingsConfig;
use crate::message::{Message, SearchMessage, SessionId, SessionMessage};
use crate::terminal::TerminalBackend;
use crate::terminal::backend::{EventProxy, TerminalEvent, TerminalSize};
use crate::terminal::metrics::TerminalMetrics;
use crate::terminal::search::TerminalSearchState;
use crate::terminal::widget::TerminalWidget;
use crate::theme::{BORDER_RADIUS, RADIUS_MD, ScaledFonts, Theme};
use std::sync::atomic::AtomicU64;
use tokio::sync::mpsc;

use super::terminal_status_bar::terminal_status_bar;
use alacritty_terminal::term::Term;

/// Widget id of the terminal search bar's text input (for focus handling).
pub fn terminal_search_input_id() -> iced::widget::Id {
    iced::widget::Id::new("terminal_search_input")
}

/// Terminal session state
pub struct TerminalSession {
    pub id: SessionId,
    pub backend: TerminalBackend,
}

impl TerminalSession {
    /// Create a new terminal session
    pub fn new(_title: impl Into<String>) -> (Self, mpsc::Receiver<TerminalEvent>) {
        Self::new_with_size(_title, 80, 24)
    }

    /// Create a new terminal session with an initial grid size.
    pub fn new_with_size(
        _title: impl Into<String>,
        columns: u16,
        rows: u16,
    ) -> (Self, mpsc::Receiver<TerminalEvent>) {
        let size = TerminalSize::new(columns, rows);
        let (backend, event_rx) = TerminalBackend::new(size);
        (
            Self {
                id: Uuid::new_v4(),
                backend,
            },
            event_rx,
        )
    }

    /// Get the terminal for rendering
    pub fn term(&self) -> Arc<Mutex<Term<EventProxy>>> {
        self.backend.term()
    }

    pub fn render_epoch(&self) -> Arc<AtomicU64> {
        self.backend.render_epoch()
    }

    pub fn set_terminal_colors(&self, colors: crate::theme::TerminalColors) {
        self.backend.set_colors(colors);
    }

    pub fn set_cell_size(&self, cell_width: f32, cell_height: f32) {
        self.backend.set_cell_size(cell_width, cell_height);
    }

    /// Get the current terminal grid size.
    pub fn size(&self) -> (u16, u16) {
        self.backend.size()
    }

    /// Process input bytes (from SSH or PTY)
    pub fn process_output(&self, bytes: &[u8]) {
        self.backend.process_input(bytes);
    }

    /// Replace the visible terminal state with the final rendered state of a byte stream.
    pub fn replace_with_rendered_snapshot(&self, bytes: &[u8]) {
        self.backend.replace_with_rendered_snapshot(bytes);
    }

    /// Resize the terminal to new dimensions
    pub fn resize(&mut self, cols: u16, rows: u16) -> bool {
        self.backend.resize(cols, rows)
    }
}

/// Build a terminal view element
/// Build a terminal view element with status bar
#[allow(clippy::too_many_arguments)]
pub fn terminal_view_with_status<'a>(
    theme: Theme,
    fonts: ScaledFonts,
    session: &'a TerminalSession,
    session_start: Instant,
    host_name: &'a str,
    status_message: Option<String>,
    font_size: f32,
    scroll_speed: f32,
    terminal_font: TerminalFont,
    terminal_metric_adjustments: TerminalMetricAdjustments,
    keybindings: KeybindingsConfig,
    focus_token: u64,
    search: &'a TerminalSearchState,
    on_input: impl Fn(SessionId, Vec<u8>) -> Message + 'a,
    on_resize: impl Fn(SessionId, u16, u16) -> Message + 'a,
    on_paste: impl Fn(SessionId) -> Message + 'a,
) -> Element<'a, Message> {
    let session_id = session.id;
    session.set_terminal_colors(theme.terminal);
    let metrics = TerminalMetrics::for_font_with_adjustments(
        terminal_font,
        font_size,
        terminal_metric_adjustments,
    );
    session.set_cell_size(metrics.cell_width, metrics.cell_height);
    let term = session.term();

    // Highlight styles picked from the terminal palette: yellow reads well on
    // both dark and light theme backgrounds. The active match additionally
    // gets an outline (drawn by the widget) so it stands out from the rest.
    let active_color = Color {
        a: 0.95,
        ..theme.terminal.ansi[11]
    };
    let inactive_color = Color {
        a: 0.30,
        ..theme.terminal.ansi[3]
    };

    let terminal_widget = TerminalWidget::new(term, move |bytes| on_input(session_id, bytes))
        .render_epoch(session.render_epoch())
        .on_resize(move |cols, rows| on_resize(session_id, cols, rows))
        .on_paste(move || on_paste(session_id))
        .font_size(font_size)
        .scroll_speed(scroll_speed)
        .font(terminal_font)
        .metric_adjustments(terminal_metric_adjustments)
        .keybindings(keybindings)
        .focus_token(focus_token)
        .keyboard_input(!search.open)
        .search_highlights(
            &search.matches,
            search.current,
            search.version,
            active_color,
            inactive_color,
        )
        .terminal_colors(theme.terminal);

    let terminal_container =
        container(terminal_widget)
            .width(Fill)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(theme.terminal.background.into()),
                ..Default::default()
            });

    let terminal_area: Element<'a, Message> = if search.open {
        stack![
            terminal_container,
            container(terminal_search_bar(session_id, search, theme, fonts))
                .width(Fill)
                .align_x(Alignment::End)
                .padding([8, 16]),
        ]
        .into()
    } else {
        terminal_container.into()
    };

    let status_bar = terminal_status_bar(theme, fonts, host_name, session_start, status_message);

    column![terminal_area, status_bar].into()
}

/// Small icon/text button used inside the terminal search bar.
fn search_bar_button<'a>(
    content: impl Into<Element<'a, Message>>,
    message: Option<Message>,
    active: bool,
    theme: Theme,
) -> Element<'a, Message> {
    button(
        container(content)
            .width(22)
            .height(22)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center),
    )
    .padding(0)
    .style(move |_theme, status| {
        let background = if active {
            Some(theme.selected.into())
        } else {
            match status {
                button::Status::Hovered | button::Status::Pressed => Some(theme.hover.into()),
                _ => None,
            }
        };
        button::Style {
            background,
            text_color: theme.text_primary,
            border: iced::Border {
                color: if active {
                    theme.focus_ring
                } else {
                    iced::Color::TRANSPARENT
                },
                width: if active { 1.0 } else { 0.0 },
                radius: RADIUS_MD.into(),
            },
            ..Default::default()
        }
    })
    .on_press_maybe(message)
    .into()
}

/// The find-in-buffer bar overlaid at the top-right of the terminal.
fn terminal_search_bar<'a>(
    session_id: SessionId,
    search: &TerminalSearchState,
    theme: Theme,
    fonts: ScaledFonts,
) -> Element<'a, Message> {
    let search_message =
        |message: SearchMessage| Message::Session(SessionMessage::Search(message));

    let input = text_input("Find", &search.query)
        .id(terminal_search_input_id())
        .on_input(move |query| search_message(SearchMessage::QueryChanged(session_id, query)))
        .padding([4, 8])
        .size(fonts.caption)
        .width(190)
        .style(move |_theme, status| {
            let border_color = match status {
                text_input::Status::Focused { .. } => theme.focus_ring,
                _ => theme.border,
            };
            text_input::Style {
                background: theme.background.into(),
                border: iced::Border {
                    color: border_color,
                    width: 1.0,
                    radius: RADIUS_MD.into(),
                },
                icon: theme.text_muted,
                placeholder: theme.text_muted,
                value: theme.text_primary,
                selection: theme.selected,
            }
        });

    let counter_color = if search.matches.is_empty() && !search.query.is_empty() {
        theme.text_muted
    } else {
        theme.text_secondary
    };
    let counter = text(search.counter_label().unwrap_or_default())
        .size(fonts.small)
        .color(counter_color);

    let has_matches = !search.matches.is_empty();
    let previous_button = search_bar_button(
        text("↑").size(fonts.caption).color(theme.text_primary),
        has_matches.then(|| search_message(SearchMessage::PreviousMatch(session_id))),
        false,
        theme,
    );
    let next_button = search_bar_button(
        text("↓").size(fonts.caption).color(theme.text_primary),
        has_matches.then(|| search_message(SearchMessage::NextMatch(session_id))),
        false,
        theme,
    );
    let case_button = search_bar_button(
        text("Aa").size(fonts.small).color(theme.text_primary),
        Some(search_message(SearchMessage::CaseSensitiveToggled(
            session_id,
        ))),
        search.case_sensitive,
        theme,
    );
    let close_button = search_bar_button(
        icon_with_color(ui::X, 12, theme.text_secondary),
        Some(search_message(SearchMessage::Close(session_id))),
        false,
        theme,
    );

    container(
        row![
            input,
            counter,
            previous_button,
            next_button,
            case_button,
            close_button,
        ]
        .spacing(6)
        .align_y(Alignment::Center),
    )
    .padding([6, 8])
    .style(move |_theme| container::Style {
        background: Some(theme.surface.into()),
        border: iced::Border {
            color: theme.border,
            width: 1.0,
            radius: BORDER_RADIUS.into(),
        },
        shadow: iced::Shadow {
            color: Color::from_rgba8(0, 0, 0, 0.3),
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 8.0,
        },
        ..Default::default()
    })
    .into()
}
