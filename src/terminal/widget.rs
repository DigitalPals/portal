//! Custom iced widget for terminal rendering
//!
//! This implements the iced Widget trait for rendering terminal content.

use std::sync::Arc;
use std::time::{Duration, Instant};

use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::CursorShape;
use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer::{self, Quad};
use iced::advanced::widget::{self, Tree, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::keyboard::{self, Key, Modifiers};
use iced::mouse::{self, Cursor};
use iced::{Background, Border, Color, Element, Event, Length, Rectangle, Shadow, Size};
use parking_lot::Mutex;

use super::backend::{CursorInfo, EventProxy, RenderCell, TerminalSize};
use super::colors::{ansi_to_iced, DEFAULT_BG, DEFAULT_FG};

/// Default cell dimensions
const CELL_WIDTH: f32 = 9.0;
const CELL_HEIGHT: f32 = 18.0;

/// Cursor blink interval
const CURSOR_BLINK_INTERVAL: Duration = Duration::from_millis(500);

/// Terminal widget for iced
pub struct TerminalWidget<'a, Message> {
    term: Arc<Mutex<Term<EventProxy>>>,
    size: TerminalSize,
    on_input: Box<dyn Fn(Vec<u8>) -> Message + 'a>,
    font_size: f32,
}

impl<'a, Message> TerminalWidget<'a, Message> {
    /// Create a new terminal widget
    pub fn new(
        term: Arc<Mutex<Term<EventProxy>>>,
        size: TerminalSize,
        on_input: impl Fn(Vec<u8>) -> Message + 'a,
    ) -> Self {
        Self {
            term,
            size,
            on_input: Box::new(on_input),
            font_size: 14.0,
        }
    }

    /// Set font size
    pub fn font_size(mut self, size: f32) -> Self {
        self.font_size = size;
        self
    }

    /// Get renderable cells from the terminal
    fn get_cells(&self) -> Vec<RenderCell> {
        use alacritty_terminal::vte::ansi::NamedColor;

        let term = self.term.lock();
        let content = term.renderable_content();
        let mut cells = Vec::new();

        for indexed in content.display_iter {
            let cell = &indexed.cell;
            // Include cells with content or non-default background
            if cell.c != ' '
                || cell.bg
                    != alacritty_terminal::vte::ansi::Color::Named(NamedColor::Background)
                || !cell.flags.is_empty()
            {
                cells.push(RenderCell {
                    column: indexed.point.column.0,
                    line: indexed.point.line.0 as usize,
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
    fn get_cursor(&self) -> CursorInfo {
        let term = self.term.lock();
        let content = term.renderable_content();
        let cursor = content.cursor;

        CursorInfo {
            column: cursor.point.column.0,
            line: cursor.point.line.0 as usize,
            shape: cursor.shape,
            visible: true, // Cursor visibility handled by mode flags
        }
    }
}

/// Widget state stored in the tree
#[derive(Debug)]
struct TerminalState {
    is_focused: bool,
    cursor_visible: bool,
    last_blink: Instant,
}

impl Default for TerminalState {
    fn default() -> Self {
        Self {
            is_focused: true,
            cursor_visible: true,
            last_blink: Instant::now(),
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
        &self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits
            .width(Length::Fill)
            .height(Length::Fill)
            .resolve(self.size.pixel_width(), self.size.pixel_height(), Size::ZERO);

        layout::Node::new(size)
    }

    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<TerminalState>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(TerminalState {
            is_focused: true,
            cursor_visible: true,
            last_blink: Instant::now(),
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

        // Draw background
        renderer.fill_quad(
            Quad {
                bounds,
                border: Border::default(),
                shadow: Shadow::default(),
            },
            Background::Color(DEFAULT_BG),
        );

        let cell_width = self.size.cell_width;
        let cell_height = self.size.cell_height;

        // Draw cells
        let cells = self.get_cells();
        for cell in cells {
            let x = bounds.x + cell.column as f32 * cell_width;
            let y = bounds.y + cell.line as f32 * cell_height;

            // Draw cell background if not default
            let bg_color = ansi_to_iced(cell.bg);
            if bg_color != DEFAULT_BG {
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
                    },
                    Background::Color(bg_color),
                );
            }

            // Draw character
            if cell.character != ' ' {
                let mut fg_color = ansi_to_iced(cell.fg);

                // Handle flags
                if cell.flags.contains(CellFlags::DIM) {
                    fg_color = Color::from_rgba(
                        fg_color.r * 0.66,
                        fg_color.g * 0.66,
                        fg_color.b * 0.66,
                        fg_color.a,
                    );
                }

                if cell.flags.contains(CellFlags::INVERSE) {
                    // Use bg color as fg (inverse)
                    fg_color = ansi_to_iced(cell.bg);
                }

                // Draw the character using text renderer
                let text = iced::advanced::Text {
                    content: cell.character.to_string(),
                    bounds: Size::new(cell_width, cell_height),
                    size: iced::Pixels(self.font_size),
                    line_height: iced::advanced::text::LineHeight::Absolute(iced::Pixels(
                        cell_height,
                    )),
                    font: iced::Font::MONOSPACE,
                    horizontal_alignment: iced::alignment::Horizontal::Left,
                    vertical_alignment: iced::alignment::Vertical::Top,
                    shaping: iced::advanced::text::Shaping::Basic,
                    wrapping: iced::advanced::text::Wrapping::None,
                };

                renderer.fill_text(
                    text,
                    iced::Point::new(x, y),
                    fg_color,
                    bounds,
                );
            }
        }

        // Draw cursor
        if state.is_focused && state.cursor_visible {
            let cursor_info = self.get_cursor();
            if cursor_info.visible {
                let cursor_x = bounds.x + cursor_info.column as f32 * cell_width;
                let cursor_y = bounds.y + cursor_info.line as f32 * cell_height;

                let cursor_color = DEFAULT_FG;

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
                            },
                            Background::Color(Color::TRANSPARENT),
                        );
                    }
                }
            }
        }
    }

    fn on_event(
        &mut self,
        tree: &mut Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) -> iced::event::Status {
        let state = tree.state.downcast_mut::<TerminalState>();
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds) {
                    state.is_focused = true;
                    return iced::event::Status::Captured;
                } else {
                    state.is_focused = false;
                }
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                if state.is_focused && cursor.is_over(bounds) {
                    // Handle scroll - could emit message for scrollback
                    return iced::event::Status::Captured;
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => {
                if state.is_focused {
                    if let Some(bytes) = key_to_escape_sequence(&key, modifiers, text.as_deref()) {
                        shell.publish((self.on_input)(bytes));
                        return iced::event::Status::Captured;
                    }
                }
            }
            _ => {}
        }

        iced::event::Status::Ignored
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<TerminalState>();
        if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

/// Convert a keyboard key to terminal escape sequence
fn key_to_escape_sequence(
    key: &Key,
    modifiers: Modifiers,
    text: Option<&str>,
) -> Option<Vec<u8>> {
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
                keyboard::key::Named::Tab => b"\t".to_vec(),
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
