//! A custom mouse area widget that detects left and right clicks with modifier keys

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, Clipboard, Shell};
use iced::event::Status;
use iced::{Element, Event, Length, Rectangle, Size};

/// A wrapper widget that detects mouse clicks and modifier keys
pub struct MouseArea<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Renderer: renderer::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    on_press: Option<Message>,
    on_right_press: Option<Box<dyn Fn(f32, f32) -> Message + 'a>>,
    on_ctrl_press: Option<Message>,
    on_shift_press: Option<Message>,
    width: Length,
    height: Length,
}

impl<'a, Message, Theme, Renderer> MouseArea<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// Creates a new [`MouseArea`] with the given content.
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            on_press: None,
            on_right_press: None,
            on_ctrl_press: None,
            on_shift_press: None,
            width: Length::Shrink,
            height: Length::Shrink,
        }
    }

    /// Sets the width of the [`MouseArea`].
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the height of the [`MouseArea`].
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Sets the message to emit on left click
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// Sets the function to call on right click, passing cursor position
    pub fn on_right_press<F>(mut self, f: F) -> Self
    where
        F: Fn(f32, f32) -> Message + 'a,
    {
        self.on_right_press = Some(Box::new(f));
        self
    }

    /// Sets the message to emit on Ctrl+click
    pub fn on_ctrl_press(mut self, message: Message) -> Self {
        self.on_ctrl_press = Some(message);
        self
    }

    /// Sets the message to emit on Shift+click
    pub fn on_shift_press(mut self, message: Message) -> Self {
        self.on_shift_press = Some(message);
        self
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for MouseArea<'a, Message, Theme, Renderer>
where
    Message: Clone,
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        self.content.as_widget().tag()
    }

    fn state(&self) -> widget::tree::State {
        self.content.as_widget().state()
    }

    fn children(&self) -> Vec<widget::Tree> {
        self.content.as_widget().children()
    }

    fn diff(&self, tree: &mut widget::Tree) {
        self.content.as_widget().diff(tree);
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(self.width).height(self.height);
        self.content.as_widget().layout(tree, renderer, &limits)
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .as_widget()
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn on_event(
        &mut self,
        tree: &mut widget::Tree,
        event: Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) -> Status {
        // Handle right-click BEFORE passing to content, since buttons capture all clicks
        if let Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Right)) = event {
            if cursor.is_over(layout.bounds()) {
                if let Some(ref on_right_press) = self.on_right_press {
                    // Use cursor position in window coordinates
                    if let Some(pos) = cursor.position() {
                        shell.publish(on_right_press(pos.x, pos.y));
                        return Status::Captured;
                    }
                }
            }
        }

        // Let the content handle other events
        let content_status = self.content.as_widget_mut().on_event(
            tree, event.clone(), layout, cursor, renderer, clipboard, shell, viewport,
        );

        // If content handled it, we're done
        if content_status == Status::Captured {
            return Status::Captured;
        }

        // Handle left-click if content didn't handle it
        if let Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) = event {
            if let Some(_position) = cursor.position_over(layout.bounds()) {
                if let Some(ref message) = self.on_press {
                    shell.publish(message.clone());
                    return Status::Captured;
                }
            }
        }

        content_status
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content
            .as_widget()
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn operate(
        &self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget()
            .operate(tree, layout, renderer, operation);
    }
}

impl<'a, Message, Theme, Renderer> From<MouseArea<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(mouse_area: MouseArea<'a, Message, Theme, Renderer>) -> Self {
        Element::new(mouse_area)
    }
}

/// Helper function to create a mouse area
pub fn mouse_area<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> MouseArea<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    MouseArea::new(content)
}
