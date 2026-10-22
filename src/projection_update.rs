//! Conservative incremental projection for ordinary word edits. Structural
//! changes, links, HTML, definitions, escapes, and delimiter edges use a reparse.

use super::{OffsetMap, Projection, ProjectionChange, RichDocument, TextChange, difference};
use std::ops::Range;
use std::sync::Arc;

fn word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b' '
}

fn shift(offset: usize, edit: &Range<usize>, inserted: usize) -> usize {
    if offset >= edit.end {
        offset - edit.len() + inserted
    } else {
        offset
    }
}
fn rebase(range: &mut Range<usize>, edit: &Range<usize>, inserted: usize) {
    range.start = shift(range.start, edit, inserted);
    range.end = shift(range.end, edit, inserted);
}

impl Projection {
    /// Word characters strictly within a parsed text run cannot change Markdown
    /// structure or delimiter flanking. Everything else takes the full parser.
    pub fn updating(&self, source: &str, caret: Option<usize>) -> Option<Self> {
        let diff = difference(&self.source, source);
        let inserted = &source[diff.new.clone()];
        let removed = &self.source[diff.old.clone()];
        if diff.old.len() + inserted.len() == 0
            || diff.old.len() + inserted.len() > 1024
            || !inserted.bytes().chain(removed.bytes()).all(word_byte)
            || !self.source[..diff.old.start]
                .as_bytes()
                .last()
                .is_some_and(|&b| word_byte(b))
            || self
                .source
                .as_bytes()
                .get(diff.old.end)
                .is_some_and(|&b| !word_byte(b))
        {
            return None;
        }
        let i = self
            .parsed
            .safe_text
            .partition_point(|r| r.end < diff.old.end);
        let safe = self.parsed.safe_text.get(i)?;
        if safe.start >= diff.old.start
            || safe.end < diff.old.end
            || safe.end == diff.old.end && diff.old.end != self.source.len()
        {
            return None;
        }
        // Unresolved reference labels and autolinks are ordinary Text events.
        // Editing a word inside them can make a distant definition resolve or
        // turn literal text into markup, despite unchanged delimiter flanking.
        let prefix = &self.source[..diff.old.start];
        let block_start = prefix
            .rfind("\n\n")
            .map_or(0, |n| n + 2)
            .max(prefix.rfind("\r\n\r\n").map_or(0, |n| n + 4));
        let mut brackets = 0_usize;
        let mut angle = false;
        for byte in prefix.as_bytes()[block_start..].iter().copied() {
            match byte {
                b'[' => brackets += 1,
                b']' => brackets = brackets.saturating_sub(1),
                b'<' => angle = true,
                b'>' => angle = false,
                _ => {}
            }
        }
        if brackets > 0 || angle {
            return None;
        }
        let i = self
            .mapping
            .pieces
            .partition_point(|p| p.source.end < diff.old.end);
        let piece = self.mapping.pieces.get(i)?;
        if !piece.identity
            || piece.source.start >= diff.old.start
            || piece.source.end < diff.old.end
        {
            return None;
        }

        let display = self.to_display(diff.old.start)..self.to_display(diff.old.end);
        let mut text = self.text.clone();
        text.replace_range(display.clone(), inserted);
        let mut pieces = self.mapping.pieces.clone();
        for piece in &mut pieces {
            rebase(&mut piece.source, &diff.old, inserted.len());
            rebase(&mut piece.display, &display, inserted.len());
        }
        let mut parsed = (*self.parsed).clone();
        for decoration in &mut parsed.decorations {
            rebase(&mut decoration.range, &diff.old, inserted.len());
        }
        for mark in &mut parsed.marks {
            rebase(&mut mark.range, &diff.old, inserted.len());
        }
        for pair in &mut parsed.pairs {
            rebase(&mut pair.open, &diff.old, inserted.len());
            rebase(&mut pair.body, &diff.old, inserted.len());
            rebase(&mut pair.close, &diff.old, inserted.len());
        }
        for range in parsed
            .block_prefixes
            .iter_mut()
            .chain(&mut parsed.safe_text)
        {
            rebase(range, &diff.old, inserted.len());
        }
        rebase_rich(&mut parsed.rich, &diff.old, inserted.len(), false);
        let disclosure = self.disclosure_after(&diff.old, inserted.len());
        if disclosure != Self::resolve_disclosure(source, &parsed, caret) {
            return None;
        }
        let line = self
            .line_starts
            .partition_point(|&n| n <= display.start)
            .saturating_sub(1);
        let local = display.start - self.line_starts[line]..display.end - self.line_starts[line];
        let mut highlights = (*self.highlights).clone();
        let mut runs = highlights[line].to_vec();
        for (range, _) in &mut runs {
            rebase(range, &local, inserted.len());
        }
        highlights[line] = runs.into();
        let mut line_starts = self.line_starts.to_vec();
        for start in &mut line_starts[line + 1..] {
            *start = shift(*start, &display, inserted.len());
        }
        let mut rich = self.rich.clone();
        // Source ranges and displayed ranges have different offsets.
        rebase_rich(&mut rich, &display, inserted.len(), true);
        for table in &mut rich.tables {
            rebase(&mut table.source_range, &diff.old, inserted.len());
            for range in table.source_rows.iter_mut().flatten() {
                rebase(range, &diff.old, inserted.len());
            }
        }
        Some(Self {
            mapping: Arc::new(OffsetMap {
                pieces,
                source_length: source.len(),
                display_length: text.len(),
            }),
            text,
            source: Arc::from(source),
            highlights: Arc::new(highlights),
            rich,
            line_starts: line_starts.into(),
            parsed: Arc::new(parsed),
            disclosure,
            change: Some(ProjectionChange {
                regions: vec![TextChange {
                    old: display.clone(),
                    new: display.start..display.start + inserted.len(),
                }],
                previous: Arc::clone(&self.highlights),
            }),
        })
    }
}

fn rebase_rich(rich: &mut RichDocument, edit: &Range<usize>, inserted: usize, display: bool) {
    for heading in &mut rich.headings {
        rebase(&mut heading.range, edit, inserted);
    }
    for image in &mut rich.images {
        rebase(&mut image.range, edit, inserted);
    }
    for table in &mut rich.tables {
        rebase(&mut table.range, edit, inserted);
        for cell in table.rows.iter_mut().flatten() {
            rebase(cell, edit, inserted);
        }
        if !display {
            rebase(&mut table.source_range, edit, inserted);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projection::Affinity;

    #[test]
    fn accepted_word_edits_match_full_parse_and_every_source_mapping() {
        let fixtures = [
            "## Heading words\n\nA **strong *nested* word** and plain words.\n\nEndword",
            "Title words\n===========\n\n- [x] First words\n- Second words\n\n> Quoted words\n",
            "| Header words | Other words |\n|---|---|\n| Plain words | **Strong words** |\n\n![alt](image.png)\n\nTailwords",
            "日本語\r\n\r\nAlpha words [label][ref] <https://example.com> and `code words`.\r\n\r\n[ref]: https://example.com\r\n",
            "<div>\nHTML words\n</div>\n\n```rust\nlet words = 1;\n```\n\nPlainwords",
            "Plain [abxc] words\n\n[abc]: https://example.com\n\nA <htxtps://example.com> and words",
        ];
        let mut accepted = 0;
        for source in fixtures {
            for (point, _) in source.char_indices() {
                for (removed, inserted) in [(0, "x"), (0, " "), (1, "new"), (1, "")] {
                    if point + removed > source.len() || !source.is_char_boundary(point + removed) {
                        continue;
                    }
                    let old = Projection::with_context(source, Some(point));
                    let mut next = source.to_owned();
                    next.replace_range(point..point + removed, inserted);
                    let head = point + inserted.len();
                    let Some(fast) = old.updating(&next, Some(head)) else {
                        continue;
                    };
                    accepted += 1;
                    let full = Projection::with_context(&next, Some(head));
                    assert_eq!(
                        fast.text, full.text,
                        "{source:?} at {point} -> {inserted:?}"
                    );
                    assert_eq!(fast.highlights, full.highlights, "{next:?}");
                    assert_eq!(fast.rich, full.rich, "{next:?}");
                    assert_eq!(fast.line_starts, full.line_starts);
                    for raw in next
                        .char_indices()
                        .map(|(n, _)| n)
                        .chain(std::iter::once(next.len()))
                    {
                        assert_eq!(
                            fast.to_display(raw),
                            full.to_display(raw),
                            "source {raw} in {next:?}"
                        );
                    }
                    for display in full
                        .text
                        .char_indices()
                        .map(|(n, _)| n)
                        .chain(std::iter::once(full.text.len()))
                    {
                        for affinity in [Affinity::Before, Affinity::After] {
                            assert_eq!(
                                fast.to_source(display, affinity, None),
                                full.to_source(display, affinity, None),
                                "display {display} in {next:?}"
                            );
                        }
                    }
                }
            }
        }
        assert!(
            accepted > 100,
            "exercise actual incremental edits, got {accepted}"
        );
    }

    #[test]
    fn structural_and_reference_edits_require_full_parsing() {
        for (source, old, replacement) in [
            ("Plain words", "words", "**bold**"),
            ("Plain words", "words", "next\nline"),
            (
                "[label][ref]\n\n[ref]: https://example.com",
                "label",
                "other",
            ),
            (
                "[label][ref]\n\n[ref]: https://example.com",
                "example",
                "other",
            ),
            ("<span title=\"words\">", "words", "other"),
            ("![words](image.png)", "words", "other"),
            ("**words**", "words", ""),
            ("A [abxc] text\n\n[abc]: https://example.com", "x", ""),
            ("A <htxtps://example.com>", "x", ""),
        ] {
            let projection = Projection::new(source);
            let point = source.find(old).unwrap();
            let mut next = source.to_owned();
            next.replace_range(point..point + old.len(), replacement);
            assert!(projection.updating(&next, None).is_none(), "{source:?}");
        }
    }

    #[test]
    fn chunked_differences_are_utf8_safe_and_round_trip() {
        let values = ["", "a", "é", "ê", "日本語", "🦀", "🦁", "aé🦀z", "áalpha"];
        for a in values {
            for b in values {
                for padding in [0, 63, 64, 65, 256] {
                    let before = format!("{}{a}{}", "é".repeat(padding), "🦀".repeat(padding));
                    let after = format!("{}{b}{}", "é".repeat(padding), "🦀".repeat(padding));
                    let diff = difference(&before, &after);
                    let mut actual = before;
                    actual.replace_range(diff.old, &after[diff.new]);
                    assert_eq!(actual, after);
                }
            }
        }
    }
}
