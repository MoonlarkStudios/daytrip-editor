//! Deferred native initialization and a reusable snapshot of Iced's text.

use std::ops::Range;
use std::sync::Arc;

use iced::advanced::graphics::text::cosmic_text::LineIter;
use iced::widget::text_editor::{Action, Content, Cursor, Edit, LineEnding, Motion, Position};

use crate::document::selection::Selection;

/// Owns the native buffer and its text mirror so mutations cannot bypass tracking.
///
/// Iced's `Content::text` clones every physical line into a new String. Instead,
/// read the affected lines after native edits and splice a reusable snapshot.
/// The index follows *native* physical lines, including mixed line endings; it
/// must not be reconstructed by splitting the concatenated text after an edit.
pub(crate) struct Buffer {
    content: Content,
    snapshot: String,
    line_starts: Vec<usize>,
    #[cfg(test)]
    copied_bytes: usize,
}

impl Buffer {
    pub(crate) fn new(text: &str) -> Self {
        let content = with_text(text);
        let mut line_starts = vec![0];
        for (range, ending) in LineIter::new(text) {
            if !ending.as_str().is_empty() {
                line_starts.push(range.end + ending.as_str().len());
            }
        }
        let mut buffer = Self {
            content,
            snapshot: text.into(),
            line_starts,
            #[cfg(test)]
            copied_bytes: 0,
        };
        // The deferred constructor and LineIter normally agree. Keep a complete
        // native read as recovery if the backend's initialization changes.
        if buffer.line_starts.len() != buffer.content.line_count() {
            buffer.refresh(
                0..buffer.snapshot.len(),
                0..buffer.content.line_count(),
                buffer.line_starts.len(),
            );
        }
        buffer
    }

    pub(crate) fn content(&self) -> &Content {
        &self.content
    }

    pub(crate) fn snapshot(&self) -> &str {
        &self.snapshot
    }

    /// Applies a display/source synchronization splice to the native buffer.
    pub(crate) fn replace(&mut self, range: Range<usize>, text: &str) {
        let start = self.offset(self.position(range.start));
        let end_position = self.position(range.end);
        let end = self.offset(end_position);
        let end = if end < range.end {
            self.line_start(end_position.line + 1)
        } else {
            end
        };
        // A minimal byte diff can split CRLF/LFCR, but a native cursor cannot.
        // Include the entire ending and retain its unchanged bytes in the paste.
        let mut replacement =
            String::with_capacity(text.len() + range.start - start + end - range.end);
        replacement.push_str(&self.snapshot[start..range.start]);
        replacement.push_str(text);
        replacement.push_str(&self.snapshot[range.end..end]);
        self.select(Selection {
            anchor: start,
            head: end,
        });
        self.perform(Action::Edit(Edit::Paste(Arc::new(replacement))));
    }

    /// Returns whether text changed, without reconstructing untouched lines.
    pub(crate) fn perform(&mut self, action: Action) -> bool {
        match &action {
            Action::Edit(edit) => match edit {
                Edit::Insert(_)
                | Edit::Paste(_)
                | Edit::Enter
                | Edit::Backspace
                | Edit::Delete
                | Edit::Indent
                | Edit::Unindent => {}
            },
            _ => {
                self.content.perform(action);
                return false;
            }
        }
        let cursor = self.content.cursor();
        let anchor = cursor.selection.unwrap_or(cursor.position);
        let old_count = self.content.line_count();
        // Selection replacement and indentation affect the endpoint lines.
        // Include their neighbors for backspace/delete joins and line endings.
        let start = anchor.line.min(cursor.position.line).saturating_sub(1);
        let end = (anchor.line.max(cursor.position.line) + 2).min(old_count);
        let bytes = self.line_start(start)..self.line_start(end);
        self.content.perform(action);
        let new_count = self.content.line_count();
        let new_end = end
            .checked_add(new_count)
            .and_then(|n| n.checked_sub(old_count));
        if let Some(new_end) = new_end.filter(|&end| start < end && end <= new_count) {
            self.refresh(bytes, start..new_end, end)
        } else {
            // Fall back to a complete native read if the line-count change
            // cannot fit the supported action's window.
            self.refresh(0..self.snapshot.len(), 0..new_count, old_count)
        }
    }

    /// Converts the native anchor/head using physical lines, without scanning text.
    pub(crate) fn selection(&self) -> Selection {
        let cursor = self.content.cursor();
        let head = self.offset(cursor.position);
        Selection {
            anchor: cursor.selection.map_or(head, |anchor| self.offset(anchor)),
            head,
        }
    }

    pub(crate) fn select(&mut self, selection: Selection) {
        let selection = selection.clamp(&self.snapshot);
        if selection.is_caret() && self.content.cursor().selection.is_some() {
            // Content::move_to(None) retains an existing native anchor.
            self.content.perform(Action::Move(Motion::Left));
        }
        self.content.move_to(Cursor {
            position: self.position(selection.head),
            selection: (!selection.is_caret()).then(|| self.position(selection.anchor)),
        });
    }

    fn line_start(&self, line: usize) -> usize {
        self.line_starts
            .get(line)
            .copied()
            .unwrap_or(self.snapshot.len())
    }

    fn position(&self, offset: usize) -> Position {
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);
        let start = self.line_start(line);
        let end = start
            + self.snapshot[start..self.line_start(line + 1)]
                .trim_end_matches(['\r', '\n'])
                .len();
        Position {
            line,
            column: offset.min(end) - start,
        }
    }

    fn offset(&self, position: Position) -> usize {
        let start = self.line_start(position.line);
        let mut offset = (start + position.column).min(self.line_start(position.line + 1));
        while !self.snapshot.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }

    fn refresh(&mut self, bytes: Range<usize>, lines: Range<usize>, old_end: usize) -> bool {
        let mut replacement = String::new();
        let mut starts = Vec::with_capacity(lines.len());
        let count = self.content.line_count();
        for index in lines.clone() {
            starts.push(bytes.start + replacement.len());
            let line = self
                .content
                .line(index)
                .expect("bounded native line exists");
            replacement.push_str(&line.text);
            if index + 1 < count {
                replacement.push_str(match line.ending {
                    LineEnding::None | LineEnding::Lf => "\n",
                    LineEnding::CrLf => "\r\n",
                    LineEnding::Cr => "\r",
                    LineEnding::LfCr => "\n\r",
                });
            }
        }
        #[cfg(test)]
        {
            self.copied_bytes += replacement.len();
        }
        let changed = self.snapshot[bytes.clone()] != replacement;
        if bytes.len() != replacement.len() {
            for start in &mut self.line_starts[old_end..] {
                *start = *start - bytes.len() + replacement.len();
            }
        }
        self.line_starts.splice(lines.start..old_end, starts);
        if changed {
            self.snapshot.replace_range(bytes, &replacement);
        }
        debug_assert_eq!(self.line_starts.len(), count);
        changed
    }
}

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

    fn assert_snapshot(buffer: &Buffer, expected: &Content) {
        assert_same(buffer.content(), expected);
        assert_eq!(buffer.snapshot(), expected.text());
        let mut starts = Vec::new();
        let mut offset = 0;
        for (index, line) in expected.lines().enumerate() {
            starts.push(offset);
            offset += line.text.len();
            if index + 1 < expected.line_count() {
                offset += match line.ending {
                    LineEnding::None => 1,
                    ending => ending.as_str().len(),
                };
            }
        }
        assert_eq!(buffer.line_starts, starts);
        assert_eq!(
            buffer.position(buffer.selection().head),
            expected.cursor().position
        );
        if let Some(anchor) = expected.cursor().selection {
            assert_eq!(buffer.position(buffer.selection().anchor), anchor);
        }
    }

    #[test]
    fn snapshots_match_native_edits_at_unicode_boundaries_and_selections() {
        for source in [
            "",
            "\n",
            "café e\u{301} 🧑‍💻\n\tאבג العربية\n\nend",
            "first\r\n\t日本語\r\n\r\nlast\r\n",
            "a\nb\rc\r\nd\n\re\r\n",
        ] {
            let buffer = Buffer::new(source);
            let points = buffer
                .content
                .lines()
                .enumerate()
                .flat_map(|(line, text)| {
                    let len = text.text.len();
                    text.text
                        .char_indices()
                        .map(|(column, _)| Position { line, column })
                        .chain([Position { line, column: len }])
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            for &head in &points {
                for anchor in [None, points.first().copied(), points.last().copied()] {
                    for edit in [
                        Edit::Insert('é'),
                        Edit::Insert('\n'),
                        Edit::Insert('\r'),
                        Edit::Paste(Arc::new("日本語 e\u{301} 🧑‍💻".into())),
                        Edit::Paste(Arc::new("new\r\n\n\r\tlast\n".into())),
                        Edit::Paste(Arc::new(String::new())),
                        Edit::Enter,
                        Edit::Backspace,
                        Edit::Delete,
                        Edit::Indent,
                        Edit::Unindent,
                    ] {
                        let mut buffer = Buffer::new(source);
                        let mut expected = with_text(source);
                        let selection = Selection {
                            anchor: anchor
                                .map_or_else(|| buffer.offset(head), |a| buffer.offset(a)),
                            head: buffer.offset(head),
                        };
                        buffer.select(selection);
                        expected.move_to(Cursor {
                            position: head,
                            selection: anchor.filter(|&a| a != head),
                        });
                        let before = expected.text();
                        let changed = buffer.perform(Action::Edit(edit.clone()));
                        expected.perform(Action::Edit(edit.clone()));
                        assert_eq!(
                            changed,
                            before != expected.text(),
                            "{source:?}, {head:?}, {anchor:?}, {edit:?}"
                        );
                        assert_snapshot(&buffer, &expected);
                    }
                }
            }
        }
    }

    #[test]
    fn snapshots_follow_native_lines_through_repeated_joins_and_motions() {
        let source = "café e\u{301} 🧑‍💻\r\n\tאבג العربية words\n\r".repeat(140);
        let mut buffer = Buffer::new(&source);
        let mut expected = with_text(&source);
        for index in 0..180 {
            let line = (index * 127) % buffer.content.line_count();
            let text = buffer.content.line(line).unwrap();
            let column = if index % 2 == 0 { 0 } else { text.text.len() };
            let head = Position { line, column };
            let selection = Selection::caret(buffer.offset(head));
            buffer.select(selection);
            if expected.cursor().selection.is_some() {
                expected.perform(Action::Move(Motion::Left));
            }
            expected.move_to(Cursor {
                position: head,
                selection: None,
            });
            for action in [
                Action::Select(Motion::WordLeft),
                Action::Select(Motion::WordRight),
                Action::Edit(Edit::Paste(Arc::new("日\n\r\n\t".into()))),
                Action::Edit(Edit::Backspace),
                Action::Edit(Edit::Delete),
                Action::Edit(Edit::Enter),
                Action::Edit(Edit::Indent),
                Action::Edit(Edit::Unindent),
                Action::SelectLine,
                Action::Edit(Edit::Insert('🦀')),
                Action::SelectWord,
                Action::Edit(Edit::Backspace),
                Action::SelectAll,
                Action::Move(Motion::Right),
            ] {
                buffer.perform(action.clone());
                expected.perform(action);
                assert_snapshot(&buffer, &expected);
            }
        }
    }

    #[test]
    fn local_snapshots_copy_bounded_lines_and_navigation_copies_nothing() {
        let source = "Paragraph café e\u{301} 🧑‍💻 words.\n\n".repeat(10_000);
        let mut buffer = Buffer::new(&source);
        buffer.select(Selection::caret(source.len() / 2));
        for _ in 0..100 {
            buffer.perform(Action::Edit(Edit::Insert('日')));
        }
        assert!(buffer.copied_bytes < 64 * 1024);
        assert_eq!(buffer.snapshot(), buffer.content().text());
        let copied = buffer.copied_bytes;
        for action in [
            Action::SelectAll,
            Action::Move(Motion::Left),
            Action::Select(Motion::DocumentEnd),
        ] {
            buffer.perform(action);
        }
        assert_eq!(buffer.copied_bytes, copied);
        assert_eq!(
            buffer.selection(),
            Selection {
                anchor: 0,
                head: buffer.snapshot().len()
            }
        );
    }

    #[test]
    fn synchronization_splices_preserve_exact_bytes_inside_paired_endings() {
        for source in ["a\r\nb\n\rc\r\nd", "\r\n\r\n", "é\n\r終", "last\r\n"] {
            let boundaries = source
                .char_indices()
                .map(|(i, _)| i)
                .chain([source.len()])
                .collect::<Vec<_>>();
            for &start in &boundaries {
                for &end in boundaries.iter().filter(|&&end| end >= start) {
                    for text in ["", "é", "\n", "\r", "日\r\n\n\r"] {
                        let mut buffer = Buffer::new(source);
                        let mut expected = source.to_owned();
                        expected.replace_range(start..end, text);
                        buffer.replace(start..end, text);
                        assert_eq!(
                            buffer.snapshot(),
                            expected,
                            "{source:?}, {start}..{end}, {text:?}"
                        );
                        assert_eq!(buffer.snapshot(), buffer.content().text());
                    }
                }
            }
        }
    }

    #[test]
    #[ignore = "run using mise run bench; native action and snapshot costs only"]
    fn performance_native_snapshots() {
        use std::time::Instant;

        for blocks in [10_000, 50_000] {
            let source = "Paragraph café e\u{301} 🧑‍💻 words.\n\n".repeat(blocks);
            let mut native = with_text(&source);
            Selection::caret(source.len() / 2).restore(&mut native, &source);
            let start = Instant::now();
            let mut full_bytes = 0;
            for _ in 0..100 {
                native.perform(Action::Edit(Edit::Insert('日')));
                full_bytes += std::hint::black_box(native.text()).len();
            }
            let full_time = start.elapsed();
            let mut buffer = Buffer::new(&source);
            buffer.select(Selection::caret(source.len() / 2));
            let start = Instant::now();
            for _ in 0..100 {
                buffer.perform(Action::Edit(Edit::Insert('日')));
            }
            println!(
                "blocks={blocks} bytes={} native_edits=100 full_snapshot={full_time:?} retained_snapshot={:?} full_snapshot_bytes={full_bytes} local_snapshot_bytes={}",
                source.len(),
                start.elapsed(),
                buffer.copied_bytes
            );
            assert_eq!(buffer.snapshot(), native.text());
            assert_eq!(buffer.content().cursor(), native.cursor());
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
