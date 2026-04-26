//! Terminal backend using alacritty_terminal
//!
//! This module wraps the alacritty_terminal Term for use with iced.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, CursorShape, NamedColor, Processor, Rgb};
use iced::Color;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::colors::{ANSI_COLORS, DEFAULT_BG, DEFAULT_FG};
use crate::theme::TerminalColors;

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
    /// Write response back to the PTY (e.g. device attribute queries)
    PtyWrite(Vec<u8>),
    /// Terminal exited
    Exit,
    /// Wakeup (content changed)
    Wakeup,
}

/// Event proxy that forwards alacritty events to our channel
#[derive(Clone)]
pub struct EventProxy {
    sender: mpsc::Sender<TerminalEvent>,
    colors: Arc<Mutex<TerminalColors>>,
    window_size: Arc<Mutex<WindowSize>>,
}

impl EventProxy {
    pub fn new(
        sender: mpsc::Sender<TerminalEvent>,
        colors: Arc<Mutex<TerminalColors>>,
        window_size: Arc<Mutex<WindowSize>>,
    ) -> Self {
        Self {
            sender,
            colors,
            window_size,
        }
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
            Event::ColorRequest(index, format) => {
                let colors = *self.colors.lock();
                TerminalEvent::PtyWrite(format(osc_color_for_index(index, &colors)).into_bytes())
            }
            Event::TextAreaSizeRequest(format) => {
                let window_size = *self.window_size.lock();
                TerminalEvent::PtyWrite(format(window_size).into_bytes())
            }
            Event::PtyWrite(text) => TerminalEvent::PtyWrite(text.into_bytes()),
            _ => return, // Ignore other events for now
        };
        if let Err(error) = self.sender.try_send(terminal_event) {
            tracing::debug!("Terminal event dropped: {}", error);
        }
    }
}

fn iced_to_rgb(color: Color) -> Rgb {
    fn component(value: f32) -> u8 {
        (value.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    Rgb {
        r: component(color.r),
        g: component(color.g),
        b: component(color.b),
    }
}

fn dim_color(color: Color) -> Color {
    Color::from_rgba(color.r * 0.66, color.g * 0.66, color.b * 0.66, color.a)
}

fn indexed_color(index: usize) -> Option<Color> {
    let idx = u8::try_from(index).ok()?;
    if idx < 16 {
        Some(ANSI_COLORS[idx as usize])
    } else if idx < 232 {
        let idx = idx - 16;
        let r = (idx / 36) % 6;
        let g = (idx / 6) % 6;
        let b = idx % 6;
        Some(Color::from_rgb(
            if r == 0 {
                0.0
            } else {
                (r as f32 * 40.0 + 55.0) / 255.0
            },
            if g == 0 {
                0.0
            } else {
                (g as f32 * 40.0 + 55.0) / 255.0
            },
            if b == 0 {
                0.0
            } else {
                (b as f32 * 40.0 + 55.0) / 255.0
            },
        ))
    } else {
        let gray = (idx - 232) as f32 * 10.0 + 8.0;
        let v = gray / 255.0;
        Some(Color::from_rgb(v, v, v))
    }
}

fn osc_color_for_index(index: usize, colors: &TerminalColors) -> Rgb {
    let color = match index {
        0..=15 => colors.ansi[index],
        index @ 16..=255 => indexed_color(index).unwrap_or(colors.foreground),
        index if index == NamedColor::Foreground as usize => colors.foreground,
        index if index == NamedColor::Background as usize => colors.background,
        index if index == NamedColor::Cursor as usize => colors.cursor,
        index if index == NamedColor::BrightForeground as usize => colors.ansi[15],
        index if index == NamedColor::DimForeground as usize => dim_color(colors.foreground),
        index if index == NamedColor::DimBlack as usize => dim_color(colors.ansi[0]),
        index if index == NamedColor::DimRed as usize => dim_color(colors.ansi[1]),
        index if index == NamedColor::DimGreen as usize => dim_color(colors.ansi[2]),
        index if index == NamedColor::DimYellow as usize => dim_color(colors.ansi[3]),
        index if index == NamedColor::DimBlue as usize => dim_color(colors.ansi[4]),
        index if index == NamedColor::DimMagenta as usize => dim_color(colors.ansi[5]),
        index if index == NamedColor::DimCyan as usize => dim_color(colors.ansi[6]),
        index if index == NamedColor::DimWhite as usize => dim_color(colors.ansi[7]),
        _ => colors.foreground,
    };

    iced_to_rgb(color)
}

/// Terminal dimensions
#[derive(Debug, Clone, Copy)]
pub struct TerminalSize {
    pub columns: u16,
    pub lines: u16,
    pub history_size: usize,
}

/// Default scrollback history size (number of lines to keep)
const DEFAULT_HISTORY_SIZE: usize = 10000;

impl TerminalSize {
    pub fn new(columns: u16, lines: u16) -> Self {
        Self {
            columns,
            lines,
            history_size: DEFAULT_HISTORY_SIZE,
        }
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
    pub zerowidth: String,
    pub fg: AnsiColor,
    pub bg: AnsiColor,
    pub flags: CellFlags,
    pub constraint_width: u8,
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
    render_epoch: Arc<AtomicU64>,
    colors: Arc<Mutex<TerminalColors>>,
    window_size: Arc<Mutex<WindowSize>>,
}

impl TerminalBackend {
    /// Create a new terminal backend with the given size
    pub fn new(size: TerminalSize) -> (Self, mpsc::Receiver<TerminalEvent>) {
        let (event_tx, event_rx) = mpsc::channel(256);
        let colors = Arc::new(Mutex::new(TerminalColors {
            foreground: DEFAULT_FG,
            background: DEFAULT_BG,
            cursor: DEFAULT_FG,
            ansi: ANSI_COLORS,
        }));
        let window_size = Arc::new(Mutex::new(WindowSize {
            num_lines: size.lines,
            num_cols: size.columns,
            cell_width: 1,
            cell_height: 1,
        }));
        let event_proxy = EventProxy::new(event_tx, colors.clone(), window_size.clone());
        let render_epoch = Arc::new(AtomicU64::new(1));

        // Create terminal config with scrollback history
        let config = TermConfig {
            scrolling_history: size.history_size,
            ..TermConfig::default()
        };

        // Create the terminal
        let term = Term::new(config, &size, event_proxy);

        let backend = Self {
            term: Arc::new(Mutex::new(term)),
            processor: Mutex::new(Processor::new()),
            size,
            render_epoch,
            colors,
            window_size,
        };

        (backend, event_rx)
    }

    /// Get a clone of the term for rendering
    pub fn term(&self) -> Arc<Mutex<Term<EventProxy>>> {
        self.term.clone()
    }

    /// Render version for change detection (incremented on output/resize).
    pub fn render_epoch(&self) -> Arc<AtomicU64> {
        self.render_epoch.clone()
    }

    /// Set the palette used for terminal color query responses.
    pub fn set_colors(&self, colors: TerminalColors) {
        *self.colors.lock() = colors;
    }

    /// Set the terminal cell dimensions used for text-area pixel-size reports.
    pub fn set_cell_size(&self, cell_width: f32, cell_height: f32) {
        let mut window_size = self.window_size.lock();
        window_size.cell_width = cell_width.round().clamp(1.0, u16::MAX as f32) as u16;
        window_size.cell_height = cell_height.round().clamp(1.0, u16::MAX as f32) as u16;
    }

    /// Get the terminal size
    /// Process input bytes from PTY/SSH
    pub fn process_input(&self, bytes: &[u8]) {
        let mut term = self.term.lock();
        let mut processor = self.processor.lock();

        processor.advance(&mut *term, bytes);
        if !bytes.is_empty() {
            self.render_epoch.fetch_add(1, Ordering::Relaxed);
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
        {
            let mut window_size = self.window_size.lock();
            window_size.num_cols = cols;
            window_size.num_lines = rows;
        }

        let mut term = self.term.lock();
        term.resize(self.size);
        self.render_epoch.fetch_add(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use alacritty_terminal::index::{Column, Line};

    #[test]
    fn process_input_preserves_utf8_box_drawing_cells() {
        let (backend, _event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));

        backend.process_input("┌─┐│x│└─┘".as_bytes());

        let term = backend.term.lock();
        let grid = term.grid();
        let line = &grid[Line(0)];

        assert_eq!(line[Column(0)].c, '┌');
        assert_eq!(line[Column(1)].c, '─');
        assert_eq!(line[Column(2)].c, '┐');
        assert_eq!(line[Column(3)].c, '│');
        assert_eq!(line[Column(4)].c, 'x');
        assert_eq!(line[Column(5)].c, '│');
        assert_eq!(line[Column(6)].c, '└');
        assert_eq!(line[Column(7)].c, '─');
        assert_eq!(line[Column(8)].c, '┘');
    }

    #[test]
    fn osc_color_request_uses_terminal_theme_colors() {
        let colors = Theme::portal_default().terminal;

        assert_eq!(
            osc_color_for_index(NamedColor::Foreground as usize, &colors),
            Rgb {
                r: 0xe6,
                g: 0xe6,
                b: 0xe6,
            }
        );
        assert_eq!(
            osc_color_for_index(NamedColor::Background as usize, &colors),
            Rgb {
                r: 0x1e,
                g: 0x1e,
                b: 0x2e,
            }
        );
        assert_eq!(
            osc_color_for_index(NamedColor::Cursor as usize, &colors),
            Rgb {
                r: 0xe6,
                g: 0xe6,
                b: 0xe6,
            }
        );
    }

    #[test]
    fn osc_color_request_supports_indexed_palette_queries() {
        let colors = Theme::portal_default().terminal;

        assert_eq!(
            osc_color_for_index(12, &colors),
            Rgb {
                r: 0x7a,
                g: 0xa2,
                b: 0xff,
            }
        );
        assert_eq!(osc_color_for_index(16, &colors), Rgb { r: 0, g: 0, b: 0 });
        assert_eq!(
            osc_color_for_index(231, &colors),
            Rgb {
                r: 255,
                g: 255,
                b: 255,
            }
        );
    }

    #[test]
    fn osc_color_request_can_match_noctalia_ghostty_palette() {
        let colors = Theme::noctalia().terminal;

        assert_eq!(
            osc_color_for_index(NamedColor::Foreground as usize, &colors),
            Rgb {
                r: 0xcd,
                g: 0xd6,
                b: 0xf4,
            }
        );
        assert_eq!(
            osc_color_for_index(NamedColor::Background as usize, &colors),
            Rgb {
                r: 0x1e,
                g: 0x1e,
                b: 0x2e,
            }
        );
        assert_eq!(
            osc_color_for_index(5, &colors),
            Rgb {
                r: 0xf5,
                g: 0xc2,
                b: 0xe7,
            }
        );
        assert_eq!(
            osc_color_for_index(12, &colors),
            Rgb {
                r: 0x74,
                g: 0xa8,
                b: 0xfc,
            }
        );
    }

    #[test]
    fn process_input_answers_osc_color_query() {
        let (backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));
        backend.set_colors(Theme::portal_default().terminal);

        backend.process_input(b"\x1b]10;?\x1b\\");

        let event = event_rx.try_recv().expect("OSC color response event");
        assert!(matches!(
            event,
            TerminalEvent::PtyWrite(bytes)
                if bytes == b"\x1b]10;rgb:e6e6/e6e6/e6e6\x1b\\"
        ));
    }

    #[test]
    fn process_input_answers_text_area_pixel_query() {
        let (mut backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));
        backend.set_cell_size(9.0, 18.0);
        backend.resize(120, 40);

        backend.process_input(b"\x1b[14t");

        let event = event_rx.try_recv().expect("text area size response event");
        assert!(matches!(
            event,
            TerminalEvent::PtyWrite(bytes) if bytes == b"\x1b[4;720;1080t"
        ));
    }
}
