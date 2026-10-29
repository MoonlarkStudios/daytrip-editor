//! Commands retain source intent and use the ordinary CRDT transaction history.

use super::{App, EditorMode, Message, Overlay, shortcut};
use crate::document::selection::Selection;
use iced::keyboard::{Key, Modifiers, key};
use iced::widget::text_editor::{Action, Edit};
use std::sync::Arc;

fn app(source: &str) -> App {
    let (mut app, task) = App::new();
    drop(task);
    app.set_document(source.into(), None, "Tasks.md".into());
    app.editor_focused = true;
    app.sync_editor();
    app
}

fn send(app: &mut App, message: Message) {
    drop(app.update(message));
}

#[test]
fn task_toggle_preserves_reverse_source_selection_and_one_step_history() {
    let source = include_str!("../../tests/fixtures/live-tasks.md").replace('\n', "\r\n");
    for mode in [EditorMode::Live, EditorMode::Source] {
        let mut app = app(&source);
        app.mode = mode;
        let selection = Selection {
            anchor: source.len(),
            head: source.find("café").unwrap(),
        };
        app.selection = selection;
        app.sync_editor();
        let start = source.find("[X]").unwrap();
        let revision = Arc::clone(&app.projection.source_id);
        send(
            &mut app,
            Message::ToggleTaskSource {
                marker: start..start + 3,
                revision,
            },
        );
        let mut expected = source.clone();
        expected.replace_range(start..start + 3, "[ ]");
        assert_eq!(app.document.source(), expected);
        assert_eq!(app.selection, selection);
        assert!(app.dirty);
        assert_eq!(
            app.editor.snapshot(),
            if mode == EditorMode::Source {
                app.document.source()
            } else {
                &app.projection.text
            }
        );
        send(&mut app, Message::Undo);
        assert_eq!(app.document.source(), source);
        assert_eq!(app.selection, selection);
        send(&mut app, Message::Redo);
        assert_eq!(app.document.source(), expected);
        assert_eq!(app.selection, selection);
    }
}

#[test]
fn task_toggle_has_separate_history_from_typing_and_other_toggles() {
    let source = "- [ ] First\n- [x] Second\n\nText";
    let mut app = app(source);
    send(
        &mut app,
        Message::SelectSource(Selection::caret(source.len())),
    );
    send(&mut app, Message::Edit(Action::Edit(Edit::Insert('!'))));
    let after_typing = app.document.source().to_owned();
    for start in [2, source.find("[x]").unwrap()] {
        let revision = Arc::clone(&app.projection.source_id);
        send(
            &mut app,
            Message::ToggleTaskSource {
                marker: start..start + 3,
                revision,
            },
        );
    }
    send(&mut app, Message::Undo);
    assert_eq!(
        app.document.source(),
        after_typing.replacen("[ ]", "[x]", 1)
    );
    send(&mut app, Message::Undo);
    assert_eq!(app.document.source(), after_typing);
    send(&mut app, Message::Undo);
    assert_eq!(app.document.source(), source);
}

#[test]
fn task_toggle_rejects_stale_non_task_loading_and_modal_commands() {
    let source = "- [ ] Task\n\nA literal [ ]";
    let mut app = app(source);
    let revision = Arc::clone(&app.projection.source_id);
    send(
        &mut app,
        Message::SelectSource(Selection::caret(source.len())),
    );
    send(&mut app, Message::Edit(Action::Edit(Edit::Insert('!'))));
    let expected = app.document.source().to_owned();
    send(
        &mut app,
        Message::ToggleTaskSource {
            marker: 2..5,
            revision,
        },
    );
    assert_eq!(app.document.source(), expected);
    let literal = source.rfind("[ ]").unwrap();
    for (loading, overlay, marker) in [
        (false, None, literal..literal + 3),
        (true, None, 2..5),
        (false, Some(Overlay::Settings), 2..5),
    ] {
        app.loading = loading;
        app.overlay = overlay;
        let revision = Arc::clone(&app.projection.source_id);
        send(&mut app, Message::ToggleTaskSource { marker, revision });
        assert_eq!(app.document.source(), expected);
    }
}

#[test]
fn keyboard_task_command_targets_only_its_physical_line_and_preserves_caret() {
    let source = "> - [X] café\r\n>   continuation\r\n\r\n- [ ] sibling";
    let mut app = app(source);
    let at = source.find("café").unwrap() + "café".len();
    send(&mut app, Message::SelectSource(Selection::caret(at)));
    let message = shortcut(&Key::Named(key::Named::Enter), Modifiers::COMMAND).unwrap();
    assert!(matches!(message, Message::ToggleCurrentTask));
    send(&mut app, message);
    assert_eq!(app.document.source(), source.replacen("[X]", "[ ]", 1));
    assert_eq!(app.selection, Selection::caret(at));
    send(&mut app, Message::Undo);
    assert_eq!(app.document.source(), source);
    for at in [
        source.find("continuation").unwrap(),
        source.find("\r\n\r\n").unwrap() + 2,
    ] {
        send(&mut app, Message::SelectSource(Selection::caret(at)));
        send(&mut app, Message::ToggleCurrentTask);
        assert_eq!(app.document.source(), source);
    }
    assert!(
        shortcut(
            &Key::Named(key::Named::Enter),
            Modifiers::COMMAND | Modifiers::SHIFT
        )
        .is_none()
    );
}

#[test]
fn read_task_controls_use_the_same_source_preserving_transaction() {
    let source = "- [ ] First\n  - [x] Nested\n\nAfter";
    let mut app = app(source);
    app.mode = EditorMode::Read;
    let selection = Selection {
        anchor: source.len(),
        head: 0,
    };
    app.selection = selection;
    send(&mut app, Message::ToggleTask { block: 0, task: 1 });
    assert_eq!(app.document.source(), source.replacen("[x]", "[ ]", 1));
    assert_eq!(app.selection, selection);
    send(&mut app, Message::Undo);
    assert_eq!(app.document.source(), source);
    assert_eq!(app.selection, selection);
}
