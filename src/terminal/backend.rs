//! Terminal backend using alacritty_terminal
//!
//! This module wraps the alacritty_terminal Term for use with iced.

use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, Processor};
use parking_lot::Mutex;
use tokio::sync::mpsc;

/// Events emitted by the terminal backend
#[derive(Debug, Clone)]
pub enum TerminalEvent {
    /// Terminal title changed
    Title(String),
    /// Bell rang
    Bell,
    /// Clipboard request (copy)
    ClipboardStore(String),
    /// Clipboard request (paste)
    ClipboardLoad,
    /// Terminal exited
    Exit,
    /// Wakeup (content changed)
    Wakeup,
}

/// Event proxy that forwards alacritty events to our channel
#[derive(Clone)]
pub struct EventProxy {
    sender: mpsc::UnboundedSender<TerminalEvent>,
}

impl EventProxy {
    pub fn new(sender: mpsc::UnboundedSender<TerminalEvent>) -> Self {
        Self { sender }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let terminal_event = match event {
            Event::Wakeup => TerminalEvent::Wakeup,
            Event::Bell => TerminalEvent::Bell,
            Event::Exit => TerminalEvent::Exit,
            Event::Title(title) => TerminalEvent::Title(title),
            Event::ClipboardStore(_, data) => TerminalEvent::ClipboardStore(data),
            Event::ClipboardLoad(_, _) => TerminalEvent::ClipboardLoad,
            _ => return, // Ignore other events for now
        };
        let _ = self.sender.send(terminal_event);
    }
}

/// Terminal dimensions
#[derive(Debug, Clone, Copy)]
pub struct TerminalSize {
    pub columns: u16,
    pub lines: u16,
    pub cell_width: f32,
    pub cell_height: f32,
    pub history_size: usize,
}

/// Default scrollback history size (number of lines to keep)
const DEFAULT_HISTORY_SIZE: usize = 10000;

impl TerminalSize {
    pub fn new(columns: u16, lines: u16, cell_width: f32, cell_height: f32) -> Self {
        Self {
            columns,
            lines,
            cell_width,
            cell_height,
            history_size: DEFAULT_HISTORY_SIZE,
        }
    }

    /// Calculate pixel dimensions
    pub fn pixel_width(&self) -> f32 {
        self.columns as f32 * self.cell_width
    }

    pub fn pixel_height(&self) -> f32 {
        self.lines as f32 * self.cell_height
    }
}

impl Dimensions for TerminalSize {
    fn total_lines(&self) -> usize {
        self.lines as usize + self.history_size
    }

    fn screen_lines(&self) -> usize {
        self.lines as usize
    }

    fn columns(&self) -> usize {
        self.columns as usize
    }

    fn last_column(&self) -> alacritty_terminal::index::Column {
        alacritty_terminal::index::Column(self.columns.saturating_sub(1) as usize)
    }

    fn bottommost_line(&self) -> alacritty_terminal::index::Line {
        alacritty_terminal::index::Line((self.lines as i32) - 1)
    }

    fn topmost_line(&self) -> alacritty_terminal::index::Line {
        alacritty_terminal::index::Line(0)
    }
}

/// Information about a cell to render
#[derive(Debug, Clone)]
pub struct RenderCell {
    pub column: usize,
    pub line: usize,
    pub character: char,
    pub fg: AnsiColor,
    pub bg: AnsiColor,
    pub flags: CellFlags,
}

/// Cursor information for rendering
#[derive(Debug, Clone)]
pub struct CursorInfo {
    pub column: usize,
    pub line: usize,
    pub shape: CursorShape,
    pub visible: bool,
}

/// Terminal backend wrapping alacritty_terminal
pub struct TerminalBackend {
    term: Arc<Mutex<Term<EventProxy>>>,
    processor: Mutex<Processor>,
    size: TerminalSize,
}

impl TerminalBackend {
    /// Create a new terminal backend with the given size
    pub fn new(size: TerminalSize) -> Self {
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let event_proxy = EventProxy::new(event_tx);

        // Create terminal config with scrollback history
        let config = TermConfig {
            scrolling_history: size.history_size,
            ..TermConfig::default()
        };

        // Create the terminal
        let term = Term::new(config, &size, event_proxy);

        Self {
            term: Arc::new(Mutex::new(term)),
            processor: Mutex::new(Processor::new()),
            size,
        }
    }

    /// Get a clone of the term for rendering
    pub fn term(&self) -> Arc<Mutex<Term<EventProxy>>> {
        self.term.clone()
    }

    /// Get the terminal size
    pub fn size(&self) -> TerminalSize {
        self.size
    }

    /// Process input bytes from PTY/SSH
    pub fn process_input(&self, bytes: &[u8]) {
        let mut term = self.term.lock();
        let mut processor = self.processor.lock();

        for byte in bytes {
            processor.advance(&mut *term, *byte);
        }
    }

    /// Resize the terminal to new dimensions
    pub fn resize(&mut self, cols: u16, rows: u16) {
        // Enforce minimum size
        let cols = cols.max(10);
        let rows = rows.max(3);

        // Only resize if actually changed
        if cols == self.size.columns && rows == self.size.lines {
            return;
        }

        self.size.columns = cols;
        self.size.lines = rows;

        let mut term = self.term.lock();
        term.resize(self.size);
    }
}
