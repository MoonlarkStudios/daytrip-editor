//! Lossless Markdown storage with cached block rendering and bounded patch history.

use std::collections::VecDeque;
use std::ops::Range;
use std::time::{Duration, Instant};

use iced::widget::markdown;
use pulldown_cmark::{Event, Options, Parser, Tag};

use crate::selection::Selection;

const HISTORY_BUDGET: usize = 16 * 1024 * 1024;
const COALESCE_INTERVAL: Duration = Duration::from_millis(650);

#[derive(Debug)]
pub struct Block {
    pub range: Range<usize>,
    pub rendered: Vec<markdown::Item>,
    pub heading: Option<(usize, String)>,
    words: usize,
    characters: usize,
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

    /// Re-establishes CommonMark block boundaries when an edit is committed.
    pub fn commit(&mut self) {
        self.last_edit = None;
        self.reparse();
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
        self.commit();
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
        self.commit();
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
        let mut ranges = Vec::new();
        let mut depth = 0_usize;
        let mut start = 0;
        for (event, range) in Parser::new_ext(&self.source, Options::all()).into_offset_iter() {
            match event {
                Event::Start(_) => {
                    if depth == 0 {
                        start = range.start;
                    }
                    depth += 1;
                }
                Event::End(_) => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        ranges.push(start..range.end);
                    }
                }
                Event::Rule | Event::Html(_) if depth == 0 => ranges.push(range),
                _ => {}
            }
        }
        // Preserve invisible Markdown such as reference definitions and blank documents.
        let mut covered = 0;
        let mut complete = Vec::new();
        for range in ranges {
            if !self.source[covered..range.start].trim().is_empty() {
                complete.push(covered..range.start);
            }
            covered = range.end;
            complete.push(range);
        }
        if !self.source[covered..].trim().is_empty() || complete.is_empty() {
            complete.push(covered..self.source.len());
        }
        let mut cache = VecDeque::from(cache);
        self.blocks = complete
            .into_iter()
            .map(|mut range| {
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
                    cache.pop_front().expect("front was checked")
                } else {
                    parse_block(&self.source[range.clone()], range)
                }
            })
            .collect();
        self.words = self.blocks.iter().map(|block| block.words).sum();
        self.characters = self.blocks.iter().map(|block| block.characters).sum();
    }
}

fn parse_block(source: &str, range: Range<usize>) -> Block {
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
    }
}

#[cfg(test)]
mod tests {
    use super::Document;
    fn replace_block(document: &mut Document, index: usize, replacement: &str) {
        let range = document.blocks[index].range.clone();
        document.replace(range, replacement);
    }

    #[test]
    fn round_trips_crlf_unicode_fences_and_references_losslessly() {
        let source = "\r\n# 日本語 🦀\r\n\r\nA [link][ref].\r\n\r\n```rust\r\n\r\nlet x = 1;\r\n```\r\n\r\n[ref]: https://example.com\r\n";
        let mut document = Document::new(source.to_owned());
        document.commit();
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
