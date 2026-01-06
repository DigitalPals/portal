//! A custom mouse area widget that detects left and right clicks with modifier keys

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{mouse, Clipboard, Shell};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// Local state of the [`MouseArea`].
#[derive(Default)]
struct State;

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
    capture_all_events: bool,
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
            capture_all_events: false,
            width: Length::Fill,
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

    /// Captures all mouse events while the cursor is over the area.
    pub fn capture_all_events(mut self, capture: bool) -> Self {
        self.capture_all_events = capture;
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
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State)
    }

    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        // First, let the content handle the event
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        // If the content captured the event, don't process further
        if shell.is_event_captured() {
            return;
        }

        // Handle right-click with position callback
        if let Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) = event {
            if cursor.is_over(layout.bounds()) {
                if let Some(ref on_right_press) = self.on_right_press {
                    if let Some(pos) = cursor.position() {
                        shell.publish(on_right_press(pos.x, pos.y));
                        shell.capture_event();
                        return;
                    }
                }
            }
        }

        // Capture all events if requested
        if self.capture_all_events && cursor.is_over(layout.bounds()) && matches!(event, Event::Mouse(_)) {
            shell.capture_event();
            return;
        }

        // Handle left-click
        if let Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) = event {
            if cursor.is_over(layout.bounds()) {
                if let Some(ref message) = self.on_press {
                    shell.publish(message.clone());
                }
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut widget::Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
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
