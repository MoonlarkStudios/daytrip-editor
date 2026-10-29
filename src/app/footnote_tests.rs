//! Exercise footnote commands through the real transaction and native mirror.

use super::*;
use crate::document::Document;
use crate::document::projection::Projection;
use std::sync::Arc;

fn app(source: &str) -> App {
    let (mut app, _) = App::new();
    app.set_document(source.into(), None, "Notes.md".into());
    app
}
fn send(app: &mut App, message: AppMessage) {
    drop(app.update(message));
    assert_eq!(
        app.document.source(),
        app.document.collaboration.materialize()
    );
    let fresh = Projection::with_context(
        app.document.source(),
        (app.mode == EditorMode::Live && app.editor_focused).then_some(app.selection.head),
    );
    assert_eq!(app.projection.text, fresh.text);
    assert_eq!(app.projection.footnotes(), fresh.footnotes());
    assert_eq!(
        app.editor.snapshot(),
        if app.mode == EditorMode::Source {
            app.document.source()
        } else {
            &app.projection.text
        }
    );
}
fn tool(app: &mut App, message: Message) {
    send(app, AppMessage::Footnote(message))
}
fn body(app: &mut App, value: &str) {
    tool(app, Message::Body(text_editor::Action::SelectAll));
    tool(
        app,
        Message::Body(text_editor::Action::Edit(text_editor::Edit::Paste(
            Arc::new(value.into()),
        ))),
    );
}

#[test]
fn insertion_preserves_prose_reverse_selection_crlf_and_one_undo_group() {
    for source in ["A café and 日本語.\r\n\r\nTail.", "", "Prose.\n\n"] {
        let mut app = app(source);
        let selection = if source.starts_with("A ") {
            Selection {
                anchor: "A café and 日本語".len(),
                head: 2,
            }
        } else {
            Selection::caret(source.len())
        };
        send(&mut app, AppMessage::SelectSource(selection));
        tool(&mut app, Message::Details);
        body(&mut app, "A **note**.\n\nA second paragraph.\n- One\n- Two");
        tool(&mut app, Message::Apply);
        assert!(
            app.overlay.is_none(),
            "{:?}",
            app.footnotes
                .form
                .as_ref()
                .and_then(|form| form.error.as_ref())
        );
        let edited = app.document.source().to_owned();
        let after = app.selection;
        assert_eq!(app.projection.footnotes().references.len(), 1);
        assert_eq!(app.projection.footnotes().definitions.len(), 1);
        assert!(edited.contains("[^note-1]"));
        if !selection.is_caret() {
            assert_eq!(app.selection, selection);
        }
        if source.contains("\r\n") {
            assert!(!edited.replace("\r\n", "").contains('\n'));
        }
        send(&mut app, AppMessage::Undo);
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, selection);
        send(&mut app, AppMessage::Redo);
        assert_eq!(app.document.source(), edited);
        assert_eq!(app.selection, after);
    }
}

#[test]
fn existing_multiline_definition_is_unchanged_until_explicitly_edited() {
    let source = include_str!("../../tests/fixtures/footnotes.md");
    let mut app = app(source);
    let at = source.find("[^SHARED]").unwrap();
    send(&mut app, AppMessage::SelectSource(Selection::caret(at)));
    tool(&mut app, Message::Details);
    assert_eq!(app.footnotes.form.as_ref().unwrap().number, Some(1));
    tool(&mut app, Message::Apply);
    assert_eq!(app.document.source(), source);
    assert!(!app.document.can_undo());
    tool(&mut app, Message::Details);
    body(
        &mut app,
        "A changed **note**.\n\n    Another paragraph.\n\n    - Nested list",
    );
    tool(&mut app, Message::Apply);
    assert!(app.overlay.is_none());
    assert!(
        app.document
            .source()
            .contains("[^shared]: A changed **note**.")
    );
    assert!(app.document.source().contains("[^SHARED]"));
    assert!(app.document.source().contains("[^unused]: An unreferenced"));
    send(&mut app, AppMessage::Undo);
    assert_eq!(app.document.source(), source);
}

#[test]
fn navigation_returns_to_stable_reverse_selection_after_remote_edits() {
    let source = "A note[^n] here.\n\n[^n]: Definition.\n";
    let mut app = app(source);
    let selection = Selection {
        anchor: "A note[^n]".len(),
        head: "A note".len(),
    };
    send(&mut app, AppMessage::SelectSource(selection));
    tool(&mut app, Message::Details);
    tool(&mut app, Message::Definition);
    assert_eq!(app.selection.head, source.find("Definition").unwrap());
    let mut peer = Document::from_state(&app.document.collaboration.state()).unwrap();
    peer.replace(0..0, "Remote.\n\n").unwrap();
    send(
        &mut app,
        AppMessage::ReplicaLoaded(Ok(peer.collaboration.state())),
    );
    tool(&mut app, Message::Back);
    assert_eq!(
        app.selection,
        Selection {
            anchor: selection.anchor + 9,
            head: selection.head + 9
        }
    );
    assert!(!app.document.can_undo());
}

#[test]
fn stable_dialog_allows_unrelated_remote_edits_and_rejects_body_overlap() {
    for overlap in [false, true] {
        let source = "Prose[^n].\n\n[^n]: Original note.\n\nTail.\n";
        let mut app = app(source);
        send(&mut app, AppMessage::SelectSource(Selection::caret(5)));
        tool(&mut app, Message::Details);
        body(&mut app, "Revised note.");
        let mut peer = Document::from_state(&app.document.collaboration.state()).unwrap();
        let at = if overlap {
            source.find("Original").unwrap()
        } else {
            0
        };
        peer.replace(at..at, "Remote ").unwrap();
        send(
            &mut app,
            AppMessage::ReplicaLoaded(Ok(peer.collaboration.state())),
        );
        let before = app.document.source().to_owned();
        tool(&mut app, Message::Apply);
        if overlap {
            assert_eq!(app.document.source(), before);
            assert!(app.footnotes.form.as_ref().unwrap().error.is_some());
        } else {
            assert!(app.overlay.is_none());
            assert!(app.document.source().starts_with("Remote Prose"));
            assert!(app.document.source().contains("[^n]: Revised note."));
            send(&mut app, AppMessage::Undo);
            assert_eq!(app.document.source(), before);
        }
    }
}

#[test]
fn missing_notes_can_be_defined_and_duplicate_labels_cannot_be_overwritten() {
    let source = "Missing[^n] here.";
    let mut missing = app(source);
    send(&mut missing, AppMessage::SelectSource(Selection::caret(7)));
    tool(&mut missing, Message::Details);
    body(&mut missing, "Now defined.");
    tool(&mut missing, Message::Apply);
    assert_eq!(missing.document.source().matches("[^n]").count(), 2);
    assert_eq!(missing.projection.footnotes().references[0].number, Some(1));
    let source = "Note[^n].\n\n[^n]: One.\n\n[^N]: Two.\n";
    let mut duplicate = app(source);
    send(
        &mut duplicate,
        AppMessage::SelectSource(Selection::caret(4)),
    );
    tool(&mut duplicate, Message::Details);
    body(&mut duplicate, "Overwrite neither.");
    tool(&mut duplicate, Message::Apply);
    assert_eq!(duplicate.document.source(), source);
    assert!(duplicate.footnotes.form.as_ref().unwrap().error.is_some());
}

#[test]
fn label_collision_and_stale_session_or_reference_fail_without_source_edits() {
    for scenario in [0, 1, 2] {
        let source = "Prose.";
        let mut app = app(source);
        tool(&mut app, Message::Details);
        body(&mut app, "New note.");
        let label = app.footnotes.form.as_ref().unwrap().label.clone();
        if scenario == 0 {
            let mut peer = Document::from_state(&app.document.collaboration.state()).unwrap();
            peer.replace(
                source.len()..source.len(),
                &format!("\n\n[^{label}]: Remote definition.\n"),
            )
            .unwrap();
            send(
                &mut app,
                AppMessage::ReplicaLoaded(Ok(peer.collaboration.state())),
            );
        } else if scenario == 1 {
            let mut peer = Document::from_state(&app.document.collaboration.state()).unwrap();
            peer.replace(
                source.len()..source.len(),
                &format!("\n\nUnresolved[^{label}].\n"),
            )
            .unwrap();
            send(
                &mut app,
                AppMessage::ReplicaLoaded(Ok(peer.collaboration.state())),
            );
        } else {
            let form = app.footnotes.form.take();
            app.set_document("Another document.".into(), None, "Other.md".into());
            app.footnotes.form = form;
            app.overlay = Some(Overlay::Footnote);
        }
        let before = app.document.source().to_owned();
        tool(&mut app, Message::Apply);
        assert_eq!(app.document.source(), before);
        assert!(app.footnotes.form.as_ref().unwrap().error.is_some());
    }
}

#[test]
fn invalid_continuation_and_code_context_do_not_publish_broken_notes() {
    let source = "Prose[^n].\n\n[^n]: A note.\n";
    let mut existing = app(source);
    send(&mut existing, AppMessage::SelectSource(Selection::caret(5)));
    tool(&mut existing, Message::Details);
    body(
        &mut existing,
        "Note.\n\nA paragraph outside the definition.",
    );
    tool(&mut existing, Message::Apply);
    assert_eq!(existing.document.source(), source);
    assert!(existing.footnotes.form.as_ref().unwrap().error.is_some());
    let source = "```text\nLiteral text\n```\n";
    let mut code = app(source);
    send(&mut code, AppMessage::SelectSource(Selection::caret(10)));
    tool(&mut code, Message::Details);
    body(&mut code, "Cannot insert here.");
    tool(&mut code, Message::Apply);
    assert_eq!(code.document.source(), source);
    assert!(code.footnotes.form.as_ref().unwrap().error.is_some());
}

#[test]
fn opening_a_reference_elsewhere_returns_to_that_reference_after_navigation() {
    let source = "First[^n].\n\nAn unrelated caret.\n\n[^n]: Definition.\n";
    for read in [false, true] {
        let mut app = app(source);
        let at = source.find("[^n]").unwrap();
        send(
            &mut app,
            AppMessage::SelectSource(Selection::caret(source.find("caret").unwrap())),
        );
        if read {
            send(&mut app, AppMessage::ToggleReading);
            drop(app.open_destination(format!("dosty-footnote:{at}")));
        } else {
            tool(&mut app, Message::OpenAt(at));
        }
        tool(&mut app, Message::Definition);
        let mut peer = Document::from_state(&app.document.collaboration.state()).unwrap();
        peer.replace(0..0, "Remote ").unwrap();
        send(
            &mut app,
            AppMessage::ReplicaLoaded(Ok(peer.collaboration.state())),
        );
        tool(&mut app, Message::Back);
        assert_eq!(app.selection, Selection::caret(at + "Remote ".len()));
    }
}

#[test]
fn escaped_label_details_edit_the_complete_body_and_preview_removes_note_indentation() {
    let source =
        "A[^a\\]:b].\n\n[^a\\]:b]: Actual body.\n\n    A second paragraph.\n\n    - An item\n";
    let mut app = app(source);
    send(&mut app, AppMessage::SelectSource(Selection::caret(1)));
    tool(&mut app, Message::Details);
    let form = app.footnotes.form.as_ref().unwrap();
    assert!(form.body.text().starts_with("Actual body."));
    assert!(
        form.preview
            .iter()
            .any(|item| matches!(item, markdown::Item::List { .. })),
        "{:?}",
        form.preview
    );
    body(&mut app, "Revised body.\n\n    - Keep list");
    tool(&mut app, Message::Apply);
    assert!(
        app.overlay.is_none(),
        "{:?}",
        app.footnotes
            .form
            .as_ref()
            .and_then(|form| form.error.as_ref())
    );
    assert!(app.document.source().contains("[^a\\]:b]: Revised body."));
    send(&mut app, AppMessage::Undo);
    assert_eq!(app.document.source(), source);
}

#[test]
fn command_click_uses_measured_reference_bounds_and_does_not_move_the_caret() {
    use crate::editor::rich_editor::RichEditor;
    use crate::editor::rich_layout::RichLayout;
    use iced::advanced::{Layout, Shell, Widget, clipboard, layout, mouse, widget};
    use iced::keyboard;
    use iced::widget::text_editor::Binding;
    use iced::{Event, Rectangle, Renderer, Size};

    let source = "A reference[^long-label] and unrelated caret.\n\n[^long-label]: Definition.\n";
    let mut app = app(source);
    let before = Selection::caret(source.find("caret").unwrap());
    send(&mut app, AppMessage::SelectSource(before));
    let reference = app.projection.footnotes().references[0].range.clone();
    let mut geometry = RichLayout::default();
    geometry.rebuild(
        &app.projection,
        &app.resolved_theme,
        1.0,
        708.0,
        None,
        &app.images,
    );
    let point = geometry.source_selection_rects(Selection {
        anchor: reference.start,
        head: reference.end,
    })[0]
        .center();
    let renderer = Renderer::Secondary(iced_tiny_skia::Renderer::new(
        iced::Font::DEFAULT,
        18.0.into(),
    ));
    let size = Size::new(720.0, 800.0);
    let mut messages = Vec::new();
    {
        let mut editor = RichEditor::new(
            app.editor.content(),
            &app.projection,
            app.document.source(),
            &app.resolved_theme,
            &app.images,
            Binding::from_key_press,
        )
        .collaboration(&app.document.collaboration)
        .selection(app.selection)
        .focused(true);
        let mut tree = widget::Tree::new(&editor as &dyn Widget<AppMessage, iced::Theme, Renderer>);
        let node = editor.layout(&mut tree, &renderer, &layout::Limits::new(Size::ZERO, size));
        for event in [
            Event::Keyboard(keyboard::Event::ModifiersChanged(
                keyboard::Modifiers::COMMAND,
            )),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        ] {
            editor.update(
                &mut tree,
                &event,
                Layout::new(&node),
                mouse::Cursor::Available(point),
                &renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(size),
            );
        }
    }
    assert!(messages.iter().any(|message| matches!(message, AppMessage::Footnote(Message::OpenAt(at)) if *at == reference.start)), "{messages:?}");
    assert!(
        !messages
            .iter()
            .any(|message| matches!(message, AppMessage::SelectSource(_)))
    );
    for message in messages {
        send(&mut app, message)
    }
    assert_eq!(app.selection, before);
    tool(&mut app, Message::Definition);
    tool(&mut app, Message::Back);
    assert_eq!(app.selection, Selection::caret(reference.start));
}

#[test]
fn fenced_code_is_retained_in_read_and_cached_definition_preview() {
    let source = "A[^n].\n\n[^n]: Note.\n\n    ```rust\n    let café = 1;\n    ```\n";
    let mut app = app(source);
    tool(&mut app, Message::OpenAt(1));
    let form = app.footnotes.form.as_ref().unwrap();
    assert!(form.preview.iter().any(|item| matches!(item, markdown::Item::CodeBlock { code, .. } if code.contains("let café = 1;"))));
    let definition = &app.projection.footnotes().definitions[0];
    let index = app
        .document
        .blocks
        .iter()
        .position(|block| block.range.start == definition.range.start)
        .unwrap();
    assert!(app.document.rendered_with_footnotes(index, app.projection.footnotes()).iter().any(|item| matches!(item, markdown::Item::CodeBlock { code, .. } if code.contains("let café = 1;"))));
}

#[test]
fn existing_notes_allow_harmless_trailing_lines_and_empty_no_op() {
    for source in ["A[^n].\n\n[^n]: Note.\n", "A[^n].\n\n[^n]:\n"] {
        let mut app = app(source);
        tool(&mut app, Message::OpenAt(1));
        if source.contains("Note.") {
            body(&mut app, "Revised note.\n\n");
        }
        tool(&mut app, Message::Apply);
        assert!(
            app.overlay.is_none(),
            "{:?}",
            app.footnotes
                .form
                .as_ref()
                .and_then(|form| form.error.as_ref())
        );
        if source.contains("Note.") {
            assert!(app.document.source().contains("Revised note.\n\n"));
            send(&mut app, AppMessage::Undo);
        }
        assert_eq!(app.document.source(), source);
    }
}

#[test]
#[ignore = "render footnote Live, Read, and detail surfaces without native windows"]
fn footnote_ui_previews() {
    use crate::theme::Preset;
    use iced::advanced::{Layout, Renderer as _, Shell, layout, mouse, renderer, widget};
    use iced::{Event, Rectangle, Renderer, Size};

    fn render(app: &App, size: Size, name: &str) {
        let mut renderer = Renderer::Secondary(iced_tiny_skia::Renderer::new(
            iced::Font::DEFAULT,
            18.0.into(),
        ));
        let mut root = app.view();
        let mut tree = widget::Tree::new(root.as_widget());
        let node = root.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let clip = Rectangle::with_size(size);
        let mut messages = Vec::new();
        root.as_widget_mut().update(
            &mut tree,
            &Event::Window(iced::window::Event::RedrawRequested(
                iced::time::Instant::now(),
            )),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut Shell::new(&mut messages),
            &clip,
        );
        renderer.reset(clip);
        root.as_widget().draw(
            &tree,
            &mut renderer,
            &app.theme(),
            &renderer::Style {
                text_color: app.resolved_theme.tokens.text,
            },
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &clip,
        );
        let size = Size::new(size.width as u32, size.height as u32);
        let mut pixels = tiny_skia::Pixmap::new(size.width, size.height).unwrap();
        let mut mask = tiny_skia::Mask::new(size.width, size.height).unwrap();
        if let Renderer::Secondary(renderer) = &mut renderer {
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &iced_tiny_skia::graphics::Viewport::with_physical_size(size, 1.0),
                &[clip],
                app.resolved_theme.tokens.canvas,
            );
        }
        let mut rgba = pixels.take();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2)
        }
        image::save_buffer(
            format!("target/footnote-validation/{name}.png"),
            &rgba,
            size.width,
            size.height,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }

    let mut fonts = iced_tiny_skia::graphics::text::font_system()
        .write()
        .unwrap();
    for font in [
        include_bytes!("../../assets/fonts/Newsreader-Regular.ttf").as_slice(),
        include_bytes!("../../assets/fonts/Newsreader-Bold.ttf").as_slice(),
        include_bytes!("../../assets/fonts/Newsreader-Italic.ttf").as_slice(),
        include_bytes!("../../assets/fonts/Newsreader-BoldItalic.ttf").as_slice(),
    ] {
        fonts.load_font(std::borrow::Cow::Borrowed(font));
    }
    drop(fonts);
    std::fs::create_dir_all("target/footnote-validation").unwrap();
    let source = include_str!("../../tests/fixtures/footnotes.md");
    let mut app = app(source);
    app.sidebar = false;
    app.preferences.recent.clear();
    app.preferences.custom_theme = None;
    app.notification = None;
    for preset in Preset::ALL {
        app.resolved_theme = preset.resolve();
        app.mode = EditorMode::Live;
        app.sync_editor();
        render(
            &app,
            Size::new(1180.0, 820.0),
            &format!("{}-live", preset.name()),
        );
        app.mode = EditorMode::Read;
        app.sync_editor();
        render(
            &app,
            Size::new(1180.0, 820.0),
            &format!("{}-read", preset.name()),
        );
        tool(&mut app, Message::OpenAt(source.find("[^shared]").unwrap()));
        render(
            &app,
            Size::new(1180.0, 820.0),
            &format!("{}-details", preset.name()),
        );
        render(
            &app,
            Size::new(760.0, 520.0),
            &format!("{}-details-narrow", preset.name()),
        );
        send(&mut app, AppMessage::CloseOverlay);
    }
}
