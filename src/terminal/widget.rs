//! Custom iced widget for terminal rendering
//!
//! This implements the iced Widget trait for rendering terminal content.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Boundary, Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::CursorShape;
use iced::advanced::graphics::geometry::{Frame, LineCap, Path, Stroke};
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::keyboard::{self, Key, Modifiers};
use iced::mouse::{self, Cursor};
use iced::window;
use iced::{Background, Border, Color, Element, Event, Length, Rectangle, Shadow, Size};
use parking_lot::Mutex;

use super::backend::{CursorInfo, EventProxy, RenderCell, paste_bytes_for_mode};
use super::block_elements::{TerminalGraphicCell, render_terminal_graphic};
use super::colors::{DEFAULT_BG, DEFAULT_FG, ansi_to_iced_themed, cell_fg_to_iced};
use super::glyph_constraints::GlyphSize;
use super::metrics::{TERMINAL_PADDING_LEFT, TerminalMetrics};
use super::nerd_font_attributes;
use crate::config::settings::{
    TERMINAL_SCROLL_SPEED_BASE, TERMINAL_SCROLL_SPEED_MAX, TERMINAL_SCROLL_SPEED_MIN,
    TerminalMetricAdjustments,
};
use crate::fonts::{JETBRAINS_MONO_NERD, TerminalFont};
use crate::keybindings::{AppAction, KeybindingsConfig};
use crate::theme::TerminalColors;

fn is_powerline_separator(c: char) -> bool {
    matches!(c, '\u{E0B0}' | '\u{E0B2}' | '\u{E0B4}' | '\u{E0B6}')
}

fn draw_metric_rect<Renderer>(renderer: &mut Renderer, bounds: Rectangle, color: Color)
where
    Renderer: renderer::Renderer,
{
    renderer.fill_quad(
        Quad {
            bounds,
            border: Border::default(),
            shadow: Shadow::default(),
            snap: true,
        },
        Background::Color(color),
    );
}

fn draw_text_decorations<Renderer>(
    renderer: &mut Renderer,
    flags: CellFlags,
    cell_rect: Rectangle,
    metrics: TerminalMetrics,
    color: Color,
) where
    Renderer: renderer::Renderer + iced::advanced::graphics::geometry::Renderer,
{
    if flags.contains(CellFlags::UNDERLINE) {
        draw_metric_rect(
            renderer,
            Rectangle {
                y: cell_rect.y + metrics.underline_position,
                height: metrics.underline_thickness,
                ..cell_rect
            },
            color,
        );
    }

    if flags.contains(CellFlags::DOUBLE_UNDERLINE) {
        let thickness = metrics.underline_thickness;
        let y = cell_rect.y + metrics.underline_position;
        draw_metric_rect(
            renderer,
            Rectangle {
                y: y - thickness,
                height: thickness,
                ..cell_rect
            },
            color,
        );
        draw_metric_rect(
            renderer,
            Rectangle {
                y: y + thickness,
                height: thickness,
                ..cell_rect
            },
            color,
        );
    }

    if flags.contains(CellFlags::DOTTED_UNDERLINE) {
        let diameter = (metrics.underline_thickness * std::f32::consts::SQRT_2)
            .round()
            .max(1.0);
        let y = cell_rect.y + metrics.underline_position;
        let step = (diameter * 2.0).max(2.0);
        let mut x = cell_rect.x;
        while x < cell_rect.x + cell_rect.width {
            draw_metric_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: diameter.min(cell_rect.x + cell_rect.width - x),
                    height: diameter,
                },
                color,
            );
            x += step;
        }
    }

    if flags.contains(CellFlags::DASHED_UNDERLINE) {
        let dash_width = (cell_rect.width / 3.0).ceil().max(1.0);
        let y = cell_rect.y + metrics.underline_position;
        let mut x = cell_rect.x;
        while x < cell_rect.x + cell_rect.width {
            draw_metric_rect(
                renderer,
                Rectangle {
                    x,
                    y,
                    width: dash_width.min(cell_rect.x + cell_rect.width - x),
                    height: metrics.underline_thickness,
                },
                color,
            );
            x += dash_width * 2.0;
        }
    }

    if flags.contains(CellFlags::UNDERCURL) {
        let amplitude = (cell_rect.width / std::f32::consts::PI)
            .min(cell_rect.height / 4.0)
            .max(metrics.underline_thickness);
        let y = metrics
            .underline_position
            .min(cell_rect.height - amplitude - metrics.underline_thickness);
        renderer.with_translation(iced::Vector::new(cell_rect.x, cell_rect.y), |renderer| {
            let mut frame = Frame::new(renderer, Size::new(cell_rect.width, cell_rect.height));
            let path = Path::new(|path| {
                let center = cell_rect.width / 2.0;
                let bottom = y + amplitude;
                path.move_to(iced::Point::new(0.0, bottom));
                path.bezier_curve_to(
                    iced::Point::new(center * 0.4, bottom),
                    iced::Point::new(center * 0.6, y),
                    iced::Point::new(center, y),
                );
                path.bezier_curve_to(
                    iced::Point::new(center * 1.4, y),
                    iced::Point::new(center * 1.6, bottom),
                    iced::Point::new(cell_rect.width, bottom),
                );
            });
            frame.stroke(
                &path,
                Stroke::default()
                    .with_color(color)
                    .with_width(metrics.underline_thickness)
                    .with_line_cap(LineCap::Round),
            );
            renderer.draw_geometry(frame.into_geometry());
        });
    }

    if flags.contains(CellFlags::STRIKEOUT) {
        draw_metric_rect(
            renderer,
            Rectangle {
                y: cell_rect.y + metrics.strikethrough_position,
                height: metrics.strikethrough_thickness,
                ..cell_rect
            },
            color,
        );
    }
}

fn push_selection_span(
    rects: &mut Vec<Rectangle>,
    bounds: Rectangle,
    metrics: TerminalMetrics,
    line: usize,
    start_col: usize,
    end_col: usize,
) {
    rects.push(Rectangle {
        x: bounds.x + TERMINAL_PADDING_LEFT + start_col as f32 * metrics.cell_width,
        y: bounds.y + line as f32 * metrics.cell_height,
        width: (end_col - start_col + 1) as f32 * metrics.cell_width,
        height: metrics.cell_height,
    });
}

/// Terminal widget for iced
pub struct TerminalWidget<'a, Message> {
    term: Arc<Mutex<Term<EventProxy>>>,
    on_input: Box<dyn Fn(Vec<u8>) -> Message + 'a>,
    on_paste: Option<Box<dyn Fn() -> Message + 'a>>,
    on_resize: Option<Box<dyn Fn(u16, u16) -> Message + 'a>>,
    font_size: f32,
    font: iced::Font,
    terminal_font: TerminalFont,
    terminal_metric_adjustments: TerminalMetricAdjustments,
    terminal_colors: Option<TerminalColors>,
    render_epoch: Option<Arc<AtomicU64>>,
    keybindings: KeybindingsConfig,
    scroll_speed: f32,
    focus_token: u64,
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
            on_paste: None,
            on_resize: None,
            font_size: 9.0,
            font: JETBRAINS_MONO_NERD,
            terminal_font: TerminalFont::default(),
            terminal_metric_adjustments: TerminalMetricAdjustments::default(),
            terminal_colors: None,
            render_epoch: None,
            keybindings: KeybindingsConfig::default(),
            scroll_speed: TERMINAL_SCROLL_SPEED_BASE,
            focus_token: 0,
        }
    }

    /// Set font size
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Set mouse wheel / trackpad scroll speed multiplier.
    pub fn scroll_speed(mut self, speed: f32) -> Self {
        self.scroll_speed = speed.clamp(TERMINAL_SCROLL_SPEED_MIN, TERMINAL_SCROLL_SPEED_MAX);
        self
    }

    /// Set terminal font
    pub fn font(mut self, font: TerminalFont) -> Self {
        self.font = font.to_iced_font();
        self.terminal_font = font;
        self
    }

    /// Set Ghostty-style terminal metric adjustments.
    pub fn metric_adjustments(mut self, adjustments: TerminalMetricAdjustments) -> Self {
        self.terminal_metric_adjustments = adjustments;
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

    /// Request keyboard focus when this token changes.
    pub fn focus_token(mut self, token: u64) -> Self {
        self.focus_token = token;
        self
    }

    /// Set resize callback
    pub fn on_resize(mut self, callback: impl Fn(u16, u16) -> Message + 'a) -> Self {
        self.on_resize = Some(Box::new(callback));
        self
    }

    /// Set paste callback for app-level clipboard handling.
    pub fn on_paste(mut self, callback: impl Fn() -> Message + 'a) -> Self {
        self.on_paste = Some(Box::new(callback));
        self
    }

    fn cell_metrics(&self) -> TerminalMetrics {
        TerminalMetrics::for_font_with_adjustments(
            self.terminal_font,
            self.font_size,
            self.terminal_metric_adjustments,
        )
    }

    /// Calculate cell width based on font size.
    fn cell_width(&self) -> f32 {
        self.cell_metrics().cell_width
    }

    /// Calculate cell height based on font size.
    fn cell_height(&self) -> f32 {
        self.cell_metrics().cell_height
    }

    /// Get renderable cells from the terminal
    fn get_cells(&self) -> Vec<RenderCell> {
        use alacritty_terminal::vte::ansi::NamedColor;

        let term = self.term.lock();
        let content = term.renderable_content();
        let display_offset = content.display_offset;
        let rows = term.screen_lines();
        let cols = term.columns();
        let mut row_chars = vec![vec!['\0'; cols]; rows];
        let mut pending = Vec::new();

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
            if line >= rows {
                continue;
            }
            let column = indexed.point.column.0;
            if column >= cols {
                continue;
            }

            row_chars[line][column] = cell.c;

            // Skip wide character spacer cells (placeholder for 2nd column of wide chars)
            if cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                continue;
            }

            // Include cells with content or non-default background
            if cell.c != ' '
                || cell.bg != alacritty_terminal::vte::ansi::Color::Named(NamedColor::Background)
                || !cell.flags.is_empty()
            {
                pending.push(RenderCell {
                    column,
                    line,
                    character: cell.c,
                    zerowidth: cell
                        .zerowidth()
                        .map(|chars| chars.iter().copied().collect())
                        .unwrap_or_default(),
                    fg: cell.fg,
                    bg: cell.bg,
                    flags: cell.flags,
                    constraint_width: 1,
                });
            }
        }

        pending
            .into_iter()
            .map(|mut cell| {
                let grid_width = if cell.flags.contains(CellFlags::WIDE_CHAR) {
                    2
                } else {
                    1
                };
                cell.constraint_width = nerd_font_attributes::constraint_width(
                    &row_chars[cell.line],
                    cell.column,
                    grid_width,
                );
                cell
            })
            .collect()
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

    fn selection_rects(&self, bounds: Rectangle, metrics: TerminalMetrics) -> Vec<Rectangle> {
        let term = self.term.lock();
        let content = term.renderable_content();
        let selection = match content.selection {
            Some(selection) => selection,
            None => return Vec::new(),
        };

        let display_offset = content.display_offset as i32;
        let cursor = content.cursor;
        let rows = term.screen_lines();
        let cols = term.columns();

        let mut rects = Vec::new();
        let mut active_span: Option<(usize, usize, usize)> = None;

        for indexed in content.display_iter {
            let screen_line = indexed.point.line.0 + display_offset;
            if screen_line < 0 {
                continue;
            }

            let screen_line = screen_line as usize;
            let col = indexed.point.column.0;
            if screen_line >= rows || col >= cols {
                continue;
            }

            if selection.contains_cell(&indexed, cursor.point, cursor.shape) {
                match active_span {
                    Some((line, start_col, end_col))
                        if line == screen_line && col == end_col + 1 =>
                    {
                        active_span = Some((line, start_col, col));
                    }
                    Some((line, start_col, end_col)) => {
                        push_selection_span(&mut rects, bounds, metrics, line, start_col, end_col);
                        active_span = Some((screen_line, col, col));
                    }
                    None => active_span = Some((screen_line, col, col)),
                }
            } else if let Some((line, start_col, end_col)) = active_span.take() {
                push_selection_span(&mut rects, bounds, metrics, line, start_col, end_col);
            }
        }

        if let Some((line, start_col, end_col)) = active_span {
            push_selection_span(&mut rects, bounds, metrics, line, start_col, end_col);
        }

        rects
    }

    /// Convert pixel coordinates to terminal cell coordinates (screen-relative)
    fn pixel_to_cell(&self, bounds: &Rectangle, position: iced::Point) -> Option<(usize, usize)> {
        if !bounds.contains(position)
            || !bounds.width.is_finite()
            || !bounds.height.is_finite()
            || !position.x.is_finite()
            || !position.y.is_finite()
        {
            return None;
        }
        // Account for left padding when converting to cell coordinates
        let col = terminal_cell_index(
            position.x - bounds.x - TERMINAL_PADDING_LEFT,
            self.cell_width(),
        )?;
        let row = terminal_cell_index(position.y - bounds.y, self.cell_height())?;
        Some((col, row))
    }

    fn selection_anchor_at_position(
        &self,
        bounds: &Rectangle,
        position: iced::Point,
    ) -> Option<(Point, Side, (usize, usize))> {
        let (screen_col, screen_line) = self.pixel_to_cell(bounds, position)?;
        let side = selection_side(
            position.x - bounds.x - TERMINAL_PADDING_LEFT,
            screen_col,
            self.cell_width(),
        )?;

        let term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let raw_point = Point::new(
            Line(screen_line as i32 - display_offset),
            Column(screen_col),
        );
        let point = raw_point.grid_clamp(&*term, Boundary::Grid);
        let side = if screen_col > point.column.0 {
            Side::Right
        } else {
            side
        };

        Some((point, side, (screen_col, screen_line)))
    }

    fn clamped_selection_anchor_at_position(
        &self,
        bounds: &Rectangle,
        position: iced::Point,
    ) -> Option<(Point, Side, (usize, usize))> {
        if !bounds.width.is_finite() || !bounds.height.is_finite() {
            return None;
        }

        let max_x = (bounds.x + bounds.width - 1.0).max(bounds.x);
        let max_y = (bounds.y + bounds.height - 1.0).max(bounds.y);
        let position = iced::Point::new(
            position.x.clamp(bounds.x, max_x),
            position.y.clamp(bounds.y, max_y),
        );

        self.selection_anchor_at_position(bounds, position)
    }

    fn begin_selection(&self, ty: SelectionType, point: Point, side: Side) {
        let mut term = self.term.lock();
        term.selection = Some(Selection::new(ty, point, side));
    }

    fn update_selection(&self, point: Point, side: Side) {
        let mut term = self.term.lock();
        if let Some(selection) = term.selection.as_mut() {
            selection.update(point, side);
        }
    }

    fn clear_selection(&self) {
        let mut term = self.term.lock();
        term.selection = None;
    }

    fn select_visible_content(&self) {
        let mut term = self.term.lock();
        let display_offset = term.grid().display_offset() as i32;
        let start = Point::new(Line(-display_offset), Column(0)).grid_clamp(&*term, Boundary::Grid);
        let end = Point::new(
            Line(term.screen_lines().saturating_sub(1) as i32 - display_offset),
            term.last_column(),
        )
        .grid_clamp(&*term, Boundary::Grid);

        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);
    }

    fn selected_text(&self) -> Option<String> {
        let term = self.term.lock();
        term.selection_to_string()
    }

    fn terminal_mode(&self) -> TermMode {
        let term = self.term.lock();
        *term.mode()
    }

    fn mouse_reporting_enabled(&self) -> bool {
        self.terminal_mode().intersects(TermMode::MOUSE_MODE)
    }

    fn mouse_button_report(
        &self,
        button: mouse::Button,
        bounds: Rectangle,
        position: Option<iced::Point>,
        metrics: TerminalMetrics,
        kind: MouseReportKind,
        modifiers: Modifiers,
    ) -> Option<(u8, Vec<u8>)> {
        let code = mouse_button_code(button)?;
        let (column, row) = mouse_cell(bounds, position?, metrics)?;
        let bytes =
            mouse_report_sequence(self.terminal_mode(), code, column, row, kind, modifiers)?;
        Some((code, bytes))
    }

    fn mouse_button_release_report(
        &self,
        button: mouse::Button,
        active_button: Option<u8>,
        bounds: Rectangle,
        position: Option<iced::Point>,
        metrics: TerminalMetrics,
        modifiers: Modifiers,
    ) -> Option<Vec<u8>> {
        let code = active_button.or_else(|| mouse_button_code(button))?;
        let (column, row) = mouse_cell(bounds, position?, metrics)?;
        mouse_report_sequence(
            self.terminal_mode(),
            code,
            column,
            row,
            MouseReportKind::Release,
            modifiers,
        )
    }

    fn mouse_motion_report(
        &self,
        active_button: Option<u8>,
        bounds: Rectangle,
        position: iced::Point,
        metrics: TerminalMetrics,
    ) -> Option<Vec<u8>> {
        let mode = self.terminal_mode();
        let code = if let Some(code) = active_button {
            if !mode.contains(TermMode::MOUSE_DRAG) && !mode.contains(TermMode::MOUSE_MOTION) {
                return None;
            }
            code
        } else if mode.contains(TermMode::MOUSE_MOTION) {
            35
        } else {
            return None;
        };
        let (column, row) = mouse_cell(bounds, position, metrics)?;
        mouse_report_sequence(
            mode,
            code,
            column,
            row,
            MouseReportKind::Motion,
            Modifiers::NONE,
        )
    }

    fn mouse_wheel_report(
        &self,
        bounds: Rectangle,
        position: Option<iced::Point>,
        metrics: TerminalMetrics,
        delta: &mouse::ScrollDelta,
    ) -> Option<Vec<u8>> {
        let y = scroll_delta_y(delta);
        if y == 0.0 {
            return None;
        }
        let code = if y > 0.0 { 64 } else { 65 };
        let (column, row) = mouse_cell(bounds, position?, metrics)?;
        mouse_report_sequence(
            self.terminal_mode(),
            code,
            column,
            row,
            MouseReportKind::Press,
            Modifiers::NONE,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MouseReportKind {
    Press,
    Release,
    Motion,
}

fn terminal_cell_index(offset: f32, cell_size: f32) -> Option<usize> {
    if !offset.is_finite() || !cell_size.is_finite() || cell_size <= 0.0 {
        return None;
    }

    Some(
        (offset.max(0.0) / cell_size)
            .floor()
            .clamp(0.0, u16::MAX as f32) as usize,
    )
}

fn mouse_cell(
    bounds: Rectangle,
    position: iced::Point,
    metrics: TerminalMetrics,
) -> Option<(usize, usize)> {
    let column = terminal_cell_index(
        position.x - bounds.x - TERMINAL_PADDING_LEFT,
        metrics.cell_width,
    )?;
    let row = terminal_cell_index(position.y - bounds.y, metrics.cell_height)?;
    Some((column, row))
}

fn mouse_button_code(button: mouse::Button) -> Option<u8> {
    match button {
        mouse::Button::Left => Some(0),
        mouse::Button::Middle => Some(1),
        mouse::Button::Right => Some(2),
        _ => None,
    }
}

fn mouse_report_sequence(
    mode: TermMode,
    code: u8,
    column: usize,
    row: usize,
    kind: MouseReportKind,
    modifiers: Modifiers,
) -> Option<Vec<u8>> {
    let code = match kind {
        MouseReportKind::Press => code,
        MouseReportKind::Release if mode.contains(TermMode::SGR_MOUSE) => code,
        MouseReportKind::Release => 3,
        MouseReportKind::Motion => code | 32,
    } + mouse_modifier_bits(modifiers);

    let column = column.saturating_add(1);
    let row = row.saturating_add(1);

    if mode.contains(TermMode::SGR_MOUSE) {
        let suffix = if kind == MouseReportKind::Release {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{};{};{}{}", code, column, row, suffix).into_bytes());
    }

    legacy_mouse_report(code, column, row)
}

fn legacy_mouse_report(code: u8, column: usize, row: usize) -> Option<Vec<u8>> {
    let x = u8::try_from(column.checked_add(32)?).ok()?;
    let y = u8::try_from(row.checked_add(32)?).ok()?;
    Some(vec![0x1b, b'[', b'M', code.saturating_add(32), x, y])
}

fn mouse_modifier_bits(modifiers: Modifiers) -> u8 {
    (u8::from(modifiers.shift()) * 4)
        + (u8::from(modifiers.alt()) * 8)
        + (u8::from(modifiers.control()) * 16)
}

fn scroll_delta_y(delta: &mouse::ScrollDelta) -> f32 {
    match delta {
        mouse::ScrollDelta::Lines { y, .. } | mouse::ScrollDelta::Pixels { y, .. } => *y,
    }
}

fn alternate_scroll_sequence(delta: &mouse::ScrollDelta) -> Option<Vec<u8>> {
    let y = scroll_delta_y(delta);
    if y > 0.0 {
        Some(b"\x1b[A".to_vec())
    } else if y < 0.0 {
        Some(b"\x1b[B".to_vec())
    } else {
        None
    }
}

fn focus_report_sequence(mode: TermMode, focused: bool) -> Option<Vec<u8>> {
    if !mode.contains(TermMode::FOCUS_IN_OUT) {
        return None;
    }

    if focused {
        Some(b"\x1b[I".to_vec())
    } else {
        Some(b"\x1b[O".to_vec())
    }
}

fn selection_side(offset: f32, column: usize, cell_width: f32) -> Option<Side> {
    if !offset.is_finite() || !cell_width.is_finite() || cell_width <= 0.0 {
        return None;
    }

    let cell_left = column as f32 * cell_width;
    if offset - cell_left >= cell_width / 2.0 {
        Some(Side::Right)
    } else {
        Some(Side::Left)
    }
}

/// Widget state stored in the tree
#[derive(Debug)]
struct TerminalState {
    is_focused: bool,
    cursor_visible: bool,
    last_size: Option<(u16, u16)>,
    scroll_pixels: f32, // Accumulated scroll pixels for trackpad
    scroll_lines: f32,  // Accumulated fractional line scroll for mouse wheels
    // Cached render data (updated only when terminal content changes)
    render_cache: RefCell<RenderCache>,
    is_selecting: bool,
    last_drag_anchor: Option<(Point, Side)>,
    last_drag_update: Option<std::time::Instant>,
    // Click tracking for double/triple click
    last_click_time: Option<std::time::Instant>,
    last_click_position: Option<(usize, usize)>,
    click_count: u8, // 1 = single, 2 = double, 3 = triple
    // Auto-scroll during selection
    last_auto_scroll: Option<std::time::Instant>,
    last_focus_token: u64,
    last_focus_reported: Option<bool>,
    mouse_button: Option<u8>,
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
            scroll_lines: 0.0,
            render_cache: RefCell::new(RenderCache {
                needs_refresh: true,
                ..RenderCache::default()
            }),
            is_selecting: false,
            last_drag_anchor: None,
            last_drag_update: None,
            last_click_time: None,
            last_click_position: None,
            click_count: 0,
            last_auto_scroll: None,
            last_focus_token: 0,
            last_focus_reported: None,
            mouse_button: None,
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for TerminalWidget<'_, Message>
where
    Renderer: renderer::Renderer
        + iced::advanced::graphics::geometry::Renderer
        + iced::advanced::text::Renderer<Font = iced::Font>,
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
            scroll_lines: 0.0,
            render_cache: RefCell::new(RenderCache {
                needs_refresh: true,
                ..RenderCache::default()
            }),
            is_selecting: false,
            last_drag_anchor: None,
            last_drag_update: None,
            last_click_time: None,
            last_click_position: None,
            click_count: 0,
            last_auto_scroll: None,
            last_focus_token: 0,
            last_focus_reported: None,
            mouse_button: None,
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
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<TerminalState>();
        let is_focused = state.is_focused || state.last_focus_token != self.focus_token;
        let cursor_visible = state.cursor_visible;

        // Get terminal colors (from theme or defaults)
        let default_colors = TerminalColors {
            foreground: DEFAULT_FG,
            background: DEFAULT_BG,
            cursor: DEFAULT_FG,
            ansi: super::colors::ANSI_COLORS,
        };
        let colors = self.terminal_colors.as_ref().unwrap_or(&default_colors);

        let Some(widget_clip) = bounds.intersection(viewport) else {
            return;
        };

        renderer.with_layer(widget_clip, |renderer| {
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

            let metrics = self.cell_metrics();
            let cell_width = metrics.cell_width;
            let cell_height = metrics.cell_height;

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

            let render_cache = state.render_cache.borrow();

            // Draw cell backgrounds first. Some terminal glyphs (Powerline
            // separators, Nerd Font icons) intentionally overhang their cell.
            // Painting backgrounds in the same pass can cover those overhangs.
            for cell in &render_cache.cells {
                let x = bounds.x + TERMINAL_PADDING_LEFT + cell.column as f32 * cell_width;
                let y = bounds.y + cell.line as f32 * cell_height;

                let mut fg_color = cell_fg_to_iced(cell.fg, cell.flags, colors);
                let mut bg_color = ansi_to_iced_themed(cell.bg, colors);

                if cell.flags.contains(CellFlags::INVERSE) {
                    std::mem::swap(&mut fg_color, &mut bg_color);
                }

                // Draw cell background if not default (after inverse swap)
                if bg_color != colors.background {
                    let bg_width = if cell.flags.contains(CellFlags::WIDE_CHAR) {
                        cell_width * 2.0
                    } else {
                        cell_width
                    };

                    renderer.fill_quad(
                        Quad {
                            bounds: Rectangle {
                                x,
                                y,
                                width: bg_width,
                                height: cell_height,
                            },
                            border: Border::default(),
                            shadow: Shadow::default(),
                            snap: true,
                        },
                        Background::Color(bg_color),
                    );
                }
            }

            for rect in self.selection_rects(bounds, metrics) {
                renderer.fill_quad(
                    Quad {
                        bounds: rect,
                        border: Border::default(),
                        shadow: Shadow::default(),
                        snap: true,
                    },
                    Background::Color(Color::from_rgba(0.3, 0.5, 0.8, 0.45)),
                );
            }

            // Draw glyphs after all backgrounds so non-standard terminal glyphs are
            // not clipped by the next cell's background.
            for cell in &render_cache.cells {
                let x = bounds.x + TERMINAL_PADDING_LEFT + cell.column as f32 * cell_width;
                let y = bounds.y + cell.line as f32 * cell_height;

                let mut fg_color = cell_fg_to_iced(cell.fg, cell.flags, colors);
                let mut bg_color = ansi_to_iced_themed(cell.bg, colors);

                if cell.flags.contains(CellFlags::INVERSE) {
                    std::mem::swap(&mut fg_color, &mut bg_color);
                }

                // Draw character
                if cell.character != ' ' && !cell.flags.contains(CellFlags::HIDDEN) {
                    // Wide characters (e.g. CJK, emoji) occupy 2 cells
                    let char_width = if cell.flags.contains(CellFlags::WIDE_CHAR) {
                        cell_width * 2.0
                    } else {
                        cell_width
                    };

                    // Try to render terminal graphics as rectangles for pixel-perfect rendering
                    if render_terminal_graphic(
                        renderer,
                        cell.character,
                        TerminalGraphicCell {
                            rect: Rectangle {
                                x,
                                y,
                                width: cell_width,
                                height: cell_height,
                            },
                            box_thickness: metrics.box_thickness,
                        },
                        fg_color,
                    ) {
                        // Block element was rendered as rectangles
                    } else {
                        let bold = cell.flags.contains(CellFlags::BOLD);
                        let italic = cell.flags.contains(CellFlags::ITALIC);
                        let font = self.terminal_font.variant(bold, italic);

                        let mut text_x = x;
                        let mut text_y = y;
                        let mut text_size = self.font_size;
                        let mut text_width = char_width;
                        let mut glyph_clip_width = char_width;

                        if let Some(constraint) =
                            nerd_font_attributes::constraint_for(cell.character)
                        {
                            let constraint_width = cell.constraint_width.max(1);
                            glyph_clip_width = cell_width * constraint_width as f32;
                            text_width = glyph_clip_width;

                            let glyph = GlyphSize {
                                width: metrics.face_width,
                                height: metrics.face_height,
                                x: 0.0,
                                y: metrics.face_y,
                            };
                            let constrained =
                                constraint.constrain(glyph, metrics, constraint_width);
                            let scale = (constrained.height / glyph.height).clamp(0.25, 4.0);
                            text_size *= scale;
                            text_x += constrained.x;
                            text_y += constrained.y;
                        }

                        // Draw the character using text renderer.
                        let text = iced::advanced::Text {
                            content: {
                                let mut content = cell.character.to_string();
                                content.push_str(&cell.zerowidth);
                                content
                            },
                            bounds: Size::new(text_width, cell_height),
                            size: iced::Pixels(text_size),
                            line_height: iced::advanced::text::LineHeight::Absolute(iced::Pixels(
                                cell_height,
                            )),
                            font,
                            align_x: iced::alignment::Horizontal::Left.into(),
                            align_y: iced::alignment::Vertical::Top,
                            shaping: iced::advanced::text::Shaping::Advanced,
                            wrapping: iced::advanced::text::Wrapping::None,
                        };

                        let clip_bounds = if is_powerline_separator(cell.character) {
                            Rectangle {
                                x: bounds.x,
                                y,
                                width: bounds.width,
                                height: cell_height,
                            }
                        } else if nerd_font_attributes::is_symbol(cell.character) {
                            Rectangle {
                                x,
                                y,
                                width: glyph_clip_width,
                                height: cell_height,
                            }
                        } else {
                            bounds
                        };

                        renderer.fill_text(
                            text,
                            iced::Point::new(text_x, text_y),
                            fg_color,
                            clip_bounds,
                        );
                    }
                }
            }

            // Draw line decorations as metric sprites so underline styles line up
            // across adjacent cells independent of the selected font face.
            for cell in &render_cache.cells {
                if !cell
                    .flags
                    .intersects(CellFlags::ALL_UNDERLINES | CellFlags::STRIKEOUT)
                {
                    continue;
                }

                let x = bounds.x + TERMINAL_PADDING_LEFT + cell.column as f32 * cell_width;
                let y = bounds.y + cell.line as f32 * cell_height;
                let mut fg_color = cell_fg_to_iced(cell.fg, cell.flags, colors);
                let mut bg_color = ansi_to_iced_themed(cell.bg, colors);

                if cell.flags.contains(CellFlags::INVERSE) {
                    std::mem::swap(&mut fg_color, &mut bg_color);
                }

                let decoration_width = if cell.flags.contains(CellFlags::WIDE_CHAR) {
                    cell_width * 2.0
                } else {
                    cell_width
                };

                draw_text_decorations(
                    renderer,
                    cell.flags,
                    Rectangle {
                        x,
                        y,
                        width: decoration_width,
                        height: cell_height,
                    },
                    metrics,
                    fg_color,
                );
            }

            // Draw cursor (only if visible and in valid position)
            if is_focused
                && cursor_visible
                && let Some(cursor_info) = cached_cursor
                && cursor_info.visible
            {
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
                        let thickness = metrics.cursor_thickness.max(1.0);
                        renderer.fill_quad(
                            Quad {
                                bounds: Rectangle {
                                    x: cursor_x,
                                    y: cursor_y + cell_height - thickness,
                                    width: cell_width,
                                    height: thickness,
                                },
                                border: Border::default(),
                                shadow: Shadow::default(),
                                snap: true,
                            },
                            Background::Color(cursor_color),
                        );
                    }
                    CursorShape::Beam => {
                        let thickness = metrics.cursor_thickness.max(1.0);
                        renderer.fill_quad(
                            Quad {
                                bounds: Rectangle {
                                    x: cursor_x,
                                    y: cursor_y,
                                    width: thickness,
                                    height: metrics.cursor_height.min(cell_height),
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
        });
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
        let metrics = self.cell_metrics();

        if state.last_focus_token != self.focus_token {
            state.last_focus_token = self.focus_token;
            state.is_focused = true;
            if let Some(bytes) = focus_report_sequence(self.terminal_mode(), true)
                && state.last_focus_reported != Some(true)
            {
                state.last_focus_reported = Some(true);
                shell.publish((self.on_input)(bytes));
            }
            shell.request_redraw();
        }

        // Detect size changes and emit resize message
        if let Some(ref on_resize) = self.on_resize {
            // Calculate terminal dimensions from pixel bounds (accounting for padding)
            let cols = metrics.columns_for_bounds(bounds) as u16;
            let rows = metrics.rows_for_bounds(bounds) as u16;

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
            Event::Window(window::Event::Focused) => {
                if state.is_focused
                    && let Some(bytes) = focus_report_sequence(self.terminal_mode(), true)
                    && state.last_focus_reported != Some(true)
                {
                    state.last_focus_reported = Some(true);
                    shell.publish((self.on_input)(bytes));
                }
            }
            Event::Window(window::Event::Unfocused) => {
                if state.is_focused
                    && let Some(bytes) = focus_report_sequence(self.terminal_mode(), false)
                    && state.last_focus_reported != Some(false)
                {
                    state.last_focus_reported = Some(false);
                    shell.publish((self.on_input)(bytes));
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(button))
                if cursor.is_over(bounds) && self.mouse_reporting_enabled() =>
            {
                state.is_focused = true;
                if let Some(bytes) = focus_report_sequence(self.terminal_mode(), true)
                    && state.last_focus_reported != Some(true)
                {
                    state.last_focus_reported = Some(true);
                    shell.publish((self.on_input)(bytes));
                }
                if let Some((code, bytes)) = self.mouse_button_report(
                    *button,
                    bounds,
                    cursor.position(),
                    metrics,
                    MouseReportKind::Press,
                    Modifiers::NONE,
                ) {
                    state.mouse_button = Some(code);
                    shell.publish((self.on_input)(bytes));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(button))
                if self.mouse_reporting_enabled() =>
            {
                if let Some(bytes) = self.mouse_button_release_report(
                    *button,
                    state.mouse_button,
                    bounds,
                    cursor.position(),
                    metrics,
                    Modifiers::NONE,
                ) {
                    shell.publish((self.on_input)(bytes));
                }
                state.mouse_button = None;
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if cursor.is_over(bounds) =>
            {
                state.is_focused = true;
                if let Some(bytes) = focus_report_sequence(self.terminal_mode(), true)
                    && state.last_focus_reported != Some(true)
                {
                    state.last_focus_reported = Some(true);
                    shell.publish((self.on_input)(bytes));
                }

                if let Some(position) = cursor.position()
                    && let Some((point, side, cell)) =
                        self.selection_anchor_at_position(&bounds, position)
                {
                    let now = std::time::Instant::now();

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

                    let selection_type = match state.click_count {
                        2 => SelectionType::Semantic,
                        3 => SelectionType::Lines,
                        _ => SelectionType::Simple,
                    };
                    self.begin_selection(selection_type, point, side);
                    state.is_selecting = true;
                    state.last_drag_anchor = Some((point, side));
                    state.last_drag_update = None;
                    state.last_auto_scroll = None;
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.is_focused = false;
                if let Some(bytes) = focus_report_sequence(self.terminal_mode(), false)
                    && state.last_focus_reported != Some(false)
                {
                    state.last_focus_reported = Some(false);
                    shell.publish((self.on_input)(bytes));
                }
                self.clear_selection();
                state.is_selecting = false;
                state.click_count = 0;
                state.last_drag_anchor = None;
                state.last_auto_scroll = None;
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if self.mouse_reporting_enabled() {
                    if let Some(bytes) =
                        self.mouse_motion_report(state.mouse_button, bounds, *position, metrics)
                    {
                        shell.publish((self.on_input)(bytes));
                    }
                    return;
                }

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
                    let near_top_edge = (0.0..AUTO_SCROLL_ZONE).contains(&edge_distance_top);
                    let near_bottom_edge = (0.0..AUTO_SCROLL_ZONE).contains(&edge_distance_bottom);

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
                                    let distance_factor = (AUTO_SCROLL_ZONE
                                        - edge_distance_top.max(0.0))
                                        / AUTO_SCROLL_ZONE;
                                    1.max((distance_factor * 3.0) as i32)
                                } else {
                                    // Near bottom - scroll down (negative delta scrolls viewport down)
                                    let distance_factor = (AUTO_SCROLL_ZONE
                                        - edge_distance_bottom.max(0.0))
                                        / AUTO_SCROLL_ZONE;
                                    -(1.max((distance_factor * 3.0) as i32))
                                };

                                let mut term = self.term.lock();
                                term.scroll_display(Scroll::Delta(scroll_lines));
                                drop(term);

                                state.render_cache.borrow_mut().needs_refresh = true;
                                state.last_auto_scroll = Some(std::time::Instant::now());
                                shell.request_redraw();

                                if let Some((point, side, _)) =
                                    self.clamped_selection_anchor_at_position(&bounds, *position)
                                {
                                    self.update_selection(point, side);
                                    state.last_drag_anchor = Some((point, side));
                                }
                            }
                        }
                    }

                    // Normal selection update when cursor is over bounds
                    if cursor.is_over(bounds)
                        && let Some((point, side, _)) =
                            self.selection_anchor_at_position(&bounds, *position)
                    {
                        // Throttle selection updates to avoid excessive redraws.
                        if let Some(last) = state.last_drag_update
                            && last.elapsed() < std::time::Duration::from_millis(8)
                        {
                            return;
                        }
                        if state.last_drag_anchor == Some((point, side)) {
                            return;
                        }
                        state.last_drag_anchor = Some((point, side));
                        state.last_drag_update = Some(std::time::Instant::now());
                        self.update_selection(point, side);
                        shell.request_redraw();
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if state.is_selecting =>
            {
                state.is_selecting = false;
                state.last_drag_anchor = None;
                state.last_drag_update = None;
                state.last_auto_scroll = None;
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                // Focus the terminal on scroll
                state.is_focused = true;

                if self.mouse_reporting_enabled() {
                    if let Some(bytes) =
                        self.mouse_wheel_report(bounds, cursor.position(), metrics, delta)
                    {
                        shell.publish((self.on_input)(bytes));
                    }
                    return;
                }

                // Check if in alternate screen mode (vim, htop, etc.) - no scrollback there
                let (in_alt_screen, alternate_scroll) = {
                    let term = self.term.lock();
                    (
                        term.mode().contains(TermMode::ALT_SCREEN),
                        term.mode().contains(TermMode::ALTERNATE_SCROLL),
                    )
                };

                if !in_alt_screen {
                    // Calculate scroll lines from delta
                    let lines = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => {
                            // Reset pixel accumulator on line-based scroll
                            state.scroll_pixels = 0.0;
                            state.scroll_lines += y * self.scroll_speed;
                            let lines = state.scroll_lines as i32;
                            state.scroll_lines -= lines as f32;
                            lines
                        }
                        mouse::ScrollDelta::Pixels { y, .. } => {
                            state.scroll_lines = 0.0;
                            // Accumulate pixels for smooth trackpad scrolling
                            state.scroll_pixels += y * self.scroll_speed;
                            let line_height = metrics.cell_height;
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
                } else if alternate_scroll && let Some(bytes) = alternate_scroll_sequence(delta) {
                    shell.publish((self.on_input)(bytes));
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) if state.is_focused => {
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
                    if let Some(text_content) = self.selected_text()
                        && !text_content.is_empty()
                    {
                        clipboard.write(iced::advanced::clipboard::Kind::Standard, text_content);
                    }
                    return;
                }

                if is_paste_shortcut {
                    if let Some(on_paste) = &self.on_paste {
                        shell.publish(on_paste());
                        return;
                    }

                    // Paste from clipboard
                    if let Some(text_content) =
                        clipboard.read(iced::advanced::clipboard::Kind::Standard)
                    {
                        let bytes = {
                            let term = self.term.lock();
                            paste_bytes_for_mode(&text_content, term.mode())
                        };
                        if !bytes.is_empty() {
                            shell.publish((self.on_input)(bytes));
                        }
                    }
                    return;
                }

                if is_select_all {
                    self.select_visible_content();
                    shell.request_redraw();
                    return;
                }

                // Suppress all Super/Logo key combinations to prevent garbage being sent
                // Super key on Linux is often intercepted by window manager anyway
                if modifiers.logo() {
                    return;
                }

                // Clear selection on any other key press (typing)
                let has_selection = {
                    let term = self.term.lock();
                    term.selection.is_some()
                };
                if !modifiers.control() && has_selection {
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
                        self.clear_selection();
                    }
                }

                let app_cursor = {
                    let term = self.term.lock();
                    term.mode().contains(TermMode::APP_CURSOR)
                };

                if let Some(bytes) =
                    key_to_escape_sequence(key, *modifiers, text.as_deref(), app_cursor)
                {
                    shell.publish((self.on_input)(bytes));

                    // Scroll back to bottom when user types (after scrolling up in history)
                    let mut term = self.term.lock();
                    term.scroll_display(Scroll::Bottom);
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

/// Convert a keyboard key to terminal escape sequence.
fn key_to_escape_sequence(
    key: &Key,
    modifiers: Modifiers,
    text: Option<&str>,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    if modifiers.control() && matches!(key, Key::Named(keyboard::key::Named::Space)) {
        return Some(vec![0]);
    }

    // Handle Ctrl+key combinations
    if modifiers.control()
        && let Key::Character(c) = key
    {
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

    // Handle special keys
    match key {
        Key::Named(named) => named_key_sequence(*named, modifiers, app_cursor),
        Key::Character(_) => {
            // Use the text representation for regular characters.
            let bytes = text.map(|t| t.as_bytes().to_vec())?;
            if modifiers.alt() && !bytes.starts_with(b"\x1b") {
                let mut escaped = Vec::with_capacity(bytes.len() + 1);
                escaped.push(0x1b);
                escaped.extend_from_slice(&bytes);
                Some(escaped)
            } else {
                Some(bytes)
            }
        }
        _ => None,
    }
}

fn named_key_sequence(
    named: keyboard::key::Named,
    modifiers: Modifiers,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    let seq = match named {
        keyboard::key::Named::Enter => {
            if modifiers.alt() {
                b"\x1b\r".to_vec()
            } else {
                b"\r".to_vec()
            }
        }
        keyboard::key::Named::Backspace => {
            if modifiers.alt() {
                b"\x1b\x7f".to_vec()
            } else {
                vec![127]
            }
        }
        keyboard::key::Named::Tab => {
            if modifiers.shift() {
                b"\x1b[Z".to_vec()
            } else if modifiers.alt() {
                b"\x1b\t".to_vec()
            } else {
                b"\t".to_vec()
            }
        }
        keyboard::key::Named::Escape => vec![27],
        keyboard::key::Named::ArrowUp => cursor_key_sequence(b'A', modifiers, app_cursor),
        keyboard::key::Named::ArrowDown => cursor_key_sequence(b'B', modifiers, app_cursor),
        keyboard::key::Named::ArrowRight => cursor_key_sequence(b'C', modifiers, app_cursor),
        keyboard::key::Named::ArrowLeft => cursor_key_sequence(b'D', modifiers, app_cursor),
        keyboard::key::Named::Home => csi_final_sequence(b'H', modifiers, b"\x1b[H"),
        keyboard::key::Named::End => csi_final_sequence(b'F', modifiers, b"\x1b[F"),
        keyboard::key::Named::PageUp => csi_tilde_sequence(5, modifiers),
        keyboard::key::Named::PageDown => csi_tilde_sequence(6, modifiers),
        keyboard::key::Named::Insert => csi_tilde_sequence(2, modifiers),
        keyboard::key::Named::Delete => csi_tilde_sequence(3, modifiers),
        keyboard::key::Named::F1 => function_key_sequence(b'P', None, modifiers),
        keyboard::key::Named::F2 => function_key_sequence(b'Q', None, modifiers),
        keyboard::key::Named::F3 => function_key_sequence(b'R', None, modifiers),
        keyboard::key::Named::F4 => function_key_sequence(b'S', None, modifiers),
        keyboard::key::Named::F5 => function_key_sequence(0, Some(15), modifiers),
        keyboard::key::Named::F6 => function_key_sequence(0, Some(17), modifiers),
        keyboard::key::Named::F7 => function_key_sequence(0, Some(18), modifiers),
        keyboard::key::Named::F8 => function_key_sequence(0, Some(19), modifiers),
        keyboard::key::Named::F9 => function_key_sequence(0, Some(20), modifiers),
        keyboard::key::Named::F10 => function_key_sequence(0, Some(21), modifiers),
        keyboard::key::Named::F11 => function_key_sequence(0, Some(23), modifiers),
        keyboard::key::Named::F12 => function_key_sequence(0, Some(24), modifiers),
        keyboard::key::Named::Space => b" ".to_vec(),
        _ => return None,
    };
    Some(seq)
}

fn cursor_key_sequence(final_byte: u8, modifiers: Modifiers, app_cursor: bool) -> Vec<u8> {
    let modifier_value = xterm_modifier_value(modifiers);

    if modifier_value > 1 {
        format!("\x1b[1;{}{}", modifier_value, final_byte as char).into_bytes()
    } else if app_cursor {
        vec![0x1b, b'O', final_byte]
    } else {
        vec![0x1b, b'[', final_byte]
    }
}

fn xterm_modifier_value(modifiers: Modifiers) -> u8 {
    1 + u8::from(modifiers.shift())
        + (u8::from(modifiers.alt()) * 2)
        + (u8::from(modifiers.control()) * 4)
}

fn csi_final_sequence(final_byte: u8, modifiers: Modifiers, unmodified: &[u8]) -> Vec<u8> {
    let modifier_value = xterm_modifier_value(modifiers);
    if modifier_value > 1 {
        format!("\x1b[1;{}{}", modifier_value, final_byte as char).into_bytes()
    } else {
        unmodified.to_vec()
    }
}

fn csi_tilde_sequence(number: u8, modifiers: Modifiers) -> Vec<u8> {
    let modifier_value = xterm_modifier_value(modifiers);
    if modifier_value > 1 {
        format!("\x1b[{};{}~", number, modifier_value).into_bytes()
    } else {
        format!("\x1b[{}~", number).into_bytes()
    }
}

fn function_key_sequence(final_byte: u8, number: Option<u8>, modifiers: Modifiers) -> Vec<u8> {
    let modifier_value = xterm_modifier_value(modifiers);
    match (number, modifier_value > 1) {
        (None, false) => vec![0x1b, b'O', final_byte],
        (None, true) => format!("\x1b[1;{}{}", modifier_value, final_byte as char).into_bytes(),
        (Some(number), false) => format!("\x1b[{}~", number).into_bytes(),
        (Some(number), true) => format!("\x1b[{};{}~", number, modifier_value).into_bytes(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::backend::{TerminalBackend, TerminalSize};

    #[test]
    fn cell_metrics_round_to_integer_pixels() {
        let metrics = TerminalMetrics::for_font(TerminalFont::JetBrainsMono, 13.0);

        assert_eq!(metrics.cell_width.fract(), 0.0);
        assert_eq!(metrics.cell_height.fract(), 0.0);
    }

    #[test]
    fn terminal_cell_index_rejects_non_finite_values() {
        assert_eq!(terminal_cell_index(f32::NAN, 12.0), None);
        assert_eq!(terminal_cell_index(24.0, f32::INFINITY), None);
        assert_eq!(terminal_cell_index(24.0, 0.0), None);
    }

    #[test]
    fn terminal_cell_index_clamps_large_values() {
        assert_eq!(terminal_cell_index(f32::MAX, 1.0), Some(u16::MAX as usize));
    }

    #[test]
    fn selection_side_uses_cell_midpoint() {
        assert_eq!(selection_side(4.9, 0, 10.0), Some(Side::Left));
        assert_eq!(selection_side(5.0, 0, 10.0), Some(Side::Right));
        assert_eq!(selection_side(14.9, 1, 10.0), Some(Side::Left));
        assert_eq!(selection_side(15.0, 1, 10.0), Some(Side::Right));
    }

    #[test]
    fn selection_side_rejects_invalid_values() {
        assert_eq!(selection_side(f32::NAN, 0, 10.0), None);
        assert_eq!(selection_side(4.0, 0, f32::INFINITY), None);
        assert_eq!(selection_side(4.0, 0, 0.0), None);
    }

    #[test]
    fn semantic_selection_rect_includes_word_endpoints() {
        let (backend, _events) = TerminalBackend::new(TerminalSize::new(10, 3));
        backend.process_input(b"test");

        let term = backend.term();
        {
            let mut term = term.lock();
            term.selection = Some(Selection::new(
                SelectionType::Semantic,
                Point::new(Line(0), Column(1)),
                Side::Left,
            ));
        }

        let widget = TerminalWidget::<()>::new(term, |_| ());
        let metrics = widget.cell_metrics();
        let rects = widget.selection_rects(
            Rectangle {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 100.0,
            },
            metrics,
        );

        assert_eq!(widget.selected_text(), Some("test".to_string()));
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].x, TERMINAL_PADDING_LEFT);
        assert_eq!(rects[0].y, 0.0);
        assert_eq!(rects[0].width, metrics.cell_width * 4.0);
        assert_eq!(rects[0].height, metrics.cell_height);
    }

    #[test]
    fn arrow_keys_use_normal_cursor_sequences_by_default() {
        assert_eq!(
            key_to_escape_sequence(
                &Key::Named(keyboard::key::Named::ArrowUp),
                Modifiers::NONE,
                None,
                false
            ),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            key_to_escape_sequence(
                &Key::Named(keyboard::key::Named::ArrowLeft),
                Modifiers::NONE,
                None,
                false
            ),
            Some(b"\x1b[D".to_vec())
        );
    }

    #[test]
    fn arrow_keys_follow_application_cursor_mode() {
        assert_eq!(
            key_to_escape_sequence(
                &Key::Named(keyboard::key::Named::ArrowUp),
                Modifiers::NONE,
                None,
                true
            ),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            key_to_escape_sequence(
                &Key::Named(keyboard::key::Named::ArrowRight),
                Modifiers::NONE,
                None,
                true
            ),
            Some(b"\x1bOC".to_vec())
        );
    }

    #[test]
    fn modified_arrow_keys_use_xterm_modifier_sequences() {
        assert_eq!(
            key_to_escape_sequence(
                &Key::Named(keyboard::key::Named::ArrowDown),
                Modifiers::SHIFT,
                None,
                false
            ),
            Some(b"\x1b[1;2B".to_vec())
        );
        assert_eq!(
            key_to_escape_sequence(
                &Key::Named(keyboard::key::Named::ArrowRight),
                Modifiers::CTRL | Modifiers::ALT,
                None,
                true
            ),
            Some(b"\x1b[1;7C".to_vec())
        );
    }

    #[test]
    fn modified_navigation_and_function_keys_use_xterm_sequences() {
        assert_eq!(
            key_to_escape_sequence(
                &Key::Named(keyboard::key::Named::Home),
                Modifiers::CTRL,
                None,
                false
            ),
            Some(b"\x1b[1;5H".to_vec())
        );
        assert_eq!(
            key_to_escape_sequence(
                &Key::Named(keyboard::key::Named::PageDown),
                Modifiers::SHIFT,
                None,
                false
            ),
            Some(b"\x1b[6;2~".to_vec())
        );
        assert_eq!(
            key_to_escape_sequence(
                &Key::Named(keyboard::key::Named::F2),
                Modifiers::ALT,
                None,
                false
            ),
            Some(b"\x1b[1;3Q".to_vec())
        );
    }

    #[test]
    fn alt_character_prefixes_escape() {
        assert_eq!(
            key_to_escape_sequence(
                &Key::Character("x".into()),
                Modifiers::ALT,
                Some("x"),
                false
            ),
            Some(b"\x1bx".to_vec())
        );
    }

    #[test]
    fn ctrl_space_sends_nul() {
        assert_eq!(
            key_to_escape_sequence(
                &Key::Named(keyboard::key::Named::Space),
                Modifiers::CTRL,
                None,
                false
            ),
            Some(vec![0])
        );
    }

    #[test]
    fn sgr_mouse_reports_press_release_and_motion() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            mouse_report_sequence(mode, 0, 4, 2, MouseReportKind::Press, Modifiers::NONE),
            Some(b"\x1b[<0;5;3M".to_vec())
        );
        assert_eq!(
            mouse_report_sequence(mode, 0, 4, 2, MouseReportKind::Release, Modifiers::NONE),
            Some(b"\x1b[<0;5;3m".to_vec())
        );
        assert_eq!(
            mouse_report_sequence(
                TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE,
                35,
                4,
                2,
                MouseReportKind::Motion,
                Modifiers::NONE
            ),
            Some(b"\x1b[<35;5;3M".to_vec())
        );
    }

    #[test]
    fn legacy_mouse_report_uses_x10_coordinates() {
        assert_eq!(
            mouse_report_sequence(
                TermMode::MOUSE_REPORT_CLICK,
                0,
                4,
                2,
                MouseReportKind::Press,
                Modifiers::NONE
            ),
            Some(vec![0x1b, b'[', b'M', 32, 37, 35])
        );
    }

    #[test]
    fn alternate_scroll_maps_wheel_to_cursor_keys() {
        assert_eq!(
            alternate_scroll_sequence(&mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 }),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            alternate_scroll_sequence(&mouse::ScrollDelta::Pixels { x: 0.0, y: -30.0 }),
            Some(b"\x1b[B".to_vec())
        );
    }

    #[test]
    fn focus_reports_follow_requested_mode() {
        assert_eq!(focus_report_sequence(TermMode::default(), true), None);
        assert_eq!(
            focus_report_sequence(TermMode::FOCUS_IN_OUT, true),
            Some(b"\x1b[I".to_vec())
        );
        assert_eq!(
            focus_report_sequence(TermMode::FOCUS_IN_OUT, false),
            Some(b"\x1b[O".to_vec())
        );
    }
}
