//! Custom iced widget for terminal rendering
//!
//! This implements the iced Widget trait for rendering terminal content.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::CursorShape;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::keyboard::{self, Key, Modifiers};
use iced::mouse::{self, Cursor};
use iced::{Background, Border, Color, Element, Event, Length, Rectangle, Shadow, Size};
use parking_lot::Mutex;

use super::backend::{CursorInfo, EventProxy, RenderCell};
use super::block_elements::render_block_element;
use super::colors::{DEFAULT_BG, DEFAULT_FG, ansi_to_iced_themed};
use crate::fonts::{JETBRAINS_MONO_NERD, TerminalFont};
use crate::keybindings::{AppAction, KeybindingsConfig};
use crate::theme::TerminalColors;

/// Left padding for terminal content (matches Termius style)
const TERMINAL_PADDING_LEFT: f32 = 12.0;

/// Terminal widget for iced
pub struct TerminalWidget<'a, Message> {
    term: Arc<Mutex<Term<EventProxy>>>,
    on_input: Box<dyn Fn(Vec<u8>) -> Message + 'a>,
    on_resize: Option<Box<dyn Fn(u16, u16) -> Message + 'a>>,
    font_size: f32,
    font: iced::Font,
    terminal_colors: Option<TerminalColors>,
    render_epoch: Option<Arc<AtomicU64>>,
    keybindings: KeybindingsConfig,
}

impl<'a, Message> TerminalWidget<'a, Message> {
    /// Create a new terminal widget
    pub fn new(
        term: Arc<Mutex<Term<EventProxy>>>,
        on_input: impl Fn(Vec<u8>) -> Message + 'a,
    ) -> Self {
        Self {
            term,
            on_input: Box::new(on_input),
            on_resize: None,
            font_size: 9.0,
            font: JETBRAINS_MONO_NERD,
            terminal_colors: None,
            render_epoch: None,
            keybindings: KeybindingsConfig::default(),
        }
    }

    /// Set font size
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Set terminal font
    pub fn font(mut self, font: TerminalFont) -> Self {
        self.font = font.to_iced_font();
        self
    }

    /// Set terminal colors from theme
    pub fn terminal_colors(mut self, colors: TerminalColors) -> Self {
        self.terminal_colors = Some(colors);
        self
    }

    /// Set render epoch for change detection.
    pub fn render_epoch(mut self, epoch: Arc<AtomicU64>) -> Self {
        self.render_epoch = Some(epoch);
        self
    }

    /// Set keybindings for terminal actions
    pub fn keybindings(mut self, keybindings: KeybindingsConfig) -> Self {
        self.keybindings = keybindings;
        self
    }

    /// Set resize callback
    pub fn on_resize(mut self, callback: impl Fn(u16, u16) -> Message + 'a) -> Self {
        self.on_resize = Some(Box::new(callback));
        self
    }

    /// Calculate cell width based on font size (JetBrains Mono aspect ratio)
    fn cell_width(&self) -> f32 {
        self.font_size * 0.6
    }

    /// Calculate cell height based on font size (line height)
    fn cell_height(&self) -> f32 {
        self.font_size * 1.4
    }

    /// Get renderable cells from the terminal
    fn get_cells(&self) -> Vec<RenderCell> {
        use alacritty_terminal::vte::ansi::NamedColor;

        let term = self.term.lock();
        let content = term.renderable_content();
        let display_offset = content.display_offset;
        let mut cells = Vec::new();

        for indexed in content.display_iter {
            let cell = &indexed.cell;

            // Convert grid line to screen line by adding display_offset
            // When scrolled back, cells have negative line numbers
            // e.g., with display_offset=24, line=-24 should render at screen line 0
            let screen_line = indexed.point.line.0 + display_offset as i32;

            // Skip if outside visible screen
            if screen_line < 0 {
                continue;
            }
            let line = screen_line as usize;

            // Skip wide character spacer cells (placeholder for 2nd column of wide chars)
            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }

            // Include cells with content or non-default background
            if cell.c != ' '
                || cell.bg != alacritty_terminal::vte::ansi::Color::Named(NamedColor::Background)
                || !cell.flags.is_empty()
            {
                cells.push(RenderCell {
                    column: indexed.point.column.0,
                    line,
                    character: cell.c,
                    fg: cell.fg,
                    bg: cell.bg,
                    flags: cell.flags,
                });
            }
        }

        cells
    }

    /// Get cursor information
    fn get_cursor(&self) -> Option<CursorInfo> {
        let term = self.term.lock();
        let content = term.renderable_content();
        let cursor = content.cursor;

        // Convert grid line to screen line by adding display_offset
        let screen_line = cursor.point.line.0 + content.display_offset as i32;

        // Skip cursor if outside visible screen (scrolled out of view)
        if screen_line < 0 {
            return None;
        }

        Some(CursorInfo {
            column: cursor.point.column.0,
            line: screen_line as usize,
            shape: cursor.shape,
            visible: cursor.shape != CursorShape::Hidden,
        })
    }

    /// Convert pixel coordinates to terminal cell coordinates (screen-relative)
    fn pixel_to_cell(&self, bounds: &Rectangle, position: iced::Point) -> Option<(usize, usize)> {
        if !bounds.contains(position) {
            return None;
        }
        // Account for left padding when converting to cell coordinates
        let col =
            ((position.x - bounds.x - TERMINAL_PADDING_LEFT).max(0.0) / self.cell_width()) as usize;
        let row = ((position.y - bounds.y) / self.cell_height()) as usize;
        Some((col, row))
    }

    /// Convert screen coordinates to buffer-absolute coordinates
    fn screen_to_buffer(&self, screen_col: usize, screen_line: usize) -> (usize, i32) {
        let term = self.term.lock();
        let content = term.renderable_content();
        let display_offset = content.display_offset;
        
        // Buffer line = screen line - display_offset
        // When scrolled back (display_offset > 0), screen line 0 maps to negative buffer line
        let buffer_line = screen_line as i32 - display_offset as i32;
        
        (screen_col, buffer_line)
    }

    /// Convert buffer-absolute coordinates to screen coordinates
    fn buffer_to_screen(&self, buffer_col: usize, buffer_line: i32) -> Option<(usize, usize)> {
        let term = self.term.lock();
        let content = term.renderable_content();
        let display_offset = content.display_offset;
        
        // Screen line = buffer line + display_offset
        let screen_line = buffer_line + display_offset as i32;
        
        if screen_line < 0 {
            None // Off-screen (scrolled out of view above)
        } else {
            Some((buffer_col, screen_line as usize))
        }
    }

    /// Check if a character is a word boundary
    fn is_word_boundary(c: char) -> bool {
        c.is_whitespace()
            || matches!(
                c,
                '(' | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '"'
                    | '\''
                    | '<'
                    | '>'
                    | ','
                    | '.'
                    | ';'
                    | ':'
                    | '!'
                    | '?'
                    | '`'
                    | '|'
                    | '&'
                    | '='
                    | '+'
                    | '-'
                    | '*'
                    | '/'
                    | '\\'
                    | '@'
                    | '#'
                    | '$'
                    | '%'
                    | '^'
            )
    }

    /// Get line content as a vector of characters for a specific screen line
    fn get_line_chars(&self, line: usize, cols: usize) -> Vec<char> {
        let term = self.term.lock();
        let content = term.renderable_content();
        let display_offset = content.display_offset;

        let mut chars = vec![' '; cols];

        for indexed in content.display_iter {
            let screen_line = indexed.point.line.0 + display_offset as i32;
            if screen_line < 0 {
                continue;
            }
            let cell_line = screen_line as usize;
            let col = indexed.point.column.0;

            if cell_line == line && col < cols {
                chars[col] = indexed.cell.c;
            }
        }

        chars
    }

    /// Find word boundaries at a given position
    /// Returns (start_col, end_col) of the word at the position
    fn find_word_at(&self, col: usize, line: usize, cols: usize) -> (usize, usize) {
        let chars = self.get_line_chars(line, cols);

        if col >= chars.len() {
            return (col, col);
        }

        // If clicking on a boundary character, select just that character
        if Self::is_word_boundary(chars[col]) {
            return (col, col);
        }

        // Scan left to find word start
        let mut start = col;
        while start > 0 && !Self::is_word_boundary(chars[start - 1]) {
            start -= 1;
        }

        // Scan right to find word end
        let mut end = col;
        while end < chars.len() - 1 && !Self::is_word_boundary(chars[end + 1]) {
            end += 1;
        }

        (start, end)
    }

    /// Get selected text from the terminal using buffer-absolute coordinates
    fn get_selected_text(&self, start: (usize, i32), end: (usize, i32), cols: usize) -> String {
        // Normalize selection (ensure start comes before end)
        let (start, end) = if start.1 < end.1 || (start.1 == end.1 && start.0 <= end.0) {
            (start, end)
        } else {
            (end, start)
        };

        let term = self.term.lock();
        let content = term.renderable_content();

        // Build a grid of characters from the buffer
        // start.1 and end.1 are buffer-absolute line numbers (can be negative for scrollback)
        let rows = ((end.1 - start.1 + 1) as usize).min(1000); // Limit to prevent huge allocations
        let mut grid: Vec<Vec<char>> = vec![vec![' '; cols]; rows];

        for indexed in content.display_iter {
            let cell = &indexed.cell;

            // indexed.point.line.0 is the buffer line (signed, can be negative)
            let buffer_line = indexed.point.line.0;
            let col = indexed.point.column.0;

            // Check if within our selection range (buffer coordinates)
            if buffer_line >= start.1 && buffer_line <= end.1 && col < cols {
                let grid_line = (buffer_line - start.1) as usize;
                if grid_line < grid.len() {
                    grid[grid_line][col] = cell.c;
                }
            }
        }

        // Build result string from grid
        let mut result = String::new();
        for (i, buffer_line) in (start.1..=end.1).enumerate() {
            if i >= grid.len() {
                break;
            }

            let start_col = if buffer_line == start.1 { start.0 } else { 0 };
            let end_col = if buffer_line == end.1 {
                end.0.min(cols.saturating_sub(1))
            } else {
                cols.saturating_sub(1)
            };

            // Extract characters for this line
            let line_chars: String = grid[i][start_col..=end_col.min(grid[i].len() - 1)]
                .iter()
                .collect();

            // Trim trailing whitespace from each line
            result.push_str(line_chars.trim_end());

            // Add newline between lines (but not after the last line)
            if buffer_line < end.1 {
                result.push('\n');
            }
        }

        result
    }
}

/// Widget state stored in the tree
#[derive(Debug)]
struct TerminalState {
    is_focused: bool,
    cursor_visible: bool,
    last_size: Option<(u16, u16)>,
    scroll_pixels: f32, // Accumulated scroll pixels for trackpad
    // Cached render data (updated only when terminal content changes)
    render_cache: RefCell<RenderCache>,
    // Selection state (stored in buffer-absolute coordinates)
    selection_start: Option<(usize, i32)>, // (column, buffer_line) - buffer-absolute coords
    selection_end: Option<(usize, i32)>,   // buffer_line can be negative (scrollback)
    is_selecting: bool, // Mouse button held during drag
    last_drag_cell: Option<(usize, usize)>,
    last_drag_update: Option<std::time::Instant>,
    // Click tracking for double/triple click
    last_click_time: Option<std::time::Instant>,
    last_click_position: Option<(usize, usize)>,
    click_count: u8, // 1 = single, 2 = double, 3 = triple
    selection_mode: SelectionMode,
    // Auto-scroll during selection
    last_auto_scroll: Option<std::time::Instant>,
}

/// Selection granularity mode
#[derive(Debug, Clone, Copy, Default, PartialEq)]
enum SelectionMode {
    #[default]
    Character, // Normal character-by-character selection
    Word, // Double-click: select by word
    Line, // Triple-click: select by line
}

#[derive(Debug, Default)]
struct RenderCache {
    cells: Vec<RenderCell>,
    cursor: Option<CursorInfo>,
    epoch: u64,
    needs_refresh: bool,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            is_focused: true,
            cursor_visible: true,
            last_size: None,
            scroll_pixels: 0.0,
            render_cache: RefCell::new(RenderCache {
                needs_refresh: true,
                ..RenderCache::default()
            }),
            selection_start: None,
            selection_end: None,
            is_selecting: false,
            last_drag_cell: None,
            last_drag_update: None,
            last_click_time: None,
            last_click_position: None,
            click_count: 0,
            selection_mode: SelectionMode::Character,
            last_auto_scroll: None,
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for TerminalWidget<'_, Message>
where
    Renderer: renderer::Renderer + iced::advanced::text::Renderer<Font = iced::Font>,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fill,
            height: Length::Fill,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // Fill all available space - the resize detection will adjust the terminal grid
        layout::Node::new(limits.max())
    }

    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<TerminalState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(TerminalState {
            is_focused: true,
            cursor_visible: true,
            last_size: None,
            scroll_pixels: 0.0,
            render_cache: RefCell::new(RenderCache {
                needs_refresh: true,
                ..RenderCache::default()
            }),
            selection_start: None,
            selection_end: None,
            is_selecting: false,
            last_drag_cell: None,
            last_drag_update: None,
            last_click_time: None,
            last_click_position: None,
            click_count: 0,
            selection_mode: SelectionMode::Character,
            last_auto_scroll: None,
        })
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: Cursor,
        _viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<TerminalState>();
        let selection = (state.selection_start, state.selection_end);
        let is_focused = state.is_focused;
        let cursor_visible = state.cursor_visible;

        // Get terminal colors (from theme or defaults)
        let default_colors = TerminalColors {
            foreground: DEFAULT_FG,
            background: DEFAULT_BG,
            cursor: DEFAULT_FG,
            ansi: super::colors::ANSI_COLORS,
        };
        let colors = self.terminal_colors.as_ref().unwrap_or(&default_colors);

        // Draw background
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border::default(),
                shadow: Shadow::default(),
                snap: true,
            },
            Background::Color(colors.background),
        );

        let cell_width = self.cell_width();
        let cell_height = self.cell_height();

        // Refresh cached render data if terminal content changed.
        let mut cache = state.render_cache.borrow_mut();
        let mut needs_refresh = cache.needs_refresh;
        if let Some(epoch) = self.render_epoch.as_ref() {
            let current = epoch.load(Ordering::Relaxed);
            if current != cache.epoch {
                cache.epoch = current;
                needs_refresh = true;
            }
        } else {
            needs_refresh = true;
        }

        if needs_refresh {
            cache.cells = self.get_cells();
            cache.cursor = self.get_cursor();
            cache.needs_refresh = false;
        }

        let cached_cursor = cache.cursor.clone();
        drop(cache);

        // Draw cells
        for cell in &state.render_cache.borrow().cells {
            let x = bounds.x + TERMINAL_PADDING_LEFT + cell.column as f32 * cell_width;
            let y = bounds.y + cell.line as f32 * cell_height;

            let mut fg_color = ansi_to_iced_themed(cell.fg, colors);
            let mut bg_color = ansi_to_iced_themed(cell.bg, colors);

            if cell.flags.contains(CellFlags::INVERSE) {
                std::mem::swap(&mut fg_color, &mut bg_color);
            }

            // Draw cell background if not default (after inverse swap)
            if bg_color != colors.background {
                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x,
                            y,
                            width: cell_width,
                            height: cell_height,
                        },
                        border: Border::default(),
                        shadow: Shadow::default(),
                        snap: true,
                    },
                    Background::Color(bg_color),
                );
            }

            // Draw character
            if cell.character != ' ' && !cell.flags.contains(CellFlags::HIDDEN) {
                // Handle flags
                if cell.flags.contains(CellFlags::DIM) {
                    fg_color = Color::from_rgba(
                        fg_color.r * 0.66,
                        fg_color.g * 0.66,
                        fg_color.b * 0.66,
                        fg_color.a,
                    );
                }

                // Wide characters (e.g. CJK, emoji) occupy 2 cells
                let char_width = if cell.flags.contains(CellFlags::WIDE_CHAR) {
                    cell_width * 2.0
                } else {
                    cell_width
                };

                // Try to render block elements as rectangles for pixel-perfect rendering
                if render_block_element(
                    renderer,
                    cell.character,
                    x,
                    y,
                    cell_width,
                    cell_height,
                    fg_color,
                ) {
                    // Block element was rendered as rectangles
                } else {
                    // Draw the character using text renderer
                    let text = iced::advanced::Text {
                        content: cell.character.to_string(),
                        bounds: Size::new(char_width, cell_height),
                        size: iced::Pixels(self.font_size),
                        line_height: iced::advanced::text::LineHeight::Absolute(iced::Pixels(
                            cell_height,
                        )),
                        font: self.font,
                        align_x: iced::alignment::Horizontal::Left.into(),
                        align_y: iced::alignment::Vertical::Top,
                        shaping: iced::advanced::text::Shaping::Advanced,
                        wrapping: iced::advanced::text::Wrapping::None,
                    };

                    renderer.fill_text(text, iced::Point::new(x, y), fg_color, bounds);
                }
            }
        }

        // Draw selection highlight (convert buffer coords to screen coords for rendering)
        if let (Some(start_buf), Some(end_buf)) = selection {
            // Normalize selection (ensure start comes before end)
            let (start_buf, end_buf) = if start_buf.1 < end_buf.1 || (start_buf.1 == end_buf.1 && start_buf.0 <= end_buf.0) {
                (start_buf, end_buf)
            } else {
                (end_buf, start_buf)
            };

            let cols = (bounds.width / cell_width) as usize;
            let selection_color = Color::from_rgba(0.3, 0.5, 0.8, 0.4);

            // Convert buffer range to screen range for rendering
            let term = self.term.lock();
            let content = term.renderable_content();
            let display_offset = content.display_offset as i32;
            drop(term);

            // Iterate through buffer lines in selection
            for buffer_line in start_buf.1..=end_buf.1 {
                // Convert buffer line to screen line
                let screen_line = buffer_line + display_offset;
                
                // Skip if not visible in current viewport
                if screen_line < 0 {
                    continue;
                }
                let screen_line = screen_line as usize;

                let start_col = if buffer_line == start_buf.1 { start_buf.0 } else { 0 };
                let end_col = if buffer_line == end_buf.1 {
                    end_buf.0
                } else {
                    cols.saturating_sub(1)
                };

                let x = bounds.x + TERMINAL_PADDING_LEFT + start_col as f32 * cell_width;
                let y = bounds.y + screen_line as f32 * cell_height;
                let width = (end_col - start_col + 1) as f32 * cell_width;

                renderer.fill_quad(
                    Quad {
                        bounds: Rectangle {
                            x,
                            y,
                            width,
                            height: cell_height,
                        },
                        border: Border::default(),
                        shadow: Shadow::default(),
                        snap: true,
                    },
                    Background::Color(selection_color),
                );
            }
        }

        // Draw cursor (only if visible and in valid position)
        if is_focused && cursor_visible {
            if let Some(cursor_info) = cached_cursor {
                if cursor_info.visible {
                    let cursor_x =
                        bounds.x + TERMINAL_PADDING_LEFT + cursor_info.column as f32 * cell_width;
                    let cursor_y = bounds.y + cursor_info.line as f32 * cell_height;

                    let cursor_color = colors.cursor;

                    match cursor_info.shape {
                        CursorShape::Block => {
                            renderer.fill_quad(
                                Quad {
                                    bounds: Rectangle {
                                        x: cursor_x,
                                        y: cursor_y,
                                        width: cell_width,
                                        height: cell_height,
                                    },
                                    border: Border::default(),
                                    shadow: Shadow::default(),
                                    snap: true,
                                },
                                Background::Color(Color::from_rgba(
                                    cursor_color.r,
                                    cursor_color.g,
                                    cursor_color.b,
                                    0.7,
                                )),
                            );
                        }
                        CursorShape::Underline => {
                            renderer.fill_quad(
                                Quad {
                                    bounds: Rectangle {
                                        x: cursor_x,
                                        y: cursor_y + cell_height - 2.0,
                                        width: cell_width,
                                        height: 2.0,
                                    },
                                    border: Border::default(),
                                    shadow: Shadow::default(),
                                    snap: true,
                                },
                                Background::Color(cursor_color),
                            );
                        }
                        CursorShape::Beam => {
                            renderer.fill_quad(
                                Quad {
                                    bounds: Rectangle {
                                        x: cursor_x,
                                        y: cursor_y,
                                        width: 2.0,
                                        height: cell_height,
                                    },
                                    border: Border::default(),
                                    shadow: Shadow::default(),
                                    snap: true,
                                },
                                Background::Color(cursor_color),
                            );
                        }
                        _ => {
                            // Default to block for hidden/other
                            renderer.fill_quad(
                                Quad {
                                    bounds: Rectangle {
                                        x: cursor_x,
                                        y: cursor_y,
                                        width: cell_width,
                                        height: cell_height,
                                    },
                                    border: Border {
                                        color: cursor_color,
                                        width: 1.0,
                                        radius: 0.0.into(),
                                    },
                                    shadow: Shadow::default(),
                                    snap: true,
                                },
                                Background::Color(Color::TRANSPARENT),
                            );
                        }
                    }
                }
            }
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<TerminalState>();
        let bounds = layout.bounds();

        // Detect size changes and emit resize message
        if let Some(ref on_resize) = self.on_resize {
            // Calculate terminal dimensions from pixel bounds (accounting for padding)
            let cols = ((bounds.width - TERMINAL_PADDING_LEFT) / self.cell_width()) as u16;
            let rows = (bounds.height / self.cell_height()) as u16;

            // Enforce minimum size
            let cols = cols.max(10);
            let rows = rows.max(3);

            // Check if size changed
            let size_changed = match state.last_size {
                Some((last_cols, last_rows)) => cols != last_cols || rows != last_rows,
                None => true, // First time - emit initial size
            };

            if size_changed {
                state.last_size = Some((cols, rows));
                state.render_cache.borrow_mut().needs_refresh = true;
                shell.publish((on_resize)(cols, rows));
            }
        }

        // Double/triple click detection threshold (400ms)
        const MULTI_CLICK_THRESHOLD: std::time::Duration = std::time::Duration::from_millis(400);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                state.is_focused = true;

                if let Some(position) = cursor.position() {
                    if let Some(cell) = self.pixel_to_cell(&bounds, position) {
                        let now = std::time::Instant::now();
                        let cols = (bounds.width / self.cell_width()) as usize;

                        // Check for multi-click (same position, within time threshold)
                        let is_multi_click = state
                            .last_click_time
                            .is_some_and(|t| now.duration_since(t) < MULTI_CLICK_THRESHOLD)
                            && state.last_click_position.is_some_and(|pos| {
                                // Allow 1-cell tolerance for position
                                let col_diff = (pos.0 as i32 - cell.0 as i32).abs();
                                let row_diff = (pos.1 as i32 - cell.1 as i32).abs();
                                col_diff <= 1 && row_diff == 0
                            });

                        if is_multi_click {
                            state.click_count = (state.click_count % 3) + 1;
                        } else {
                            state.click_count = 1;
                        }

                        state.last_click_time = Some(now);
                        state.last_click_position = Some(cell);

                        // Convert screen cell to buffer coordinates
                        let (buf_col, buf_line) = self.screen_to_buffer(cell.0, cell.1);

                        match state.click_count {
                            2 => {
                                // Double-click: select word
                                state.selection_mode = SelectionMode::Word;
                                let (word_start, word_end) =
                                    self.find_word_at(cell.0, cell.1, cols);
                                let (word_start_buf, _) = self.screen_to_buffer(word_start, cell.1);
                                let (word_end_buf, _) = self.screen_to_buffer(word_end, cell.1);
                                state.selection_start = Some((word_start_buf, buf_line));
                                state.selection_end = Some((word_end_buf, buf_line));
                                state.is_selecting = true;
                                state.last_drag_cell = Some(cell);
                                shell.request_redraw();
                            }
                            3 => {
                                // Triple-click: select line
                                state.selection_mode = SelectionMode::Line;
                                state.selection_start = Some((0, buf_line));
                                state.selection_end = Some((cols.saturating_sub(1), buf_line));
                                state.is_selecting = true;
                                state.last_drag_cell = Some(cell);
                                shell.request_redraw();
                            }
                            _ => {
                                // Single click: character selection
                                state.selection_mode = SelectionMode::Character;
                                state.selection_start = Some((buf_col, buf_line));
                                state.selection_end = Some((buf_col, buf_line));
                                state.is_selecting = true;
                                state.last_drag_cell = Some(cell);
                                shell.request_redraw();
                            }
                        }
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.is_focused = false;
                // Clear selection when clicking outside
                state.selection_start = None;
                state.selection_end = None;
                state.is_selecting = false;
                state.click_count = 0;
                state.selection_mode = SelectionMode::Character;
                state.last_drag_cell = None;
                state.last_auto_scroll = None;
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                // Update selection while dragging
                if state.is_selecting {
                    // Auto-scroll zone (pixels from edge to trigger scrolling)
                    const AUTO_SCROLL_ZONE: f32 = 30.0;
                    // Minimum time between auto-scroll updates (milliseconds)
                    const AUTO_SCROLL_INTERVAL: std::time::Duration =
                        std::time::Duration::from_millis(50);

                    // Check if cursor is near viewport edges for auto-scroll
                    let should_auto_scroll = if cursor.is_over(bounds) {
                        false
                    } else {
                        // Cursor outside bounds - check if near top or bottom edge
                        position.y < bounds.y + AUTO_SCROLL_ZONE
                            || position.y > bounds.y + bounds.height - AUTO_SCROLL_ZONE
                    };

                    // Alternative: also support auto-scroll when cursor is inside but near edges
                    let edge_distance_top = position.y - bounds.y;
                    let edge_distance_bottom = bounds.y + bounds.height - position.y;
                    let near_top_edge = edge_distance_top >= 0.0 && edge_distance_top < AUTO_SCROLL_ZONE;
                    let near_bottom_edge = edge_distance_bottom >= 0.0 && edge_distance_bottom < AUTO_SCROLL_ZONE;

                    // Auto-scroll if near edges and not in alternate screen mode
                    if should_auto_scroll || near_top_edge || near_bottom_edge {
                        let can_scroll = state
                            .last_auto_scroll
                            .map(|t| t.elapsed() >= AUTO_SCROLL_INTERVAL)
                            .unwrap_or(true);

                        if can_scroll {
                            let in_alt_screen = {
                                let term = self.term.lock();
                                term.mode().contains(TermMode::ALT_SCREEN)
                            };

                            if !in_alt_screen {
                                // Determine scroll direction and amount
                                let scroll_lines = if position.y < bounds.y + AUTO_SCROLL_ZONE {
                                    // Near top - scroll up (positive delta scrolls viewport up)
                                    let distance_factor = (AUTO_SCROLL_ZONE - edge_distance_top.max(0.0)) / AUTO_SCROLL_ZONE;
                                    1.max((distance_factor * 3.0) as i32)
                                } else {
                                    // Near bottom - scroll down (negative delta scrolls viewport down)
                                    let distance_factor = (AUTO_SCROLL_ZONE - edge_distance_bottom.max(0.0)) / AUTO_SCROLL_ZONE;
                                    -(1.max((distance_factor * 3.0) as i32))
                                };

                                let mut term = self.term.lock();
                                term.scroll_display(Scroll::Delta(scroll_lines));
                                drop(term);

                                state.render_cache.borrow_mut().needs_refresh = true;
                                state.last_auto_scroll = Some(std::time::Instant::now());
                                shell.request_redraw();

                                // NO coordinate compensation needed - selection is in buffer coordinates!
                                // Buffer coords don't change when viewport scrolls.

                                // After scrolling, update the selection endpoint
                                // Convert current mouse position to cell (clamped to viewport)
                                let clamped_y = position.y.clamp(bounds.y, bounds.y + bounds.height - 1.0);
                                let clamped_pos = iced::Point::new(position.x, clamped_y);
                                if let Some(cell) = self.pixel_to_cell(&bounds, clamped_pos) {
                                    let cols = (bounds.width / self.cell_width()) as usize;
                                    let (buf_col, buf_line) = self.screen_to_buffer(cell.0, cell.1);

                                    match state.selection_mode {
                                        SelectionMode::Word => {
                                            let (word_start, word_end) =
                                                self.find_word_at(cell.0, cell.1, cols);
                                            let (word_start_buf, _) = self.screen_to_buffer(word_start, cell.1);
                                            let (word_end_buf, _) = self.screen_to_buffer(word_end, cell.1);
                                            if let Some((start_col, start_line)) = state.selection_start {
                                                if buf_line < start_line
                                                    || (buf_line == start_line && word_start_buf < start_col)
                                                {
                                                    state.selection_end = Some((word_start_buf, buf_line));
                                                } else {
                                                    state.selection_end = Some((word_end_buf, buf_line));
                                                }
                                            }
                                        }
                                        SelectionMode::Line => {
                                            if let Some((_, start_line)) = state.selection_start {
                                                if buf_line < start_line {
                                                    state.selection_start = Some((0, buf_line));
                                                    state.selection_end =
                                                        Some((cols.saturating_sub(1), start_line));
                                                } else {
                                                    state.selection_start = Some((0, start_line));
                                                    state.selection_end =
                                                        Some((cols.saturating_sub(1), buf_line));
                                                }
                                            }
                                        }
                                        SelectionMode::Character => {
                                            state.selection_end = Some((buf_col, buf_line));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Normal selection update when cursor is over bounds
                    if cursor.is_over(bounds) {
                        if let Some(cell) = self.pixel_to_cell(&bounds, *position) {
                            // Throttle selection updates to avoid excessive redraws.
                            if let Some(last) = state.last_drag_update {
                                if last.elapsed() < std::time::Duration::from_millis(8) {
                                    return;
                                }
                            }
                            if state.last_drag_cell == Some(cell) {
                                return;
                            }
                            state.last_drag_cell = Some(cell);
                            state.last_drag_update = Some(std::time::Instant::now());
                            let cols = (bounds.width / self.cell_width()) as usize;

                            // Convert screen cell to buffer coordinates
                            let (buf_col, buf_line) = self.screen_to_buffer(cell.0, cell.1);

                            match state.selection_mode {
                                SelectionMode::Word => {
                                    // Extend selection by word
                                    let (word_start, word_end) =
                                        self.find_word_at(cell.0, cell.1, cols);
                                    let (word_start_buf, _) = self.screen_to_buffer(word_start, cell.1);
                                    let (word_end_buf, _) = self.screen_to_buffer(word_end, cell.1);
                                    if let Some((start_col, start_line)) = state.selection_start {
                                        // Determine direction and extend appropriately
                                        if buf_line < start_line
                                            || (buf_line == start_line && word_start_buf < start_col)
                                        {
                                            state.selection_end = Some((word_start_buf, buf_line));
                                        } else {
                                            state.selection_end = Some((word_end_buf, buf_line));
                                        }
                                    }
                                }
                                SelectionMode::Line => {
                                    // Extend selection by line
                                    if let Some((_, start_line)) = state.selection_start {
                                        if buf_line < start_line {
                                            state.selection_start = Some((0, buf_line));
                                            state.selection_end =
                                                Some((cols.saturating_sub(1), start_line));
                                        } else {
                                            state.selection_start = Some((0, start_line));
                                            state.selection_end =
                                                Some((cols.saturating_sub(1), buf_line));
                                        }
                                    }
                                }
                                SelectionMode::Character => {
                                    state.selection_end = Some((buf_col, buf_line));
                                }
                            }
                            shell.request_redraw();
                        }
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.is_selecting =>
            {
                state.is_selecting = false;
                state.last_drag_cell = None;
                state.last_drag_update = None;
                state.last_auto_scroll = None;
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                // Focus the terminal on scroll
                state.is_focused = true;

                // Check if in alternate screen mode (vim, htop, etc.) - no scrollback there
                let in_alt_screen = {
                    let term = self.term.lock();
                    term.mode().contains(TermMode::ALT_SCREEN)
                };

                if !in_alt_screen {
                    // Calculate scroll lines from delta
                    let lines = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => {
                            // Reset pixel accumulator on line-based scroll
                            state.scroll_pixels = 0.0;
                            // 1:1 scrolling - OS handles scroll direction preference
                            *y as i32
                        }
                        mouse::ScrollDelta::Pixels { y, .. } => {
                            // Accumulate pixels for smooth trackpad scrolling
                            state.scroll_pixels += y;
                            let line_height = self.cell_height();
                            let lines = (state.scroll_pixels / line_height) as i32;
                            // Keep remainder for next scroll event
                            state.scroll_pixels -= lines as f32 * line_height;
                            lines
                        }
                    };

                    if lines != 0 {
                        let mut term = self.term.lock();
                        term.scroll_display(Scroll::Delta(lines));
                        state.render_cache.borrow_mut().needs_refresh = true;
                        shell.request_redraw();
                    }
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => {
                if state.is_focused {
                    // Handle copy/paste shortcuts:
                    // - Ctrl+Insert (copy) / Shift+Insert (paste) - X11/Hyprland style
                    // - Ctrl+Shift+C/V - Linux terminal style
                    // - Super+C/V - macOS style (if not intercepted by WM)
                    let is_copy_shortcut = self
                        .keybindings
                        .matches_action(AppAction::Copy, key, modifiers)
                        ||
                        // Ctrl+Insert (Hyprland sends this for Super+C)
                        (modifiers.control()
                            && !modifiers.shift()
                            && matches!(&key, Key::Named(keyboard::key::Named::Insert)))
                        // Ctrl+Shift+C
                        || (modifiers.control()
                            && modifiers.shift()
                            && matches!(&key, Key::Character(c) if c.as_str().to_lowercase() == "c"))
                        // Super+C (if WM doesn't intercept)
                        || (modifiers.logo()
                            && matches!(&key, Key::Character(c) if c.as_str() == "c"));

                    let is_paste_shortcut = self
                        .keybindings
                        .matches_action(AppAction::Paste, key, modifiers)
                        ||
                        // Shift+Insert (Hyprland sends this for Super+V)
                        (modifiers.shift()
                            && !modifiers.control()
                            && matches!(&key, Key::Named(keyboard::key::Named::Insert)))
                        // Ctrl+Shift+V
                        || (modifiers.control()
                            && modifiers.shift()
                            && matches!(&key, Key::Character(c) if c.as_str().to_lowercase() == "v"))
                        // Super+V (if WM doesn't intercept)
                        || (modifiers.logo()
                            && matches!(&key, Key::Character(c) if c.as_str() == "v"));

                    let is_select_all = modifiers.logo()
                        && matches!(&key, Key::Character(c) if c.as_str() == "a")
                        || (modifiers.control()
                            && modifiers.shift()
                            && matches!(&key, Key::Character(c) if c.as_str().to_lowercase() == "a"));

                    if is_copy_shortcut {
                        // Copy selected text to clipboard
                        if let (Some(start), Some(end)) =
                            (state.selection_start, state.selection_end)
                        {
                            let cols = (bounds.width / self.cell_width()) as usize;
                            let text_content = self.get_selected_text(start, end, cols);
                            if !text_content.is_empty() {
                                clipboard
                                    .write(iced::advanced::clipboard::Kind::Standard, text_content);
                            }
                        }
                        return;
                    }

                    if is_paste_shortcut {
                        // Paste from clipboard
                        if let Some(text_content) =
                            clipboard.read(iced::advanced::clipboard::Kind::Standard)
                        {
                            let bytes = text_content.into_bytes();
                            if !bytes.is_empty() {
                                shell.publish((self.on_input)(bytes));
                            }
                        }
                        return;
                    }

                    if is_select_all {
                        // Select all visible content (in buffer coordinates)
                        let cols = (bounds.width / self.cell_width()) as usize;
                        let rows = (bounds.height / self.cell_height()) as usize;
                        
                        // Convert screen coordinates to buffer coordinates
                        let (start_col, start_line) = self.screen_to_buffer(0, 0);
                        let (end_col, end_line) = self.screen_to_buffer(cols.saturating_sub(1), rows.saturating_sub(1));
                        
                        state.selection_start = Some((start_col, start_line));
                        state.selection_end = Some((end_col, end_line));
                        shell.request_redraw();
                        return;
                    }

                    // Suppress all Super/Logo key combinations to prevent garbage being sent
                    // Super key on Linux is often intercepted by window manager anyway
                    if modifiers.logo() {
                        return;
                    }

                    // Clear selection on any other key press (typing)
                    if !modifiers.control() && state.selection_start.is_some() {
                        // Don't clear on modifier-only or navigation keys
                        let is_nav_key = matches!(
                            key,
                            Key::Named(
                                keyboard::key::Named::Shift
                                    | keyboard::key::Named::Control
                                    | keyboard::key::Named::Alt
                                    | keyboard::key::Named::Super
                            )
                        );
                        if !is_nav_key {
                            state.selection_start = None;
                            state.selection_end = None;
                        }
                    }

                    if let Some(bytes) = key_to_escape_sequence(key, *modifiers, text.as_deref()) {
                        shell.publish((self.on_input)(bytes));

                        // Scroll back to bottom when user types (after scrolling up in history)
                        let mut term = self.term.lock();
                        term.scroll_display(Scroll::Bottom);
                    }
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Convert a keyboard key to terminal escape sequence
fn key_to_escape_sequence(key: &Key, modifiers: Modifiers, text: Option<&str>) -> Option<Vec<u8>> {
    // Handle Ctrl+key combinations
    if modifiers.control() {
        if let Key::Character(c) = key {
            let c = c.chars().next()?;
            let ctrl_char = match c.to_ascii_lowercase() {
                'a'..='z' => (c.to_ascii_lowercase() as u8) - b'a' + 1,
                '@' => 0,
                '[' => 27,
                '\\' => 28,
                ']' => 29,
                '^' => 30,
                '_' => 31,
                _ => return None,
            };
            return Some(vec![ctrl_char]);
        }
    }

    // Handle special keys
    match key {
        Key::Named(named) => {
            let seq = match named {
                keyboard::key::Named::Enter => b"\r".to_vec(),
                keyboard::key::Named::Backspace => vec![127],
                keyboard::key::Named::Tab => {
                    if modifiers.shift() {
                        b"\x1b[Z".to_vec()
                    } else {
                        b"\t".to_vec()
                    }
                }
                keyboard::key::Named::Escape => vec![27],
                keyboard::key::Named::ArrowUp => b"\x1b[A".to_vec(),
                keyboard::key::Named::ArrowDown => b"\x1b[B".to_vec(),
                keyboard::key::Named::ArrowRight => b"\x1b[C".to_vec(),
                keyboard::key::Named::ArrowLeft => b"\x1b[D".to_vec(),
                keyboard::key::Named::Home => b"\x1b[H".to_vec(),
                keyboard::key::Named::End => b"\x1b[F".to_vec(),
                keyboard::key::Named::PageUp => b"\x1b[5~".to_vec(),
                keyboard::key::Named::PageDown => b"\x1b[6~".to_vec(),
                keyboard::key::Named::Insert => b"\x1b[2~".to_vec(),
                keyboard::key::Named::Delete => b"\x1b[3~".to_vec(),
                keyboard::key::Named::F1 => b"\x1bOP".to_vec(),
                keyboard::key::Named::F2 => b"\x1bOQ".to_vec(),
                keyboard::key::Named::F3 => b"\x1bOR".to_vec(),
                keyboard::key::Named::F4 => b"\x1bOS".to_vec(),
                keyboard::key::Named::F5 => b"\x1b[15~".to_vec(),
                keyboard::key::Named::F6 => b"\x1b[17~".to_vec(),
                keyboard::key::Named::F7 => b"\x1b[18~".to_vec(),
                keyboard::key::Named::F8 => b"\x1b[19~".to_vec(),
                keyboard::key::Named::F9 => b"\x1b[20~".to_vec(),
                keyboard::key::Named::F10 => b"\x1b[21~".to_vec(),
                keyboard::key::Named::F11 => b"\x1b[23~".to_vec(),
                keyboard::key::Named::F12 => b"\x1b[24~".to_vec(),
                keyboard::key::Named::Space => b" ".to_vec(),
                _ => return None,
            };
            Some(seq)
        }
        Key::Character(_) => {
            // Use the text representation for regular characters
            text.map(|t| t.as_bytes().to_vec())
        }
        _ => None,
    }
}

impl<'a, Message> From<TerminalWidget<'a, Message>> for Element<'a, Message>
where
    Message: 'a,
{
    fn from(widget: TerminalWidget<'a, Message>) -> Self {
        Element::new(widget)
    }
}
