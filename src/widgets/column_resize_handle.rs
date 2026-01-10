//! Column resize handle widget for detecting drag operations on column borders

use iced::advanced::layout::{self, Layout};
use iced::advanced::mouse;
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell};
use iced::{Element, Event, Length, Rectangle, Size};

/// State for tracking drag operations
#[derive(Default)]
struct State {
    is_dragging: bool,
}

/// An invisible widget that detects horizontal drag for column resizing
pub struct ColumnResizeHandle<'a, Message> {
    width: f32,
    height: f32,
    on_drag_start: Option<Box<dyn Fn(f32) -> Message + 'a>>,
    on_drag: Option<Box<dyn Fn(f32) -> Message + 'a>>,
    on_drag_end: Option<Message>,
}

impl<'a, Message> ColumnResizeHandle<'a, Message> {
    /// Creates a new column resize handle
    pub fn new() -> Self {
        Self {
            width: 8.0,
            height: 20.0, // Fixed height to prevent row expansion
            on_drag_start: None,
            on_drag: None,
            on_drag_end: None,
        }
    }

    /// Sets the callback for when a drag operation starts
    pub fn on_drag_start<F>(mut self, f: F) -> Self
    where
        F: Fn(f32) -> Message + 'a,
    {
        self.on_drag_start = Some(Box::new(f));
        self
    }

    /// Sets the callback for during drag operations
    pub fn on_drag<F>(mut self, f: F) -> Self
    where
        F: Fn(f32) -> Message + 'a,
    {
        self.on_drag = Some(Box::new(f));
        self
    }

    /// Sets the message to emit when the drag operation ends
    pub fn on_drag_end(mut self, message: Message) -> Self {
        self.on_drag_end = Some(message);
        self
    }
}

impl<'a, Message> Default for ColumnResizeHandle<'a, Message> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ColumnResizeHandle<'a, Message>
where
    Message: Clone,
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Length::Fixed(self.width),
            height: Length::Fixed(self.height),
        }
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(Size::new(self.width, self.height))
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        _renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        // Invisible widget - no drawing needed
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if cursor.is_over(bounds) {
                    state.is_dragging = true;
                    if let Some(ref on_drag_start) = self.on_drag_start {
                        if let Some(pos) = cursor.position() {
                            shell.publish(on_drag_start(pos.x));
                            shell.capture_event();
                        }
                    }
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                if state.is_dragging {
                    state.is_dragging = false;
                    if let Some(ref message) = self.on_drag_end {
                        shell.publish(message.clone());
                        shell.capture_event();
                    }
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                if state.is_dragging {
                    if let Some(ref on_drag) = self.on_drag {
                        if let Some(pos) = cursor.position() {
                            shell.publish(on_drag(pos.x));
                            shell.capture_event();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();

        // Show resize cursor when dragging or hovering over the handle
        if state.is_dragging || cursor.is_over(layout.bounds()) {
            mouse::Interaction::ResizingHorizontally
        } else {
            mouse::Interaction::default()
        }
    }
}

impl<'a, Message, Theme, Renderer> From<ColumnResizeHandle<'a, Message>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(handle: ColumnResizeHandle<'a, Message>) -> Self {
        Element::new(handle)
    }
}

/// Helper function to create a column resize handle
pub fn column_resize_handle<'a, Message>() -> ColumnResizeHandle<'a, Message> {
    ColumnResizeHandle::new()
}
