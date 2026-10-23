//! Preserve the standard Source widget while routing IME through stable intent.

use iced::advanced::widget::{self, Tree};
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, input_method, layout, mouse, overlay, renderer,
};
use iced::widget::text_editor::{Action, Edit};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use crate::app::{Message, WritingMessage};
use crate::document::collaboration::{Collaboration, CompositionTarget};
use crate::document::selection::Selection;

pub(crate) struct SourceEditor<'a> {
    child: Element<'a, Message>,
    collaboration: &'a Collaboration,
    selection: Selection,
}
struct State {
    owner: (uuid::Uuid, u64),
    target: Option<CompositionTarget>,
}

impl<'a> SourceEditor<'a> {
    pub fn new(
        child: Element<'a, Message>,
        collaboration: &'a Collaboration,
        selection: Selection,
    ) -> Self {
        Self {
            child,
            collaboration,
            selection,
        }
    }
    fn reconcile(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State>();
        if state.owner != (self.collaboration.id(), self.collaboration.replica_id()) {
            state.owner = (self.collaboration.id(), self.collaboration.replica_id());
            state.target = None;
            tree.children = vec![Tree::new(self.child.as_widget())];
        }
    }
}
impl Widget<Message, Theme, Renderer> for SourceEditor<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State {
            owner: (self.collaboration.id(), self.collaboration.replica_id()),
            target: None,
        })
    }
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.child.as_widget())]
    }
    fn diff(&self, tree: &mut Tree) {
        self.reconcile(tree);
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
        self.reconcile(tree);
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
        self.reconcile(tree);
        let state = tree.state.downcast_mut::<State>();
        match event {
            Event::InputMethod(input_method::Event::Opened | input_method::Event::Closed) => {
                state.target = None
            }
            Event::InputMethod(input_method::Event::Preedit(content, _))
                if !content.is_empty() && state.target.is_none() =>
            {
                state.target = Some(self.collaboration.composition_target(self.selection));
            }
            _ => {}
        }
        if matches!(event, Event::InputMethod(input_method::Event::Commit(_))) {
            let target = state
                .target
                .take()
                .unwrap_or_else(|| self.collaboration.composition_target(self.selection));
            let mut messages = Vec::new();
            let mut child_shell = Shell::new(&mut messages);
            self.child.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                &mut child_shell,
                viewport,
            );
            // Preserve native focus, preedit, redraw, and layout handling. Replace
            // its one paste message, rather than adding a second commit message.
            shell.merge(child_shell, |message| match message {
                Message::Edit(Action::Edit(Edit::Paste(text))) => Message::CommitComposition {
                    target: target.clone(),
                    text: (*text).clone(),
                },
                message => message,
            });
        } else if matches!(event, Event::Keyboard(iced::keyboard::Event::KeyPressed { key: iced::keyboard::Key::Character(character), modifiers, .. }) if modifiers.command() && character.eq_ignore_ascii_case("v"))
        {
            let fallback = clipboard.read(iced::advanced::clipboard::Kind::Standard);
            let mut messages = Vec::new();
            let mut child_shell = Shell::new(&mut messages);
            self.child.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                &mut child_shell,
                viewport,
            );
            shell.merge(child_shell, |message| match message {
                Message::Writing(WritingMessage::Paste(None)) => {
                    Message::Writing(WritingMessage::Paste(fallback.clone()))
                }
                other => other,
            });
        } else {
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
    }
    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
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
impl<'a> From<SourceEditor<'a>> for Element<'a, Message> {
    fn from(editor: SourceEditor<'a>) -> Self {
        Self::new(editor)
    }
}
