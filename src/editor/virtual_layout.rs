//! Bounded rich geometry over a document-wide, lightweight height index.
//!
//! Unvisited heights are estimates. They never determine a pointer hit, visual
//! motion, or selection rectangle: those operations use resident shaped rows.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use iced::{Point, Rectangle, Size};

use super::rich_layout::{LayoutInput, LayoutUnit, RichLayout, UnitKind, layout_units};
use crate::document::projection::{HighlightLines, OffsetMap};
use crate::theme::{Tokens, Typography, styles};

const TOP: f64 = 5.0;
const BOTTOM: f64 = 80.0;
// Small documents fit a modest fixed geometry budget and keep exact extents.
const EAGER_UNITS: usize = 256;

/// Fenwick sums use f64 to avoid accumulated rounding on million-pixel pages.
#[derive(Default)]
struct Heights {
    values: Vec<f64>,
    tree: Vec<f64>,
}
impl Heights {
    fn new(values: Vec<f64>) -> Self {
        let mut tree = vec![0.0; values.len() + 1];
        for (i, &value) in values.iter().enumerate() {
            tree[i + 1] += value;
            let parent = (i + 1) + ((i + 1) & (i + 1).wrapping_neg());
            if parent < tree.len() {
                tree[parent] += tree[i + 1];
            }
        }
        Self { values, tree }
    }
    fn prefix(&self, mut end: usize) -> f64 {
        let mut sum = 0.0;
        while end > 0 {
            sum += self.tree[end];
            end &= end - 1;
        }
        sum
    }
    fn set(&mut self, index: usize, value: f64) {
        let delta = value - self.values[index];
        self.values[index] = value;
        let mut i = index + 1;
        while i < self.tree.len() {
            self.tree[i] += delta;
            i += i & i.wrapping_neg();
        }
    }
    fn at_y(&self, y: f64) -> usize {
        let mut index = 0;
        let mut sum = 0.0;
        let mut bit = self.tree.len().next_power_of_two() / 2;
        while bit > 0 {
            let next = index + bit;
            if next < self.tree.len() && sum + self.tree[next] <= y {
                sum += self.tree[next];
                index = next;
            }
            bit /= 2;
        }
        index.min(self.values.len().saturating_sub(1))
    }
}

#[derive(PartialEq)]
struct Metrics {
    tokens: Tokens,
    typography: Typography,
    zoom: f32,
    width: f32,
    base: Option<PathBuf>,
    images: u64,
}

#[derive(Default)]
pub(super) struct VirtualLayout {
    geometry: RichLayout,
    units: Vec<LayoutUnit>,
    heights: Heights,
    // Only scalar height hints survive eviction. Hash matches are hints, never
    // permission to reuse geometry: resident paragraphs use exact cache keys.
    known_heights: HashMap<u64, f64>,
    keys: Vec<u64>,
    resident: Range<usize>,
    mapping: Option<Arc<OffsetMap>>,
    highlights: Option<Arc<HighlightLines>>,
    metrics: Option<Metrics>,
    eager: bool,
}

impl VirtualLayout {
    /// Only this window has exact geometry; the height includes offscreen hints.
    pub fn resident(&self) -> &RichLayout {
        &self.geometry
    }
    pub fn height(&self) -> f32 {
        self.geometry.height
    }

    pub fn rebuild(&mut self, input: LayoutInput<'_>) {
        let metrics = Metrics {
            tokens: input.theme.tokens,
            typography: input.theme.typography,
            zoom: input.zoom,
            width: input.width,
            base: input.base.map(|p| p.to_path_buf()),
            images: input.images.revision(),
        };
        let same_metrics = self.metrics.as_ref() == Some(&metrics);
        if same_metrics && !self.eager && self.patch_index(input) {
            return;
        }
        if !same_metrics {
            self.known_heights.clear();
        }
        self.metrics = Some(metrics);
        self.mapping = Some(Arc::clone(&input.projection.mapping));
        self.highlights = Some(Arc::clone(&input.projection.highlights));
        self.units = layout_units(input.projection);
        self.eager = self.units.len() <= EAGER_UNITS;
        self.resident = 0..0;
        if self.eager {
            self.heights = Heights::default();
            self.keys = Vec::new();
            self.known_heights.clear();
            self.geometry.rebuild(
                input.projection,
                input.theme,
                input.zoom,
                input.width,
                input.base,
                input.images,
            );
            return;
        }
        self.keys.clear();
        let values = self
            .units
            .iter()
            .map(|unit| {
                let key = height_key(unit, input);
                self.keys.push(key);
                self.known_heights
                    .get(&key)
                    .copied()
                    .unwrap_or_else(|| estimate(unit, input) as f64)
            })
            .collect();
        self.heights = Heights::new(values);
        // Bound height metadata as well, including after replacing a document.
        if self.known_heights.len() > self.units.len().saturating_mul(2) {
            self.known_heights.clear();
        }
        self.geometry.height = self.total_height();
    }

    /// A proven predecessor and changes contained in existing physical units
    /// let us rebase offsets without rehashing the entire document. Multiline
    /// and unrelated revisions rebuild the lightweight index instead.
    fn patch_index(&mut self, input: LayoutInput<'_>) -> bool {
        let Some(change) = &input.projection.change else {
            return false;
        };
        if change.regions.is_empty()
            || self.highlights.as_ref().is_none_or(|old| {
                !Arc::ptr_eq(old, &change.previous)
                    || old.len() != input.projection.highlights.len()
            })
        {
            return false;
        }
        let mut affected: Vec<(usize, isize)> = Vec::new();
        let mut delta = 0_isize;
        for region in &change.regions {
            let i = self
                .units
                .partition_point(|unit| unit.range.end < region.old.start);
            let Some(unit) = self.units.get(i) else {
                return false;
            };
            if region.old.start < unit.range.start
                || region.old.end > unit.range.end
                || input.projection.text[region.new.clone()].contains('\n')
                || region.old.start.checked_add_signed(delta) != Some(region.new.start)
            {
                return false;
            }
            let difference = region.new.len() as isize - region.old.len() as isize;
            if let Some(last) = affected.last_mut()
                && last.0 == i
            {
                last.1 += difference;
            } else {
                affected.push((i, difference));
            }
            delta += difference;
        }
        // Validate the resulting physical boundaries before mutating anything.
        delta = 0;
        for &(i, difference) in &affected {
            let unit = &self.units[i];
            let Some(start) = unit.range.start.checked_add_signed(delta) else {
                return false;
            };
            let Some(end) = unit.range.end.checked_add_signed(delta + difference) else {
                return false;
            };
            if start > end || end > input.projection.text.len() {
                return false;
            }
            if let UnitKind::Line(_) = unit.kind
                && (start > 0 && input.projection.text.as_bytes()[start - 1] != b'\n'
                    || input
                        .projection
                        .text
                        .as_bytes()
                        .get(end)
                        .is_some_and(|&c| c != b'\n'))
            {
                return false;
            }
            delta += difference;
        }
        let mut start = 0;
        delta = 0;
        for (i, difference) in affected {
            if delta != 0 {
                for unit in &mut self.units[start..i] {
                    shift_range(&mut unit.range, delta);
                }
            }
            self.units[i].range.start = self.units[i]
                .range
                .start
                .checked_add_signed(delta)
                .expect("validated unit start");
            self.units[i].range.end = self.units[i]
                .range
                .end
                .checked_add_signed(delta + difference)
                .expect("validated unit end");
            self.keys[i] = height_key(&self.units[i], input);
            delta += difference;
            start = i + 1;
        }
        if delta != 0 {
            for unit in &mut self.units[start..] {
                shift_range(&mut unit.range, delta);
            }
        }
        self.mapping = Some(Arc::clone(&input.projection.mapping));
        self.highlights = Some(Arc::clone(&input.projection.highlights));
        self.resident = 0..0;
        self.geometry.height = self.total_height();
        true
    }

    fn total_height(&self) -> f32 {
        (TOP + self.heights.prefix(self.units.len()) + BOTTOM) as f32
    }
    fn top(&self, unit: usize) -> f32 {
        (TOP + self.heights.prefix(unit)) as f32
    }
    fn unit_at(&self, source: usize) -> usize {
        let offset = self
            .mapping
            .as_ref()
            .map_or(source, |map| map.to_display(source));
        self.units
            .partition_point(|unit| unit.range.end < offset)
            .min(self.units.len().saturating_sub(1))
    }
    fn units_in(&self, y: Range<f32>) -> Range<usize> {
        self.heights
            .at_y((y.start as f64 - TOP).max(0.0))
            .saturating_sub(1)
            ..(self.heights.at_y((y.end as f64 - TOP).max(0.0)) + 2).min(self.units.len())
    }
    fn source_start(&self, unit: usize) -> usize {
        self.mapping
            .as_ref()
            .map_or(self.units[unit].range.start, |map| {
                map.to_source(
                    self.units[unit].range.start,
                    crate::document::projection::Affinity::After,
                    None,
                )
            })
    }
    pub fn source_caret(&self, source: usize, y: Option<f32>) -> Rectangle {
        let unit = self.unit_at(source);
        if self.eager || self.resident.contains(&unit) {
            self.geometry.source_caret(source, y)
        } else {
            let height = self.metrics.as_ref().map_or(24.0, |m| {
                m.typography.body_size * m.zoom * m.typography.line_height
            });
            Rectangle::new(Point::new(0.0, self.top(unit)), Size::new(1.5, height))
        }
    }
    pub fn contains_source(&self, source: usize) -> bool {
        self.eager || self.resident.contains(&self.unit_at(source))
    }
    pub fn source_caret_on_side(&self, source: usize, upstream: bool) -> Rectangle {
        if self.eager || self.resident.contains(&self.unit_at(source)) {
            self.geometry.source_caret_on_side(source, upstream)
        } else {
            self.source_caret(source, None)
        }
    }

    /// Measure a distant navigation target before deriving its exact caret.
    /// The returned scroll places that source near the middle of the viewport.
    pub fn show_source(&mut self, input: LayoutInput<'_>, source: usize, height: f32) -> f32 {
        let unit = self.unit_at(source);
        let top = if self.eager {
            self.geometry.source_caret(source, None).y
        } else {
            self.top(unit)
        };
        self.show_viewport(input, (top - height * 0.4).max(0.0), height)
    }

    /// Fill the viewport plus one viewport of overscan on either side, evicting
    /// old paragraphs. Correct scroll using an exact resident row when possible,
    /// otherwise the estimated target unit's start, before refining its height.
    pub fn show_viewport(&mut self, input: LayoutInput<'_>, mut scroll: f32, height: f32) -> f32 {
        if self.eager {
            return scroll;
        }
        let padding = height.max(400.0);
        let mut wanted = self.units_in(scroll - padding..scroll + height + padding);
        if self.resident.start <= wanted.start && wanted.end <= self.resident.end {
            return scroll;
        }
        let at = self.heights.at_y((scroll as f64 - TOP).max(0.0));
        let (source, y) = if self.resident.contains(&at) {
            self.geometry
                .viewport_anchor(scroll)
                .unwrap_or((self.source_start(at), self.top(at)))
        } else {
            (self.source_start(at), self.top(at))
        };
        let screen_y = y - scroll;
        loop {
            let heights = self.geometry.rebuild_units(
                input,
                &self.units[wanted.clone()],
                self.top(wanted.start),
            );
            for (i, height) in wanted.clone().zip(heights) {
                self.heights.set(i, height as f64);
                self.known_heights.insert(self.keys[i], height as f64);
            }
            if self.known_heights.len() > self.units.len().saturating_mul(2) {
                self.known_heights.clear();
            }
            self.resident = wanted.clone();
            self.geometry.height = self.total_height();
            scroll = (self.source_caret(source, None).y - screen_y).max(0.0);
            let cover = self.units_in(scroll - padding..scroll + height + padding);
            if wanted.start <= cover.start && cover.end <= wanted.end {
                break;
            }
            wanted = wanted.start.min(cover.start)..wanted.end.max(cover.end);
        }
        scroll
    }
}

fn shift_range(range: &mut Range<usize>, delta: isize) {
    range.start = range
        .start
        .checked_add_signed(delta)
        .expect("valid projected offset");
    range.end = range
        .end
        .checked_add_signed(delta)
        .expect("valid projected offset");
}

fn height_key(unit: &LayoutUnit, input: LayoutInput<'_>) -> u64 {
    let mut hash = DefaultHasher::new();
    input.projection.text[unit.range.clone()].hash(&mut hash);
    match unit.kind {
        UnitKind::Line(level) => level.hash(&mut hash),
        UnitKind::TableRow { table, row } => {
            let table = &input.projection.rich.tables[table];
            (row == 0).hash(&mut hash);
            for alignment in &table.alignment {
                let value = match alignment {
                    pulldown_cmark::Alignment::None => 0_u8,
                    pulldown_cmark::Alignment::Left => 1,
                    pulldown_cmark::Alignment::Center => 2,
                    pulldown_cmark::Alignment::Right => 3,
                };
                value.hash(&mut hash);
            }
        }
    }
    let line = input
        .projection
        .line_starts
        .partition_point(|&start| start <= unit.range.start)
        .saturating_sub(1);
    for (range, style) in input.projection.highlights[line].iter() {
        range.hash(&mut hash);
        style.0.hash(&mut hash);
    }
    hash.finish()
}

fn estimate(unit: &LayoutUnit, input: LayoutInput<'_>) -> f32 {
    let body = input.theme.typography.body_size * input.zoom;
    let line_height = input.theme.typography.line_height;
    let flow = |range: Range<usize>, size: f32, width: f32, bold: bool| {
        let characters = input.projection.text[range.clone()].chars().count();
        let rows = (characters as f32 * size * 0.5 / width.max(1.0))
            .ceil()
            .max(1.0);
        let images = &input.projection.rich.images;
        let first = images.partition_point(|image| image.range.end <= range.start);
        let count = images[first..]
            .iter()
            .take_while(|image| image.range.end <= range.end)
            .count();
        rows * size * if bold { 1.3 } else { line_height } + count as f32 * 128.0 * input.zoom
    };
    match unit.kind {
        UnitKind::Line(level) => flow(
            unit.range.clone(),
            level.map_or(body, |level| styles::heading_size(body, level)),
            input.width,
            level.is_some(),
        ),
        UnitKind::TableRow { table, row } => {
            let table = &input.projection.rich.tables[table];
            let width = input.width / table.alignment.len().max(1) as f32;
            let padding = (body * 0.65).min(width / 4.0);
            table.rows[row]
                .iter()
                .map(|cell| {
                    flow(cell.clone(), body, width - padding * 2.0, row == 0) + padding * 2.0
                })
                .fold(body * line_height + padding * 2.0, f32::max)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::projection::Projection;
    use crate::document::selection::Selection;
    use crate::io::image_store::ImageStore;
    use crate::theme::Preset;

    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 0.15, "{a} != {b}");
    }
    fn rectangle(a: Rectangle, b: Rectangle) {
        close(a.x, b.x);
        close(a.y, b.y);
        close(a.width, b.width);
        close(a.height, b.height);
    }

    #[test]
    fn height_sums_and_inverse_queries_follow_updates() {
        let values = (0..1501)
            .map(|n| 7.25 + (n % 23) as f64 * 0.3)
            .collect::<Vec<_>>();
        let mut heights = Heights::new(values);
        for step in 0..200 {
            let index = step * 37 % heights.values.len();
            heights.set(index, 12.3 + step as f64);
            let mut sum = 0.0;
            for (i, value) in heights.values.iter().enumerate() {
                assert!((heights.prefix(i) - sum).abs() < 1e-7);
                assert_eq!(heights.at_y(sum + value * 0.5), i);
                sum += value;
            }
            assert!((heights.prefix(heights.values.len()) - sum).abs() < 1e-7);
        }
    }

    #[test]
    fn resident_geometry_and_selection_match_full_layout_after_distant_evictions() {
        let source = (0..150).map(|i| format!(
            "## Heading {i}\n\nParagraph {i}: **bold words** and *café 🦀*, a [link](long/path), and enough words to wrap across a few rows.\n\n| Left | Right |\n|:---|---:|\n| word {i} | words which wrap over several lines |\n\n![image](missing.png)\n\n"
        )).collect::<String>();
        let projection = Projection::new(&source);
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let input = LayoutInput {
            projection: &projection,
            theme: &theme,
            images: &images,
            zoom: 1.0,
            width: 250.0,
            base: None,
        };
        let mut full = RichLayout::default();
        full.rebuild(&projection, &theme, 1.0, 250.0, None, &images);
        let mut window = VirtualLayout::default();
        window.rebuild(input);
        for chapter in [0, 80, 149, 2, 110, 40, 0] {
            let head = source.find(&format!("Paragraph {chapter}:")).unwrap() + 3;
            let scroll = window.show_source(input, head, 500.0);
            let dy = window.source_caret(head, None).y - full.source_caret(head, None).y;
            assert!(window.geometry.fragments.len() < 100);
            assert!(window.geometry.rows.len() < 160);
            assert!(window.contains_source(head));
            for fragment in &window.geometry.fragments {
                let raw = projection.to_source(
                    fragment.range.start,
                    crate::document::projection::Affinity::After,
                    None,
                );
                let actual = window.source_caret(raw, None);
                let mut expected = full.source_caret(raw, None);
                expected.y += dy;
                rectangle(actual, expected);
            }
            for selection in [
                Selection {
                    anchor: 0,
                    head: source.len(),
                },
                Selection {
                    anchor: source.len(),
                    head: 0,
                },
            ] {
                let actual = window
                    .geometry
                    .source_selection_rects_in(selection, scroll..scroll + 500.0);
                let expected =
                    full.source_selection_rects_in(selection, scroll - dy..scroll + 500.0 - dy);
                assert_eq!(actual.len(), expected.len());
                for (actual, mut expected) in actual.into_iter().zip(expected) {
                    expected.y += dy;
                    rectangle(actual, expected);
                }
            }
            let caret = window.source_caret(head, None);
            assert_eq!(
                window
                    .geometry
                    .hit_source_with_row(Point::new(caret.x + 0.1, caret.center_y()))
                    .0,
                head
            );
            for direction in [-1.0, 1.0] {
                let target = window
                    .geometry
                    .vertical_source(caret, caret.x, direction, Some(425.0))
                    .map(|(source, _)| source);
                let expected = full
                    .vertical_source(
                        full.source_caret(head, None),
                        caret.x,
                        direction,
                        Some(425.0),
                    )
                    .map(|(source, _)| source);
                assert_eq!(target, expected);
            }
        }
    }

    #[test]
    fn index_patches_match_fresh_units_and_geometry_through_edits_and_disclosure() {
        let mut source = (0..400).map(|i| format!(
            "## Heading {i}\n\nParagraph {i} with **bold words**.\n\n| A | B |\n|---|---|\n| Cell {i} words | other words |\n\n"
        )).collect::<String>();
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let mut projection = Projection::new(&source);
        let mut window = VirtualLayout::default();
        let block_edit =
            |source: &mut String, projection: &mut Projection, head: &mut usize, inserted: &str| {
                let mut document = crate::document::Document::new(source.clone());
                document.replace(*head..*head, inserted);
                *source = document.source().to_owned();
                *head += inserted.len();
                assert!(projection.update_blocks(
                    source,
                    document.block_change.as_ref().unwrap(),
                    Some(*head)
                ));
            };
        for step in 0..56 {
            let mut head = source.find("Paragraph 152").unwrap() + "Paragraph 152 w".len();
            if matches!(step % 7, 0 | 2 | 5 | 6) {
                projection.set_disclosure(projection.disclosure_at(None));
                let input = LayoutInput {
                    projection: &projection,
                    theme: &theme,
                    images: &images,
                    zoom: 1.0,
                    width: 240.0,
                    base: None,
                };
                window.rebuild(input);
                window.show_source(input, head, 500.0);
            }
            match step % 7 {
                0 => {
                    let mut document = crate::document::Document::new(source.clone());
                    document.replace(head..head, "x");
                    source.insert(head, 'x');
                    head += 1;
                    assert!(
                        projection.update_words(&source, Some(head))
                            || projection.update_blocks(
                                &source,
                                document.block_change.as_ref().unwrap(),
                                Some(head)
                            )
                    );
                }
                1 => projection.set_disclosure(
                    projection.disclosure_at(Some(source.find("bold words").unwrap() + 3)),
                ),
                2 => {
                    head = source.find("Cell 311").unwrap() + "Cell 311 w".len();
                    block_edit(&mut source, &mut projection, &mut head, "**z**")
                }
                3 => projection.set_disclosure(projection.disclosure_at(Some(4))),
                4 => {
                    source.insert_str(0, "New line\n\n");
                    head += "New line\n\n".len();
                    projection = Projection::with_context(&source, Some(head));
                }
                5 => block_edit(&mut source, &mut projection, &mut head, "日本語 e\u{301}"),
                _ => block_edit(
                    &mut source,
                    &mut projection,
                    &mut head,
                    "\n\nNew paragraph\n\n",
                ),
            };
            if step == 0 {
                // Skip an intermediate projection, then replace text without
                // changing offsets. Shared maps must not authorize stale geometry.
                source.replace_range(head - 1..head, "z");
                assert!(projection.update_words(&source, Some(head)));
            }
            let input = LayoutInput {
                projection: &projection,
                theme: &theme,
                images: &images,
                zoom: 1.0,
                width: 240.0,
                base: None,
            };
            window.rebuild(input);
            assert_eq!(window.units, layout_units(&projection));
            window.show_source(input, head, 500.0);
            let mut fresh = RichLayout::default();
            fresh.rebuild_units(
                input,
                &window.units[window.resident.clone()],
                window.top(window.resident.start),
            );
            assert_eq!(window.geometry.rows.len(), fresh.rows.len());
            for fragment in &window.geometry.fragments {
                let raw = projection.to_source(
                    fragment.range.start,
                    crate::document::projection::Affinity::After,
                    None,
                );
                rectangle(
                    window.source_caret(raw, None),
                    fresh.source_caret(raw, None),
                );
            }
            assert!(window.known_heights.len() <= window.units.len() * 2);
        }
    }

    #[test]
    fn large_table_rows_and_fifty_thousand_blocks_keep_geometry_bounded() {
        let source = "Plain words here.\n\n".repeat(50_000);
        let projection = Projection::new(&source);
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let input = LayoutInput {
            projection: &projection,
            theme: &theme,
            images: &images,
            zoom: 1.0,
            width: 600.0,
            base: None,
        };
        let mut window = VirtualLayout::default();
        window.rebuild(input);
        for head in [0, source.len() / 2, source.len(), source.len() / 4, 0] {
            window.show_source(input, head, 800.0);
            assert!(window.geometry.fragments.len() < 110);
            assert!(window.geometry.rows.len() < 110);
        }
        let source = format!(
            "| A | B |\n|---|---|\n{}",
            (0..2000)
                .map(|i| format!("| Row {i} | Some long words wrapping inside the second cell |\n"))
                .collect::<String>()
        );
        let projection = Projection::new(&source);
        let input = LayoutInput {
            projection: &projection,
            width: 200.0,
            ..input
        };
        window.rebuild(input);
        let head = source.find("Row 1999").unwrap() + 4;
        window.show_source(input, head, 500.0);
        assert!(window.geometry.fragments.len() < 50);
        assert!(window.geometry.decorations.len() < 75);
        let caret = window.source_caret(head, None);
        assert_eq!(
            window
                .geometry
                .hit_source_with_row(Point::new(caret.x + 0.1, caret.center_y()))
                .0,
            head
        );
        let row_start = window.source_caret(source.find("Row 1999").unwrap(), None);
        let previous = window
            .geometry
            .vertical_source(row_start, row_start.x, -1.0, None)
            .unwrap()
            .0;
        let prior_cell = source.find("Row 1998").unwrap();
        assert!(
            (prior_cell..=prior_cell + "Row 1998".len()).contains(&previous),
            "previous={previous}, cell={prior_cell}, caret={row_start:?}"
        );
    }
}
