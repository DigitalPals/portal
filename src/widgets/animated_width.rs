//! Clips content while easing its occupied width from zero to its intrinsic width.
//! This lets newly-created tabs push their siblings aside instead of popping in.

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{self, Widget};
use iced::advanced::{Clipboard, Shell, mouse};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

pub struct AnimatedWidth<'a, Message, Theme = iced::Theme, Renderer = iced::Renderer>
where
    Renderer: renderer::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    progress: f32,
}

impl<'a, Message, Theme, Renderer> AnimatedWidth<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>, progress: f32) -> Self {
        Self {
            content: content.into(),
            progress: progress.clamp(0.0, 1.0),
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for AnimatedWidth<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
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
        let child = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &limits.loose().width(Length::Shrink),
        );
        let child_size = child.size();
        let size = limits.resolve(
            Length::Shrink,
            Length::Shrink,
            Size::new(child_size.width * self.progress, child_size.height),
        );
        layout::Node::with_children(size, vec![child])
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
        let Some(clip) = layout.bounds().intersection(viewport) else {
            return;
        };
        let child_layout = layout.children().next().expect("animated width child");
        renderer.with_layer(clip, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                &clip,
            );
        });
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
        let child_layout = layout.children().next().expect("animated width child");
        let child_cursor = if matches!(event, Event::Mouse(_) | Event::Touch(_))
            && !cursor.is_over(layout.bounds())
        {
            mouse::Cursor::Unavailable
        } else {
            cursor
        };
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child_layout,
            child_cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if !cursor.is_over(layout.bounds()) {
            return mouse::Interaction::default();
        }
        let child_layout = layout.children().next().expect("animated width child");
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            child_layout,
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
        let child_layout = layout.children().next().expect("animated width child");
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            child_layout,
            renderer,
            operation,
        );
    }

    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut widget::Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<iced::advanced::overlay::Element<'a, Message, Theme, Renderer>> {
        let child_layout = layout.children().next().expect("animated width child");
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child_layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<AnimatedWidth<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(animated: AnimatedWidth<'a, Message, Theme, Renderer>) -> Self {
        Element::new(animated)
    }
}

pub fn animated_width<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
    progress: f32,
) -> AnimatedWidth<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    AnimatedWidth::new(content, progress)
}
