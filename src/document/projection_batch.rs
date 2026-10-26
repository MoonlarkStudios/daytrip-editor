//! Atomic preflight and sparse projection updates for certified block batches.

use std::ops::Range;
use std::sync::Arc;

use super::{ParsedPlan, RichPlan, translate};
use crate::document::block_parser::{BlockChange, MAX_INCREMENTAL_WINDOWS};
use crate::document::projection::{
    Disclosure, OffsetMap, Piece, Projection, ProjectionChange, TextChange,
};

struct Window {
    local: Projection,
    raw: TextChange,
    shown: TextChange,
    region: TextChange,
    parsed: ParsedPlan,
    rich: RichPlan,
    lines: Range<usize>,
    count: usize,
}

impl Projection {
    pub(crate) fn update_block_batch(
        &mut self,
        source: &str,
        changes: &[BlockChange],
        caret: Option<usize>,
    ) -> bool {
        if changes.len() == 1 {
            return self.update_blocks(source, &changes[0], caret);
        }
        self.block_batch(source, changes, caret).is_some()
    }

    fn block_batch(
        &mut self,
        source: &str,
        changes: &[BlockChange],
        caret: Option<usize>,
    ) -> Option<()> {
        if changes.is_empty()
            || changes.len() > MAX_INCREMENTAL_WINDOWS
            || self.parsed.global_dependencies
        {
            return None;
        }
        let mut windows = Vec::with_capacity(changes.len());
        let (mut old_end, mut new_end, mut display_shift) = (0, 0, 0isize);
        for change in changes {
            let (old, new) = (&change.old, &change.new);
            // Check every unchanged gap once, including skipped/stale revisions.
            if old.start < old_end
                || new.start < new_end
                || self.source.get(old_end..old.start)? != source.get(new_end..new.start)?
            {
                return None;
            }
            let local = Self::with_context(
                source.get(new.clone())?,
                caret
                    .filter(|&head| new.contains(&head))
                    .map(|head| head - new.start),
            );
            if local.parsed.global_dependencies {
                return None;
            }
            self.source.get(old.clone())?;
            let display = self.to_display(old.start)..self.to_display(old.end);
            let final_start = display.start.checked_add_signed(display_shift)?;
            let raw = TextChange {
                old: old.clone(),
                new: old.start..old.start + new.len(),
            };
            let shown = TextChange {
                old: display.clone(),
                new: display.start..display.start + local.text.len(),
            };
            let parsed = ParsedPlan::new(&self.parsed, &raw)?;
            let rich = RichPlan::new(&self.rich, &shown)?;
            let first = self.line_starts.binary_search(&display.start).ok()?;
            let last = if display.end == self.text.len() && !self.text.ends_with('\n') {
                self.highlights.len()
            } else {
                self.line_starts.binary_search(&display.end).ok()?
            };
            let tail = last < self.highlights.len();
            if tail && !local.text.ends_with('\n') {
                return None;
            }
            let count = local.highlights.len().checked_sub(usize::from(tail))?;
            let mut region = TextChange {
                old: display.clone(),
                new: final_start..final_start + local.text.len(),
            };
            if local.rich.headings.is_empty()
                && local.rich.code_blocks.is_empty()
                && local.rich.tables.is_empty()
                && local.rich.images.is_empty()
                && parsed.rich.headings.is_empty()
                && parsed.rich.code_blocks.is_empty()
                && parsed.rich.tables.is_empty()
                && parsed.rich.images.is_empty()
                && self.text[display.clone()].ends_with('\n')
                && local.text.ends_with('\n')
            {
                region.old.end -= 1;
                region.new.end -= 1;
            }
            display_shift += shown.new.len() as isize - shown.old.len() as isize;
            windows.push(Window {
                local,
                raw,
                shown,
                region,
                parsed,
                rich,
                lines: first..last,
                count,
            });
            old_end = old.end;
            new_end = new.end;
        }
        if self.source.get(old_end..)? != source.get(new_end..)? {
            return None;
        }

        // Disclosure may change inside the certified windows. A change elsewhere
        // needs the general path, so never silently retain an old syntax context.
        let map_old = |offset: usize| {
            let i = changes.partition_point(|change| change.new.start <= offset);
            i.checked_sub(1).map_or(offset, |i| {
                TextChange {
                    old: changes[i].old.clone(),
                    new: changes[i].new.clone(),
                }
                .previous_offset(offset)
            })
        };
        let map_new = |offset: usize| {
            let i = changes.partition_point(|change| change.old.end <= offset);
            i.checked_sub(1)
                .map_or(offset, |i| offset - changes[i].old.end + changes[i].new.end)
        };
        let disclosure = if let Some((i, change)) = changes
            .iter()
            .enumerate()
            .find(|(_, change)| caret.is_some_and(|head| change.new.contains(&head)))
        {
            Disclosure(
                windows[i]
                    .local
                    .disclosure
                    .0
                    .iter()
                    .map(|r| r.start + change.new.start..r.end + change.new.start)
                    .collect(),
            )
        } else {
            let mut disclosure = self.disclosure_at(caret.map(map_old));
            for range in &mut disclosure.0 {
                *range = map_new(range.start)..map_new(range.end);
            }
            disclosure
        };
        let outside = |ranges: &[Range<usize>], old: bool| {
            ranges
                .iter()
                .filter(|r| {
                    changes.iter().all(|c| {
                        let window = if old { &c.old } else { &c.new };
                        r.end <= window.start || r.start >= window.end
                    })
                })
                .map(|r| {
                    if old {
                        map_new(r.start)..map_new(r.end)
                    } else {
                        r.clone()
                    }
                })
                .collect::<Vec<_>>()
        };
        if outside(&self.disclosure.0, true) != outside(&disclosure.0, false) {
            return None;
        }

        // Keep predecessor map/line/highlight snapshots immutable. All fallible
        // checks finish before touching the retained source, text or metadata.
        let mut pieces = Vec::with_capacity(
            self.mapping.pieces.len()
                + windows
                    .iter()
                    .map(|w| w.local.mapping.pieces.len() + 2)
                    .sum::<usize>(),
        );
        let (mut raw_from, mut raw_to, mut shown_from, mut shown_to) = (0, 0, 0, 0);
        for (window, change) in windows.iter().zip(changes) {
            append_gap(
                &mut pieces,
                &self.mapping.pieces,
                raw_from..change.old.start,
                raw_to,
                shown_from,
                shown_to,
            )?;
            pieces.extend(
                window
                    .local
                    .mapping
                    .pieces
                    .iter()
                    .cloned()
                    .map(|mut piece| {
                        translate(&mut piece.source, 0, change.new.start);
                        translate(&mut piece.display, 0, window.region.new.start);
                        piece
                    }),
            );
            raw_from = change.old.end;
            raw_to = change.new.end;
            shown_from = window.shown.old.end;
            shown_to = window.region.new.start + window.local.text.len();
        }
        append_gap(
            &mut pieces,
            &self.mapping.pieces,
            raw_from..self.source.len(),
            raw_to,
            shown_from,
            shown_to,
        )?;
        let mut highlights = (*self.highlights).clone();
        let mut starts = self.line_starts.to_vec();
        for window in windows.iter().rev() {
            let first = window.lines.start;
            highlights.splice(
                window.lines.clone(),
                window.local.highlights[..window.count].iter().cloned(),
            );
            starts.splice(
                window.lines.clone(),
                window.local.line_starts[..window.count]
                    .iter()
                    .map(|start| start + window.shown.old.start),
            );
            for start in &mut starts[first + window.count..] {
                *start = *start - window.shown.old.len() + window.shown.new.len();
            }
        }
        if starts.len() != highlights.len() {
            return None;
        }
        let regions = windows.iter().map(|w| w.region.clone()).collect();
        for window in windows.into_iter().rev() {
            window
                .parsed
                .apply(&mut self.parsed, window.local.parsed, &window.raw);
            window.rich.apply(
                &mut self.rich,
                window.local.rich,
                &window.raw,
                &window.shown,
            );
            self.text
                .replace_range(window.shown.old, &window.local.text);
            self.source
                .replace_range(window.raw.old, &window.local.source);
        }
        self.mapping = Arc::new(OffsetMap {
            pieces,
            source_length: source.len(),
            display_length: self.text.len(),
        });
        self.line_starts = starts.into();
        self.source_id = Arc::new(());
        self.disclosure = disclosure;
        self.change = Some(ProjectionChange {
            previous: std::mem::replace(&mut self.highlights, Arc::new(highlights)),
            regions,
        });
        Some(())
    }
}

/// Copy each unchanged mapping segment once into the immutable successor.
/// Only identity pieces may cross an independently parsed window boundary.
fn append_gap(
    target: &mut Vec<Piece>,
    old: &[Piece],
    gap: Range<usize>,
    source_start: usize,
    display_from: usize,
    display_to: usize,
) -> Option<()> {
    if gap.is_empty() {
        return Some(());
    }
    let first = old.partition_point(|piece| piece.source.end <= gap.start);
    let end = old.partition_point(|piece| piece.source.start < gap.end);
    for piece in &old[first..end] {
        let mut piece = piece.clone();
        if piece.source.start < gap.start {
            if !piece.identity {
                return None;
            }
            piece.display.start += gap.start - piece.source.start;
            piece.source.start = gap.start;
        }
        if piece.source.end > gap.end {
            if !piece.identity {
                return None;
            }
            piece.display.end -= piece.source.end - gap.end;
            piece.source.end = gap.end;
        }
        translate(&mut piece.source, gap.start, source_start);
        translate(&mut piece.display, display_from, display_to);
        target.push(piece);
    }
    Some(())
}
