//! Selections use UTF-8 byte offsets, independent of source or display layout.

use std::ops::Range;

use iced::widget::text_editor::{Action, Content, Cursor, Motion, Position};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: usize,
    pub head: usize,
}

impl Selection {
    pub fn caret(offset: usize) -> Self {
        Self {
            anchor: offset,
            head: offset,
        }
    }
    pub fn range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
    pub fn is_caret(self) -> bool {
        self.anchor == self.head
    }
    pub fn from_cursor(text: &str, cursor: Cursor) -> Self {
        let head = offset_at(text, cursor.position);
        Self {
            head,
            anchor: cursor
                .selection
                .map_or(head, |position| offset_at(text, position)),
        }
    }
    pub fn cursor(self, text: &str) -> Cursor {
        Cursor {
            position: position_at(text, self.head),
            selection: (!self.is_caret()).then(|| position_at(text, self.anchor)),
        }
    }
    /// Restores both endpoints, including clearing a previous native selection.
    pub fn restore(self, content: &mut Content, text: &str) {
        // Iced 0.14's `move_to` only sets an anchor when one is provided; a
        // cursor with `selection: None` leaves the previous anchor in place.
        if self.is_caret() && content.cursor().selection.is_some() {
            content.perform(Action::Move(Motion::Left));
        }
        content.move_to(self.cursor(text));
    }
    pub fn clamp(self, text: &str) -> Self {
        Self {
            anchor: floor_boundary(text, self.anchor),
            head: floor_boundary(text, self.head),
        }
    }
}

pub fn position_at(text: &str, offset: usize) -> Position {
    let prefix = &text[..floor_boundary(text, offset)];
    Position {
        line: prefix.bytes().filter(|&byte| byte == b'\n').count(),
        column: prefix.rsplit('\n').next().unwrap_or("").len(),
    }
}

pub fn offset_at(text: &str, position: Position) -> usize {
    let mut start = 0;
    for _ in 0..position.line {
        let Some(end) = text[start..].find('\n') else {
            return text.len();
        };
        start += end + 1;
    }
    let line_end = text[start..]
        .find('\n')
        .map_or(text.len(), |end| start + end);
    floor_boundary(text, (start + position.column).min(line_end))
}

fn floor_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}
