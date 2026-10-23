//! Lossless Markdown storage with cached block rendering and bounded patch history.

use std::collections::VecDeque;
use std::ops::Range;
use std::time::{Duration, Instant};

use iced::widget::markdown;
use pulldown_cmark::{Event, Options, Parser, Tag};

use crate::document::block_parser::{self, BlockChange};
use crate::document::selection::Selection;

const HISTORY_BUDGET: usize = 16 * 1024 * 1024;
const COALESCE_INTERVAL: Duration = Duration::from_millis(650);

#[derive(Debug)]
pub struct Block {
    pub range: Range<usize>,
    pub rendered: Vec<markdown::Item>,
    pub heading: Option<(usize, String)>,
    words: usize,
    characters: usize,
    independent: bool,
}

#[derive(Debug)]
pub struct Document {
    source: String,
    pub blocks: Vec<Block>,
    pub words: usize,
    pub characters: usize,
    pub revision: u64,
    undo: Vec<Patch>,
    redo: Vec<Patch>,
    history_bytes: usize,
    last_edit: Option<Instant>,
    global_dependencies: bool,
    pub(crate) block_change: Option<BlockChange>,
}

#[derive(Debug)]
struct Patch {
    start: usize,
    removed: String,
    inserted: String,
    before: Option<Selection>,
    after: Option<Selection>,
}

impl Document {
    pub fn new(source: String) -> Self {
        let mut document = Self {
            source,
            blocks: Vec::new(),
            words: 0,
            characters: 0,
            revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            history_bytes: 0,
            last_edit: None,
            global_dependencies: false,
            block_change: None,
        };
        document.reparse();
        document
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    pub fn block_source(&self, index: usize) -> &str {
        &self.source[self.blocks[index].range.clone()]
    }

    pub fn replace(&mut self, range: Range<usize>, replacement: &str) {
        if self.source[range.clone()] == *replacement {
            return;
        }
        self.record(range.clone(), replacement);
        self.apply_change(range, replacement);
        self.revision += 1;
    }

    pub fn replace_edit(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        before: Selection,
        after: Selection,
    ) {
        let revision = self.revision;
        self.replace(range, replacement);
        if revision != self.revision
            && let Some(patch) = self.undo.last_mut()
        {
            patch.before.get_or_insert(before);
            patch.after = Some(after);
        }
    }

    pub fn break_history_group(&mut self) {
        self.last_edit = None;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo(&mut self) -> Option<Selection> {
        let patch = self.undo.pop()?;
        self.history_bytes -= patch.removed.len() + patch.inserted.len();
        self.apply_change(
            patch.start..patch.start + patch.inserted.len(),
            &patch.removed,
        );
        let selection = patch
            .before
            .unwrap_or_else(|| Selection::caret(patch.start));
        self.redo.push(patch);
        self.revision += 1;
        self.break_history_group();
        Some(selection)
    }

    pub fn redo(&mut self) -> Option<Selection> {
        let patch = self.redo.pop()?;
        self.apply_change(
            patch.start..patch.start + patch.removed.len(),
            &patch.inserted,
        );
        let selection = patch
            .after
            .unwrap_or_else(|| Selection::caret(patch.start + patch.inserted.len()));
        self.history_bytes += patch.removed.len() + patch.inserted.len();
        self.undo.push(patch);
        self.revision += 1;
        self.break_history_group();
        Some(selection)
    }

    pub fn block_at(&self, offset: usize) -> usize {
        self.blocks
            .partition_point(|block| block.range.start <= offset)
            .saturating_sub(1)
    }

    /// Exact Unicode-safe matches. Case folding is deliberately not used: byte
    /// offsets must always refer to the original text (İ, ß, etc. change length).
    pub fn matches(&self, query: &str) -> Vec<Range<usize>> {
        if query.is_empty() {
            return Vec::new();
        }
        self.source
            .match_indices(query)
            .map(|(start, text)| start..start + text.len())
            .collect()
    }

    pub fn replace_all(&mut self, query: &str, replacement: &str) -> usize {
        if query.is_empty() {
            return 0;
        }
        let count = self.source.matches(query).count();
        if count > 0 {
            let new_source = self.source.replace(query, replacement);
            self.break_history_group();
            self.replace(0..self.source.len(), &new_source);
            self.break_history_group();
        }
        count
    }

    fn record(&mut self, range: Range<usize>, replacement: &str) {
        self.redo.clear();
        let now = Instant::now();
        let coalesce = self
            .last_edit
            .is_some_and(|last| now.duration_since(last) < COALESCE_INTERVAL);
        if let Some(previous) = self.undo.last_mut()
            && coalesce
            && range.start >= previous.start
            && range.end <= previous.start + previous.inserted.len()
        {
            self.history_bytes -= previous.inserted.len();
            previous.inserted.replace_range(
                range.start - previous.start..range.end - previous.start,
                replacement,
            );
            self.history_bytes += previous.inserted.len();
        } else {
            let removed = self.source[range.clone()].to_owned();
            self.history_bytes += removed.len() + replacement.len();
            self.undo.push(Patch {
                start: range.start,
                removed,
                inserted: replacement.to_owned(),
                before: None,
                after: None,
            });
        }
        // Keep at least the last operation, including a paste larger than the budget.
        while self.history_bytes > HISTORY_BUDGET && self.undo.len() > 1 {
            let oldest = self.undo.remove(0);
            self.history_bytes -= oldest.removed.len() + oldest.inserted.len();
        }
        self.last_edit = Some(now);
    }

    fn reparse(&mut self) {
        let cache = std::mem::take(&mut self.blocks);
        self.reparse_cached(cache);
    }

    fn apply_change(&mut self, range: Range<usize>, replacement: &str) {
        if self.replace_island(&range, replacement) {
            return;
        }
        let cache = std::mem::take(&mut self.blocks)
            .into_iter()
            .filter_map(|mut block| {
                if block.range.end <= range.start {
                    Some(block)
                } else if block.range.start >= range.end {
                    block.range.start = block.range.start - range.len() + replacement.len();
                    block.range.end = block.range.end - range.len() + replacement.len();
                    Some(block)
                } else {
                    None
                }
            })
            .collect();
        self.source.replace_range(range, replacement);
        self.reparse_cached(cache);
    }

    fn reparse_cached(&mut self, cache: Vec<Block>) {
        self.block_change = None;
        let scan = block_parser::scan(&self.source);
        self.global_dependencies = scan.global_dependencies;
        let mut cache = VecDeque::from(cache);
        self.blocks = scan
            .spans
            .into_iter()
            .map(|span| {
                let mut range = span.range;
                range.end = range.start
                    + self.source[range.clone()]
                        .trim_end_matches(['\r', '\n'])
                        .len();
                while cache
                    .front()
                    .is_some_and(|block| block.range.start < range.start)
                {
                    cache.pop_front();
                }
                if cache.front().is_some_and(|block| block.range == range) {
                    let mut block = cache.pop_front().expect("front was checked");
                    block.independent = span.independent;
                    block
                } else {
                    parse_block(&self.source[range.clone()], range, span.independent)
                }
            })
            .collect();
        self.words = self.blocks.iter().map(|block| block.words).sum();
        self.characters = self.blocks.iter().map(|block| block.characters).sum();
    }

    fn replace_island(&mut self, edit: &Range<usize>, replacement: &str) -> bool {
        const MAX_ISLAND_BYTES: usize = 64 * 1024;
        if self.global_dependencies {
            return false;
        }
        let index = self.block_at(edit.start);
        let block = &self.blocks[index];
        // Container continuations and definition lists can cross blank lines.
        // Keep their neighboring blocks on the general path as well. A literal
        // colon paragraph is not certified by the scanner: changing an earlier
        // rule into a term could otherwise absorb it as a definition.
        if index > 0 && !self.blocks[index - 1].independent
            || self
                .blocks
                .get(index + 1)
                .is_some_and(|next| !next.independent)
        {
            return false;
        }
        if !block.independent || edit.start < block.range.start || edit.end > block.range.end {
            return false;
        }
        let Some(old) = block_parser::island(&self.source, &block.range) else {
            return false;
        };
        let new = old.start..old.end - edit.len() + replacement.len();
        if old.len() > MAX_ISLAND_BYTES || new.len() > MAX_ISLAND_BYTES {
            return false;
        }
        let mut fragment = self.source[old.clone()].to_owned();
        fragment.replace_range(edit.start - old.start..edit.end - old.start, replacement);
        let scan = block_parser::scan(&fragment);
        if scan.global_dependencies || scan.spans.iter().any(|span| !span.independent) {
            return false;
        }
        let blocks = scan
            .spans
            .into_iter()
            .map(|span| {
                let end = span.range.start
                    + fragment[span.range.clone()]
                        .trim_end_matches(['\r', '\n'])
                        .len();
                let range = span.range.start..end;
                parse_block(
                    &fragment[range.clone()],
                    range.start + old.start..range.end + old.start,
                    true,
                )
            })
            .collect::<Vec<_>>();
        self.words =
            self.words - block.words + blocks.iter().map(|block| block.words).sum::<usize>();
        self.characters = self.characters - block.characters
            + blocks.iter().map(|block| block.characters).sum::<usize>();
        self.source.replace_range(edit.clone(), replacement);
        if edit.len() != replacement.len() {
            for block in &mut self.blocks[index + 1..] {
                block.range.start = block.range.start - edit.len() + replacement.len();
                block.range.end = block.range.end - edit.len() + replacement.len();
            }
        }
        self.blocks.splice(index..index + 1, blocks);
        self.block_change = Some(BlockChange {
            previous_revision: self.revision,
            old,
            new,
        });
        true
    }
}

fn parse_block(source: &str, range: Range<usize>, independent: bool) -> Block {
    let mut heading = None;
    let mut level = None;
    let mut heading_text = String::new();
    for event in Parser::new_ext(source, Options::all()) {
        match event {
            Event::Start(Tag::Heading {
                level: heading_level,
                ..
            }) if level.is_none() => level = Some(heading_level as usize),
            Event::Text(text) | Event::Code(text) if level.is_some() => {
                heading_text.push_str(&text)
            }
            Event::End(pulldown_cmark::TagEnd::Heading(_)) => {
                heading = level.map(|level| (level, heading_text.clone()));
                break;
            }
            _ => {}
        }
    }
    let mut plain = String::new();
    for event in Parser::new_ext(source, Options::all()) {
        match event {
            Event::Text(text) | Event::Code(text) => {
                plain.push_str(&text);
            }
            Event::SoftBreak
            | Event::HardBreak
            | Event::End(
                pulldown_cmark::TagEnd::Paragraph
                | pulldown_cmark::TagEnd::Heading(_)
                | pulldown_cmark::TagEnd::Item,
            ) => plain.push(' '),
            _ => {}
        }
    }
    Block {
        range,
        rendered: markdown::parse(source).collect(),
        heading,
        words: plain.split_whitespace().count(),
        characters: source.chars().count(),
        independent,
    }
}

#[cfg(test)]
mod tests {
    use super::Document;

    fn assert_full(document: &Document) {
        let full = Document::new(document.source().into());
        assert_eq!(
            (document.words, document.characters),
            (full.words, full.characters)
        );
        assert_eq!(document.global_dependencies, full.global_dependencies);
        assert_eq!(
            document.blocks.len(),
            full.blocks.len(),
            "{:?}",
            document.source()
        );
        for (actual, expected) in document.blocks.iter().zip(&full.blocks) {
            assert_eq!(actual.range, expected.range, "{:?}", document.source());
            assert_eq!(actual.heading, expected.heading);
            assert_eq!(
                (actual.words, actual.characters, actual.independent),
                (expected.words, expected.characters, expected.independent)
            );
        }
    }

    #[test]
    fn local_and_fallback_edits_match_full_commonmark_boundaries() {
        let mut incremental = 0;
        let mut fallback = 0;
        for fixture in
            include_str!("../../tests/fixtures/block-parsing.md").split("\n<!-- case -->\n")
        {
            for source in [fixture.to_owned(), fixture.replace('\n', "\r\n")] {
                let boundaries = source
                    .char_indices()
                    .map(|(n, _)| n)
                    .chain([source.len()])
                    .collect::<Vec<_>>();
                for (index, &point) in boundaries.iter().enumerate() {
                    for replacement in [
                        "x",
                        "_",
                        "日本 e\u{301}",
                        "\n\n## New",
                        "```\n",
                        "  ",
                        "[ref]: /new\n",
                        "",
                    ] {
                        for end in [point, *boundaries.get(index + 1).unwrap_or(&point)] {
                            let mut document = Document::new(source.clone());
                            document.replace(point..end, replacement);
                            if point == end && replacement.is_empty() {
                                continue;
                            }
                            if document.block_change.is_some() {
                                incremental += 1;
                            } else {
                                fallback += 1;
                            }
                            assert_full(&document);
                        }
                    }
                }
            }
        }
        assert!(
            incremental > 100 && fallback > 100,
            "{incremental} / {fallback}"
        );
        println!("block boundaries: {incremental} incremental, {fallback} fallback edits");
    }

    #[test]
    fn repeated_splits_joins_and_history_keep_full_parse_equivalence() {
        let mut document = Document::new("# First\n\nPlain **words**.\n\nTail\n".into());
        for replacement in [
            "日本語",
            "one\n\ntwo",
            "## Heading\n\n| a | b |\n|---|---|\n| x | y |",
            "```\nunclosed",
            "- list\n\n  continuation",
            "[ref]: /target\n\n[ref]",
            "Plain again",
        ] {
            document.break_history_group();
            let start = document.source().find("\n\n").unwrap() + 2;
            let end = document
                .source()
                .rfind("\n\nTail")
                .unwrap_or(document.source().len());
            document.replace(start..end, replacement);
            assert_full(&document);
        }
        while document.can_undo() {
            document.undo();
            assert_full(&document);
        }
        while document.can_redo() {
            document.redo();
            assert_full(&document);
        }
    }
    fn replace_block(document: &mut Document, index: usize, replacement: &str) {
        let range = document.blocks[index].range.clone();
        document.replace(range, replacement);
    }

    #[test]
    fn round_trips_crlf_unicode_fences_and_references_losslessly() {
        let source = "\r\n# 日本語 🦀\r\n\r\nA [link][ref].\r\n\r\n```rust\r\n\r\nlet x = 1;\r\n```\r\n\r\n[ref]: https://example.com\r\n";
        let mut document = Document::new(source.to_owned());
        document.reparse();
        assert_eq!(document.source(), source);
        assert!(
            document
                .blocks
                .iter()
                .any(|block| !block.rendered.is_empty())
        );
    }

    #[test]
    fn localized_edits_adjust_offsets_and_undo_exactly() {
        let mut document = Document::new("# Title\n\nHello\n\n世界\n".to_owned());
        replace_block(&mut document, 1, "Hello 🦀");
        replace_block(&mut document, 1, "Hello 🦀!");
        assert_eq!(document.block_source(2), "世界");
        assert_eq!(document.source(), "# Title\n\nHello 🦀!\n\n世界\n");
        document.undo();
        assert_eq!(document.source(), "# Title\n\nHello\n\n世界\n");
        document.redo();
        assert!(document.source().contains("🦀!"));
    }

    #[test]
    fn replacement_and_redo_branching_are_unicode_safe() {
        let mut document = Document::new("İstanbul 🦀 İstanbul".to_owned());
        assert_eq!(document.matches("İstanbul"), vec![0..9, 15..24]);
        assert_eq!(document.replace_all("İstanbul", "東京"), 2);
        assert_eq!(document.source(), "東京 🦀 東京");
        document.undo();
        replace_block(&mut document, 0, "New branch");
        assert!(!document.can_redo());
    }

    #[test]
    fn nested_structures_form_single_editable_blocks() {
        let document =
            Document::new("- one\n  - nested\n- two\n\n> quote\n>\n> next\n\n---\n".to_owned());
        assert_eq!(document.blocks.len(), 3);
    }

    #[test]
    fn word_count_does_not_count_inline_delimiters_or_punctuation_as_words() {
        let document = Document::new("Hello **world**! And `code`.\n\n- First\n- Second\n".into());
        assert_eq!(document.words, 6);
    }

    #[test]
    #[ignore = "run using mise run bench"]
    fn performance_large_document_source_edits() {
        use std::time::Instant;
        let source = (0..3000)
            .map(|i| format!("## Heading {i}\n\nA paragraph with **bold** and a few words.\n\n"))
            .collect::<String>();
        let start = Instant::now();
        let mut document = Document::new(source);
        let parse = start.elapsed();
        let start = Instant::now();
        for i in 0..1000 {
            replace_block(&mut document, 3001, &format!("A new thought {i}"));
        }
        let edit = start.elapsed();
        println!(
            "6000 blocks: parse={parse:?}, 1000 source edits={edit:?}, per edit={:?}",
            edit / 1000
        );
        assert_eq!(document.blocks.len(), 6000);
    }
}
