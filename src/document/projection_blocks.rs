//! Reparse an isolated block window and splice owned projection metadata.

use std::ops::Range;
use std::sync::Arc;

use super::{Disclosure, OffsetMap, Parsed, Piece, Projection, ProjectionChange, TextChange};
use crate::document::block_parser::BlockChange;
use crate::document::rich_document::RichDocument;

#[path = "projection_batch.rs"]
mod batch;

impl Projection {
    /// Preflight every splice before mutating the retained source or metadata.
    pub(crate) fn update_blocks(
        &mut self,
        source: &str,
        change: &BlockChange,
        caret: Option<usize>,
    ) -> bool {
        self.block_update(source, change, caret).is_some()
    }

    fn block_update(
        &mut self,
        source: &str,
        change: &BlockChange,
        caret: Option<usize>,
    ) -> Option<()> {
        let old = &change.old;
        let new = &change.new;
        if self.parsed.global_dependencies
            || old.start != new.start
            || old.end > self.source.len()
            || new.end > source.len()
            || self.source[..old.start] != source[..new.start]
            || self.source[old.end..] != source[new.end..]
        {
            return None;
        }
        let local = Self::with_context(
            &source[new.clone()],
            caret
                .filter(|&head| new.start <= head && head <= new.end)
                .map(|head| head - new.start),
        );
        if local.parsed.global_dependencies {
            return None;
        }
        let raw = TextChange {
            old: old.clone(),
            new: new.clone(),
        };
        let plan = ParsedPlan::new(&self.parsed, &raw)?;
        let disclosure = if caret.is_some_and(|head| new.start <= head && head <= new.end) {
            Disclosure(
                local
                    .disclosure
                    .0
                    .iter()
                    .map(|r| r.start + new.start..r.end + new.start)
                    .collect(),
            )
        } else {
            let mut disclosure = self.disclosure_at(caret.map(|head| raw.previous_offset(head)));
            for range in &mut disclosure.0 {
                if range.start >= old.end {
                    translate(range, old.end, new.end);
                }
            }
            disclosure
        };
        let outside = |ranges: &[Range<usize>], window: &Range<usize>, base: usize| {
            ranges
                .iter()
                .filter(|r| r.end <= window.start || r.start >= window.end)
                .map(|r| {
                    if r.start >= window.end {
                        r.start - window.end + base..r.end - window.end + base
                    } else {
                        r.clone()
                    }
                })
                .collect::<Vec<_>>()
        };
        if outside(&self.disclosure.0, old, new.end) != outside(&disclosure.0, new, new.end) {
            return None;
        }
        let display = self.to_display(old.start)..self.to_display(old.end);
        let next_display = display.start..display.start + local.text.len();
        let shown = TextChange {
            old: display.clone(),
            new: next_display.clone(),
        };
        let rich_plan = RichPlan::new(&self.rich, &shown)?;
        let a = self.line_starts.binary_search(&display.start).ok()?;
        let b = if display.end == self.text.len() && !self.text.ends_with('\n') {
            self.highlights.len()
        } else {
            self.line_starts.binary_search(&display.end).ok()?
        };
        let shared_tail = b < self.highlights.len();
        if shared_tail && !local.text.ends_with('\n') {
            return None;
        }
        let count = local.highlights.len() - usize::from(shared_tail);
        let mut highlights = (*self.highlights).clone();
        highlights.splice(a..b, local.highlights[..count].iter().cloned());
        let mut starts = self.line_starts.to_vec();
        starts.splice(
            a..b,
            local.line_starts[..count]
                .iter()
                .map(|start| start + display.start),
        );
        if shown.old.len() != shown.new.len() {
            for start in &mut starts[a + count..] {
                *start = *start - shown.old.len() + shown.new.len();
            }
        }
        if starts.len() != highlights.len() {
            return None;
        }
        let mapping = Arc::new(OffsetMap {
            pieces: patch_pieces(&self.mapping.pieces, &local.mapping.pieces, &raw, &shown)?,
            source_length: source.len(),
            display_length: self.text.len() - display.len() + local.text.len(),
        });
        let plain = local.rich.headings.is_empty()
            && local.rich.code_blocks.is_empty()
            && local.rich.tables.is_empty()
            && local.rich.images.is_empty()
            && plan.rich.headings.is_empty()
            && plan.rich.code_blocks.is_empty()
            && plan.rich.tables.is_empty()
            && plan.rich.images.is_empty();
        let mut region = shown.clone();
        if plain && self.text[region.old.clone()].ends_with('\n') && local.text.ends_with('\n') {
            region.old.end -= 1;
            region.new.end -= 1;
        }
        // All fallible boundary checks are complete. Retained Strings, table
        // rows and parsed vectors keep their allocations; only suffix offsets move.
        plan.apply(&mut self.parsed, local.parsed, &raw);
        rich_plan.apply(&mut self.rich, local.rich, &raw, &shown);
        self.text.replace_range(display, &local.text);
        self.source.replace_range(old.clone(), &source[new.clone()]);
        self.source_id = Arc::new(());
        self.mapping = mapping;
        self.line_starts = starts.into();
        self.disclosure = disclosure;
        self.change = Some(ProjectionChange {
            previous: std::mem::replace(&mut self.highlights, Arc::new(highlights)),
            regions: vec![region],
        });
        Some(())
    }
}

fn translate(range: &mut Range<usize>, from: usize, to: usize) {
    *range = range.start - from + to..range.end - from + to;
}

/// Retained ranges must be entirely outside the window; empty boundary nodes
/// belong to it. Check contiguity as well as containment before any mutation.
fn window<T>(
    old: &[T],
    change: &TextChange,
    extent: impl Fn(&T) -> Range<usize>,
) -> Option<Range<usize>> {
    let mut first = 0;
    let mut removed = 0;
    let mut after = false;
    for (index, item) in old.iter().enumerate() {
        let range = extent(item);
        let empty =
            range.is_empty() && change.old.start <= range.start && range.end <= change.old.end;
        if !empty && range.end <= change.old.start {
            if removed > 0 || after {
                return None;
            }
            first = index + 1;
        } else if !empty && range.start >= change.old.end {
            after = true;
        } else {
            if after || range.start < change.old.start || range.end > change.old.end {
                return None;
            }
            removed += 1;
        }
    }
    Some(first..first + removed)
}

fn patch<T>(
    old: &mut Vec<T>,
    local: Vec<T>,
    window: Range<usize>,
    change: &TextChange,
    move_item: impl Fn(&mut T, usize, usize),
) {
    let tail = window.start + local.len();
    old.splice(
        window,
        local.into_iter().map(|mut item| {
            if change.new.start != 0 {
                move_item(&mut item, 0, change.new.start);
            }
            item
        }),
    );
    if change.old.end != change.new.end {
        for item in &mut old[tail..] {
            move_item(item, change.old.end, change.new.end);
        }
    }
}

struct RichPlan {
    headings: Range<usize>,
    images: Range<usize>,
    tables: Range<usize>,
    code_blocks: Range<usize>,
}
impl RichPlan {
    fn new(old: &RichDocument, c: &TextChange) -> Option<Self> {
        Some(Self {
            headings: window(&old.headings, c, |h| h.range.clone())?,
            images: window(&old.images, c, |i| i.range.clone())?,
            tables: window(&old.tables, c, |t| t.range.clone())?,
            code_blocks: window(&old.code_blocks, c, |b| b.range.clone())?,
        })
    }
    fn apply(
        self,
        old: &mut RichDocument,
        local: RichDocument,
        raw: &TextChange,
        display: &TextChange,
    ) {
        patch(
            &mut old.headings,
            local.headings,
            self.headings,
            display,
            |h, a, b| translate(&mut h.range, a, b),
        );
        patch(
            &mut old.images,
            local.images,
            self.images,
            display,
            |i, a, b| translate(&mut i.range, a, b),
        );
        let tail = self.code_blocks.start + local.code_blocks.len();
        old.code_blocks.splice(
            self.code_blocks,
            local.code_blocks.into_iter().map(|mut block| {
                translate(&mut block.range, 0, display.new.start);
                translate(&mut block.body, 0, display.new.start);
                translate(&mut block.source_range, 0, raw.new.start);
                translate(&mut block.source_body, 0, raw.new.start);
                block
            }),
        );
        for block in &mut old.code_blocks[tail..] {
            translate(&mut block.range, display.old.end, display.new.end);
            translate(&mut block.body, display.old.end, display.new.end);
            translate(&mut block.source_range, raw.old.end, raw.new.end);
            translate(&mut block.source_body, raw.old.end, raw.new.end);
        }
        // Table source cells and displayed cells use different coordinate systems.
        let tail = self.tables.start + local.tables.len();
        old.tables.splice(
            self.tables,
            local.tables.into_iter().map(|mut table| {
                translate(&mut table.range, 0, display.new.start);
                translate(&mut table.source_range, 0, raw.new.start);
                for cell in table.rows.iter_mut().flatten() {
                    translate(cell, 0, display.new.start);
                }
                for cell in table.source_rows.iter_mut().flatten() {
                    translate(cell, 0, raw.new.start);
                }
                table
            }),
        );
        for table in &mut old.tables[tail..] {
            if display.old.end != display.new.end {
                translate(&mut table.range, display.old.end, display.new.end);
                for cell in table.rows.iter_mut().flatten() {
                    translate(cell, display.old.end, display.new.end);
                }
            }
            if raw.old.end != raw.new.end {
                translate(&mut table.source_range, raw.old.end, raw.new.end);
                for cell in table.source_rows.iter_mut().flatten() {
                    translate(cell, raw.old.end, raw.new.end);
                }
            }
        }
    }
}

struct ParsedPlan {
    decorations: Range<usize>,
    marks: Range<usize>,
    pairs: Range<usize>,
    safe_text: Range<usize>,
    prefixes: Range<usize>,
    rich: RichPlan,
}
impl ParsedPlan {
    fn new(old: &Parsed, c: &TextChange) -> Option<Self> {
        Some(Self {
            decorations: window(&old.decorations, c, |d| d.range.clone())?,
            marks: window(&old.marks, c, |m| m.range.clone())?,
            pairs: window(&old.pairs, c, |p| p.open.start..p.close.end)?,
            safe_text: window(&old.safe_text, c, Clone::clone)?,
            prefixes: window(&old.block_prefixes, c, Clone::clone)?,
            rich: RichPlan::new(&old.rich, c)?,
        })
    }
    fn apply(self, old: &mut Parsed, local: Parsed, c: &TextChange) {
        patch(
            &mut old.decorations,
            local.decorations,
            self.decorations,
            c,
            |d, a, b| translate(&mut d.range, a, b),
        );
        patch(&mut old.marks, local.marks, self.marks, c, |m, a, b| {
            translate(&mut m.range, a, b)
        });
        patch(&mut old.pairs, local.pairs, self.pairs, c, |p, a, b| {
            translate(&mut p.open, a, b);
            translate(&mut p.body, a, b);
            translate(&mut p.close, a, b);
        });
        patch(
            &mut old.safe_text,
            local.safe_text,
            self.safe_text,
            c,
            translate,
        );
        patch(
            &mut old.block_prefixes,
            local.block_prefixes,
            self.prefixes,
            c,
            translate,
        );
        self.rich.apply(&mut old.rich, local.rich, c, c);
    }
}

fn patch_pieces(
    old: &[Piece],
    local: &[Piece],
    raw: &TextChange,
    display: &TextChange,
) -> Option<Vec<Piece>> {
    // Mapping snapshots are retained by old layout, so build one immutable
    // successor. Size it once instead of growing separate prefix/tail buffers.
    let prefix_end = old.partition_point(|piece| piece.source.start < raw.old.start);
    let suffix_start = old.partition_point(|piece| piece.source.end <= raw.old.end);
    let mut pieces = Vec::with_capacity(prefix_end + local.len() + old.len() - suffix_start);
    for piece in &old[..prefix_end] {
        let mut prefix = piece.clone();
        if prefix.source.end > raw.old.start {
            if !prefix.identity {
                return None;
            }
            prefix.display.end -= prefix.source.end - raw.old.start;
            prefix.source.end = raw.old.start;
        }
        pieces.push(prefix);
    }
    pieces.extend(local.iter().cloned().map(|mut piece| {
        translate(&mut piece.source, 0, raw.new.start);
        translate(&mut piece.display, 0, display.new.start);
        piece
    }));
    for piece in &old[suffix_start..] {
        let mut suffix = piece.clone();
        if suffix.source.start < raw.old.end {
            if !suffix.identity {
                return None;
            }
            suffix.display.start += raw.old.end - suffix.source.start;
            suffix.source.start = raw.old.end;
        }
        if raw.old.end != raw.new.end {
            translate(&mut suffix.source, raw.old.end, raw.new.end);
        }
        if display.old.end != display.new.end {
            translate(&mut suffix.display, display.old.end, display.new.end);
        }
        pieces.push(suffix);
    }
    Some(pieces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::document::projection::Affinity;

    fn assert_full(actual: &Projection, source: &str, caret: Option<usize>) {
        let full = Projection::with_context(source, caret);
        assert_eq!(actual.text, full.text, "{source:?} at {caret:?}");
        assert_eq!(actual.highlights, full.highlights, "{source:?}");
        assert_eq!(actual.rich, full.rich);
        assert_eq!(actual.disclosure, full.disclosure);
        assert_eq!(actual.line_starts, full.line_starts);
        assert_eq!(actual.parsed, full.parsed);
        for point in source.char_indices().map(|(n, _)| n).chain([source.len()]) {
            assert_eq!(
                actual.to_display(point),
                full.to_display(point),
                "source {point} in {source:?}"
            );
        }
        for point in full
            .text
            .char_indices()
            .map(|(n, _)| n)
            .chain([full.text.len()])
        {
            for affinity in [Affinity::Before, Affinity::After] {
                assert_eq!(
                    actual.to_source(point, affinity, None),
                    full.to_source(point, affinity, None),
                    "display {point} in {source:?}"
                );
            }
        }
    }

    fn payloads(rich: &RichDocument) -> Vec<usize> {
        rich.images
            .iter()
            .flat_map(|image| [image.url.as_ptr() as usize, image.alt.as_ptr() as usize])
            .chain(rich.tables.iter().flat_map(|table| {
                [
                    table.alignment.as_ptr() as usize,
                    table.rows.as_ptr() as usize,
                    table.source_rows.as_ptr() as usize,
                ]
                .into_iter()
                .chain(
                    table
                        .rows
                        .iter()
                        .chain(&table.source_rows)
                        .map(|row| row.as_ptr() as usize),
                )
            }))
            .collect()
    }

    #[test]
    fn local_updates_retain_owned_payloads_and_immutable_layout_snapshots() {
        let source = "# Before\n\nPlain **bold** and words.\n\n| A | B |\n|---|---|\n| c | d |\n\n![alt](somewhere.png)\n\nTail";
        let point = source.find("Plain").unwrap() + 3;
        let mut document = Document::new(source.into());
        let mut projection = Projection::with_context(source, Some(point));
        // Allow for local insertions without triggering normal capacity growth.
        projection.source.reserve(128);
        projection.text.reserve(128);
        let source_storage = projection.source.as_ptr();
        let display_storage = projection.text.as_ptr();
        let parsed_payloads = payloads(&projection.parsed.rich);
        let display_payloads = payloads(&projection.rich);
        let marks_storage = projection.parsed.marks.as_ptr();
        let old_mapping = Arc::clone(&projection.mapping);
        let old_lines = Arc::clone(&projection.line_starts);
        let old_highlights = Arc::clone(&projection.highlights);
        let old_source_id = Arc::clone(&projection.source_id);
        let old_offsets = (0..=source.len())
            .map(|offset| old_mapping.to_display(offset))
            .collect::<Vec<_>>();
        document.replace(point..point, "x").unwrap();
        assert!(projection.update_words(document.source(), Some(point + 1)));
        assert_full(&projection, document.source(), Some(point + 1));
        assert_eq!(source_storage, projection.source.as_ptr());
        assert_eq!(display_storage, projection.text.as_ptr());
        assert_eq!(marks_storage, projection.parsed.marks.as_ptr());
        assert_eq!(parsed_payloads, payloads(&projection.parsed.rich));
        assert_eq!(display_payloads, payloads(&projection.rich));
        assert!(!Arc::ptr_eq(&old_source_id, &projection.source_id));
        assert!(Arc::ptr_eq(
            &old_highlights,
            &projection.change.as_ref().unwrap().previous
        ));

        document.replace(point + 1..point + 1, "日本_").unwrap();
        let id = Arc::clone(&projection.source_id);
        assert!(!projection.update_words(document.source(), Some(point + 8)));
        assert!(Arc::ptr_eq(&id, &projection.source_id));
        assert!(projection.update_blocks(
            document.source(),
            document.block_changes.first().unwrap(),
            Some(point + 8)
        ));
        assert_full(&projection, document.source(), Some(point + 8));
        assert_eq!(source_storage, projection.source.as_ptr());
        assert_eq!(display_storage, projection.text.as_ptr());
        assert_eq!(parsed_payloads, payloads(&projection.parsed.rich));
        assert_eq!(display_payloads, payloads(&projection.rich));
        assert_eq!(
            old_offsets,
            (0..=source.len())
                .map(|offset| old_mapping.to_display(offset))
                .collect::<Vec<_>>()
        );
        let full_old = Projection::with_context(source, Some(point));
        assert_eq!(old_lines, full_old.line_starts);
        assert_eq!(old_highlights, full_old.highlights);
    }

    #[test]
    fn equal_length_word_replacements_share_unchanged_offsets() {
        let source = "Plain words here.\n\n| a | b |\n|---|---|\n| c | d |\n\n![alt](image.png)";
        let point = source.find("words").unwrap() + 1;
        let mut projection = Projection::with_context(source, Some(point));
        let mapping = Arc::clone(&projection.mapping);
        let lines = Arc::clone(&projection.line_starts);
        let previous = Arc::clone(&projection.highlights);
        let parsed = projection.parsed.clone();
        let mut next = source.to_owned();
        next.replace_range(point..point + 2, "ab");
        assert!(projection.update_words(&next, Some(point + 2)));
        assert!(Arc::ptr_eq(&mapping, &projection.mapping));
        assert!(Arc::ptr_eq(&lines, &projection.line_starts));
        assert!(!Arc::ptr_eq(&previous, &projection.highlights));
        assert_eq!(parsed, projection.parsed);
        assert_full(&projection, &next, Some(point + 2));
    }

    #[test]
    fn block_updates_match_full_projection_styles_and_every_mapping() {
        let mut accepted = 0;
        // Normalize the corpus before splitting, then exercise both line endings below.
        let corpus = include_str!("../../tests/fixtures/block-parsing.md").replace("\r\n", "\n");
        let fixtures = corpus.split("\n<!-- case -->\n").collect::<Vec<_>>();
        assert_eq!(fixtures.len(), 19, "keep every fixture as a separate case");
        for fixture in fixtures {
            for source in [fixture.to_owned(), fixture.replace('\n', "\r\n")] {
                for point in source
                    .char_indices()
                    .map(|(n, _)| n)
                    .chain([source.len()])
                    .step_by(3)
                {
                    for inserted in [
                        "_",
                        "**new**",
                        "日本語 e\u{301}",
                        "\n\n## New\n",
                        "\n|a|b|\n|---|---|\n",
                    ] {
                        let mut document = Document::new(source.clone());
                        let mut next = Projection::with_context(&source, Some(point));
                        document.replace(point..point, inserted).unwrap();
                        let Some(change) = document.block_changes.first() else {
                            continue;
                        };
                        let head = Some(point + inserted.len());
                        if !next.update_blocks(document.source(), change, head) {
                            continue;
                        };
                        accepted += 1;
                        assert_full(&next, document.source(), head);
                        for caret in [None, Some(0), head, Some(document.source().len())] {
                            next.set_disclosure(next.disclosure_at(caret));
                            assert_full(&next, document.source(), caret);
                        }
                    }
                }
            }
        }
        assert!(accepted > 100, "accepted {accepted}");
        println!("block projections: {accepted} accepted edits, compared at every UTF-8 boundary");
    }

    #[test]
    fn consecutive_deletions_and_topology_changes_keep_reusable_metadata_exact() {
        let mut document =
            Document::new("# Before\n\nPlain **bold** and café 🦀.\n\nTail\n".into());
        let mut projection = Projection::new(document.source());
        let mut accepted = 0;
        for replacement in [
            "Plain 日本語 e\u{301} words",
            "## New **title** ###",
            "one\n\ntwo",
            "| a | b |\n|---|---|\n| x | y |",
            "![new](image.png)",
            "Plain again",
            "```\nopen",
            "Plain **last**",
        ] {
            let start = document.source().find("\n\n").unwrap() + 2;
            let end = document.source().rfind("\n\nTail").unwrap();
            document.replace(start..end, replacement).unwrap();
            let head = Some(start + replacement.len());
            if document
                .block_changes
                .first()
                .is_some_and(|change| projection.update_blocks(document.source(), change, head))
            {
                accepted += 1;
            } else {
                projection = Projection::with_context(document.source(), head);
            }
            assert_full(&projection, document.source(), head);
            let point = document.source().find("last").unwrap_or(start);
            let mut next = document.source().to_owned();
            next.insert(point, 'x');
            if projection.update_words(&next, Some(point + 1)) {
                assert_full(&projection, &next, Some(point + 1));
            }
            projection = Projection::with_context(document.source(), head);
        }
        assert!(accepted >= 5, "accepted {accepted}");
    }

    #[test]
    fn stale_windows_and_distant_disclosure_take_the_general_path() {
        let source = "# First\n\nPlain words.\n\n# Last";
        let mut document = Document::new(source.into());
        let mut old = Projection::with_context(source, Some(4));
        let point = source.find("words").unwrap();
        document.replace(point..point, "日本").unwrap();
        let change = document.block_changes.first().unwrap();
        assert!(!old.update_blocks(document.source(), change, Some(point)));
        let mut stale = Projection::new(&format!("Other {source}"));
        assert!(!stale.update_blocks(document.source(), change, None));
    }

    #[test]
    fn sparse_batches_match_full_projection_at_every_disclosure_position() {
        use crate::document::collaboration::{EditOrigin, SourceEdit};
        let source = "# First **title**\r\n\r\nAlpha **bold** and café 🧑‍💻.\n\n| A | B |\n|---|---|\n| cat | dog |\n\n![alt](image.png)\n\nTail _words_.\n\n# Last\n";
        for head in source.char_indices().map(|(n, _)| n).chain([source.len()]) {
            let mut document = Document::new(source.into());
            let mut projection = Projection::with_context(source, Some(head));
            let old_map = Arc::clone(&projection.mapping);
            let old_lines = Arc::clone(&projection.line_starts);
            let old_highlights = Arc::clone(&projection.highlights);
            let target = document.collaboration.position(head, yrs::Assoc::After);
            let edits: Vec<_> = [
                ("First", "New 🦀"),
                ("Alpha", "Alpha日"),
                ("café", "tea"),
                ("cat", "日本語"),
                ("alt", "new"),
                ("words", "words é"),
            ]
            .into_iter()
            .map(|(old, new)| {
                let point = source.find(old).unwrap();
                SourceEdit {
                    range: point..point + old.len(),
                    text: new.into(),
                }
            })
            .collect();
            document
                .transact(&edits, EditOrigin::Agent, None, true)
                .unwrap();
            assert_eq!(document.block_changes.len(), 5);
            let next_head = document.collaboration.resolve(&target);
            assert!(
                projection.update_block_batch(
                    document.source(),
                    &document.block_changes,
                    next_head
                ),
                "head={head}"
            );
            assert_full(&projection, document.source(), next_head);
            assert_eq!(projection.change.as_ref().unwrap().regions.len(), 5);
            let original = Projection::with_context(source, Some(head));
            assert_eq!(old_lines, original.line_starts);
            assert_eq!(old_highlights, original.highlights);
            for (point, _) in source.char_indices() {
                assert_eq!(old_map.to_display(point), original.to_display(point));
            }
        }
    }

    #[test]
    fn rejected_batch_preflight_keeps_the_predecessor_intact() {
        use crate::document::collaboration::{EditOrigin, SourceEdit};
        let source = "# Before\n\nFirst paragraph.\n\nLast paragraph.\n\n# After\n";
        let mut document = Document::new(source.into());
        let edits = ["First", "Last"].map(|text| {
            let point = source.find(text).unwrap();
            SourceEdit {
                range: point..point + text.len(),
                text: "new".into(),
            }
        });
        document
            .transact(&edits, EditOrigin::Agent, None, true)
            .unwrap();
        for (next, caret) in [
            (format!("{}stale", document.source()), None),
            (
                document.source().into(),
                Some(document.source().find("After").unwrap()),
            ),
        ] {
            let mut projection = Projection::with_context(source, Some(3));
            assert!(!projection.update_block_batch(&next, &document.block_changes, caret));
            assert_full(&projection, source, Some(3));
        }
    }
}
