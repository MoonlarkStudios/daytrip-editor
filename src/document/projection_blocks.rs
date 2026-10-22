//! Reparse an isolated block window and retain projection metadata elsewhere.

use std::ops::Range;
use std::sync::Arc;

use super::{OffsetMap, Parsed, Piece, Projection, ProjectionChange, TextChange};
use crate::document::block_parser::BlockChange;
use crate::document::rich_document::RichDocument;

impl Projection {
    /// The source model certifies independent boundaries. Validate its snapshot
    /// before reusing metadata; references and footnotes always use a full parse.
    pub(crate) fn updating_blocks(
        &self,
        source: &str,
        change: &BlockChange,
        caret: Option<usize>,
    ) -> Option<Self> {
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
        let parsed = Arc::new(patch_parsed(&self.parsed, &local.parsed, &raw)?);
        let disclosure = Self::resolve_disclosure(source, &parsed, caret);
        // Updating another block must not leave previously revealed syntax
        // visible. If disclosure also moves outside the island, use the general
        // path, which already handles distant and multiline contexts.
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
        let a = self.line_starts.binary_search(&display.start).ok()?;
        let b = if display.end == self.text.len() && !self.text.ends_with('\n') {
            self.highlights.len()
        } else {
            self.line_starts.binary_search(&display.end).ok()?
        };
        let mut highlights = self.highlights[..a].to_vec();
        let shared_tail = b < self.highlights.len();
        if shared_tail && !local.text.ends_with('\n') {
            return None;
        }
        let count = local.highlights.len() - usize::from(shared_tail);
        highlights.extend_from_slice(&local.highlights[..count]);
        highlights.extend_from_slice(&self.highlights[b..]);
        let mut text = self.text.clone();
        text.replace_range(display.clone(), &local.text);
        let mapping = Arc::new(OffsetMap {
            pieces: patch_pieces(
                &self.mapping.pieces,
                &local.mapping.pieces,
                &raw,
                &TextChange {
                    old: display.clone(),
                    new: next_display.clone(),
                },
            )?,
            source_length: source.len(),
            display_length: text.len(),
        });
        let line_starts: Arc<[usize]> = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(n, _)| n + 1))
            .collect();
        if line_starts.len() != highlights.len() {
            return None;
        }
        let mut rich = parsed.rich.clone();
        rich.map_ranges(|offset| mapping.to_display(offset));
        let mut region = TextChange {
            old: display,
            new: next_display,
        };
        let overlaps = |range: &Range<usize>| range.start < old.end && old.start < range.end;
        // An unchanged terminating newline is not part of an inline relayout.
        // Keep structural windows broad: heading levels, table rows and image
        // flow can change the layout unit's kind, requiring index discovery.
        let plain = local.rich.headings.is_empty()
            && local.rich.tables.is_empty()
            && local.rich.images.is_empty()
            && !self.parsed.rich.headings.iter().any(|h| overlaps(&h.range))
            && !self.parsed.rich.tables.iter().any(|t| overlaps(&t.range))
            && !self.parsed.rich.images.iter().any(|i| overlaps(&i.range));
        if plain && self.text[region.old.clone()].ends_with('\n') && local.text.ends_with('\n') {
            region.old.end -= 1;
            region.new.end -= 1;
        }
        Some(Self {
            text,
            mapping,
            rich,
            parsed,
            disclosure,
            line_starts,
            source: Arc::from(source),
            highlights: Arc::new(highlights),
            change: Some(ProjectionChange {
                previous: Arc::clone(&self.highlights),
                regions: vec![region],
            }),
        })
    }
}

fn translate(range: &mut Range<usize>, from: usize, to: usize) {
    *range = range.start - from + to..range.end - from + to;
}

/// No retained node may cross the certified window. Metadata within it is
/// replaced; suffix offsets move, and unchanged values retain their semantics.
fn patch<T: Clone>(
    old: &[T],
    local: &[T],
    change: &TextChange,
    extent: impl Fn(&T) -> Range<usize>,
    move_item: impl Fn(&mut T, usize, usize),
) -> Option<Vec<T>> {
    let mut before = Vec::new();
    let mut after = Vec::new();
    for item in old {
        let range = extent(item);
        // Empty table-prefix/tail decorations belong to this isolated block,
        // including at EOF. Retaining them would duplicate local metadata.
        if range.is_empty() && change.old.start <= range.start && range.end <= change.old.end {
            continue;
        }
        if range.end <= change.old.start {
            before.push(item.clone());
        } else if range.start >= change.old.end {
            let mut item = item.clone();
            move_item(&mut item, change.old.end, change.new.end);
            after.push(item);
        } else if range.start < change.old.start || range.end > change.old.end {
            return None;
        }
    }
    before.extend(local.iter().cloned().map(|mut item| {
        move_item(&mut item, 0, change.new.start);
        item
    }));
    before.extend(after);
    Some(before)
}

fn patch_parsed(old: &Parsed, local: &Parsed, c: &TextChange) -> Option<Parsed> {
    let range = |r: &Range<usize>| r.clone();
    let mut rich = RichDocument::default();
    rich.headings = patch(
        &old.rich.headings,
        &local.rich.headings,
        c,
        |h| h.range.clone(),
        |h, a, b| translate(&mut h.range, a, b),
    )?;
    rich.images = patch(
        &old.rich.images,
        &local.rich.images,
        c,
        |i| i.range.clone(),
        |i, a, b| translate(&mut i.range, a, b),
    )?;
    rich.tables = patch(
        &old.rich.tables,
        &local.rich.tables,
        c,
        |t| t.range.clone(),
        |t, a, b| {
            translate(&mut t.range, a, b);
            translate(&mut t.source_range, a, b);
            for cell in t.rows.iter_mut().chain(&mut t.source_rows).flatten() {
                translate(cell, a, b);
            }
        },
    )?;
    Some(Parsed {
        decorations: patch(
            &old.decorations,
            &local.decorations,
            c,
            |d| d.range.clone(),
            |d, a, b| translate(&mut d.range, a, b),
        )?,
        marks: patch(
            &old.marks,
            &local.marks,
            c,
            |m| m.range.clone(),
            |m, a, b| translate(&mut m.range, a, b),
        )?,
        pairs: patch(
            &old.pairs,
            &local.pairs,
            c,
            |p| p.open.start..p.close.end,
            |p, a, b| {
                translate(&mut p.open, a, b);
                translate(&mut p.body, a, b);
                translate(&mut p.close, a, b);
            },
        )?,
        safe_text: patch(&old.safe_text, &local.safe_text, c, range, translate)?,
        block_prefixes: patch(
            &old.block_prefixes,
            &local.block_prefixes,
            c,
            range,
            translate,
        )?,
        rich,
        global_dependencies: false,
    })
}

fn patch_pieces(
    old: &[Piece],
    local: &[Piece],
    raw: &TextChange,
    display: &TextChange,
) -> Option<Vec<Piece>> {
    let mut before = Vec::new();
    let mut after = Vec::new();
    for piece in old {
        if piece.source.start < raw.old.start {
            let mut prefix = piece.clone();
            if prefix.source.end > raw.old.start {
                if !prefix.identity {
                    return None;
                }
                prefix.display.end -= prefix.source.end - raw.old.start;
                prefix.source.end = raw.old.start;
            }
            before.push(prefix);
        }
        if piece.source.end > raw.old.end {
            let mut suffix = piece.clone();
            if suffix.source.start < raw.old.end {
                if !suffix.identity {
                    return None;
                }
                suffix.display.start += raw.old.end - suffix.source.start;
                suffix.source.start = raw.old.end;
            }
            translate(&mut suffix.source, raw.old.end, raw.new.end);
            translate(&mut suffix.display, display.old.end, display.new.end);
            after.push(suffix);
        }
    }
    before.extend(local.iter().cloned().map(|mut piece| {
        translate(&mut piece.source, 0, raw.new.start);
        translate(&mut piece.display, 0, display.new.start);
        piece
    }));
    before.extend(after);
    Some(before)
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

    #[test]
    fn block_updates_match_full_projection_styles_and_every_mapping() {
        let mut accepted = 0;
        for fixture in
            include_str!("../../tests/fixtures/block-parsing.md").split("\n<!-- case -->\n")
        {
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
                        let old = Projection::with_context(&source, Some(point));
                        document.replace(point..point, inserted);
                        let Some(change) = &document.block_change else {
                            continue;
                        };
                        let head = Some(point + inserted.len());
                        let Some(next) = old.updating_blocks(document.source(), change, head)
                        else {
                            continue;
                        };
                        accepted += 1;
                        assert_full(&next, document.source(), head);
                        for caret in [None, Some(0), head, Some(document.source().len())] {
                            let disclosed = next.disclosing(next.disclosure_at(caret));
                            assert_full(&disclosed, document.source(), caret);
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
            document.replace(start..end, replacement);
            let head = Some(start + replacement.len());
            projection = document
                .block_change
                .as_ref()
                .and_then(|change| projection.updating_blocks(document.source(), change, head))
                .inspect(|_| {
                    accepted += 1;
                })
                .unwrap_or_else(|| Projection::with_context(document.source(), head));
            assert_full(&projection, document.source(), head);
            let point = document.source().find("last").unwrap_or(start);
            let mut next = document.source().to_owned();
            next.insert(point, 'x');
            if let Some(word) = projection.updating(&next, Some(point + 1)) {
                assert_full(&word, &next, Some(point + 1));
            }
        }
        assert!(accepted >= 5, "accepted {accepted}");
    }

    #[test]
    fn stale_windows_and_distant_disclosure_take_the_general_path() {
        let source = "# First\n\nPlain words.\n\n# Last";
        let mut document = Document::new(source.into());
        let old = Projection::with_context(source, Some(4));
        let point = source.find("words").unwrap();
        document.replace(point..point, "日本");
        let change = document.block_change.as_ref().unwrap();
        assert!(
            old.updating_blocks(document.source(), change, Some(point))
                .is_none()
        );
        let stale = Projection::new(&format!("Other {source}"));
        assert!(
            stale
                .updating_blocks(document.source(), change, None)
                .is_none()
        );
    }
}
