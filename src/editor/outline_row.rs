//! Drag handles and drop indicators around ordinary outline buttons.

use iced::advanced::widget::{self, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use crate::app::{Message, OutlineMessage};
use crate::document::outline::DropTarget;
use crate::theme::Tokens;

pub(crate) struct OutlineRow<'a> {
    child: Element<'a, Message>,
    index: usize,
    dragging: bool,
    hover: Option<DropTarget>,
    indicator: Option<bool>,
    tokens: Tokens,
}

impl<'a> OutlineRow<'a> {
    pub fn new(
        child: Element<'a, Message>,
        index: usize,
        dragging: bool,
        hover: Option<DropTarget>,
        tokens: Tokens,
    ) -> Self {
        Self {
            child,
            index,
            dragging,
            hover,
            indicator: None,
            tokens,
        }
    }

    pub fn indicator(mut self, after: Option<bool>) -> Self {
        self.indicator = after;
        self
    }
}

impl Widget<Message, Theme, Renderer> for OutlineRow<'_> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.child.as_widget())]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(self.child.as_widget());
    }
    fn size(&self) -> Size<Length> {
        self.child.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.child
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.child
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let over = cursor.is_over(bounds) && cursor.is_over(*viewport);
        let handle = Rectangle {
            x: bounds.x + bounds.width - 22.0,
            width: 22.0,
            ..bounds
        };
        let target = cursor.position().map(|point| DropTarget {
            section: self.index,
            after: point.y >= bounds.center_y(),
        });
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if over && cursor.is_over(handle) =>
            {
                shell.publish(Message::Outline(OutlineMessage::BeginDrag(self.index)));
                shell.capture_event();
                return;
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if self.dragging => {
                if over && let Some(target) = target {
                    if self.hover != Some(target) {
                        shell.publish(Message::Outline(OutlineMessage::Hover(target)));
                    }
                } else if self
                    .hover
                    .is_some_and(|target| target.section == self.index)
                {
                    shell.publish(Message::Outline(OutlineMessage::Leave(self.index)));
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if self.dragging && over =>
            {
                if let Some(target) = target {
                    shell.publish(Message::Outline(OutlineMessage::Drop(target)));
                }
                shell.capture_event();
                return;
            }
            _ => {}
        }
        self.child.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.child.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        if let Some(after) = self.indicator {
            use iced::advanced::Renderer as _;
            let bounds = layout.bounds();
            let indicator = Rectangle {
                y: if after {
                    bounds.y + bounds.height - 2.0
                } else {
                    bounds.y
                },
                height: 2.0,
                ..bounds
            };
            if let Some(bounds) = indicator.intersection(viewport) {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        ..renderer::Quad::default()
                    },
                    self.tokens.accent,
                );
            }
        }
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        if self.dragging {
            return mouse::Interaction::Grabbing;
        }
        if cursor.is_over(*viewport)
            && cursor.is_over(Rectangle {
                x: bounds.x + bounds.width - 22.0,
                width: 22.0,
                ..bounds
            })
        {
            return mouse::Interaction::Grab;
        }
        self.child.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Message, Theme, Renderer>> {
        self.child.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<OutlineRow<'a>> for Element<'a, Message> {
    fn from(row: OutlineRow<'a>) -> Self {
        Self::new(row)
    }
}
