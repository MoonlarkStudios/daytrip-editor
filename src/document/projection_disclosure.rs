//! Disclosure shares unchanged line styles instead of repainting the document.

use std::ops::Range;
use std::sync::Arc;

use super::{
    Concealment, Disclosure, OffsetMap, Projection, ProjectionChange, Style, TextChange, project,
};

type Runs = Arc<[(Range<usize>, Style)]>;

impl Projection {
    pub(super) fn set_disclosure_lines(&mut self, disclosure: &Disclosure) -> bool {
        let mut lines = Vec::new();
        for range in self.disclosure.0.iter().chain(&disclosure.0) {
            // Setext underlines and multiline link syntax change the line count.
            if self.source[range.clone()].contains('\n') {
                return false;
            }
            let start = self.source[..range.start].rfind('\n').map_or(0, |n| n + 1);
            let end = self.source[range.end..]
                .find('\n')
                .map_or(self.source.len(), |n| range.end + n);
            lines.push(start..end);
        }
        lines.sort_by_key(|r| r.start);
        lines.dedup();
        let visible = project(&self.source, &self.parsed, false, |decoration| {
            decoration.kind == Concealment::Structural
                || !disclosure
                    .0
                    .iter()
                    .any(|r| decoration.range.start < r.end && r.start < decoration.range.end)
        });
        let starts: Arc<[usize]> = std::iter::once(0)
            .chain(visible.text.match_indices('\n').map(|(n, _)| n + 1))
            .collect();
        if starts.len() != self.line_starts.len() {
            return false;
        }
        let mut highlights = (*self.highlights).clone();
        let mut regions = Vec::new();
        for raw in lines {
            let old = self.to_display(raw.start)..self.to_display(raw.end);
            let new = visible.mapping.to_display(raw.start)..visible.mapping.to_display(raw.end);
            let line = starts
                .partition_point(|&n| n <= new.start)
                .saturating_sub(1);
            // A table row may be collapsed entirely; only actual displayed lines
            // can be repainted independently.
            if starts[line] != new.start {
                return false;
            }
            let end = starts.get(line + 1).map_or(visible.text.len(), |n| n - 1);
            highlights[line] =
                self.disclosure_runs(&raw, new.start..end, &visible.mapping, disclosure);
            if self.text[old.clone()] != visible.text[new.clone()]
                || highlights[line] != self.highlights[line]
            {
                regions.push(TextChange { old, new });
            }
        }
        self.change = Some(ProjectionChange {
            previous: std::mem::replace(&mut self.highlights, Arc::new(highlights)),
            regions,
        });
        self.text = visible.text;
        self.mapping = visible.mapping;
        self.rich = visible.rich;
        self.line_starts = starts;
        self.disclosure = disclosure.clone();
        true
    }

    fn disclosure_runs(
        &self,
        raw: &Range<usize>,
        display: Range<usize>,
        map: &OffsetMap,
        disclosure: &Disclosure,
    ) -> Runs {
        let mut events = Vec::new();
        let mut add = |range: &Range<usize>, style: u8| {
            if range.start >= raw.end || range.end <= raw.start {
                return;
            }
            let start = map.to_display(range.start).max(display.start);
            let end = map.to_display(range.end).min(display.end);
            if start < end {
                let kind = style.trailing_zeros() as usize;
                events.extend([
                    (start - display.start, kind, 1_i32),
                    (end - display.start, kind, -1_i32),
                ]);
            }
        };
        for mark in &self.parsed.marks {
            add(&mark.range, mark.style);
        }
        for range in &disclosure.0 {
            add(range, Style::SYNTAX);
        }
        events.sort_by_key(|event| event.0);
        let mut counters = [0_i32; u8::BITS as usize];
        let mut last = 0;
        let mut runs = Vec::new();
        for (position, kind, delta) in events {
            let flags = counters
                .iter()
                .enumerate()
                .fold(0, |flags, (i, &n)| flags | if n > 0 { 1 << i } else { 0 });
            if position > last && flags != 0 {
                runs.push((last..position, Style(flags)));
            }
            counters[kind] += delta;
            last = position;
        }
        runs.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_disclosure_matches_full_styles_and_structure_at_every_caret() {
        for source in [
            "Before `inline`\n\n````rust\nfn main() {\n\n    let café = \"🦀\";\n}\n````\n\nAfter",
            "### A **bold *nested* title** ###\n\nA **span** and [a **label**](https://example.com) and `code`.\n\nOther *words*.",
            "| **Heading** | [Label](url) |\n|:---|---:|\n| *words* | `code` |\n\n![**alt**](image.png)",
            "Setext **title**\r\n================\r\n\r\nA **strong\r\nmultiline** span with `code\r\nwords`.",
            "## 日本語 🦀\n\n[label][ref] and <https://example.com>\n\n[ref]: https://example.com",
        ] {
            let mut actual = Projection::new(source);
            for caret in source
                .char_indices()
                .map(|(n, _)| Some(n))
                .chain([Some(source.len()), None])
            {
                actual.set_disclosure(actual.disclosure_at(caret));
                let full = Projection::with_context(source, caret);
                assert_eq!(actual.text, full.text, "at {caret:?} in {source:?}");
                assert_eq!(
                    actual.highlights, full.highlights,
                    "at {caret:?} in {source:?}"
                );
                assert_eq!(actual.rich, full.rich);
                assert_eq!(actual.line_starts, full.line_starts);
            }
        }
    }
}
