//! Heading hierarchy and lossless, atomic section moves in source coordinates.

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag};

use super::Document;
use super::block_parser::MARKDOWN_OPTIONS;
use super::collaboration::SourceEdit;
use super::selection::Selection;

#[derive(Debug, Clone)]
pub(crate) struct Section {
    pub block: usize,
    pub level: usize,
    pub title: String,
    pub range: Range<usize>,
    pub parent: Option<usize>,
    pub descendants: usize,
}

pub(crate) fn sections(document: &Document) -> Vec<Section> {
    let mut result: Vec<Section> = Vec::new();
    let mut parents: Vec<usize> = Vec::new();
    for (block, parsed) in document.blocks.iter().enumerate() {
        let Some((level, title)) = &parsed.heading else {
            continue;
        };
        // Include indentation in the move; parser offsets can start at the '#'.
        let start = document.source()[..parsed.range.start]
            .rfind('\n')
            .map_or(0, |at| at + 1);
        while let Some(&previous) = parents.last() {
            if result[previous].level < *level {
                break;
            }
            result[previous].range.end = start;
            result[previous].descendants = result.len() - previous - 1;
            parents.pop();
        }
        result.push(Section {
            block,
            level: *level,
            title: title.clone(),
            range: start..document.source().len(),
            parent: parents.last().copied(),
            descendants: 0,
        });
        parents.push(result.len() - 1);
    }
    for parent in parents {
        result[parent].descendants = result.len() - parent - 1;
    }
    result
}

pub(crate) fn current(sections: &[Section], offset: usize) -> Option<usize> {
    sections
        .partition_point(|section| section.range.start <= offset)
        .checked_sub(1)
}

pub(crate) fn sibling(sections: &[Section], index: usize, direction: i32) -> Option<usize> {
    let section = sections.get(index)?;
    let matches = |other: &Section| other.parent == section.parent && other.level == section.level;
    if direction < 0 {
        (0..index).rev().find(|&other| matches(&sections[other]))
    } else {
        (index + section.descendants + 1..sections.len()).find(|&other| matches(&sections[other]))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropTarget {
    pub section: usize,
    pub after: bool,
}

pub(crate) fn destination(sections: &[Section], from: usize, target: DropTarget) -> Option<usize> {
    let source = sections.get(from)?;
    let other = sections.get(target.section)?;
    // Dropping across parents is allowed, but never changes heading levels.
    if source.level != other.level || from == target.section {
        return None;
    }
    let at = if target.after {
        other.range.end
    } else {
        other.range.start
    };
    (!source.range.contains(&at) && at != source.range.end).then_some(at)
}

pub(crate) struct Move {
    pub edits: Vec<SourceEdit>,
    source: Range<usize>,
    destination: usize,
    prefix: usize,
    inserted: usize,
    source_len: usize,
}

impl Move {
    /// Map source positions by the moved bytes, including a selected end boundary.
    pub fn offset(&self, offset: usize, end: bool) -> usize {
        if self.source.contains(&offset)
            || (end || offset == self.source_len) && offset == self.source.end
        {
            let start = if self.destination < self.source.start {
                self.destination
            } else {
                self.destination - self.source.len()
            };
            return start + self.prefix + offset - self.source.start;
        }
        let removed = if offset >= self.source.end {
            self.source.len()
        } else {
            0
        };
        let inserted = if offset >= self.destination {
            self.inserted
        } else {
            0
        };
        offset - removed + inserted
    }

    pub fn selection(&self, selection: Selection) -> Selection {
        let forward = selection.anchor <= selection.head;
        let anchor = self.offset(selection.anchor, !forward && !selection.is_caret());
        let head = self.offset(selection.head, forward && !selection.is_caret());
        if (anchor <= head) == forward || anchor == head {
            Selection { anchor, head }
        } else {
            Selection {
                anchor: head,
                head: anchor,
            }
        }
    }
}

pub(crate) fn prepare(
    source: &str,
    sections: &[Section],
    from: usize,
    target: DropTarget,
) -> Result<Option<Move>, &'static str> {
    let Some(at) = destination(sections, from, target) else {
        return Ok(None);
    };
    let range = sections[from].range.clone();
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut text = separator(&source[..at], newline);
    let prefix = text.len();
    text.push_str(&source[range.clone()]);
    if at < source.len() {
        text.push_str(&separator(&text, newline));
    }
    let inserted = text.len();
    let mut edits = vec![
        SourceEdit {
            range: range.clone(),
            text: String::new(),
        },
        SourceEdit {
            range: at..at,
            text,
        },
    ];
    edits.sort_by_key(|edit| edit.range.start);
    // An unclosed fence/HTML block at EOF must not consume another section when
    // moved into the middle. Verify the resulting heading structure before commit.
    let mut candidate = source.to_owned();
    for edit in edits.iter().rev() {
        candidate.replace_range(edit.range.clone(), &edit.text);
    }
    let movement = Move {
        edits,
        source: range,
        destination: at,
        prefix,
        inserted,
        source_len: source.len(),
    };
    let mut expected: Vec<_> = sections
        .iter()
        .map(|section| (movement.offset(section.range.start, false), section.level))
        .collect();
    expected.sort_unstable();
    let actual = heading_positions(&candidate);
    if actual != expected {
        return Err(
            "This move would change Markdown structure. Close any unfinished code or HTML block first.",
        );
    }
    Ok(Some(movement))
}

fn separator(before: &str, newline: &str) -> String {
    if before.is_empty() {
        return String::new();
    }
    let breaks = before
        .chars()
        .rev()
        .take_while(|c| c.is_whitespace())
        .filter(|&c| c == '\n')
        .count();
    newline.repeat(2usize.saturating_sub(breaks))
}

fn heading_positions(source: &str) -> Vec<(usize, usize)> {
    let mut depth = 0usize;
    let mut headings = Vec::new();
    for (event, range) in Parser::new_ext(source, MARKDOWN_OPTIONS).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0
                    && let Tag::Heading { level, .. } = tag
                {
                    let start = source[..range.start].rfind('\n').map_or(0, |at| at + 1);
                    headings.push((start, level as usize));
                }
                depth += 1;
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    headings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::collaboration::EditOrigin;

    #[test]
    fn outline_hierarchy_excludes_container_and_fenced_headings() {
        let document = Document::new(include_str!("../../tests/fixtures/outline.md").into());
        let sections = sections(&document);
        assert_eq!(
            sections
                .iter()
                .map(|s| (s.title.as_str(), s.level, s.parent, s.descendants))
                .collect::<Vec<_>>(),
            vec![
                ("First chapter", 1, None, 3),
                ("Repeated", 2, Some(0), 1),
                ("Deep child", 3, Some(1), 0),
                ("Second child", 2, Some(0), 0),
                ("Last chapter", 1, None, 1),
                ("Repeated", 2, Some(4), 0),
            ]
        );
        assert_eq!(sections[0].range.end, sections[4].range.start);
        assert_eq!(sections[1].range.end, sections[3].range.start);
        assert_eq!(current(&sections, 0), None);
        assert_eq!(current(&sections, document.source().len()), Some(5));
        assert_eq!(sibling(&sections, 1, 1), Some(3));
        assert_eq!(sibling(&sections, 3, 1), None);
    }

    #[test]
    fn section_moves_preserve_every_byte_and_reverse_selection_with_one_undo() {
        for newline in ["\n", "\r\n"] {
            let source = include_str!("../../tests/fixtures/outline.md")
                .replace("\r\n", "\n")
                .replace('\n', newline);
            let mut document = Document::new(source.clone());
            let sections = sections(&document);
            let before = Selection {
                anchor: source.find("## Second child").unwrap(),
                head: source.find("café").unwrap(),
            };
            let movement = prepare(
                &source,
                &sections,
                0,
                DropTarget {
                    section: 4,
                    after: true,
                },
            )
            .unwrap()
            .unwrap();
            let after = movement.selection(before);
            document
                .transact(
                    &movement.edits,
                    EditOrigin::Human,
                    Some((before, after)),
                    true,
                )
                .unwrap();
            assert_eq!(
                document.source(),
                format!(
                    "{}{}{newline}{}",
                    &source[..sections[0].range.start],
                    &source[sections[4].range.clone()],
                    &source[sections[0].range.clone()]
                )
            );
            assert!(after.anchor > after.head);
            assert_eq!(&document.source()[after.range()], &source[before.range()]);
            assert_eq!(document.undo(), Some(before));
            assert_eq!(document.source(), source);
            assert_eq!(document.redo(), Some(after));
            assert_eq!(document.source(), document.collaboration.materialize());
        }
    }

    #[test]
    fn setext_indentation_crlf_and_missing_final_newline_survive_moves() {
        let source = "Preamble\r\n\r\n  First\r\n  =====\r\n\r\nα\r\n\r\n  # Last\r\n\r\n🦀";
        let mut document = Document::new(source.into());
        let sections = sections(&document);
        assert_eq!(sections.len(), 2);
        let movement = prepare(
            source,
            &sections,
            1,
            DropTarget {
                section: 0,
                after: false,
            },
        )
        .unwrap()
        .unwrap();
        let before = Selection::caret(source.len());
        // A caret at EOF belongs to the last section too.
        let after = movement.selection(before);
        document
            .transact(
                &movement.edits,
                EditOrigin::Human,
                Some((before, after)),
                true,
            )
            .unwrap();
        assert_eq!(
            document.source(),
            "Preamble\r\n\r\n  # Last\r\n\r\n🦀\r\n\r\n  First\r\n  =====\r\n\r\nα\r\n\r\n"
        );
        assert_eq!(
            after.head,
            document.source().find("🦀").unwrap() + "🦀".len()
        );
        assert_eq!(document.undo(), Some(before));
        assert_eq!(document.source(), source);
    }

    #[test]
    fn rejects_unsafe_or_noop_drops_without_touching_source() {
        for tail in ["```rust\nunfinished", "<script>\nunfinished"] {
            let source = format!("# A\n\n## Child\n\n# B\n\n{tail}");
            let document = Document::new(source.clone());
            let sections = sections(&document);
            assert!(
                prepare(
                    &source,
                    &sections,
                    2,
                    DropTarget {
                        section: 0,
                        after: false
                    }
                )
                .is_err()
            );
            assert!(
                prepare(
                    &source,
                    &sections,
                    0,
                    DropTarget {
                        section: 0,
                        after: true
                    }
                )
                .unwrap()
                .is_none()
            );
            assert!(
                prepare(
                    &source,
                    &sections,
                    0,
                    DropTarget {
                        section: 1,
                        after: true
                    }
                )
                .unwrap()
                .is_none()
            );
            assert!(
                prepare(
                    &source,
                    &sections,
                    0,
                    DropTarget {
                        section: 2,
                        after: false
                    }
                )
                .unwrap()
                .is_none()
            );
            assert_eq!(document.source(), source);
            assert!(!document.can_undo());
        }
    }

    #[test]
    fn moves_across_parents_keep_child_levels_and_unrelated_selection() {
        let source = "# A\n\n## One\n\nfirst\n\n### Deep\n\ninside\n\n# B\n\n## Two\n\nlast\n\n";
        let mut document = Document::new(source.into());
        let sections = sections(&document);
        let movement = prepare(
            source,
            &sections,
            1,
            DropTarget {
                section: 4,
                after: true,
            },
        )
        .unwrap()
        .unwrap();
        let before = Selection {
            anchor: source.find("last").unwrap() + 4,
            head: source.find("last").unwrap(),
        };
        let after = movement.selection(before);
        document
            .transact(
                &movement.edits,
                EditOrigin::Human,
                Some((before, after)),
                true,
            )
            .unwrap();
        assert_eq!(
            document.source(),
            "# A\n\n# B\n\n## Two\n\nlast\n\n## One\n\nfirst\n\n### Deep\n\ninside\n\n"
        );
        assert_eq!(&document.source()[after.range()], "last");
        assert!(after.anchor > after.head);
        assert_eq!(document.undo(), Some(before));
        assert_eq!(document.source(), source);
    }
}
