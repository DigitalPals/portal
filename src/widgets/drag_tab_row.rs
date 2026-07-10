//! A wrapper around the tab row that enables drag-to-reorder.
//!
//! Tracks a left-button drag started over one of the first `drag_count`
//! children of the wrapped row and emits reorder messages live as the cursor
//! crosses the horizontal midpoint of sibling tabs, so tabs shuffle under the
//! cursor while dragging (browser-style).

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// Horizontal movement (px) required before a press turns into a drag,
/// so plain clicks keep selecting tabs.
const DRAG_THRESHOLD: f32 = 5.0;

#[derive(Default)]
struct State {
    drag: Option<Drag>,
}

struct Drag {
    /// Current index of the dragged tab (updated as reorders are emitted).
    index: usize,
    press_x: f32,
    active: bool,
}

/// A wrapper widget that makes the leading `drag_count` children of the
/// wrapped row draggable for reordering.
pub struct DragTabRow<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Renderer: renderer::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    drag_count: usize,
    on_reorder: Option<Box<dyn Fn(usize, usize) -> Message + 'a>>,
}

impl<'a, Message, Theme, Renderer> DragTabRow<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// Creates a new [`DragTabRow`]. Only the first `drag_count` children of
    /// `content` participate in dragging (trailing children like a "+" button
    /// are ignored).
    pub fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        drag_count: usize,
    ) -> Self {
        Self {
            content: content.into(),
            drag_count,
            on_reorder: None,
        }
    }

    /// Sets the function producing the message emitted when the dragged tab
    /// should move `from` one index `to` another.
    pub fn on_reorder<F>(mut self, f: F) -> Self
    where
        F: Fn(usize, usize) -> Message + 'a,
    {
        self.on_reorder = Some(Box::new(f));
        self
    }
}

impl<'a, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for DragTabRow<'a, Message, Theme, Renderer>
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

        let Some(on_reorder) = &self.on_reorder else {
            return;
        };
        let state = tree.state.downcast_mut::<State>();

        match event {
            // Tab buttons capture presses, so record a potential drag even
            // when the event is already captured.
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position() {
                    state.drag = layout
                        .children()
                        .take(self.drag_count)
                        .position(|child| child.bounds().contains(position))
                        .map(|index| Drag {
                            index,
                            press_x: position.x,
                            active: false,
                        });
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { position, .. }) => {
                let Some(drag) = &mut state.drag else {
                    return;
                };
                if !drag.active && (position.x - drag.press_x).abs() < DRAG_THRESHOLD {
                    return;
                }
                drag.active = true;

                // Move to the farthest sibling whose midpoint the cursor has
                // crossed. Checking midpoints keeps the swap stable: right
                // after a reorder the cursor sits inside the dragged tab, not
                // past a neighbor's midpoint.
                let mut target = drag.index;
                for (index, child) in layout.children().take(self.drag_count).enumerate() {
                    let mid = child.bounds().center_x();
                    if index < drag.index && position.x < mid {
                        target = target.min(index);
                    } else if index > drag.index && position.x > mid {
                        target = target.max(index);
                    }
                }
                if target != drag.index {
                    shell.publish(on_reorder(drag.index, target));
                    drag.index = target;
                }
                shell.capture_event();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag = None;
            }
            _ => {}
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
        let state = tree.state.downcast_ref::<State>();
        if state.drag.as_ref().is_some_and(|drag| drag.active) {
            return mouse::Interaction::Grabbing;
        }

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

impl<'a, Message, Theme, Renderer> From<DragTabRow<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(drag_tab_row: DragTabRow<'a, Message, Theme, Renderer>) -> Self {
        Element::new(drag_tab_row)
    }
}

/// Helper function to create a [`DragTabRow`].
pub fn drag_tab_row<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    drag_count: usize,
) -> DragTabRow<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    DragTabRow::new(content, drag_count)
}
