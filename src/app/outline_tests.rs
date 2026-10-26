//! Exercise section commands through the document, native buffer, and real UI.

use super::super::{SidebarTab, event_message};
use super::*;
use crate::document::{Document, projection::Projection};
use iced::advanced::{Layout, Shell, clipboard, layout, mouse, widget};
use iced::keyboard::{self, Key, key};
use iced::{Event, Point, Rectangle, Renderer, Size};

fn app(source: &str) -> App {
    let (mut app, _) = App::new();
    app.set_document(source.into(), None, "Outline.md".into());
    app.sidebar_tab = SidebarTab::Outline;
    app
}

fn send(app: &mut App, message: AppMessage) {
    drop(app.update_message(message));
    assert_eq!(
        app.document.source(),
        app.document.collaboration.materialize()
    );
    let fresh = Projection::with_context(
        app.document.source(),
        (app.mode == EditorMode::Live && app.editor_focused).then_some(app.selection.head),
    );
    assert_eq!(app.projection.text, fresh.text);
    assert_eq!(app.projection.rich, fresh.rich);
    assert_eq!(
        app.editor.snapshot(),
        if app.mode == EditorMode::Source {
            app.document.source()
        } else {
            &app.projection.text
        }
    );
}

fn command(app: &mut App, message: Message) {
    send(app, AppMessage::Outline(message));
}

#[test]
fn outline_moves_are_atomic_in_live_source_and_read_and_keep_fold_identity() {
    let source = "# A\n\n## Child\n\n🦀 body\n\n# B\n\nlast\n\n";
    for mode in [EditorMode::Live, EditorMode::Source, EditorMode::Read] {
        let mut app = app(source);
        app.mode = mode;
        let before = Selection {
            anchor: source.find("body").unwrap() + 4,
            head: source.find("🦀").unwrap(),
        };
        app.selection = before;
        app.sync_editor();
        command(&mut app, Message::Toggle(0));
        assert_eq!(app.outline.visible(), vec![0, 2]);
        command(&mut app, Message::BeginDrag(0));
        command(
            &mut app,
            Message::Drop(DropTarget {
                section: 2,
                after: true,
            }),
        );
        assert_eq!(
            app.document.source(),
            "# B\n\nlast\n\n# A\n\n## Child\n\n🦀 body\n\n"
        );
        assert!(app.selection.anchor > app.selection.head);
        assert_eq!(&app.document.source()[app.selection.range()], "🦀 body");
        assert_eq!(app.outline.visible(), vec![0, 1]);
        let after = app.selection;
        send(&mut app, AppMessage::Undo);
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, before);
        assert_eq!(app.outline.visible(), vec![0, 2]);
        send(&mut app, AppMessage::Redo);
        assert_eq!(app.selection, after);
        assert_eq!(app.outline.visible(), vec![0, 1]);
        command(&mut app, Message::Toggle(1));
        assert_eq!(app.outline.visible(), vec![0, 1, 2]);
        assert!(app.dirty);
    }
}

#[test]
fn outline_keyboard_moves_are_separate_from_typing_and_restore_selection() {
    let source = "# A\n\nfirst\n\n# B\n\nsecond\n\n# C\n\nthird\n\n";
    let mut app = app(source);
    app.selection = Selection::caret(source.find("second").unwrap());
    app.sync_editor();
    send(
        &mut app,
        AppMessage::Edit(iced::widget::text_editor::Action::Edit(
            iced::widget::text_editor::Edit::Insert('X'),
        )),
    );
    let typed = app.document.source().to_owned();
    let before = app.selection;
    command(&mut app, Message::Move(-1));
    assert!(app.document.source().starts_with("# B\n\nXsecond"));
    let moved = app.document.source().to_owned();
    send(&mut app, AppMessage::Undo);
    assert_eq!(app.document.source(), typed);
    assert_eq!(app.selection, before);
    send(&mut app, AppMessage::Undo);
    assert_eq!(app.document.source(), source);
    send(&mut app, AppMessage::Redo);
    send(&mut app, AppMessage::Redo);
    assert_eq!(app.document.source(), moved);
    command(&mut app, Message::Move(1));
    assert_eq!(app.document.source(), typed);
}

#[test]
fn outline_search_includes_folded_duplicate_headings_and_parent_context() {
    let source = include_str!("../../tests/fixtures/outline.md");
    let mut app = app(source);
    command(&mut app, Message::FoldAll(true));
    let original_selection = app.selection;
    command(&mut app, Message::QuickJump);
    command(&mut app, Message::Query("REPEATED".into()));
    assert_eq!(app.outline.results, vec![1, 5, 2]);
    command(&mut app, Message::Step(1));
    command(&mut app, Message::Accept);
    assert_eq!(app.selection.head, source.rfind("## Repeated").unwrap());
    assert!(app.overlay.is_none());
    assert!(!app.outline.folded[4]);
    command(&mut app, Message::QuickJump);
    command(&mut app, Message::Query("first repeated".into()));
    assert_eq!(app.outline.results, vec![1, 2]);
    command(&mut app, Message::Query("absent".into()));
    command(&mut app, Message::Accept);
    assert!(matches!(app.overlay, Some(Overlay::QuickJump)));
    command(&mut app, Message::CancelQuickJump);
    assert!(app.overlay.is_none());
    assert_ne!(app.selection, original_selection);
    assert_eq!(app.document.source(), source);
    assert!(!app.dirty);
    assert!(!app.document.can_undo());
}

#[test]
fn outline_fold_anchors_survive_remote_prefix_and_undo_but_not_deleted_headings() {
    let source = "# A\n\n## Child\n\nbody\n\n# B\n\n## Other\n\nlast\n\n";
    let mut app = app(source);
    command(&mut app, Message::Toggle(0));
    command(&mut app, Message::BeginDrag(0));
    let mut peer = Document::from_state(&app.document.collaboration.state()).unwrap();
    peer.replace(0..0, "Preface\n\n").unwrap();
    send(
        &mut app,
        AppMessage::ReplicaLoaded(Ok(peer.collaboration.state())),
    );
    assert!(app.outline.drag.is_none());
    assert!(app.outline.folded[0]);
    command(
        &mut app,
        Message::Drop(DropTarget {
            section: 2,
            after: true,
        }),
    );
    assert!(app.document.source().starts_with("Preface\n\n# A"));
    let start = app.outline.sections[0].range.start;
    let end = app.outline.sections[2].range.start;
    app.document.replace(start..end, "").unwrap();
    app.sync_editor();
    assert!(
        !app.outline.folded[0],
        "a deleted fold must not attach to B"
    );
    send(&mut app, AppMessage::Undo);
    assert!(app.outline.folded[0]);
    app.set_document(source.into(), None, "Another.md".into());
    assert!(app.outline.folded.iter().all(|folded| !folded));
}

#[test]
fn outline_commands_obey_loading_and_modal_guards_and_cancel_cleanly() {
    let source = "# A\n\n## Child\n\n# B\n\n";
    let mut app = app(source);
    for loading in [true, false] {
        app.loading = loading;
        app.overlay = (!loading).then_some(Overlay::Settings);
        for message in [
            Message::Move(1),
            Message::Toggle(0),
            Message::QuickJump,
            Message::BeginDrag(0),
        ] {
            command(&mut app, message);
        }
        assert_eq!(app.document.source(), source);
        assert!(!app.document.can_undo());
        assert!(app.outline.drag.is_none());
        assert!(!app.outline.folded[0]);
    }
    app.overlay = None;
    command(&mut app, Message::BeginDrag(0));
    send(&mut app, AppMessage::CloseOverlay);
    assert!(app.outline.drag.is_none());
    command(&mut app, Message::BeginDrag(0));
    // A focused text input can capture Escape before the global shortcut.
    command(&mut app, Message::CancelQuickJump);
    assert!(app.outline.drag.is_none());
    command(&mut app, Message::FoldCurrent(true));
    assert!(app.outline.folded[0]);
    app.selection = Selection::caret(source.find("Child").unwrap());
    command(&mut app, Message::FoldCurrent(false));
    assert!(!app.outline.folded[0]);
}

#[test]
fn outline_read_jump_updates_current_section_without_leaving_read_mode() {
    let source = "# A\n\ntext\n\n# B\n\ntext\n\n";
    let mut app = app(source);
    app.mode = EditorMode::Read;
    command(&mut app, Message::QuickJump);
    command(&mut app, Message::Query("B".into()));
    command(&mut app, Message::Accept);
    assert_eq!(app.mode, EditorMode::Read);
    assert_eq!(app.selection, Selection::caret(source.find("# B").unwrap()));
    assert_eq!(app.document.source(), source);
    assert!(!app.document.can_undo());
}

struct Harness {
    app: App,
    tree: widget::Tree,
    renderer: Renderer,
    size: Size,
}

impl Harness {
    fn new(source: &str, size: Size) -> Self {
        let app = app(source);
        let tree = widget::Tree::new(app.view().as_widget());
        Self {
            app,
            tree,
            renderer: Renderer::Secondary(iced_tiny_skia::Renderer::new(
                iced::Font::DEFAULT,
                18.0.into(),
            )),
            size,
        }
    }
    fn operate(&mut self, operation: &mut dyn widget::Operation) {
        let mut root = self.app.view();
        self.tree.diff(root.as_widget());
        let node = root.as_widget_mut().layout(
            &mut self.tree,
            &self.renderer,
            &layout::Limits::new(Size::ZERO, self.size),
        );
        root.as_widget_mut().operate(
            &mut self.tree,
            Layout::new(&node),
            &self.renderer,
            operation,
        );
    }
    fn focus(&mut self, id: &'static str) {
        self.operate(&mut widget::operation::focusable::focus::<()>(
            widget::Id::from(id),
        ));
    }
    fn bounds(&mut self, id: String) -> Rectangle {
        struct Bounds {
            id: widget::Id,
            result: Option<Rectangle>,
        }
        impl widget::Operation for Bounds {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn widget::Operation)) {
                operate(self);
            }
            fn container(&mut self, id: Option<&widget::Id>, bounds: Rectangle) {
                if id == Some(&self.id) {
                    self.result = Some(bounds);
                }
            }
        }
        let mut operation = Bounds {
            id: id.into(),
            result: None,
        };
        self.operate(&mut operation);
        operation.result.unwrap()
    }
    fn event(&mut self, event: Event, cursor: mouse::Cursor) {
        let mut messages = Vec::new();
        let status;
        {
            let mut root = self.app.view();
            self.tree.diff(root.as_widget());
            let node = root.as_widget_mut().layout(
                &mut self.tree,
                &self.renderer,
                &layout::Limits::new(Size::ZERO, self.size),
            );
            let mut shell = Shell::new(&mut messages);
            root.as_widget_mut().update(
                &mut self.tree,
                &event,
                Layout::new(&node),
                cursor,
                &self.renderer,
                &mut clipboard::Null,
                &mut shell,
                &Rectangle::with_size(self.size),
            );
            status = shell.event_status();
        }
        if let Some(message) = event_message(event, status, iced::window::Id::unique()) {
            messages.push(message);
        }
        for message in messages {
            send(&mut self.app, message);
        }
    }
    fn key(&mut self, key: Key, modifiers: keyboard::Modifiers) {
        self.event(
            Event::Keyboard(keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: key::Physical::Unidentified(key::NativeCode::Unidentified),
                location: keyboard::Location::Standard,
                modifiers,
                text: None,
                repeat: false,
            }),
            mouse::Cursor::Unavailable,
        );
    }
    fn pointer(&mut self, event: mouse::Event, point: Point) {
        self.event(Event::Mouse(event), mouse::Cursor::Available(point));
    }

    fn render(&mut self, path: &str) {
        use iced::advanced::{Renderer as _, renderer};
        let clip = Rectangle::with_size(self.size);
        let mut root = self.app.view();
        self.tree.diff(root.as_widget());
        let node = root.as_widget_mut().layout(
            &mut self.tree,
            &self.renderer,
            &layout::Limits::new(Size::ZERO, self.size),
        );
        self.renderer.reset(clip);
        root.as_widget().draw(
            &self.tree,
            &mut self.renderer,
            &self.app.theme(),
            &renderer::Style {
                text_color: self.app.resolved_theme.tokens.text,
            },
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &clip,
        );
        let size = Size::new(self.size.width as u32, self.size.height as u32);
        let mut pixels = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
        let mut mask = tiny_skia::Mask::new(size.width, size.height).unwrap();
        if let Renderer::Secondary(renderer) = &mut self.renderer {
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &iced_tiny_skia::graphics::Viewport::with_physical_size(size, 1.0),
                &[clip],
                self.app.resolved_theme.tokens.canvas,
            );
        }
        image::save_buffer(
            path,
            pixels.data(),
            size.width,
            size.height,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}

#[test]
#[ignore = "render the full outline and picker UI for visual inspection"]
fn outline_ui_previews() {
    let mut fonts = iced_tiny_skia::graphics::text::font_system()
        .write()
        .unwrap();
    fonts.load_font(std::borrow::Cow::Borrowed(
        include_bytes!("../../assets/fonts/Newsreader-Regular.ttf").as_slice(),
    ));
    drop(fonts);
    let source = include_str!("../../tests/fixtures/outline.md");
    std::fs::create_dir_all("target/outline-validation").unwrap();
    let mut harness = Harness::new(source, Size::new(1180.0, 820.0));
    harness.app.resolved_theme = crate::theme::Preset::Paper.resolve();
    command(&mut harness.app, Message::Toggle(1));
    command(&mut harness.app, Message::BeginDrag(0));
    command(
        &mut harness.app,
        Message::Hover(DropTarget {
            section: 4,
            after: true,
        }),
    );
    harness.render("target/outline-validation/outline.png");
    command(&mut harness.app, Message::QuickJump);
    harness.focus(QUICK_JUMP);
    harness.size = Size::new(760.0, 520.0);
    harness.render("target/outline-validation/quick-jump.png");
    harness.app.resolved_theme = crate::theme::Preset::Midnight.resolve();
    harness.render("target/outline-validation/quick-jump-dark.png");
}

#[test]
#[ignore = "release measurements for heading search, collapse, section move, and undo"]
fn performance_outline_sections() {
    use std::time::{Duration, Instant};
    fn measure(app: &mut App, label: &str, messages: Vec<AppMessage>) {
        let mut durations: Vec<Duration> = Vec::new();
        for message in messages {
            let start = Instant::now();
            drop(app.update_message(message));
            durations.push(start.elapsed());
        }
        durations.sort_unstable();
        println!(
            "{label}: n={} p50={:.3}ms max={:.3}ms",
            durations.len(),
            durations[durations.len() / 2].as_secs_f64() * 1000.0,
            durations.last().unwrap().as_secs_f64() * 1000.0
        );
    }
    for blocks in [10_000, 50_000] {
        let mut source = String::new();
        for chapter in 0..blocks / 10 {
            source.push_str(&format!(
                "# Chapter {chapter}\n\nOpening café 🦀.\n\n## Details {chapter}\n\n"
            ));
            for paragraph in 0..7 {
                source.push_str(&format!("Paragraph {chapter}.{paragraph}: **bold** and *italic* prose with [a link](https://example.com). More words to form a useful long document.\n\n"));
            }
        }
        let start = Instant::now();
        let mut app = app(&source);
        println!(
            "blocks={blocks} bytes={} headings={} load={:.3}ms",
            source.len(),
            app.outline.sections.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
        measure(
            &mut app,
            "collapse-all",
            vec![AppMessage::Outline(Message::FoldAll(true))],
        );
        measure(
            &mut app,
            "expand-all",
            vec![AppMessage::Outline(Message::FoldAll(false))],
        );
        command(&mut app, Message::QuickJump);
        measure(
            &mut app,
            "quick-query",
            (0..10)
                .map(|i| AppMessage::Outline(Message::Query(format!("Details {i}"))))
                .collect(),
        );
        command(&mut app, Message::CancelQuickJump);
        let mut moves = Vec::new();
        let mut undos = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            drop(app.move_section(
                0,
                DropTarget {
                    section: app.outline.sections.len() - 2,
                    after: true,
                },
            ));
            moves.push(start.elapsed());
            let start = Instant::now();
            drop(app.update_message(AppMessage::Undo));
            undos.push(start.elapsed());
            assert_eq!(app.document.source(), source);
        }
        for (label, mut times) in [("move-section", moves), ("undo-section", undos)] {
            times.sort_unstable();
            println!(
                "{label}: n={} p50={:.3}ms max={:.3}ms",
                times.len(),
                times[times.len() / 2].as_secs_f64() * 1000.0,
                times.last().unwrap().as_secs_f64() * 1000.0
            );
        }
    }
}

#[test]
fn outline_real_drag_moves_a_folded_subtree_and_outside_release_cancels() {
    let source = "# A\n\n## Child\n\nbody\n\n# B\n\nlast\n\n";
    let mut harness = Harness::new(source, Size::new(1180.0, 820.0));
    command(&mut harness.app, Message::Toggle(0));
    let first = harness.bounds("outline-section-0".into());
    let last = harness.bounds("outline-section-2".into());
    let handle = Point::new(first.x + first.width - 10.0, first.center_y());
    let destination = Point::new(last.center_x(), last.y + last.height - 4.0);
    harness.pointer(mouse::Event::ButtonPressed(mouse::Button::Left), handle);
    assert!(harness.app.outline.drag.is_some());
    harness.pointer(
        mouse::Event::CursorMoved {
            position: destination,
        },
        destination,
    );
    assert_eq!(
        harness.app.outline.hover,
        Some(DropTarget {
            section: 2,
            after: true
        })
    );
    harness.pointer(
        mouse::Event::ButtonReleased(mouse::Button::Left),
        destination,
    );
    assert_eq!(
        harness.app.document.source(),
        "# B\n\nlast\n\n# A\n\n## Child\n\nbody\n\n"
    );
    assert!(harness.app.outline.drag.is_none());
    send(&mut harness.app, AppMessage::Undo);
    harness.pointer(mouse::Event::ButtonPressed(mouse::Button::Left), handle);
    harness.pointer(
        mouse::Event::ButtonReleased(mouse::Button::Left),
        Point::new(800.0, 700.0),
    );
    assert!(harness.app.outline.drag.is_none());
    assert_eq!(harness.app.document.source(), source);
}

#[test]
fn outline_real_keyboard_picker_navigation_escape_and_source_shortcuts() {
    let source = "# A\n\nfirst\n\n# B\n\nsecond\n\n";
    let command_key = if cfg!(target_os = "macos") {
        keyboard::Modifiers::LOGO
    } else {
        keyboard::Modifiers::CTRL
    };
    for size in [Size::new(760.0, 520.0), Size::new(1180.0, 820.0)] {
        for mode in [EditorMode::Live, EditorMode::Source] {
            let mut harness = Harness::new(source, size);
            harness.app.mode = mode;
            harness.app.editor_focused = true;
            harness.app.sync_editor();
            harness.focus(EDITOR);
            harness.key(
                Key::Character("j".into()),
                command_key | keyboard::Modifiers::SHIFT,
            );
            assert!(matches!(harness.app.overlay, Some(Overlay::QuickJump)));
            harness.focus(QUICK_JUMP);
            harness.key(
                Key::Named(key::Named::ArrowDown),
                keyboard::Modifiers::empty(),
            );
            assert_eq!(harness.app.outline.selected, 1);
            harness.key(Key::Named(key::Named::Enter), keyboard::Modifiers::empty());
            assert!(harness.app.overlay.is_none());
            assert_eq!(harness.app.selection.head, source.find("# B").unwrap());
            harness.focus(EDITOR);
            harness.key(
                Key::Named(key::Named::ArrowUp),
                command_key | keyboard::Modifiers::ALT,
            );
            assert!(harness.app.document.source().starts_with("# B\n"));
            harness.focus(EDITOR);
            harness.key(Key::Character("z".into()), command_key);
            assert_eq!(harness.app.document.source(), source);
            let selection = harness.app.selection;
            command(&mut harness.app, Message::QuickJump);
            harness.focus(QUICK_JUMP);
            harness.key(Key::Named(key::Named::Escape), keyboard::Modifiers::empty());
            assert!(harness.app.overlay.is_none());
            assert_eq!(harness.app.selection, selection);
        }
    }
}
