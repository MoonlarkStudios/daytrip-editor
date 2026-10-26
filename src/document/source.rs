//! CRDT-authoritative Markdown with an incrementally maintained parsing cache.

use crate::document::collaboration::{
    AgentProposal, Collaboration, CollaborationError, EditOrigin, ReplicaState, SourceEdit,
};
use std::cell::OnceCell;
use std::collections::VecDeque;
use std::ops::Range;

use iced::widget::markdown;
use pulldown_cmark::{Event, Options, Parser, Tag};

use crate::document::block_parser::{self, BlockChange};
use crate::document::selection::Selection;

#[derive(Debug)]
pub struct Block {
    pub range: Range<usize>,
    rendered: OnceCell<Vec<markdown::Item>>,
    pub heading: Option<(usize, String)>,
    words: usize,
    characters: usize,
    independent: bool,
}

#[cfg(test)]
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct ModelCosts {
    pub crdt: std::time::Duration,
    pub source: std::time::Duration,
}

#[derive(Debug)]
pub struct Document {
    source: String,
    #[cfg(test)]
    pub(crate) costs: ModelCosts,
    pub blocks: Vec<Block>,
    pub words: usize,
    pub characters: usize,
    pub revision: u64,
    pub(crate) collaboration: Collaboration,
    global_dependencies: bool,
    pub(crate) block_changes: Vec<BlockChange>,
}

impl Document {
    pub fn new(source: String) -> Self {
        let mut document = Self {
            collaboration: Collaboration::new(&source),
            source,
            #[cfg(test)]
            costs: ModelCosts::default(),
            blocks: Vec::new(),
            words: 0,
            characters: 0,
            revision: 0,
            global_dependencies: false,
            block_changes: Vec::new(),
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

    /// Read view has its own syntax highlighter. Build it only on demand and
    /// invalidate it together with the parsed block containing the source edit.
    pub fn rendered(&self, index: usize) -> &[markdown::Item] {
        self.blocks[index]
            .rendered
            .get_or_init(|| markdown::parse(self.block_source(index)).collect())
    }

    pub fn from_state(state: &ReplicaState) -> Result<Self, CollaborationError> {
        let (collaboration, source) = Collaboration::from_state(state)?;
        let mut document = Self {
            collaboration,
            source,
            #[cfg(test)]
            costs: ModelCosts::default(),
            blocks: Vec::new(),
            words: 0,
            characters: 0,
            revision: 0,
            global_dependencies: false,
            block_changes: Vec::new(),
        };
        document.reparse();
        Ok(document)
    }

    /// Original-state UTF-8 offsets; edits must be sorted and disjoint.
    pub fn transact(
        &mut self,
        edits: &[SourceEdit],
        origin: EditOrigin,
        selection: Option<(Selection, Selection)>,
        separate: bool,
    ) -> Result<(), CollaborationError> {
        #[cfg(test)]
        let start = std::time::Instant::now();
        let changes =
            self.collaboration
                .commit(&self.source, edits, origin, selection, separate)?;
        #[cfg(test)]
        {
            self.costs.crdt += start.elapsed();
        }
        self.consume(changes);
        Ok(())
    }
    pub fn receive(&mut self, state: &ReplicaState) -> Result<(), CollaborationError> {
        #[cfg(test)]
        let start = std::time::Instant::now();
        let changes = self.collaboration.receive(&self.source, state)?;
        #[cfg(test)]
        {
            self.costs.crdt += start.elapsed();
        }
        self.consume(changes);
        Ok(())
    }
    pub fn accept(&mut self, proposal: &AgentProposal) -> Result<(), CollaborationError> {
        let edits = self
            .collaboration
            .resolve_proposal(&self.source, proposal)?;
        self.collaboration.attribute_agent(&proposal.author);
        self.transact(&edits, EditOrigin::Agent, None, true)
    }
    fn consume(&mut self, changes: Vec<SourceEdit>) {
        #[cfg(test)]
        let start = std::time::Instant::now();
        if changes.is_empty() {
            return;
        }
        let batch = changes.len() > 1;
        if batch && !self.replace_islands(&changes) {
            let mut shifts = vec![0isize];
            for change in &changes {
                shifts.push(
                    shifts.last().expect("initial shift") + change.text.len() as isize
                        - change.range.len() as isize,
                );
            }
            let cache = std::mem::take(&mut self.blocks)
                .into_iter()
                .filter_map(|mut block| {
                    let first =
                        changes.partition_point(|change| change.range.end <= block.range.start);
                    if changes
                        .get(first)
                        .is_some_and(|change| change.range.start < block.range.end)
                    {
                        return None;
                    }
                    block.range.start = block
                        .range
                        .start
                        .checked_add_signed(shifts[first])
                        .expect("validated source size");
                    block.range.end = block
                        .range
                        .end
                        .checked_add_signed(shifts[first])
                        .expect("validated source size");
                    Some(block)
                })
                .collect();
            for change in changes.into_iter().rev() {
                self.source.replace_range(change.range, &change.text);
            }
            self.reparse_cached(cache);
        } else if !batch {
            for change in changes {
                self.apply_change(change);
            }
        }
        self.revision += 1;
        #[cfg(test)]
        {
            self.costs.source += start.elapsed();
        }
    }
    pub fn replace(
        &mut self,
        range: Range<usize>,
        replacement: &str,
    ) -> Result<(), CollaborationError> {
        let before = Selection::caret(range.start);
        let after = Selection::caret(
            range
                .start
                .checked_add(replacement.len())
                .ok_or(CollaborationError::InvalidRange)?,
        );
        self.replace_edit(range, replacement, before, after)
    }
    pub fn replace_edit(
        &mut self,
        range: Range<usize>,
        replacement: &str,
        before: Selection,
        after: Selection,
    ) -> Result<(), CollaborationError> {
        if self.source.get(range.clone()) == Some(replacement) {
            return Ok(());
        }
        self.transact(
            &[SourceEdit {
                range,
                text: replacement.to_owned(),
            }],
            EditOrigin::Human,
            Some((before, after)),
            false,
        )
    }
    pub fn break_history_group(&mut self) {
        self.collaboration.reset_group();
    }
    pub fn can_undo(&self) -> bool {
        self.collaboration.can_undo(EditOrigin::Human)
    }
    pub fn can_redo(&self) -> bool {
        self.collaboration.can_redo(EditOrigin::Human)
    }
    pub fn history(
        &mut self,
        origin: EditOrigin,
        redo: bool,
    ) -> Result<Option<Selection>, CollaborationError> {
        #[cfg(test)]
        let start = std::time::Instant::now();
        let result = self.collaboration.history(origin, redo)?;
        #[cfg(test)]
        {
            self.costs.crdt += start.elapsed();
        }
        let Some((changes, selection)) = result else {
            return Ok(None);
        };
        let fallback = changes.first().map_or(0, |edit| edit.range.start);
        self.consume(changes);
        Ok(Some(
            selection
                .unwrap_or_else(|| Selection::caret(fallback))
                .clamp(&self.source),
        ))
    }
    #[cfg(test)]
    pub fn undo(&mut self) -> Option<Selection> {
        self.history(EditOrigin::Human, false).unwrap()
    }
    #[cfg(test)]
    pub fn redo(&mut self) -> Option<Selection> {
        self.history(EditOrigin::Human, true).unwrap()
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

    pub fn replace_all(
        &mut self,
        query: &str,
        replacement: &str,
    ) -> Result<usize, CollaborationError> {
        if query.is_empty() {
            return Ok(0);
        }
        let edits: Vec<_> = self
            .source
            .match_indices(query)
            .take(crate::document::collaboration::MAX_TRANSACTION_EDITS + 1)
            .map(|(start, matched)| SourceEdit {
                range: start..start + matched.len(),
                text: replacement.to_owned(),
            })
            .collect();
        let count = edits.len();
        if count > 0 {
            self.transact(&edits, EditOrigin::Human, None, true)?;
        }
        Ok(count)
    }

    fn reparse(&mut self) {
        let cache = std::mem::take(&mut self.blocks);
        self.reparse_cached(cache);
    }

    fn apply_change(&mut self, edit: SourceEdit) {
        if self.replace_island(&edit) {
            return;
        }
        let range = edit.range;
        let replacement = &edit.text;
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
        self.block_changes.clear();
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

    fn prepare_island(&self, edits: &[SourceEdit]) -> Option<Island> {
        const MAX_ISLAND_BYTES: usize = 64 * 1024;
        if self.global_dependencies {
            return None;
        }
        let edit = edits.first()?.range.start..edits.last()?.range.end;
        let index = self.block_at(edit.start);
        let block = self.blocks.get(index)?;
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
            return None;
        }
        if !block.independent || edit.start < block.range.start || edit.end > block.range.end {
            return None;
        }
        let old = block_parser::island(&self.source, &block.range)?;
        let length = edits.iter().fold(old.len(), |len, edit| {
            len - edit.range.len() + edit.text.len()
        });
        let new = old.start..old.start + length;
        if old.len() > MAX_ISLAND_BYTES || new.len() > MAX_ISLAND_BYTES {
            return None;
        }
        let mut fragment = self.source[old.clone()].to_owned();
        for edit in edits.iter().rev() {
            fragment.replace_range(
                edit.range.start - old.start..edit.range.end - old.start,
                &edit.text,
            );
        }
        let scan = block_parser::scan(&fragment);
        if scan.global_dependencies || scan.spans.iter().any(|span| !span.independent) {
            return None;
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
        Some(Island {
            index,
            old,
            new,
            blocks,
        })
    }

    fn replace_island(&mut self, edit: &SourceEdit) -> bool {
        let Some(plan) = self.prepare_island(std::slice::from_ref(edit)) else {
            return false;
        };
        self.block_changes = vec![BlockChange {
            previous_revision: self.revision,
            old: plan.old.clone(),
            new: plan.new.clone(),
        }];
        self.apply_island(plan);
        self.source.replace_range(edit.range.clone(), &edit.text);
        true
    }

    fn replace_islands(&mut self, edits: &[SourceEdit]) -> bool {
        // Validate all windows against the immutable predecessor before changing
        // any cache. Edits in the same block are parsed together exactly once.
        let mut plans = Vec::new();
        let mut first = 0;
        while first < edits.len() {
            if plans.len() == block_parser::MAX_INCREMENTAL_WINDOWS {
                return false;
            }
            let index = self.block_at(edits[first].range.start);
            let mut end = first + 1;
            while end < edits.len() && self.block_at(edits[end].range.start) == index {
                end += 1;
            }
            let Some(plan) = self.prepare_island(&edits[first..end]) else {
                return false;
            };
            plans.push(plan);
            first = end;
        }
        let mut shift = 0isize;
        self.block_changes = plans
            .iter()
            .map(|plan| {
                let start = plan
                    .old
                    .start
                    .checked_add_signed(shift)
                    .expect("validated source size");
                shift += plan.new.len() as isize - plan.old.len() as isize;
                BlockChange {
                    previous_revision: self.revision,
                    old: plan.old.clone(),
                    new: start..start + plan.new.len(),
                }
            })
            .collect();
        for plan in plans.into_iter().rev() {
            self.apply_island(plan);
        }
        for edit in edits.iter().rev() {
            self.source.replace_range(edit.range.clone(), &edit.text);
        }
        true
    }

    fn apply_island(&mut self, plan: Island) {
        let Island {
            index,
            old,
            new,
            blocks,
        } = plan;
        let block = &self.blocks[index];
        self.words =
            self.words - block.words + blocks.iter().map(|block| block.words).sum::<usize>();
        self.characters = self.characters - block.characters
            + blocks.iter().map(|block| block.characters).sum::<usize>();
        if old.len() != new.len() {
            for block in &mut self.blocks[index + 1..] {
                block.range.start = block.range.start - old.len() + new.len();
                block.range.end = block.range.end - old.len() + new.len();
            }
        }
        self.blocks.splice(index..index + 1, blocks);
    }
}

struct Island {
    index: usize,
    old: Range<usize>,
    new: Range<usize>,
    blocks: Vec<Block>,
}

fn parse_block(source: &str, range: Range<usize>, independent: bool) -> Block {
    let mut heading = None;
    let mut level = None;
    let mut heading_text = String::new();
    for (index, event) in Parser::new_ext(source, Options::all()).enumerate() {
        // A heading inside a quote/list is not a document section.
        if index == 0 && !matches!(event, Event::Start(Tag::Heading { .. })) {
            break;
        }
        match event {
            Event::Start(Tag::Heading {
                level: heading_level,
                ..
            }) if level.is_none() => level = Some(heading_level as usize),
            Event::Text(text) | Event::Code(text) if level.is_some() => {
                heading_text.push_str(&text)
            }
            Event::SoftBreak | Event::HardBreak if level.is_some() => heading_text.push(' '),
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
        rendered: OnceCell::new(),
        heading,
        words: plain.split_whitespace().count(),
        characters: source.chars().count(),
        independent,
    }
}

#[cfg(test)]
mod tests {
    use super::Document;

    #[test]
    fn reading_cache_is_lazy_retains_unchanged_blocks_and_invalidates_code_edits() {
        let mut document = Document::new("Before\n\n```rust\nlet value = 1;\n```\n\nAfter".into());
        assert!(
            document
                .blocks
                .iter()
                .all(|block| block.rendered.get().is_none())
        );
        let index = document.block_at(document.source().find("let value").unwrap());
        let cached = document.rendered(index).as_ptr();
        document.replace(1..1, "x").unwrap();
        assert_eq!(document.rendered(index).as_ptr(), cached);
        let at = document.source().find("value").unwrap();
        document.replace(at..at + 5, "changed").unwrap();
        assert!(document.blocks[index].rendered.get().is_none());
        assert!(
            matches!(&document.rendered(index)[0], iced::widget::markdown::Item::CodeBlock { code, .. } if code.contains("let changed"))
        );
    }

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
                            document.replace(point..end, replacement).unwrap();
                            if point == end && replacement.is_empty() {
                                continue;
                            }
                            if !document.block_changes.is_empty() {
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
            document.replace(start..end, replacement).unwrap();
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
        document.replace(range, replacement).unwrap();
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
                .enumerate()
                .any(|(index, _)| !document.rendered(index).is_empty())
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
        assert_eq!(document.replace_all("İstanbul", "東京").unwrap(), 2);
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

#[cfg(test)]
#[path = "collaboration_tests.rs"]
mod collaboration_tests;
