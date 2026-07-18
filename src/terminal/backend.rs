//! Terminal backend using alacritty_terminal
//!
//! This module wraps the alacritty_terminal Term for use with iced.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Config as TermConfig;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape, NamedColor, Processor, Rgb};
use iced::Color;
use iced::advanced::text::Shaping;
use parking_lot::Mutex;
use tokio::sync::mpsc;

use super::colors::{ANSI_COLORS, DEFAULT_BG, DEFAULT_FG};
use crate::theme::TerminalColors;

const OSC_NOTIFICATION_BUFFER_LIMIT: usize = 16 * 1024;
const COMMAND_FINISH_NOTIFICATION_THRESHOLD: Duration = Duration::from_secs(5);
const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";

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
    /// Desktop notification requested by the terminal stream.
    Notification { title: String, body: String },
    /// Shell integration reported that a command finished.
    CommandFinished {
        exit_status: Option<i32>,
        duration: Duration,
    },
    /// Shell integration (OSC 7) reported the working directory.
    CwdChanged(std::path::PathBuf),
    /// Write response back to the PTY (e.g. device attribute queries)
    PtyWrite(Vec<u8>),
    /// Terminal exited
    Exit,
    /// Wakeup (content changed)
    Wakeup,
}

/// Convert clipboard text to terminal input bytes, honoring negotiated paste mode.
pub fn paste_bytes_for_mode(text: &str, mode: &TermMode) -> Vec<u8> {
    let bytes = text.as_bytes();
    if !mode.contains(TermMode::BRACKETED_PASTE) {
        return bytes.to_vec();
    }

    let mut pasted =
        Vec::with_capacity(BRACKETED_PASTE_START.len() + bytes.len() + BRACKETED_PASTE_END.len());
    pasted.extend_from_slice(BRACKETED_PASTE_START);
    pasted.extend_from_slice(bytes);
    pasted.extend_from_slice(BRACKETED_PASTE_END);
    pasted
}

/// Event proxy that forwards alacritty events to our channel
#[derive(Clone)]
pub struct EventProxy {
    sender: mpsc::Sender<TerminalEvent>,
    colors: Arc<Mutex<TerminalColors>>,
    window_size: Arc<Mutex<WindowSize>>,
    muted: Arc<AtomicBool>,
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
            muted: Arc::new(AtomicBool::new(false)),
        }
    }

    fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::Relaxed);
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        if self.muted.load(Ordering::Relaxed) {
            return;
        }

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

/// Information about a cell to render.
///
/// Colors are fully resolved (theme palette, DIM, and INVERSE swap applied) at
/// cache-refresh time so the draw passes can read them without conversions.
#[derive(Debug, Clone)]
pub struct RenderCell {
    pub column: usize,
    pub line: usize,
    pub character: char,
    /// Composed display content (`character` followed by zerowidth combining
    /// chars). `None` for the common case of a plain single-`char` cell.
    pub content: Option<String>,
    /// Resolved foreground color (INVERSE already applied).
    pub fg: Color,
    /// Resolved background color (INVERSE already applied).
    pub bg: Color,
    /// Whether `bg` differs from the default terminal background.
    pub draw_bg: bool,
    pub flags: CellFlags,
    pub constraint_width: u8,
    /// Precomputed shaping strategy: `Basic` for plain ASCII content,
    /// `Advanced` when non-ASCII or zerowidth chars require it.
    pub shaping: Shaping,
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
    event_sender: mpsc::Sender<TerminalEvent>,
    notification_parser: Mutex<OscNotificationParser>,
    size: TerminalSize,
    render_epoch: Arc<AtomicU64>,
    colors: Arc<Mutex<TerminalColors>>,
    window_size: Arc<Mutex<WindowSize>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TerminalNotification {
    title: String,
    body: String,
}

#[derive(Debug, Default)]
struct OscNotificationParser {
    buffer: Vec<u8>,
    command_started_at: Option<Instant>,
}

impl OscNotificationParser {
    fn push(&mut self, bytes: &[u8]) -> Vec<TerminalEvent> {
        self.buffer.extend_from_slice(bytes);

        let mut events = Vec::new();
        let mut search_from = 0;
        let mut drain_to = 0;

        while let Some((start, start_len)) = find_osc_start(&self.buffer, search_from) {
            if let Some((end, terminator_len)) =
                find_osc_terminator(&self.buffer, start + start_len)
            {
                let osc = self.buffer[start + start_len..end].to_vec();
                if let Some(notification) = parse_osc_notification(&osc) {
                    events.push(TerminalEvent::Notification {
                        title: notification.title,
                        body: notification.body,
                    });
                }

                if let Some(event) = self.parse_osc_command_marker(&osc) {
                    events.push(event);
                }

                if let Some(event) = parse_osc_cwd(&osc) {
                    events.push(event);
                }

                search_from = end + terminator_len;
                drain_to = search_from;
            } else {
                drain_to = start;
                break;
            }
        }

        if search_from >= self.buffer.len() {
            drain_to = self.buffer.len();
        }

        if drain_to > 0 {
            self.buffer.drain(..drain_to);
        }

        if self.buffer.len() > OSC_NOTIFICATION_BUFFER_LIMIT {
            let keep_from = self.buffer.len() - OSC_NOTIFICATION_BUFFER_LIMIT;
            self.buffer.drain(..keep_from);
        }

        events
    }

    fn parse_osc_command_marker(&mut self, bytes: &[u8]) -> Option<TerminalEvent> {
        let content = String::from_utf8_lossy(bytes);
        let mut parts = content.split(';');

        if parts.next() != Some("133") {
            return None;
        }

        match parts.next() {
            Some("C") => {
                self.command_started_at = Some(Instant::now());
                None
            }
            Some("D") => {
                let started_at = self.command_started_at.take()?;
                let duration = started_at.elapsed();
                if duration < COMMAND_FINISH_NOTIFICATION_THRESHOLD {
                    return None;
                }

                let exit_status = parts.next().and_then(|status| status.parse::<i32>().ok());
                Some(TerminalEvent::CommandFinished {
                    exit_status,
                    duration,
                })
            }
            _ => None,
        }
    }
}

fn find_osc_start(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == 0x9d {
            return Some((i, 1));
        }

        if bytes[i] == 0x1b && bytes.get(i + 1) == Some(&b']') {
            return Some((i, 2));
        }

        i += 1;
    }

    None
}

fn find_osc_terminator(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i < bytes.len() {
        match bytes[i] {
            0x07 | 0x9c => return Some((i, 1)),
            0x1b if bytes.get(i + 1) == Some(&b'\\') => return Some((i, 2)),
            _ => i += 1,
        }
    }

    None
}

fn parse_osc_notification(bytes: &[u8]) -> Option<TerminalNotification> {
    let content = String::from_utf8_lossy(bytes);

    if let Some(message) = content.strip_prefix("9;") {
        // OSC 9;4 is a progress-reporting sequence, not a notification.
        if message.starts_with("4;") {
            return None;
        }

        let body = sanitize_notification_text(message);
        return (!body.is_empty()).then(|| TerminalNotification {
            title: "Terminal notification".to_string(),
            body,
        });
    }

    let mut parts = content.splitn(4, ';');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("777"), Some("notify"), Some(title), Some(body)) => {
            let title = sanitize_notification_text(title);
            let body = sanitize_notification_text(body);

            (!title.is_empty() || !body.is_empty()).then_some(TerminalNotification {
                title: if title.is_empty() {
                    "Terminal notification".to_string()
                } else {
                    title
                },
                body,
            })
        }
        _ => None,
    }
}

/// Parse an OSC 7 working-directory report (`7;file://host/path`). The host
/// part is ignored: for SSH sessions it names the remote host the session is
/// already talking to.
fn parse_osc_cwd(bytes: &[u8]) -> Option<TerminalEvent> {
    let content = String::from_utf8_lossy(bytes);
    let uri = content.strip_prefix("7;")?;
    let rest = uri.strip_prefix("file://")?;
    let path_start = rest.find('/')?;
    let path = crate::terminal::links::percent_decode(&rest[path_start..]);
    if path.is_empty() || path.chars().any(char::is_control) {
        return None;
    }
    Some(TerminalEvent::CwdChanged(std::path::PathBuf::from(path)))
}

fn sanitize_notification_text(text: &str) -> String {
    const MAX_NOTIFICATION_TEXT_CHARS: usize = 512;

    text.chars()
        .filter(|ch| !ch.is_control() || *ch == '\n' || *ch == '\t')
        .take(MAX_NOTIFICATION_TEXT_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
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
        let event_proxy = EventProxy::new(event_tx.clone(), colors.clone(), window_size.clone());
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
            event_sender: event_tx,
            notification_parser: Mutex::new(OscNotificationParser::default()),
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

    /// Get the current terminal grid size.
    pub fn size(&self) -> (u16, u16) {
        (self.size.columns, self.size.lines)
    }

    /// Process input bytes from PTY/SSH
    pub fn process_input(&self, bytes: &[u8]) {
        let events = self.notification_parser.lock().push(bytes);
        for event in events {
            if let Err(error) = self.event_sender.try_send(event) {
                tracing::debug!("Terminal notification event dropped: {}", error);
            }
        }

        let mut term = self.term.lock();
        let mut processor = self.processor.lock();

        processor.advance(&mut *term, bytes);
        if !bytes.is_empty() {
            self.render_epoch.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Replace the visible terminal state with the final rendered state of a byte stream.
    ///
    /// This is used for Portal Hub resume snapshots: the raw log tail is parsed offscreen,
    /// then swapped into view once so reconnect does not visibly replay the output.
    pub fn replace_with_rendered_snapshot(&self, bytes: &[u8]) {
        let event_proxy = EventProxy::new(
            self.event_sender.clone(),
            self.colors.clone(),
            self.window_size.clone(),
        );
        event_proxy.set_muted(true);

        let config = TermConfig {
            scrolling_history: self.size.history_size,
            ..TermConfig::default()
        };
        let mut snapshot = Term::new(config, &self.size, event_proxy.clone());
        let mut snapshot_processor: Processor = Processor::new();
        snapshot_processor.advance(&mut snapshot, bytes);
        event_proxy.set_muted(false);

        *self.term.lock() = snapshot;
        *self.processor.lock() = Processor::new();
        self.render_epoch.fetch_add(1, Ordering::Relaxed);
    }

    /// Current render epoch value (see [`Self::render_epoch`]).
    pub fn current_epoch(&self) -> u64 {
        self.render_epoch.load(Ordering::Relaxed)
    }

    /// Find literal search matches in the whole buffer (scrollback + viewport).
    pub fn search_matches(
        &self,
        query: &str,
        case_sensitive: bool,
        max_matches: usize,
    ) -> Vec<super::search::Match> {
        let term = self.term.lock();
        super::search::find_matches(&term, query, case_sensitive, max_matches)
    }

    /// Bottommost visible grid line of the current viewport.
    pub fn viewport_bottom_line(&self) -> i32 {
        let term = self.term.lock();
        term.screen_lines() as i32 - 1 - term.grid().display_offset() as i32
    }

    /// Scroll the display so the given grid line is visible, roughly centering
    /// it when it is currently outside the viewport. No-op when already visible.
    pub fn scroll_to_line(&self, line: alacritty_terminal::index::Line) {
        let mut term = self.term.lock();
        let screen_lines = term.screen_lines() as i32;
        let display_offset = term.grid().display_offset() as i32;
        let viewport_line = line.0 + display_offset;
        if (0..screen_lines).contains(&viewport_line) {
            return;
        }

        // viewport_line = grid_line + display_offset; center the target line.
        // `scroll_display` clamps the resulting offset to the valid range.
        let target_offset = screen_lines / 2 - line.0;
        term.scroll_display(alacritty_terminal::grid::Scroll::Delta(
            target_offset - display_offset,
        ));
        drop(term);
        self.render_epoch.fetch_add(1, Ordering::Relaxed);
    }

    /// Resize the terminal to new dimensions
    pub fn resize(&mut self, cols: u16, rows: u16) -> bool {
        // Enforce minimum size
        let cols = cols.max(10);
        let rows = rows.max(3);

        // Only resize if actually changed
        if cols == self.size.columns && rows == self.size.lines {
            return false;
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
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use alacritty_terminal::index::{Column, Line};
    use tokio::sync::mpsc::error::TryRecvError;

    fn drain_notification(
        event_rx: &mut mpsc::Receiver<TerminalEvent>,
    ) -> Option<(String, String)> {
        loop {
            match event_rx.try_recv() {
                Ok(TerminalEvent::Notification { title, body }) => return Some((title, body)),
                Ok(_) => continue,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return None,
            }
        }
    }

    fn drain_command_finished(
        event_rx: &mut mpsc::Receiver<TerminalEvent>,
    ) -> Option<(Option<i32>, Duration)> {
        loop {
            match event_rx.try_recv() {
                Ok(TerminalEvent::CommandFinished {
                    exit_status,
                    duration,
                }) => return Some((exit_status, duration)),
                Ok(_) => continue,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return None,
            }
        }
    }

    #[test]
    fn paste_bytes_are_raw_without_bracketed_paste_mode() {
        assert_eq!(
            paste_bytes_for_mode("one\ntwo", &TermMode::default()),
            b"one\ntwo".to_vec()
        );
    }

    #[test]
    fn paste_bytes_are_wrapped_in_bracketed_paste_mode() {
        assert_eq!(
            paste_bytes_for_mode(
                "one\ntwo",
                &(TermMode::default() | TermMode::BRACKETED_PASTE)
            ),
            b"\x1b[200~one\ntwo\x1b[201~".to_vec()
        );
    }

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
    fn replace_with_rendered_snapshot_seeds_visible_grid_without_replay_events() {
        let (backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(20, 3));

        backend.replace_with_rendered_snapshot(b"snapshot");

        assert!(matches!(event_rx.try_recv(), Err(TryRecvError::Empty)));
        {
            let term = backend.term.lock();
            let line = &term.grid()[Line(0)];
            assert_eq!(line[Column(0)].c, 's');
            assert_eq!(line[Column(7)].c, 't');
        }

        backend.process_input(b"\r\nlive");

        let term = backend.term.lock();
        let grid = term.grid();
        assert_eq!(grid[Line(0)][Column(0)].c, 's');
        assert_eq!(grid[Line(1)][Column(0)].c, 'l');
        assert_eq!(grid[Line(1)][Column(3)].c, 'e');
    }

    #[test]
    fn process_input_emits_osc9_notification() {
        let (backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));

        backend.process_input(b"\x1b]9;Codex finished\x07");

        assert_eq!(
            drain_notification(&mut event_rx),
            Some((
                "Terminal notification".to_string(),
                "Codex finished".to_string()
            ))
        );
    }

    #[test]
    fn process_input_emits_osc777_notification() {
        let (backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));

        backend.process_input(b"\x1b]777;notify;Codex;Task complete\x07");

        assert_eq!(
            drain_notification(&mut event_rx),
            Some(("Codex".to_string(), "Task complete".to_string()))
        );
    }

    #[test]
    fn process_input_emits_split_osc777_notification_with_st() {
        let (backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));

        backend.process_input(b"\x1b]777;notify;Codex;");
        assert_eq!(drain_notification(&mut event_rx), None);

        backend.process_input(b"Task complete\x1b\\");

        assert_eq!(
            drain_notification(&mut event_rx),
            Some(("Codex".to_string(), "Task complete".to_string()))
        );
    }

    #[test]
    fn process_input_emits_cwd_change_for_osc7() {
        let (backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));

        backend.process_input(b"\x1b]7;file://beast/root/Code/my%20project\x07");

        let mut cwd = None;
        while let Ok(event) = event_rx.try_recv() {
            if let TerminalEvent::CwdChanged(path) = event {
                cwd = Some(path);
            }
        }
        assert_eq!(cwd, Some(std::path::PathBuf::from("/root/Code/my project")));
    }

    #[test]
    fn process_input_ignores_malformed_osc7() {
        let (backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));

        backend.process_input(b"\x1b]7;not-a-uri\x07");
        backend.process_input(b"\x1b]7;file://host-only\x07");

        while let Ok(event) = event_rx.try_recv() {
            assert!(!matches!(event, TerminalEvent::CwdChanged(_)));
        }
    }

    #[test]
    fn process_input_ignores_osc9_progress() {
        let (backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));

        backend.process_input(b"\x1b]9;4;1;50\x07");

        assert_eq!(drain_notification(&mut event_rx), None);
    }

    #[test]
    fn process_input_emits_command_finished_for_osc133() {
        let (backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));

        backend.process_input(b"\x1b]133;C\x07");
        backend.notification_parser.lock().command_started_at =
            Some(Instant::now() - COMMAND_FINISH_NOTIFICATION_THRESHOLD - Duration::from_secs(1));
        backend.process_input(b"\x1b]133;D;0\x07");

        let (exit_status, duration) = drain_command_finished(&mut event_rx).unwrap();
        assert_eq!(exit_status, Some(0));
        assert!(duration >= COMMAND_FINISH_NOTIFICATION_THRESHOLD);
    }

    #[test]
    fn process_input_suppresses_short_osc133_command() {
        let (backend, mut event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));

        backend.process_input(b"\x1b]133;C\x07");
        backend.process_input(b"\x1b]133;D;0\x07");

        assert_eq!(drain_command_finished(&mut event_rx), None);
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
    fn scroll_to_line_reveals_scrollback_and_is_stable_when_visible() {
        let (backend, _event_rx) = TerminalBackend::new(TerminalSize::new(10, 3));
        for i in 0..10 {
            backend.process_input(format!("line {i}\r\n").as_bytes());
        }

        let epoch_before = backend.current_epoch();

        // Line -8 is the oldest history line; it must become visible.
        backend.scroll_to_line(Line(-8));
        {
            let term = backend.term.lock();
            let display_offset = term.grid().display_offset() as i32;
            let viewport_line = -8 + display_offset;
            assert!((0..3).contains(&viewport_line));
        }
        assert_ne!(backend.current_epoch(), epoch_before);

        // Scrolling to an already-visible line neither moves nor re-renders.
        let epoch = backend.current_epoch();
        let offset = backend.term.lock().grid().display_offset();
        backend.scroll_to_line(Line(-8));
        assert_eq!(backend.term.lock().grid().display_offset(), offset);
        assert_eq!(backend.current_epoch(), epoch);
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
