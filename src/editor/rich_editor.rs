//! One focus, selection, clipboard, and input-method surface over rich layout.
//!
//! Editing actions still go through Iced's native buffer and the lossless
//! projection. Geometry-dependent actions use the exact paragraphs we draw.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use iced::advanced::image::{self, Renderer as _};
use iced::advanced::text::{self, Renderer as _};
use iced::advanced::widget::{self, operation};
use iced::advanced::{Clipboard, Layout, Shell, Widget, input_method, layout, mouse, renderer};
use iced::keyboard;
use iced::widget::text_editor::{Action, Binding, Content, Edit, KeyPress, Motion, Status};
use iced::{Element, Event, Length, Point, Rectangle, Renderer, Size, Theme, Vector, window};
use unicode_segmentation::UnicodeSegmentation;

use crate::app::Message;
use crate::document::projection::{HighlightLines, Projection};
use crate::document::selection::Selection;
use crate::editor::rich_layout::{Decoration, FragmentKind, LayoutInput};
use crate::editor::virtual_layout::VirtualLayout;
use crate::io::image_store::ImageStore;
use crate::theme::{ResolvedTheme, Tokens, Typography};

pub struct RichEditor<'a> {
    content: &'a Content,
    projection: &'a Projection,
    source: &'a str,
    theme: &'a ResolvedTheme,
    images: &'a ImageStore,
    base: Option<&'a Path>,
    zoom: f32,
    source_selection: Selection,
    app_focused: bool,
    key_binding: fn(KeyPress) -> Option<Binding<Message>>,
}

impl<'a> RichEditor<'a> {
    #[cfg(test)]
    pub(crate) fn profile_stats(tree: &widget::Tree) -> (usize, usize) {
        let state = tree.state.downcast_ref::<State>();
        (
            state.rich.resident().fragments.len(),
            state.rich.resident().rows.len(),
        )
    }

    pub fn new(
        content: &'a Content,
        projection: &'a Projection,
        source: &'a str,
        theme: &'a ResolvedTheme,
        images: &'a ImageStore,
        key_binding: fn(KeyPress) -> Option<Binding<Message>>,
    ) -> Self {
        Self {
            content,
            projection,
            source,
            theme,
            images,
            base: None,
            zoom: 1.0,
            source_selection: Selection::default(),
            app_focused: false,
            key_binding,
        }
    }

    pub fn base(mut self, base: Option<&'a Path>) -> Self {
        self.base = base;
        self
    }
    pub fn zoom(mut self, zoom: f32) -> Self {
        self.zoom = zoom;
        self
    }

    pub fn selection(mut self, selection: Selection) -> Self {
        self.source_selection = selection;
        self
    }
    pub fn focused(mut self, focused: bool) -> Self {
        self.app_focused = focused;
        self
    }
    fn layout_input(&self, width: f32) -> LayoutInput<'_> {
        LayoutInput {
            projection: self.projection,
            theme: self.theme,
            zoom: self.zoom,
            width: (width - 12.0).max(1.0),
            base: self.base,
            images: self.images,
        }
    }
    fn refresh_viewport(&self, state: &mut State, bounds: Size) {
        state.scroll =
            state
                .rich
                .show_viewport(self.layout_input(bounds.width), state.scroll, bounds.height);
        state.clamp_scroll();
    }

    fn raw_selection(&self, selection: Selection) -> Selection {
        self.projection.normalize_caret(
            self.projection
                .source_selection(selection, self.source_selection),
        )
    }

    fn display_selection(&self) -> Selection {
        self.projection.display_selection(self.source_selection)
    }

    fn word_selection(&self, offset: usize) -> Selection {
        let range = self
            .projection
            .text
            .split_word_bound_indices()
            .find(|(start, word)| *start <= offset && offset < *start + word.len())
            .map_or(offset..offset, |(start, word)| start..start + word.len());
        Selection {
            anchor: range.start,
            head: range.end,
        }
    }

    fn line_selection(&self, offset: usize) -> Selection {
        let text = &self.projection.text;
        Selection {
            anchor: text[..offset].rfind('\n').map_or(0, |n| n + 1),
            head: text[offset..]
                .find('\n')
                .map_or(text.len(), |n| offset + n + 1),
        }
    }

    fn apply_binding(
        &self,
        binding: Binding<Message>,
        state: &mut State,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        bounds: Size,
    ) {
        if !matches!(binding, Binding::Move(_) | Binding::Select(_)) {
            state.preferred_x = None;
        }
        match binding {
            Binding::Unfocus => {
                state.unfocus();
                shell.publish(Message::EditorFocused(false));
            }
            Binding::Copy | Binding::Cut => {
                let selected = self.display_selection().range();
                let has_object = self.projection.rich.images.iter().any(|image| {
                    selected.start <= image.range.start && selected.end >= image.range.end
                }) || self.projection.rich.tables.iter().any(|table| {
                    selected.start <= table.range.start && selected.end >= table.range.end
                });
                let copied = if has_object && !selected.is_empty() {
                    let mut range = if selected == (0..self.projection.text.len()) {
                        0..self.source.len()
                    } else {
                        self.projection.source_range(selected.clone())
                    };
                    for table in &self.projection.rich.tables {
                        if selected.start <= table.range.start && selected.end >= table.range.end {
                            range.start = range.start.min(table.source_range.start);
                            range.end = range.end.max(table.source_range.end);
                        }
                    }
                    Some(self.source[range].to_owned())
                } else {
                    self.content.selection()
                };
                if let Some(text) = copied {
                    clipboard.write(iced::advanced::clipboard::Kind::Standard, text);
                    if matches!(binding, Binding::Cut) {
                        shell.publish(Message::Edit(Action::Edit(Edit::Delete)));
                    }
                }
            }
            Binding::Paste => {
                if let Some(text) = clipboard.read(iced::advanced::clipboard::Kind::Standard) {
                    shell.publish(Message::Edit(Action::Edit(Edit::Paste(Arc::new(text)))));
                }
            }
            Binding::Move(motion) | Binding::Select(motion) => {
                let selecting = matches!(binding, Binding::Select(_));
                if !state.rich.contains_source(self.source_selection.head) {
                    state.scroll = state.rich.show_source(
                        self.layout_input(bounds.width),
                        self.source_selection.head,
                        bounds.height,
                    );
                }
                let caret = state.caret(self.source_selection.head);
                let target = match motion {
                    Motion::Up | Motion::Down | Motion::PageUp | Motion::PageDown => {
                        let x = *state.preferred_x.get_or_insert(caret.x);
                        let direction = if matches!(motion, Motion::Up | Motion::PageUp) {
                            -1.0
                        } else {
                            1.0
                        };
                        let page = matches!(motion, Motion::PageUp | Motion::PageDown)
                            .then_some(bounds.height * 0.85);
                        let (head, y) = state
                            .rich
                            .resident()
                            .vertical_source(caret, x, direction, page)
                            .unwrap_or((self.source_selection.head, caret.y));
                        state.set_visual_cursor(head, y);
                        Some(head)
                    }
                    Motion::Home | Motion::End => {
                        state.preferred_x = None;
                        let head = state.rich.resident().source_line_edge(
                            self.source_selection.head,
                            caret,
                            motion == Motion::End,
                        );
                        state.set_visual_cursor(head, caret.y);
                        Some(head)
                    }
                    _ => {
                        state.preferred_x = None;
                        state.visual_cursor = None;
                        None
                    }
                };
                if let Some(head) = target {
                    let head = self.projection.normalize_caret(Selection::caret(head)).head;
                    shell.publish(Message::SelectSource(Selection {
                        anchor: if selecting {
                            self.source_selection.anchor
                        } else {
                            head
                        },
                        head,
                    }));
                } else {
                    shell.publish(Message::Edit(if selecting {
                        Action::Select(motion)
                    } else {
                        Action::Move(motion)
                    }));
                }
            }
            Binding::SelectWord => shell.publish(Message::SelectSource(
                self.raw_selection(self.word_selection(self.display_selection().head)),
            )),
            Binding::SelectLine => shell.publish(Message::SelectSource(
                self.raw_selection(self.line_selection(self.display_selection().head)),
            )),
            Binding::SelectAll => shell.publish(Message::Edit(Action::SelectAll)),
            Binding::Insert(c) => shell.publish(Message::Edit(Action::Edit(Edit::Insert(c)))),
            Binding::Enter => shell.publish(Message::Edit(Action::Edit(Edit::Enter))),
            Binding::Backspace => shell.publish(Message::Edit(Action::Edit(Edit::Backspace))),
            Binding::Delete => shell.publish(Message::Edit(Action::Edit(Edit::Delete))),
            Binding::Sequence(sequence) => {
                for binding in sequence {
                    self.apply_binding(binding, state, clipboard, shell, bounds);
                }
            }
            Binding::Custom(message) => shell.publish(message),
        }
    }
}

#[derive(Default)]
struct State {
    rich: VirtualLayout,
    layout_key: Option<LayoutKey>,
    focused: bool,
    window_focused: bool,
    preedit: Option<input_method::Preedit>,
    last_click: Option<mouse::Click>,
    drag_anchor: Option<usize>,
    drag_position: Option<Point>,
    modifiers: keyboard::Modifiers,
    preferred_x: Option<f32>,
    visual_cursor: Option<(usize, bool)>,
    scroll: f32,
    scroll_drag: Option<f32>,
    viewport_height: f32,
    last_selection: Option<Selection>,
    blink_started: Option<Instant>,
    now: Option<Instant>,
}

struct LayoutKey {
    lines: Arc<HighlightLines>,
    source: Arc<str>,
    tokens: Tokens,
    typography: Typography,
    width: f32,
    zoom: f32,
    images: u64,
    base: Option<PathBuf>,
}

impl State {
    fn caret(&self, source: usize) -> Rectangle {
        if let Some((_, upstream)) = self.visual_cursor.filter(|(head, _)| *head == source) {
            self.rich.source_caret_on_side(source, upstream)
        } else {
            self.rich.source_caret(source, None)
        }
    }
    fn set_visual_cursor(&mut self, source: usize, y: f32) {
        self.visual_cursor = Some((source, self.rich.resident().upstream_affinity(source, y)));
    }
    fn reset_blink(&mut self) {
        let now = Instant::now();
        self.blink_started = Some(now);
        self.now = Some(now);
    }
    fn caret_visible(&self) -> bool {
        self.focused
            && self.window_focused
            && self.blink_started.zip(self.now).is_none_or(|(start, now)| {
                (now.saturating_duration_since(start).as_millis() / 500).is_multiple_of(2)
            })
    }
    fn unfocus(&mut self) {
        self.focused = false;
        self.drag_anchor = None;
        self.preedit = None;
    }
    fn clamp_scroll(&mut self) {
        self.scroll = self
            .scroll
            .clamp(0.0, (self.rich.height() - self.viewport_height).max(0.0));
    }
    fn scrollbar(&self) -> (f32, f32) {
        let height = (self.viewport_height * self.viewport_height / self.rich.height().max(1.0))
            .max(24.0)
            .min(self.viewport_height);
        let y = self.scroll / (self.rich.height() - self.viewport_height).max(1.0)
            * (self.viewport_height - height);
        (y, height)
    }

    fn drag_scrollbar(&mut self, y: f32, anchor: f32) {
        let (_, height) = self.scrollbar();
        self.scroll = (y - anchor) / (self.viewport_height - height).max(1.0)
            * (self.rich.height() - self.viewport_height);
        self.clamp_scroll();
    }
    fn drag(&mut self, projection: &Projection, shell: &mut Shell<'_, Message>) {
        if let (Some(anchor), Some(position)) = (self.drag_anchor, self.drag_position) {
            let point = Point::new(
                position.x,
                position.y.clamp(0.0, self.viewport_height) + self.scroll,
            );
            let (head, y) = self.rich.resident().hit_source_with_row(point);
            self.set_visual_cursor(head, y);
            let head = projection.normalize_caret(Selection::caret(head)).head;
            shell.publish(Message::SelectSource(Selection { anchor, head }));
        }
    }
}

impl operation::Focusable for State {
    fn is_focused(&self) -> bool {
        self.focused
    }
    fn focus(&mut self) {
        self.focused = true;
        self.window_focused = true;
        self.last_selection = None;
        self.reset_blink();
    }
    fn unfocus(&mut self) {
        self.unfocus();
    }
}

impl Widget<Message, Theme, Renderer> for RichEditor<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }
    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();
        let bounds = limits.width(Length::Fill).height(Length::Fill).max();
        let source_changed = state
            .layout_key
            .as_ref()
            .is_some_and(|key| !Arc::ptr_eq(&key.source, &self.projection.source));
        let content_changed = state.layout_key.as_ref().is_none_or(|key| {
            !Arc::ptr_eq(&key.source, &self.projection.source)
                || key.tokens != self.theme.tokens
                || key.typography != self.theme.typography
                || key.width != bounds.width
                || key.zoom != self.zoom
                || key.images != self.images.revision()
                || key.base.as_deref() != self.base
        });
        let changed = content_changed
            || state
                .layout_key
                .as_ref()
                .is_none_or(|key| !Arc::ptr_eq(&key.lines, &self.projection.highlights));
        let input = self.layout_input(bounds.width);
        let selection_changed = state.last_selection != Some(self.source_selection);
        let previous_caret = state.caret(self.source_selection.head);
        let was_visible = previous_caret.y >= state.scroll
            && previous_caret.y + previous_caret.height <= state.scroll + state.viewport_height;
        let disclosure_only = changed && !content_changed;
        let viewport_anchor = changed
            .then(|| {
                if was_visible {
                    Some((self.source_selection.head, previous_caret.y))
                } else {
                    state.rich.resident().viewport_anchor(state.scroll)
                }
            })
            .flatten();
        let affinity = state.visual_cursor;
        if changed {
            state.visual_cursor = None;
            state.rich.rebuild(input);
            state.layout_key = Some(LayoutKey {
                lines: Arc::clone(&self.projection.highlights),
                source: Arc::clone(&self.projection.source),
                tokens: self.theme.tokens,
                typography: self.theme.typography,
                width: bounds.width,
                zoom: self.zoom,
                images: self.images.revision(),
                base: self.base.map(Path::to_path_buf),
            });
        }
        state.viewport_height = bounds.height;
        // A source jump measures its destination, never the intervening blocks.
        let jump = (selection_changed || source_changed) && !was_visible;
        let anchored_screen_y = viewport_anchor.map(|(_, y)| y - state.scroll);
        state.scroll = if jump {
            state
                .rich
                .show_source(input, self.source_selection.head, bounds.height)
        } else if let Some((source, _)) = viewport_anchor {
            state.rich.show_source(input, source, bounds.height)
        } else {
            state.rich.show_viewport(input, state.scroll, bounds.height)
        };
        if changed {
            state.visual_cursor = affinity;
            if !jump && let Some((source, _)) = viewport_anchor {
                state.scroll =
                    state.caret(source).y - anchored_screen_y.expect("anchor has screen position");
            }
        }
        if selection_changed && !(disclosure_only && was_visible)
            || source_changed
            || content_changed && state.focused && was_visible
        {
            let caret = state.caret(self.source_selection.head);
            if caret.y < state.scroll + 8.0 {
                state.scroll = (caret.y - 8.0).max(0.0);
            }
            if caret.y + caret.height > state.scroll + bounds.height - 12.0 {
                state.scroll = caret.y + caret.height - bounds.height + 12.0;
            }
        }
        self.refresh_viewport(state, bounds);
        state.last_selection = Some(self.source_selection);
        state.clamp_scroll();
        layout::Node::new(bounds)
    }

    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        operation.focusable(
            Some(&widget::Id::from("document-editor")),
            layout.bounds(),
            tree.state.downcast_mut::<State>(),
        );
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        if matches!(event, Event::Window(window::Event::RedrawRequested(_)))
            && state.focused != self.app_focused
        {
            shell.publish(Message::EditorFocused(state.focused));
        }
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_in(bounds) {
                    if position.x >= bounds.width - 10.0 && state.rich.height() > bounds.height {
                        let (y, height) = state.scrollbar();
                        let anchor = if y <= position.y && position.y <= y + height {
                            position.y - y
                        } else {
                            height / 2.0
                        };
                        state.scroll_drag = Some(anchor);
                        state.drag_scrollbar(position.y, anchor);
                        self.refresh_viewport(state, bounds.size());
                        shell.capture_event();
                        shell.request_redraw();
                        return;
                    }
                    let click = mouse::Click::new(position, mouse::Button::Left, state.last_click);
                    let (head, y) = state
                        .rich
                        .resident()
                        .hit_source_with_row(position + Vector::new(0.0, state.scroll));
                    state.set_visual_cursor(head, y);
                    let anchor = if state.modifiers.shift() {
                        self.source_selection.anchor
                    } else {
                        head
                    };
                    let display = self.projection.to_display(head);
                    let source_selection = match click.kind() {
                        mouse::click::Kind::Double => {
                            self.raw_selection(self.word_selection(display))
                        }
                        mouse::click::Kind::Triple => {
                            self.raw_selection(self.line_selection(display))
                        }
                        _ => self.projection.normalize_caret(Selection { anchor, head }),
                    };
                    shell.publish(Message::SelectSource(source_selection));
                    state.focused = true;
                    state.window_focused = true;
                    state.last_click = Some(click);
                    state.drag_anchor = (click.kind() == mouse::click::Kind::Single)
                        .then_some(source_selection.anchor);
                    state.drag_position = Some(position);
                    state.preferred_x = None;
                    state.reset_blink();
                    shell.capture_event();
                    shell.request_redraw();
                } else if state.focused {
                    state.unfocus();
                    shell.publish(Message::EditorFocused(false));
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.scroll_drag.is_some() => {
                if let Some(position) = cursor.position() {
                    state.drag_scrollbar(position.y - bounds.y, state.scroll_drag.unwrap_or(0.0));
                    self.refresh_viewport(state, bounds.size());
                    shell.capture_event();
                    shell.request_redraw();
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.drag_anchor.is_some() => {
                state.drag_position = cursor
                    .position()
                    .map(|point| point - (bounds.position() - Point::ORIGIN));
                state.drag(self.projection, shell);
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag_anchor = None;
                state.drag_position = None;
                state.scroll_drag = None;
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) if cursor.is_over(bounds) => {
                let amount = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => {
                        -*y * self.theme.typography.body_size * 3.0
                    }
                    mouse::ScrollDelta::Pixels { y, .. } => -*y,
                };
                state.scroll += amount;
                state.clamp_scroll();
                self.refresh_viewport(state, bounds.size());
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers
            }
            Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modified_key,
                physical_key,
                modifiers,
                text,
                ..
            }) if state.focused => {
                state.modifiers = *modifiers;
                let press = KeyPress {
                    key: key.clone(),
                    modified_key: modified_key.clone(),
                    physical_key: *physical_key,
                    modifiers: *modifiers,
                    text: text.clone(),
                    status: Status::Focused {
                        is_hovered: cursor.is_over(bounds),
                    },
                };
                let in_table =
                    crate::document::table_edit::cell_at(self.projection, self.source_selection)
                        .is_some();
                if matches!(key, keyboard::Key::Named(keyboard::key::Named::Tab)) && in_table {
                    state.preferred_x = None;
                    state.visual_cursor = None;
                    shell.publish(Message::TableTab(modifiers.shift()));
                    shell.capture_event();
                } else if let Some(binding) = (self.key_binding)(press) {
                    self.apply_binding(binding, state, clipboard, shell, bounds.size());
                    shell.capture_event();
                }
                state.reset_blink();
                shell.request_redraw();
            }
            Event::InputMethod(event) if state.focused => {
                match event {
                    input_method::Event::Opened => {
                        state.preedit = Some(input_method::Preedit::new())
                    }
                    input_method::Event::Closed => state.preedit = None,
                    input_method::Event::Preedit(content, selection) => {
                        state.preedit = Some(input_method::Preedit {
                            content: content.clone(),
                            selection: selection.clone(),
                            text_size: Some((self.theme.typography.body_size * self.zoom).into()),
                        })
                    }
                    input_method::Event::Commit(text) => {
                        state.preferred_x = None;
                        state.preedit =
                            state.preedit.as_ref().map(|_| input_method::Preedit::new());
                        shell.publish(Message::Edit(Action::Edit(Edit::Paste(Arc::new(
                            text.clone(),
                        )))));
                    }
                }
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Window(window::Event::Unfocused) => {
                state.window_focused = false;
                state.drag_anchor = None;
                state.scroll_drag = None;
                shell.request_redraw();
            }
            Event::Window(window::Event::Focused) => {
                state.window_focused = true;
                state.reset_blink();
                shell.request_redraw();
            }
            Event::Window(window::Event::RedrawRequested(now))
                if state.focused && state.window_focused =>
            {
                state.now = Some(*now);
                if let Some(position) = state.drag_position
                    && state.drag_anchor.is_some()
                {
                    let overflow = if position.y < 0.0 {
                        position.y
                    } else {
                        (position.y - bounds.height).max(0.0)
                    };
                    if overflow != 0.0 {
                        state.scroll += overflow.clamp(-24.0, 24.0);
                        state.clamp_scroll();
                        self.refresh_viewport(state, bounds.size());
                        state.drag(self.projection, shell);
                        shell.request_redraw_at(*now + Duration::from_millis(16));
                    }
                }
                let elapsed = state
                    .blink_started
                    .map_or(0, |start| now.saturating_duration_since(start).as_millis());
                shell.request_redraw_at(*now + Duration::from_millis((500 - elapsed % 500) as u64));
                let caret = state.caret(self.source_selection.head)
                    + (bounds.position() - Point::ORIGIN)
                    - Vector::new(0.0, state.scroll);
                shell.request_input_method(&input_method::InputMethod::Enabled {
                    cursor: caret,
                    purpose: input_method::Purpose::Normal,
                    preedit: state.preedit.as_ref().map(input_method::Preedit::as_ref),
                });
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };
        let translation = bounds.position() - Point::ORIGIN - Vector::new(0.0, state.scroll);
        let tokens = self.theme.tokens;
        use iced::advanced::Renderer as _;
        renderer.with_layer(clip, |renderer| {
            let visible = clip.y - translation.y..clip.y + clip.height - translation.y;
            for (rect, kind) in state.rich.resident().visible_decorations(visible.clone()) {
                let rect = *rect + translation;
                if !rect.intersects(&clip) {
                    continue;
                }
                let (background, border) = match kind {
                    Decoration::Header => (tokens.surface, iced::Border::default()),
                    Decoration::Border => (
                        iced::Color::TRANSPARENT,
                        iced::Border {
                            color: tokens.border,
                            width: 0.5,
                            ..iced::Border::default()
                        },
                    ),
                };
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: rect,
                        border,
                        ..renderer::Quad::default()
                    },
                    background,
                );
            }
            if state.focused {
                for rect in state
                    .rich
                    .resident()
                    .source_selection_rects_in(self.source_selection, visible.clone())
                {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: rect + translation,
                            ..renderer::Quad::default()
                        },
                        tokens.selection,
                    );
                }
            }
            for fragment in state.rich.resident().visible_fragments(visible) {
                let rect = fragment.bounds + translation;
                if !rect.intersects(&clip) {
                    continue;
                }
                match &fragment.kind {
                    FragmentKind::Text {
                        paragraph, origin, ..
                    } => {
                        renderer.fill_paragraph(paragraph, *origin + translation, tokens.text, clip)
                    }
                    FragmentKind::Image { handle, label } => {
                        renderer.fill_quad(
                            renderer::Quad {
                                bounds: rect,
                                border: iced::Border {
                                    color: tokens.border,
                                    width: 1.0,
                                    radius: 6.0.into(),
                                },
                                ..renderer::Quad::default()
                            },
                            tokens.surface,
                        );
                        if let Some(handle) = handle {
                            let mut image = image::Image::new(handle.clone());
                            image.border_radius = 6.0.into();
                            renderer.draw_image(image, rect, clip);
                        } else {
                            renderer.fill_text(
                                text::Text {
                                    content: label.clone(),
                                    bounds: rect.size(),
                                    size: 14.0.into(),
                                    font: iced::Font::DEFAULT,
                                    line_height: text::LineHeight::Relative(1.5),
                                    align_x: text::Alignment::Center,
                                    align_y: iced::alignment::Vertical::Center,
                                    shaping: text::Shaping::Advanced,
                                    wrapping: text::Wrapping::WordOrGlyph,
                                },
                                rect.center(),
                                tokens.muted,
                                clip,
                            );
                        }
                        let selected = self.display_selection().range();
                        if selected.start <= fragment.range.start
                            && selected.end >= fragment.range.end
                            && state.focused
                        {
                            renderer.fill_quad(
                                renderer::Quad {
                                    bounds: rect,
                                    border: iced::Border {
                                        color: tokens.accent,
                                        width: 2.0,
                                        radius: 6.0.into(),
                                    },
                                    ..renderer::Quad::default()
                                },
                                tokens.selection.scale_alpha(0.35),
                            );
                        }
                    }
                }
            }
            if self.source.is_empty() {
                renderer.fill_text(
                    text::Text {
                        content: "Begin anywhere…".into(),
                        bounds: bounds.size(),
                        size: (self.theme.typography.body_size * self.zoom).into(),
                        font: crate::theme::styles::BODY_FONT,
                        line_height: text::LineHeight::Relative(self.theme.typography.line_height),
                        align_x: text::Alignment::Default,
                        align_y: iced::alignment::Vertical::Top,
                        shaping: text::Shaping::Advanced,
                        wrapping: text::Wrapping::WordOrGlyph,
                    },
                    bounds.position() + Vector::new(0.0, 5.0),
                    tokens.faint,
                    clip,
                );
            }
            if self.display_selection().is_caret() && state.caret_visible() {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: state.caret(self.source_selection.head) + translation,
                        ..renderer::Quad::default()
                    },
                    tokens.accent,
                );
            }
            if state.rich.height() > bounds.height {
                let (y, height) = state.scrollbar();
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: Rectangle::new(
                            Point::new(bounds.x + bounds.width - 4.0, bounds.y + y),
                            Size::new(3.0, height),
                        ),
                        border: iced::Border {
                            radius: 2.0.into(),
                            ..iced::Border::default()
                        },
                        ..renderer::Quad::default()
                    },
                    tokens.border,
                );
            }
        });
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
        if state.scroll_drag.is_some()
            || cursor
                .position_in(layout.bounds())
                .is_some_and(|p| p.x >= layout.bounds().width - 10.0)
                && state.rich.height() > layout.bounds().height
        {
            mouse::Interaction::Pointer
        } else if cursor.is_over(layout.bounds()) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::None
        }
    }
}

impl<'a> From<RichEditor<'a>> for Element<'a, Message> {
    fn from(editor: RichEditor<'a>) -> Self {
        Element::new(editor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::native_buffer;
    use crate::theme::Preset;

    #[test]
    fn copying_a_rich_document_retains_heading_table_and_image_markdown() {
        struct ClipboardText(String);
        impl Clipboard for ClipboardText {
            fn read(&self, _: iced::advanced::clipboard::Kind) -> Option<String> {
                Some(self.0.clone())
            }
            fn write(&mut self, _: iced::advanced::clipboard::Kind, text: String) {
                self.0 = text;
            }
        }
        let source = "# Heading\n\n| **A** | B |\n|---|---|\n| x | y |\n\n![alt](room.png)";
        let projection = Projection::new(source);
        let mut content = native_buffer::with_text(&projection.text);
        content.perform(Action::SelectAll);
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let editor = RichEditor::new(
            &content,
            &projection,
            source,
            &theme,
            &images,
            Binding::from_key_press,
        )
        .selection(Selection {
            anchor: 0,
            head: source.len(),
        });
        let mut clipboard = ClipboardText(String::new());
        editor.apply_binding(
            Binding::Copy,
            &mut State::default(),
            &mut clipboard,
            &mut Shell::new(&mut Vec::new()),
            Size::new(640.0, 600.0),
        );
        assert_eq!(clipboard.0, source);
    }

    #[test]
    fn widget_drag_selection_crosses_heading_table_image_and_paragraph_in_both_directions() {
        let source = include_str!("../../tests/fixtures/rich-blocks.md").trim_end_matches('\n');
        let projection = Projection::new(source);
        let content = native_buffer::with_text(&projection.text);
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let mut editor = RichEditor::new(
            &content,
            &projection,
            source,
            &theme,
            &images,
            Binding::from_key_press,
        );
        let renderer = Renderer::Secondary(iced_tiny_skia::Renderer::new(
            iced::Font::DEFAULT,
            18.0.into(),
        ));
        let mut tree = widget::Tree::new(&editor as &dyn Widget<Message, Theme, Renderer>);
        let node = editor.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(640.0, 1100.0)),
        );
        let layout = Layout::new(&node);
        let state = tree.state.downcast_ref::<State>();
        let start_offset = 2;
        let end_offset = projection.text.find("After").unwrap() + 3;
        let point = |offset| {
            let caret = state.rich.resident().caret(offset);
            Point::new(caret.x + 0.1, caret.center_y())
        };
        let (start, end) = (point(start_offset), point(end_offset));
        let mut clipboard = iced::advanced::clipboard::Null;
        for (start, end, expected) in [
            (
                start,
                end,
                Selection {
                    anchor: start_offset,
                    head: end_offset,
                },
            ),
            (
                end,
                start,
                Selection {
                    anchor: end_offset,
                    head: start_offset,
                },
            ),
        ] {
            let mut messages = Vec::new();
            for (event, cursor) in [
                (
                    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    start,
                ),
                (
                    Event::Mouse(mouse::Event::CursorMoved { position: end }),
                    end,
                ),
                (
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                    end,
                ),
            ] {
                editor.update(
                    &mut tree,
                    &event,
                    layout,
                    mouse::Cursor::Available(cursor),
                    &renderer,
                    &mut clipboard,
                    &mut Shell::new(&mut messages),
                    &layout.bounds(),
                );
            }
            let selected = messages
                .iter()
                .rev()
                .find_map(|message| match message {
                    Message::SelectSource(selection) => Some(*selection),
                    _ => None,
                })
                .unwrap();
            let raw = selected;
            let selected = projection.display_selection(raw);
            assert_eq!(selected, expected);
            assert_eq!(projection.source_selection(selected, raw), raw);
            assert!(
                tree.state
                    .downcast_ref::<State>()
                    .rich
                    .resident()
                    .selection_rects(selected)
                    .len()
                    > 5
            );
        }
    }

    #[test]
    fn drag_anchor_survives_contextual_disclosure_between_widget_updates() {
        let source = include_str!("../../tests/fixtures/rich-blocks.md").trim_end_matches('\n');
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let renderer = Renderer::Secondary(iced_tiny_skia::Renderer::new(
            iced::Font::DEFAULT,
            18.0.into(),
        ));
        let mut clipboard = iced::advanced::clipboard::Null;
        let heading = 4;
        let paragraph = source.find("After").unwrap() + 3;
        for (anchor, head) in [(heading, paragraph), (paragraph, heading)] {
            let mut projection = Projection::new(source);
            let mut content = native_buffer::with_text(&projection.text);
            let mut selection = Selection::caret(2);
            let mut tree = widget::Tree::new(&RichEditor::new(
                &content,
                &projection,
                source,
                &theme,
                &images,
                Binding::from_key_press,
            )
                as &dyn Widget<Message, Theme, Renderer>);
            for (event, target) in [
                (
                    Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    anchor,
                ),
                (
                    Event::Mouse(mouse::Event::CursorMoved {
                        position: Point::ORIGIN,
                    }),
                    head,
                ),
                (
                    Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
                    head,
                ),
            ] {
                let mut editor = RichEditor::new(
                    &content,
                    &projection,
                    source,
                    &theme,
                    &images,
                    Binding::from_key_press,
                )
                .selection(selection)
                .focused(true);
                let node = editor.layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, Size::new(640.0, 1100.0)),
                );
                let state = tree.state.downcast_ref::<State>();
                let caret = state.rich.resident().caret(projection.to_display(target));
                let point = Point::new(caret.x + 0.1, caret.center_y() - state.scroll);
                let event = if matches!(event, Event::Mouse(mouse::Event::CursorMoved { .. })) {
                    Event::Mouse(mouse::Event::CursorMoved { position: point })
                } else {
                    event
                };
                let mut messages = Vec::new();
                editor.update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    mouse::Cursor::Available(point),
                    &renderer,
                    &mut clipboard,
                    &mut Shell::new(&mut messages),
                    &Rectangle::with_size(node.size()),
                );
                if let Some(next) = messages.iter().rev().find_map(|message| match message {
                    Message::SelectSource(selection) => Some(*selection),
                    _ => None,
                }) {
                    selection = next;
                }
                projection = projection.disclosing(projection.disclosure_at(Some(selection.head)));
                content = native_buffer::with_text(&projection.text);
                content.move_to(
                    projection
                        .display_selection(selection)
                        .cursor(&projection.text),
                );
            }
            assert_eq!(selection, Selection { anchor, head });
        }
    }

    #[test]
    fn disclosure_reflow_anchors_caret_viewport_and_wrap_affinity() {
        let source = (0..80)
            .map(|i| format!("## Line{i}withmanywordswithmanywordswithmanywords\n\n"))
            .collect::<String>();
        let first = source.find("Line25").unwrap();
        let second = source.find("Line26").unwrap();
        let mut projection = Projection::new(&source);
        let content = native_buffer::with_text(&projection.text);
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let renderer = Renderer::Secondary(iced_tiny_skia::Renderer::new(
            iced::Font::DEFAULT,
            18.0.into(),
        ));
        let editor = RichEditor::new(
            &content,
            &projection,
            &source,
            &theme,
            &images,
            Binding::from_key_press,
        );
        let mut tree = widget::Tree::new(&editor as &dyn Widget<Message, Theme, Renderer>);
        let limits = layout::Limits::new(Size::ZERO, Size::new(180.0, 500.0));
        let relayout =
            |tree: &mut widget::Tree, projection: &Projection, selection: Selection, focused| {
                let mut content = native_buffer::with_text(&projection.text);
                projection
                    .display_selection(selection)
                    .restore(&mut content, &projection.text);
                RichEditor::new(
                    &content,
                    projection,
                    &source,
                    &theme,
                    &images,
                    Binding::from_key_press,
                )
                .selection(selection)
                .focused(focused)
                .layout(tree, &renderer, &limits);
            };
        relayout(&mut tree, &projection, Selection::caret(first), true);
        let state = tree.state.downcast_mut::<State>();
        state.focused = true;
        state.scroll = state.rich.source_caret(first, None).y - 80.0;
        state.preferred_x = Some(73.0);
        state.drag_anchor = Some(first);
        let height = state.rich.height();
        let mut reflowed = false;
        for (head, focused) in [
            (first, true),
            (first, false),
            (first, true),
            (second, true),
            (first, true),
        ] {
            let state = tree.state.downcast_ref::<State>();
            let expected_y = state.caret(head).y - state.scroll;
            projection = projection.disclosing(projection.disclosure_at(focused.then_some(head)));
            let selection = Selection {
                anchor: first,
                head,
            };
            relayout(&mut tree, &projection, selection, focused);
            let state = tree.state.downcast_ref::<State>();
            assert!((state.caret(head).y - state.scroll - expected_y).abs() < 0.01);
            assert_eq!(state.last_selection, Some(selection));
            assert_eq!(state.preferred_x, Some(73.0));
            assert_eq!(state.drag_anchor, Some(first));
            reflowed |= state.rich.height() != height;
        }
        assert!(
            reflowed,
            "heading disclosure should change wrapping in this narrow fixture"
        );

        let state = tree.state.downcast_mut::<State>();
        let caret = state.rich.source_caret(first, None);
        let edge = state.rich.resident().source_line_edge(first, caret, true);
        assert!(state.rich.source_caret_on_side(edge, false).y > caret.y);
        state.visual_cursor = Some((edge, true));
        let expected_y = state.caret(edge).y - state.scroll;
        projection = projection.disclosing(projection.disclosure_at(None));
        relayout(&mut tree, &projection, Selection::caret(edge), false);
        let state = tree.state.downcast_ref::<State>();
        assert_eq!(state.visual_cursor.unwrap().0, edge);
        assert_eq!(
            state.caret(edge),
            state.rich.source_caret_on_side(edge, true)
        );
        assert!((state.caret(edge).y - state.scroll - expected_y).abs() < 0.01);

        // Focusing an offscreen caret must retain the visible source row.
        let state = tree.state.downcast_mut::<State>();
        state.scroll = state
            .rich
            .source_caret(source.find("Line60").unwrap(), None)
            .y
            + 7.0;
        state.last_selection = Some(Selection::caret(first));
        let (anchor, y) = state.rich.resident().viewport_anchor(state.scroll).unwrap();
        let screen_y = y - state.scroll;
        projection = projection.disclosing(projection.disclosure_at(Some(first)));
        relayout(&mut tree, &projection, Selection::caret(first), true);
        let state = tree.state.downcast_ref::<State>();
        assert!((state.rich.source_caret(anchor, None).y - state.scroll - screen_y).abs() < 0.01);
        assert!(state.caret(first).y < state.scroll);
        assert!(state.scroll <= state.rich.height() - state.viewport_height);

        // An explicit offscreen navigation jump must also retain wrap intent
        // when it changes the active disclosure context.
        projection = projection.disclosing(projection.disclosure_at(None));
        relayout(&mut tree, &projection, Selection::caret(first), false);
        let state = tree.state.downcast_mut::<State>();
        let edge = state
            .rich
            .resident()
            .source_line_edge(first, state.caret(first), true);
        state.visual_cursor = Some((edge, true));
        state.last_selection = Some(Selection::caret(first));
        projection = projection.disclosing(projection.disclosure_at(Some(edge)));
        relayout(&mut tree, &projection, Selection::caret(edge), true);
        let state = tree.state.downcast_ref::<State>();
        assert_eq!(state.visual_cursor, Some((edge, true)));
        assert_eq!(
            state.caret(edge),
            state.rich.source_caret_on_side(edge, true)
        );
    }

    #[test]
    fn virtualized_navigation_disclosure_resize_and_drag_keep_source_anchors() {
        let source = (0..600).map(|i| format!(
            "## Heading {i}\n\nParagraph {i} with [label](https://example.com/a/very/long/destination/which/wraps) **bold words** and café 🦀.\n\n"
        )).collect::<String>();
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let renderer = Renderer::Secondary(iced_tiny_skia::Renderer::new(
            iced::Font::DEFAULT,
            18.0.into(),
        ));
        let head = source.find("Paragraph 300").unwrap() + "Paragraph 300 with [la".len();
        let mut projection = Projection::with_context(&source, Some(head));
        let mut content = native_buffer::with_text(&projection.text);
        let mut selection = Selection { anchor: 0, head };
        let initial = RichEditor::new(
            &content,
            &projection,
            &source,
            &theme,
            &images,
            Binding::from_key_press,
        );
        let mut tree = widget::Tree::new(&initial as &dyn Widget<Message, Theme, Renderer>);
        let relayout = |tree: &mut widget::Tree,
                        content: &Content,
                        projection: &Projection,
                        selection,
                        width| {
            RichEditor::new(
                content,
                projection,
                &source,
                &theme,
                &images,
                Binding::from_key_press,
            )
            .selection(selection)
            .focused(true)
            .layout(
                tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, Size::new(width, 500.0)),
            );
        };
        relayout(&mut tree, &content, &projection, selection, 240.0);
        tree.state.downcast_mut::<State>().focused = true;
        for focus in [false, true, false, true] {
            let state = tree.state.downcast_ref::<State>();
            let screen_y = state.caret(head).y - state.scroll;
            projection = projection.disclosing(projection.disclosure_at(focus.then_some(head)));
            content = native_buffer::with_text(&projection.text);
            relayout(&mut tree, &content, &projection, selection, 240.0);
            let state = tree.state.downcast_ref::<State>();
            assert!((state.caret(head).y - state.scroll - screen_y).abs() < 0.1);
            assert_eq!(state.last_selection, Some(selection));
            assert!(state.rich.resident().fragments.len() < 100);
        }
        // Scroll away, then disclose and resize while the selection is offscreen.
        let state = tree.state.downcast_mut::<State>();
        state.scroll += 12_000.0;
        let editor = RichEditor::new(
            &content,
            &projection,
            &source,
            &theme,
            &images,
            Binding::from_key_press,
        )
        .selection(selection);
        editor.refresh_viewport(state, Size::new(240.0, 500.0));
        let (anchor, y) = state.rich.resident().viewport_anchor(state.scroll).unwrap();
        let screen_y = y - state.scroll;
        projection = projection.disclosing(projection.disclosure_at(None));
        content = native_buffer::with_text(&projection.text);
        relayout(&mut tree, &content, &projection, selection, 180.0);
        let state = tree.state.downcast_ref::<State>();
        assert!((state.rich.source_caret(anchor, None).y - state.scroll - screen_y).abs() < 0.15);
        assert!(state.caret(head).y < state.scroll);

        // Shift+PageUp measures a distant, evicted caret before navigating.
        let mut messages = Vec::new();
        RichEditor::new(
            &content,
            &projection,
            &source,
            &theme,
            &images,
            Binding::from_key_press,
        )
        .selection(selection)
        .apply_binding(
            Binding::Select(Motion::PageUp),
            tree.state.downcast_mut::<State>(),
            &mut iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            Size::new(180.0, 500.0),
        );
        selection = messages
            .into_iter()
            .find_map(|message| match message {
                Message::SelectSource(next) => Some(next),
                _ => None,
            })
            .unwrap();
        assert_eq!(selection.anchor, 0);
        assert!(selection.head < head);
        relayout(&mut tree, &content, &projection, selection, 180.0);
        let state = tree.state.downcast_ref::<State>();
        let caret = state.caret(selection.head);
        assert!(caret.y >= state.scroll && caret.y + caret.height <= state.scroll + 500.0);

        // Drag anchors remain source offsets across several evicted windows,
        // in both directions, without retaining geometry between endpoints.
        for (anchor, positions) in [(0, [0.8, 0.2, 0.7]), (source.len(), [0.1, 0.7, 0.0])] {
            let state = tree.state.downcast_mut::<State>();
            state.drag_anchor = Some(anchor);
            state.drag_position = Some(Point::new(80.0, 250.0));
            for fraction in positions {
                state.scroll = state.rich.height() * fraction;
                let editor = RichEditor::new(
                    &content,
                    &projection,
                    &source,
                    &theme,
                    &images,
                    Binding::from_key_press,
                );
                editor.refresh_viewport(state, Size::new(180.0, 500.0));
                let expected = state
                    .rich
                    .resident()
                    .hit_source_with_row(Point::new(80.0, state.scroll + 250.0))
                    .0;
                let mut messages = Vec::new();
                state.drag(&projection, &mut Shell::new(&mut messages));
                assert!(
                    matches!(messages.as_slice(), [Message::SelectSource(next)] if next.anchor == anchor && next.head == expected)
                );
                assert!(state.rich.resident().fragments.len() < 100);
            }
        }

        // Replacing a document must reveal its initial caret even when the old
        // document was scrolled away with that same source selection.
        let state = tree.state.downcast_mut::<State>();
        state.scroll = state.rich.height() * 0.7;
        let editor = RichEditor::new(
            &content,
            &projection,
            &source,
            &theme,
            &images,
            Binding::from_key_press,
        );
        editor.refresh_viewport(state, Size::new(180.0, 500.0));
        state.last_selection = Some(Selection::caret(0));
        state.focused = false;
        let replacement = "A new document with its own text.\n\n".repeat(600);
        let projection = Projection::new(&replacement);
        let content = native_buffer::with_text(&projection.text);
        RichEditor::new(
            &content,
            &projection,
            &replacement,
            &theme,
            &images,
            Binding::from_key_press,
        )
        .layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(180.0, 500.0)),
        );
        let state = tree.state.downcast_ref::<State>();
        assert!(state.scroll <= 5.0);
        assert!(state.caret(0).y >= state.scroll);
        assert!(state.rich.resident().fragments.len() < 100);
    }

    #[test]
    fn wrap_affinity_survives_eviction_before_keyboard_navigation() {
        let source = format!("{}\n\n", "Continuouswordswithoutspaces".repeat(6)).repeat(300);
        let projection = Projection::new(&source);
        let content = native_buffer::with_text(&projection.text);
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let renderer = Renderer::Secondary(iced_tiny_skia::Renderer::new(
            iced::Font::DEFAULT,
            18.0.into(),
        ));
        let head = source.len() / 2 + 2;
        let mut editor = RichEditor::new(
            &content,
            &projection,
            &source,
            &theme,
            &images,
            Binding::from_key_press,
        )
        .selection(Selection::caret(head));
        let mut tree = widget::Tree::new(&editor as &dyn Widget<Message, Theme, Renderer>);
        editor.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(180.0, 500.0)),
        );
        let state = tree.state.downcast_mut::<State>();
        let edge = state
            .rich
            .resident()
            .source_line_edge(head, state.caret(head), true);
        assert!(
            state.rich.source_caret_on_side(edge, false).y
                > state.rich.source_caret_on_side(edge, true).y
        );
        state.visual_cursor = Some((edge, true));
        let expected = state
            .rich
            .resident()
            .source_line_edge(edge, state.caret(edge), false);
        assert_ne!(expected, edge);
        state.scroll = 0.0;
        editor.refresh_viewport(state, Size::new(180.0, 500.0));
        assert!(!state.rich.contains_source(edge));
        let mut messages = Vec::new();
        editor.source_selection = Selection::caret(edge);
        editor.apply_binding(
            Binding::Move(Motion::Home),
            state,
            &mut iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            Size::new(180.0, 500.0),
        );
        assert!(
            matches!(messages.as_slice(), [Message::SelectSource(next)] if *next == Selection::caret(expected))
        );
    }

    #[test]
    fn ime_preedit_is_transient_and_commit_produces_one_edit() {
        let projection = Projection::new("# Title");
        let content = native_buffer::with_text(&projection.text);
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let mut editor = RichEditor::new(
            &content,
            &projection,
            "# Title",
            &theme,
            &images,
            Binding::from_key_press,
        );
        let renderer = Renderer::Secondary(iced_tiny_skia::Renderer::new(
            iced::Font::DEFAULT,
            18.0.into(),
        ));
        let mut tree = widget::Tree::new(&editor as &dyn Widget<Message, Theme, Renderer>);
        let node = editor.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(640.0, 600.0)),
        );
        operation::Focusable::focus(tree.state.downcast_mut::<State>());
        let mut messages = Vec::new();
        let mut clipboard = iced::advanced::clipboard::Null;
        for event in [
            input_method::Event::Opened,
            input_method::Event::Preedit("にほん".into(), Some(0..9)),
            input_method::Event::Commit("日本語".into()),
        ] {
            editor.update(
                &mut tree,
                &Event::InputMethod(event),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(node.size()),
            );
        }
        assert_eq!(content.text(), "Title");
        assert_eq!(messages.len(), 1);
        assert!(
            matches!(&messages[0], Message::Edit(Action::Edit(Edit::Paste(text))) if text.as_str() == "日本語")
        );
        assert!(
            tree.state
                .downcast_ref::<State>()
                .preedit
                .as_ref()
                .unwrap()
                .content
                .is_empty()
        );
    }
}
