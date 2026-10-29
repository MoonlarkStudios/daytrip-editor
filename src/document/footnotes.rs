//! Source-coordinate footnotes collected with the projection's parser stream.

use std::collections::{HashMap, HashSet};
use std::ops::Range;

use crate::document::block_parser::MARKDOWN_OPTIONS;
use pulldown_cmark::{Event, Options, Parser, Tag};
use unicase::UniCase;

#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub range: Range<usize>,
    pub label: String,
    pub number: Option<usize>,
    pub definition: Option<usize>,
    recognized: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Definition {
    pub range: Range<usize>,
    pub body: Range<usize>,
    pub label: String,
    pub number: Option<usize>,
    pub ambiguous: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Footnotes {
    pub references: Vec<Reference>,
    pub definitions: Vec<Definition>,
}

impl Footnotes {
    pub fn observe(&mut self, event: &Event<'_>, range: Range<usize>, source: &str) {
        match event {
            // The current parser leaves missing references literal. Inspect
            // only their containing paragraph/heading/cell with its legacy
            // reference tokenizer, which still respects escapes and code spans.
            // These disjoint inline containers keep the extra work linear.
            Event::Start(Tag::Paragraph | Tag::Heading { .. } | Tag::TableCell)
                if source[range.clone()].contains("[^") =>
            {
                for (event, local) in Parser::new_ext(
                    &source[range.clone()],
                    MARKDOWN_OPTIONS.union(Options::ENABLE_OLD_FOOTNOTES),
                )
                .into_offset_iter()
                {
                    if let Event::FootnoteReference(label) = event {
                        self.references.push(Reference {
                            range: range.start + local.start..range.start + local.end,
                            label: label.to_string(),
                            number: None,
                            definition: None,
                            recognized: false,
                        });
                    }
                }
            }
            Event::FootnoteReference(label) => self.references.push(Reference {
                range,
                label: label.to_string(),
                number: None,
                definition: None,
                recognized: true,
            }),
            Event::Start(Tag::FootnoteDefinition(label)) => {
                let raw = &source[range.clone()];
                let Some(colon) = raw.match_indices("]:").map(|(at, _)| at).find(|&at| {
                    raw[..at]
                        .bytes()
                        .rev()
                        .take_while(|&byte| byte == b'\\')
                        .count()
                        % 2
                        == 0
                }) else {
                    return;
                };
                let start = range.start + colon + 2;
                let start = start
                    + source[start..range.end]
                        .bytes()
                        .take_while(|byte| matches!(byte, b' ' | b'\t'))
                        .count();
                let end = range.start + raw.trim_end_matches(['\r', '\n']).len();
                self.definitions.push(Definition {
                    body: start.min(end)..end,
                    range,
                    label: label.to_string(),
                    number: None,
                    ambiguous: false,
                });
            }
            _ => {}
        }
    }

    /// Use the parser's own Unicode case comparison. Ambiguous definitions stay
    /// literal and cannot be overwritten through a reference's details dialog.
    pub fn finish(&mut self) {
        self.references.sort_by(|a, b| {
            a.range
                .start
                .cmp(&b.range.start)
                .then(b.recognized.cmp(&a.recognized))
        });
        self.references.dedup_by(|a, b| a.range == b.range);
        let mut definitions = HashMap::new();
        for (index, definition) in self.definitions.iter().enumerate() {
            definitions
                .entry(UniCase::new(definition.label.clone()))
                .or_insert_with(Vec::new)
                .push(index);
        }
        let mut numbers = HashMap::new();
        self.references.retain(|reference| {
            reference.recognized
                || !definitions.contains_key(&UniCase::new(reference.label.clone()))
        });
        for reference in &mut self.references {
            let key = UniCase::new(reference.label.clone());
            if let Some(indices) = definitions.get(&key) {
                reference.definition = indices.first().copied();
                if indices.len() == 1 {
                    let next = numbers.len() + 1;
                    reference.number = Some(*numbers.entry(key).or_insert(next));
                }
            }
        }
        for definition in &mut self.definitions {
            let key = UniCase::new(definition.label.clone());
            definition.number = numbers.get(&key).copied();
            definition.ambiguous = definitions
                .get(&key)
                .is_some_and(|indices| indices.len() > 1);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.references.is_empty() && self.definitions.is_empty()
    }

    pub fn reference_at(&self, offset: usize) -> Option<&Reference> {
        let index = self
            .references
            .partition_point(|note| note.range.end <= offset);
        self.references
            .get(index)
            .filter(|note| note.range.contains(&offset))
    }

    pub fn reference_before(&self, offset: usize) -> Option<&Reference> {
        let index = self
            .references
            .partition_point(|note| note.range.end <= offset);
        index
            .checked_sub(1)
            .and_then(|index| self.references.get(index))
            .filter(|note| note.range.end == offset)
    }

    pub fn definition_at(&self, offset: usize) -> Option<&Definition> {
        let index = self
            .definitions
            .partition_point(|note| note.range.end <= offset);
        self.definitions
            .get(index)
            .filter(|note| note.range.contains(&offset))
    }

    pub fn next_label(&self) -> String {
        let used = self
            .references
            .iter()
            .map(|note| UniCase::new(note.label.as_str()))
            .chain(
                self.definitions
                    .iter()
                    .map(|note| UniCase::new(note.label.as_str())),
            )
            .collect::<HashSet<_>>();
        let mut number = used.len() + 1;
        loop {
            let label = format!("note-{number}");
            if !used.contains(&UniCase::new(label.as_str())) {
                return label;
            }
            number += 1;
        }
    }

    /// Only safe word edits reach this path: labels, definition structure, and
    /// the first-reference order are unchanged; every source range still moves.
    pub fn rebase(&mut self, edit: &Range<usize>, inserted: usize) {
        let shift = |offset: usize| {
            if offset >= edit.end {
                offset - edit.len() + inserted
            } else {
                offset
            }
        };
        let rebase = |range: &mut Range<usize>| *range = shift(range.start)..shift(range.end);
        for reference in &mut self.references {
            rebase(&mut reference.range);
        }
        for definition in &mut self.definitions {
            rebase(&mut definition.range);
            rebase(&mut definition.body);
        }
    }

    /// Iced's optional Read parser has no footnote extension. Feed it ordinary
    /// Markdown with document-wide numbers and application-local preview links.
    /// The authoritative source and block/task indices are unchanged.
    pub fn read_block(&self, source: &str, block: Range<usize>) -> String {
        let mut changes = Vec::new();
        let first = self
            .references
            .partition_point(|reference| reference.range.end <= block.start);
        for reference in self.references[first..]
            .iter()
            .take_while(|reference| reference.range.start < block.end)
        {
            if let Some(number) = reference.number
                && block.start <= reference.range.start
                && reference.range.end <= block.end
            {
                changes.push((
                    reference.range.clone(),
                    format!("[\\[{number}\\]](dosty-footnote:{})", reference.range.start),
                ));
            }
        }
        let first = self
            .definitions
            .partition_point(|definition| definition.range.end <= block.start);
        for definition in self.definitions[first..]
            .iter()
            .take_while(|definition| definition.range.start < block.end)
        {
            if block.start <= definition.range.start && definition.body.end <= block.end {
                let label = definition.number.map_or_else(
                    || format!("[^{}]", definition.label),
                    |number| format!("[{number}]"),
                );
                changes.push((
                    definition.range.start..definition.body.start,
                    format!("**{label}** "),
                ));
                let columns = continuation_columns(source, definition);
                let body = &source[definition.body.clone()];
                let mut offset = definition.body.start;
                for (index, line) in body.split_inclusive('\n').enumerate() {
                    if index > 0 {
                        let length = continuation_prefix(line, columns);
                        let outside = continuation_prefix(line, columns - 4);
                        if length > outside {
                            changes.push((offset + outside..offset + length, String::new()));
                        }
                    }
                    offset += line.len();
                }
            }
        }
        changes.sort_by_key(|(range, _)| range.start);
        let mut rendered = source[block.clone()].to_owned();
        for (range, text) in changes.into_iter().rev() {
            rendered.replace_range(range.start - block.start..range.end - block.start, &text);
        }
        rendered
    }
}

pub fn continuation_columns(source: &str, definition: &Definition) -> usize {
    let start = source[..definition.range.start]
        .rfind('\n')
        .map_or(0, |at| at + 1);
    source[start..definition.range.start]
        .bytes()
        .fold(0, |column, byte| {
            if byte == b'\t' {
                column + 4 - column % 4
            } else {
                column + 1
            }
        })
        + 4
}

fn continuation_prefix(line: &str, columns: usize) -> usize {
    if columns == 0 {
        return 0;
    }
    let mut column = 0;
    for (at, byte) in line.bytes().enumerate() {
        if !matches!(byte, b' ' | b'\t' | b'>') {
            return 0;
        }
        column += if byte == b'\t' { 4 - column % 4 } else { 1 };
        if column >= columns {
            return at + 1;
        }
    }
    0
}

pub fn display_body(body: &str, columns: usize) -> String {
    body.split_inclusive('\n')
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                line
            } else {
                let prefix = continuation_prefix(line, columns);
                if prefix == 0
                    && columns > 4
                    && line.trim_matches(['>', ' ', '\t', '\r', '\n']).is_empty()
                {
                    if line.ends_with("\r\n") {
                        "\r\n"
                    } else if line.ends_with('\n') {
                        "\n"
                    } else {
                        ""
                    }
                } else {
                    &line[prefix..]
                }
            }
        })
        .collect()
}

/// Serialize a new top-level note. Continuations retain four spaces so blank
/// lines, nested lists, code, and inline Markdown belong to the same definition.
pub fn definition(label: &str, body: &str, newline: &str) -> String {
    let normalized = body.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.trim_end_matches('\n').split('\n');
    let mut result = format!("[^{label}]: {}", lines.next().unwrap_or_default());
    for line in lines {
        result.push_str(newline);
        result.push_str("    ");
        result.push_str(line);
    }
    result.push_str(newline);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::document::projection::{Affinity, Projection};
    use crate::document::selection::Selection;

    #[test]
    fn numbers_follow_first_reference_order_with_unicode_case_folding() {
        let source = "A[^STRASSE], B[^second], A[^Straße], missing[^absent].\n\n[^second]: Two.\n\n[^straße]: One.\n\n[^unused]: Kept.\n";
        let projection = Projection::new(source);
        let notes = projection.footnotes();
        assert_eq!(
            notes
                .references
                .iter()
                .map(|note| note.number)
                .collect::<Vec<_>>(),
            [Some(1), Some(2), Some(1), None]
        );
        assert!(
            projection
                .text
                .starts_with("A[1], B[2], A[1], missing[^absent].")
        );
        assert!(projection.text.contains("[^unused]: Kept."));
        assert_eq!(notes.definitions[0].number, Some(2));
        assert_eq!(notes.definitions[1].number, Some(1));
        assert_eq!(notes.definitions[2].number, None);
    }

    #[test]
    fn disclosure_and_incremental_word_updates_match_fresh_maps_at_every_boundary() {
        let source = include_str!("../../tests/fixtures/footnotes.md");
        for source in [source.to_owned(), source.replace('\n', "\r\n")] {
            let mut projection = Projection::new(&source);
            for caret in source
                .char_indices()
                .map(|(at, _)| Some(at))
                .chain([Some(source.len()), None])
            {
                projection.set_disclosure(projection.disclosure_at(caret));
                let fresh = Projection::with_context(&source, caret);
                assert_projection(&projection, &fresh);
            }
            projection.set_disclosure(projection.disclosure_at(None));
            for (from, to) in [
                ("short", "longer"),
                ("formatted", "FORMATTED"),
                ("Column", "Heading"),
            ] {
                let next = projection.source.replacen(from, to, 1);
                let accepted = projection.update_words(&next, None);
                if !accepted {
                    projection = Projection::new(&next)
                }
                assert_projection(&projection, &Projection::new(&next));
            }
            let next = projection.source.replacen("[^shared]", "[^renamed]", 1);
            assert!(!projection.update_words(&next, None));
        }
    }

    fn assert_projection(actual: &Projection, expected: &Projection) {
        assert_eq!(actual.text, expected.text);
        assert_eq!(actual.highlights, expected.highlights);
        assert_eq!(actual.rich, expected.rich);
        assert_eq!(actual.footnotes(), expected.footnotes());
        for (at, _) in actual
            .source
            .char_indices()
            .chain([(actual.source.len(), '\0')])
        {
            assert_eq!(actual.to_display(at), expected.to_display(at));
        }
        for (at, _) in actual
            .text
            .char_indices()
            .chain([(actual.text.len(), '\0')])
        {
            for affinity in [Affinity::Before, Affinity::After] {
                assert_eq!(
                    actual.to_source(at, affinity, None),
                    expected.to_source(at, affinity, None)
                );
            }
        }
    }

    #[test]
    fn collapsed_reference_edits_use_complete_source_and_keep_selection_direction() {
        let source = "Before[^long-label] after.\n\n[^long-label]: A note.\n";
        let projection = Projection::new(source);
        let reference = &projection.footnotes().references[0];
        let display = projection.to_display(reference.range.start)
            ..projection.to_display(reference.range.end);
        assert_eq!(&projection.text[display.clone()], "[1]");
        assert_eq!(
            projection.source_range(display.start + 1..display.end),
            reference.range
        );
        for forward in [true, false] {
            let selection = if forward {
                Selection {
                    anchor: reference.range.start,
                    head: reference.range.end,
                }
            } else {
                Selection {
                    anchor: reference.range.end,
                    head: reference.range.start,
                }
            };
            let visible = projection.display_selection(selection);
            assert_eq!(projection.source_selection(visible, selection), selection);
            let mut after = projection.text.clone();
            after.replace_range(display.clone(), "");
            let edit = projection.edit(source, &after, selection, Selection::caret(display.start));
            assert_eq!(edit.range, reference.range);
            assert!(edit.text.is_empty());
        }
        let focused = Projection::with_context(source, Some(reference.range.start + 3));
        assert!(focused.text.starts_with("Before[^long-label] after."));
    }

    #[test]
    fn read_cache_refreshes_global_numbers_and_source_offset_links() {
        let source = "First[^later].\n\nDistant[^old].\n\n[^later]: Later.\n\n[^old]: Old.\n";
        let mut document = Document::new(source.into());
        let projection = Projection::new(source);
        let index = document
            .blocks
            .iter()
            .position(|block| document.source()[block.range.clone()].starts_with("Distant"))
            .unwrap();
        let before = format!(
            "{:?}",
            document.rendered_with_footnotes(index, projection.footnotes())
        );
        assert!(
            before.contains(&format!(
                "dosty-footnote:{}",
                source.find("[^old]").unwrap()
            )),
            "{before}"
        );
        document.replace(0..0, "Earlier[^old].\n\n").unwrap();
        let projection = Projection::new(document.source());
        let fresh = Document::new(document.source().into());
        for index in 0..document.blocks.len() {
            assert_eq!(
                format!(
                    "{:?}",
                    document.rendered_with_footnotes(index, projection.footnotes())
                ),
                format!(
                    "{:?}",
                    fresh.rendered_with_footnotes(index, projection.footnotes())
                )
            );
        }
        let index = document
            .blocks
            .iter()
            .position(|block| document.source()[block.range.clone()].starts_with("Distant"))
            .unwrap();
        let after = format!(
            "{:?}",
            document.rendered_with_footnotes(index, projection.footnotes())
        );
        assert_ne!(before, after);
        let shown = projection
            .footnotes()
            .read_block(document.source(), document.blocks[index].range.clone());
        assert!(shown.contains("[\\[1\\]](dosty-footnote:"), "{shown}");
    }

    #[test]
    fn new_definition_serialization_keeps_nested_markdown_and_line_endings() {
        for newline in ["\n", "\r\n"] {
            let body = "First **paragraph**.\n\n- One\n- Two\n\n    code";
            let source = format!("A[^n].{newline}{newline}{}", definition("n", body, newline));
            let projection = Projection::new(&source);
            assert_eq!(projection.footnotes().definitions.len(), 1);
            assert_eq!(projection.footnotes().references[0].number, Some(1));
            assert_eq!(
                projection.footnotes().definitions[0].range.end,
                source.len()
            );
            if newline == "\r\n" {
                assert!(!source.replace("\r\n", "").contains('\n'));
            }
        }
    }

    #[test]
    fn current_syntax_keeps_adjacent_definitions_and_nested_continuations() {
        let source = "Text[^a][^b].\n\n[^a]: First.\n\n    Second paragraph.\n\n        code\n\n    - List item\n\n[^b]: Other.\n[^c]: Adjacent.\n";
        for source in [source.to_owned(), source.replace('\n', "\r\n")] {
            let projection = Projection::new(&source);
            let notes = projection.footnotes();
            assert_eq!(notes.definitions.len(), 3);
            let definition = &notes.definitions[0];
            let body = display_body(
                &source[definition.body.clone()],
                continuation_columns(&source, definition),
            );
            assert!(body.contains("Second paragraph."));
            assert!(body.contains("- List item"));
            assert!(body.contains("    code"));
            let document = Document::new(source.clone());
            let index = document
                .blocks
                .iter()
                .position(|block| block.range.start == definition.range.start)
                .unwrap();
            let rendered = document.rendered_with_footnotes(index, notes);
            assert!(
                rendered
                    .iter()
                    .any(|item| matches!(item, iced::widget::markdown::Item::List { .. })),
                "{rendered:?}"
            );
            assert!(
                projection
                    .rich
                    .code_blocks
                    .iter()
                    .any(|block| block.text.trim() == "code")
            );
            // Stock Iced Read keeps indented code text but does not populate
            // its CodeBlock correctly. Live uses the parser's exact metadata.
            assert!(format!("{rendered:?}").contains("code"));
        }
    }

    #[test]
    fn read_continuations_preserve_outer_quotes_and_lists() {
        for source in [
            "Text[^n].\n\n> [^n]: First.\n>\n>     Second.\n>\n>     - Item\n",
            "Text[^n].\n\n- [^n]: First.\n\n      Second.\n\n      - Item\n",
        ] {
            let projection = Projection::new(source);
            let definition = &projection.footnotes().definitions[0];
            let rendered = projection.footnotes().read_block(source, 0..source.len());
            let display = display_body(
                &source[definition.body.clone()],
                continuation_columns(source, definition),
            );
            assert_eq!(display, "First.\n\nSecond.\n\n- Item");
            if source.contains("> [^") {
                assert!(rendered.contains("> Second."), "{rendered:?}");
                assert!(rendered.contains("> - Item"), "{rendered:?}");
            } else {
                assert!(rendered.contains("\n  Second."), "{rendered:?}");
                assert!(rendered.contains("\n  - Item"), "{rendered:?}");
            }
        }
    }

    #[test]
    fn escaped_labels_use_the_actual_unescaped_definition_header() {
        let source = "A[^a\\]:b].\n\n[^a\\]:b]: Actual body.\n";
        let projection = Projection::new(source);
        let notes = projection.footnotes();
        assert_eq!(notes.definitions.len(), 1);
        assert_eq!(&source[notes.definitions[0].body.clone()], "Actual body.");
        assert_eq!(notes.references[0].number, Some(1));
        let rendered = notes.read_block(source, 0..source.len());
        assert!(rendered.contains("**[1]** Actual body."), "{rendered:?}");
    }

    #[test]
    fn unresolved_only_metadata_refreshes_and_escaped_or_code_lookalikes_stay_literal() {
        let source = "Before words.\n\nMissing[^note-1].\n\n`code[^literal]` and \\[^escaped].\n\n```text\nblock[^code]\n```\n";
        let mut document = Document::new(source.into());
        let mut projection = Projection::new(source);
        assert_eq!(projection.footnotes().references.len(), 1);
        assert_eq!(projection.footnotes().references[0].label, "note-1");
        assert_eq!(projection.footnotes().references[0].number, None);
        assert_ne!(projection.footnotes().next_label(), "note-1");
        document.replace(0..0, "Earlier text.\n\n").unwrap();
        assert!(!projection.update_block_batch(document.source(), &document.block_changes, None));
        projection = Projection::new(document.source());
        assert_projection(&projection, &Projection::new(document.source()));
        document
            .replace(
                document.source().len()..document.source().len(),
                "\n[^note-1]: Now defined.\n",
            )
            .unwrap();
        projection = Projection::new(document.source());
        assert_eq!(projection.footnotes().references[0].number, Some(1));
        assert!(projection.text.contains("Missing[1]."));
        assert!(projection.text.contains("code[^literal]"));
        assert!(projection.text.contains("[^escaped]"));
        assert!(projection.text.contains("block[^code]"));
    }

    #[test]
    #[ignore = "measure release footnote parsing and cold/warm Read caching serially"]
    fn performance_footnotes_parse_and_read() {
        use std::time::Instant;
        for count in [1000, 5000] {
            let mut source = (0..count)
                .map(|index| format!("Paragraph {index}: a café reference[^note-{index}].\n\n"))
                .collect::<String>();
            source.extend((0..count).map(|index| {
                format!(
                    "[^note-{index}]: A **formatted** note {index}.\n\n    Another paragraph.\n\n"
                )
            }));
            let mut construct = Vec::new();
            let mut cold = Vec::new();
            let mut warm = Vec::new();
            for _ in 0..30 {
                let start = Instant::now();
                let document = Document::new(source.clone());
                let projection = Projection::new(&source);
                construct.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(projection.footnotes().references.len(), count);
                assert_eq!(projection.footnotes().definitions.len(), count);
                let start = Instant::now();
                let rendered = (0..document.blocks.len())
                    .map(|index| {
                        document
                            .rendered_with_footnotes(index, projection.footnotes())
                            .len()
                    })
                    .sum::<usize>();
                cold.push(start.elapsed().as_secs_f64() * 1000.0);
                assert!(rendered >= count * 2);
                let start = Instant::now();
                let reused = (0..document.blocks.len())
                    .map(|index| {
                        document
                            .rendered_with_footnotes(index, projection.footnotes())
                            .len()
                    })
                    .sum::<usize>();
                warm.push(start.elapsed().as_secs_f64() * 1000.0);
                assert_eq!(rendered, reused);
            }
            for (name, mut values) in [
                ("construct", construct),
                ("read-cold", cold),
                ("read-warm", warm),
            ] {
                values.sort_by(f64::total_cmp);
                println!(
                    "footnotes/{count}/{name} bytes={} n={} p50={:.3}ms p95={:.3}ms max={:.3}ms",
                    source.len(),
                    values.len(),
                    values[values.len() / 2],
                    values[(values.len() * 95).div_ceil(100) - 1],
                    values[values.len() - 1]
                );
            }
        }
    }
}
