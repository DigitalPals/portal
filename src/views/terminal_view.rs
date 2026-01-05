//! Terminal view component
//!
//! This wraps the terminal widget with session management.

use std::sync::Arc;

use iced::widget::container;
use iced::{Element, Fill};
use parking_lot::Mutex;
use uuid::Uuid;

use crate::message::Message;
use crate::terminal::backend::{EventProxy, TerminalSize};
use crate::terminal::TerminalBackend;
use crate::terminal::widget::TerminalWidget;
use crate::theme::THEME;

use alacritty_terminal::term::Term;

/// Session ID type
pub type SessionId = Uuid;

/// Terminal session state
pub struct TerminalSession {
    pub id: SessionId,
    pub backend: TerminalBackend,
}

impl TerminalSession {
    /// Create a new terminal session
    pub fn new(_title: impl Into<String>) -> Self {
        let size = TerminalSize::new(80, 24, 9.0, 18.0);
        Self {
            id: Uuid::new_v4(),
            backend: TerminalBackend::new(size),
        }
    }

    /// Get the terminal for rendering
    pub fn term(&self) -> Arc<Mutex<Term<EventProxy>>> {
        self.backend.term()
    }

    /// Get the terminal size
    pub fn size(&self) -> TerminalSize {
        self.backend.size()
    }

    /// Process input bytes (from SSH or PTY)
    pub fn process_output(&self, bytes: &[u8]) {
        self.backend.process_input(bytes);
    }
}

/// Build a terminal view element
pub fn terminal_view<'a>(
    session: &'a TerminalSession,
    on_input: impl Fn(SessionId, Vec<u8>) -> Message + 'a,
) -> Element<'a, Message> {
    let session_id = session.id;
    let term = session.term();
    let size = session.size();

    let terminal_widget = TerminalWidget::new(term, size, move |bytes| {
        on_input(session_id, bytes)
    });

    container(terminal_widget)
        .width(Fill)
        .height(Fill)
        .style(|_theme| container::Style {
            background: Some(THEME.background.into()),
            ..Default::default()
        })
        .into()
}
