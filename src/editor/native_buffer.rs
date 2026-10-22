//! Populate Iced's native editing buffer without shaping the whole document.

use std::sync::Arc;

use iced::advanced::graphics::text::cosmic_text::LineIter;
use iced::widget::text_editor::{Action, Content, Cursor, Edit, Position};

/// Keeps the standard Iced editor, including its Unicode editing semantics.
///
/// In Iced 0.14, `Content::with_text` shapes with an unbounded viewport. Pasting
/// into an empty content instead leaves shaping to native cursor actions and
/// widget layout. Batches end at physical line boundaries: cosmic-text inserts
/// each pasted line into a Vec, so a single document-sized paste is quadratic.
pub(crate) fn with_text(text: &str) -> Content {
    const BATCH_LINES: usize = 128;
    const BATCH_BYTES: usize = 64 * 1024;

    let mut content = Content::new();
    let mut start = 0;
    let mut lines = 0;
    for (range, ending) in LineIter::new(text) {
        lines += 1;
        let end = range.end + ending.as_str().len();
        if lines >= BATCH_LINES || end - start >= BATCH_BYTES || end == text.len() {
            content.perform(Action::Edit(Edit::Paste(Arc::new(text[start..end].into()))));
            start = end;
            lines = 0;
        }
    }
    content.move_to(Cursor {
        position: Position { line: 0, column: 0 },
        selection: None,
    });
    content
}

#[cfg(test)]
mod tests {
    use iced::widget::text_editor::Motion;

    use super::*;
    use crate::document::selection::Selection;

    fn assert_same(actual: &Content, expected: &Content) {
        assert_eq!(actual.text(), expected.text());
        assert_eq!(actual.line_count(), expected.line_count());
        assert_eq!(actual.cursor(), expected.cursor());
        assert_eq!(actual.selection(), expected.selection());
        for index in 0..actual.line_count() {
            let a = actual.line(index).unwrap();
            let b = expected.line(index).unwrap();
            assert_eq!(a.text, b.text);
            assert_eq!(a.ending, b.ending);
        }
    }

    #[test]
    fn deferred_loading_matches_native_text_cursor_and_line_endings() {
        for source in [
            String::new(),
            "\n".into(),
            "café 🦀 e\u{301}\r\nאבג العربية\r終わり\n\r".repeat(140),
            format!("{}\r\nlast", "🦀".repeat(20_000)),
            "first\n\nlast without newline".into(),
        ] {
            assert_same(&with_text(&source), &Content::with_text(&source));
        }
    }

    #[test]
    fn deferred_buffer_preserves_native_unicode_movement_and_editing() {
        // Crosses batch boundaries and includes multi-scalar graphemes, RTL,
        // tabs and CRLF. Replay on the unmodified native constructor as oracle.
        let source = "café e\u{301} 🦀 👩‍💻\r\n\tאבג العربية words\n".repeat(130);
        let mut actual = with_text(&source);
        let mut expected = Content::with_text(&source);
        for offset in [0, source.len() / 2, source.len()] {
            let selection = Selection::caret(offset).clamp(&source);
            selection.restore(&mut actual, &source);
            selection.restore(&mut expected, &source);
            for action in [
                Action::Move(Motion::Right),
                Action::Move(Motion::Left),
                Action::Select(Motion::WordLeft),
                Action::Select(Motion::WordRight),
                Action::Edit(Edit::Paste(Arc::new("日本語\nnew line".into()))),
                Action::Edit(Edit::Backspace),
                Action::Edit(Edit::Delete),
                Action::Edit(Edit::Enter),
                Action::Edit(Edit::Insert('é')),
                Action::SelectAll,
                Action::Move(Motion::DocumentStart),
            ] {
                actual.perform(action.clone());
                expected.perform(action);
                assert_same(&actual, &expected);
            }
        }
    }
}
