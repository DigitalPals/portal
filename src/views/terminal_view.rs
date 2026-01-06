//! Terminal view component
//!
//! This wraps the terminal widget with session management.

use std::sync::Arc;
use std::time::Instant;

use iced::widget::{column, container};
use iced::{Element, Fill};
use parking_lot::Mutex;
use uuid::Uuid;

use crate::fonts::TerminalFont;
use crate::message::{Message, SessionId};
use crate::terminal::TerminalBackend;
use crate::terminal::backend::{EventProxy, TerminalEvent, TerminalSize};
use tokio::sync::mpsc;
use crate::terminal::widget::TerminalWidget;
use crate::theme::Theme;

use super::terminal_status_bar::terminal_status_bar;
use alacritty_terminal::term::Term;

/// Terminal session state
pub struct TerminalSession {
    pub id: SessionId,
    pub backend: TerminalBackend,
}

impl TerminalSession {
    /// Create a new terminal session
    pub fn new(_title: impl Into<String>) -> (Self, mpsc::Receiver<TerminalEvent>) {
        let size = TerminalSize::new(80, 24);
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

    /// Process input bytes (from SSH or PTY)
    pub fn process_output(&self, bytes: &[u8]) {
        self.backend.process_input(bytes);
    }

    /// Resize the terminal to new dimensions
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.backend.resize(cols, rows);
    }
}

/// Build a terminal view element
/// Build a terminal view element with status bar
#[allow(clippy::too_many_arguments)]
pub fn terminal_view_with_status<'a>(
    theme: Theme,
    session: &'a TerminalSession,
    session_start: Instant,
    host_name: &'a str,
    status_message: Option<&'a str>,
    font_size: f32,
    terminal_font: TerminalFont,
    on_input: impl Fn(SessionId, Vec<u8>) -> Message + 'a,
    on_resize: impl Fn(SessionId, u16, u16) -> Message + 'a,
) -> Element<'a, Message> {
    let session_id = session.id;
    let term = session.term();
    let terminal_widget = TerminalWidget::new(term, move |bytes| on_input(session_id, bytes))
        .on_resize(move |cols, rows| on_resize(session_id, cols, rows))
        .font_size(font_size)
        .font(terminal_font)
        .terminal_colors(theme.terminal);

    let terminal_container =
        container(terminal_widget)
            .width(Fill)
            .height(Fill)
            .style(move |_theme| container::Style {
                background: Some(theme.terminal.background.into()),
                ..Default::default()
            });

    let status_bar = terminal_status_bar(theme, host_name, session_start, status_message);

    column![terminal_container, status_bar].into()
}
