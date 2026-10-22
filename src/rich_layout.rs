//! Shared geometry for rich text drawing, hit testing, and document navigation.

use std::collections::HashMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use iced::advanced::graphics::text::Paragraph;
use iced::advanced::image;
use iced::advanced::text::{self, Paragraph as _};
use iced::{Font, Point, Rectangle, Size};
use unicode_segmentation::UnicodeSegmentation;

use crate::image_store::{Asset, ImageStore};
use crate::projection::{Affinity, HighlightLines, OffsetMap, Projection, Style, TextChange};
use crate::selection::Selection;
use crate::styles;
use crate::theme::{ResolvedTheme, Tokens, Typography};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ParagraphKey {
    text: String,
    marks: Vec<(Range<usize>, Style)>,
    width: u32,
    size: u32,
    line_height: u32,
    alignment: u8,
    bold: bool,
}

struct CachedParagraph {
    paragraph: Paragraph,
    rows: Vec<CachedRow>,
}

struct CachedRow {
    range: Range<usize>,
    stops: Arc<[(usize, f32)]>,
    top: f32,
    height: f32,
}

/// Vertical interval index. Source order is not Y order inside wrapping tables.
#[derive(Default)]
struct VerticalIndex {
    entries: Vec<(f32, f32, usize)>,
}
impl VerticalIndex {
    fn rebuild(&mut self, bounds: impl Iterator<Item = Rectangle>) {
        self.entries.clear();
        self.entries
            .extend(bounds.enumerate().map(|(i, r)| (r.y, r.y + r.height, i)));
        self.entries.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut bottom = f32::NEG_INFINITY;
        for (_, end, _) in &mut self.entries {
            bottom = bottom.max(*end);
            *end = bottom;
        }
    }
    fn visible(&self, y: Range<f32>) -> impl Iterator<Item = usize> + '_ {
        let start = self
            .entries
            .partition_point(|(_, bottom, _)| *bottom < y.start);
        let end = self.entries.partition_point(|(top, _, _)| *top <= y.end);
        self.entries[start.min(end)..end].iter().map(|(_, _, i)| *i)
    }
}

#[derive(PartialEq)]
struct LayoutMetrics {
    tokens: Tokens,
    typography: Typography,
    zoom: f32,
    width: f32,
    base: Option<PathBuf>,
    images: u64,
}

#[derive(Clone, Copy)]
pub struct ParagraphStyle {
    size: f32,
    bold: bool,
    alignment: u8,
}

#[derive(Default)]
pub struct RichLayout {
    pub fragments: Vec<Fragment>,
    pub decorations: Vec<(Rectangle, Decoration)>,
    pub rows: Vec<VisualRow>,
    pub height: f32,
    cache: HashMap<ParagraphKey, Arc<CachedParagraph>>,
    tokens: Option<Tokens>,
    fragment_index: VerticalIndex,
    row_index: VerticalIndex,
    decoration_index: VerticalIndex,
    mapping: Option<Arc<OffsetMap>>,
    highlights: Option<Arc<HighlightLines>>,
    metrics: Option<LayoutMetrics>,
    patches: usize,
}

pub struct Fragment {
    pub range: Range<usize>,
    pub bounds: Rectangle,
    pub kind: FragmentKind,
}

pub enum FragmentKind {
    Text {
        paragraph: Paragraph,
        origin: Point,
        display_start: usize,
        style: ParagraphStyle,
    },
    Image {
        handle: Option<image::Handle>,
        label: String,
    },
}

#[derive(Clone, Copy)]
pub enum Decoration {
    Header,
    Border,
}

pub struct VisualRow {
    pub bounds: Rectangle,
    display_range: Range<usize>,
    display_stops: Arc<[(usize, f32)]>,
    display_start: usize,
    origin_x: f32,
}

impl VisualRow {
    fn display_hit(&self, x: f32) -> usize {
        self.display_stops
            .iter()
            .min_by(|(_, a), (_, b)| {
                (a + self.origin_x - x)
                    .abs()
                    .total_cmp(&(b + self.origin_x - x).abs())
            })
            .map_or(self.display_range.start, |(offset, _)| {
                self.display_start + offset
            })
    }
    fn display_caret(&self, offset: usize) -> Rectangle {
        let x = self
            .display_stops
            .iter()
            .min_by_key(|(index, _)| (self.display_start + index).abs_diff(offset))
            .map_or(self.bounds.x, |(_, x)| self.origin_x + x);
        Rectangle::new(
            Point::new(x, self.bounds.y),
            Size::new(1.5, self.bounds.height),
        )
    }
}

impl RichLayout {
    pub fn rebuild(
        &mut self,
        projection: &Projection,
        theme: &ResolvedTheme,
        zoom: f32,
        width: f32,
        base: Option<&Path>,
        images: &ImageStore,
    ) {
        let metrics = LayoutMetrics {
            tokens: theme.tokens,
            typography: theme.typography,
            zoom,
            width,
            base: base.map(Path::to_path_buf),
            images: images.revision(),
        };
        if self.metrics.as_ref() == Some(&metrics) && self.patch(projection, theme, images) {
            return;
        }
        self.metrics = Some(metrics);
        self.highlights = Some(Arc::clone(&projection.highlights));
        self.patches = 0;
        let mut previous = std::mem::take(&mut self.cache);
        if self.tokens != Some(theme.tokens) {
            previous.clear();
        }
        self.tokens = Some(theme.tokens);
        self.fragments.clear();
        self.decorations.clear();
        self.rows.clear();
        self.mapping = Some(Arc::clone(&projection.mapping));
        let mut builder = Builder {
            layout: self,
            previous,
            projection,
            tokens: theme.tokens,
            size: theme.typography.body_size * zoom,
            line_height: theme.typography.line_height,
            width: width.max(1.0),
            y: 5.0,
            line_starts: Arc::clone(&projection.line_starts),
            base,
            images,
            zoom,
        };
        let mut start = 0;
        let mut table_index = 0;
        // Physical text lines are independent shaped paragraphs. Tables replace
        // their complete range; all fragments still use global buffer offsets.
        while start <= projection.text.len() {
            if let Some(table) = projection.rich.tables.get(table_index)
                && table.range.start >= start
                && !projection.text[start..table.range.start].contains('\n')
            {
                if table.range.start > start {
                    builder.line(start..table.range.start, builder.size, false);
                }
                builder.table(table);
                table_index += 1;
                if table.range.end > start {
                    start = table.range.end;
                    continue;
                }
            }
            let end = projection.text[start..]
                .find('\n')
                .map_or(projection.text.len(), |n| start + n);
            let heading = projection
                .rich
                .headings
                .get(
                    projection
                        .rich
                        .headings
                        .partition_point(|heading| heading.range.end <= start),
                )
                .filter(|heading| heading.range.start <= start && start < heading.range.end)
                .map(|heading| heading.level);
            let size = heading.map_or(builder.size, |level| {
                styles::heading_size(builder.size, level)
            });
            builder.line(start..end, size, heading.is_some());
            if end == projection.text.len() {
                break;
            }
            start = end + 1;
        }
        builder.layout.height = builder.y + 80.0;
        self.reindex();
    }

    fn reindex(&mut self) {
        self.fragment_index
            .rebuild(self.fragments.iter().map(|f| f.bounds));
        self.row_index.rebuild(self.rows.iter().map(|r| r.bounds));
        self.decoration_index
            .rebuild(self.decorations.iter().map(|(r, _)| *r));
    }

    /// Replace one ordinary paragraph and rebase exact geometry below it. Table,
    /// image, multiline, metric and unrelated revision changes use full layout.
    fn patch(
        &mut self,
        projection: &Projection,
        theme: &ResolvedTheme,
        images: &ImageStore,
    ) -> bool {
        let Some(change) = &projection.change else {
            return false;
        };
        if self
            .highlights
            .as_ref()
            .is_none_or(|old| !Arc::ptr_eq(old, &change.previous))
        {
            return false;
        }
        let mut delta = 0_isize;
        for region in &change.regions {
            let old = region
                .old
                .start
                .checked_add_signed(delta)
                .expect("valid old region")
                ..region
                    .old
                    .end
                    .checked_add_signed(delta)
                    .expect("valid old region");
            let edit = TextChange {
                old,
                new: region.new.clone(),
            };
            if !self.patch_region(projection, theme, images, &edit) {
                return false;
            }
            delta += region.new.len() as isize - region.old.len() as isize;
        }
        self.mapping = Some(Arc::clone(&projection.mapping));
        self.highlights = Some(Arc::clone(&projection.highlights));
        self.patches += 1;
        self.reindex();
        true
    }

    fn patch_region(
        &mut self,
        projection: &Projection,
        theme: &ResolvedTheme,
        images: &ImageStore,
        edit: &TextChange,
    ) -> bool {
        let i = self
            .fragments
            .partition_point(|f| f.range.end < edit.old.start);
        let Some(fragment) = self.fragments.get(i) else {
            return false;
        };
        let FragmentKind::Text { style, .. } = &fragment.kind else {
            return false;
        };
        if fragment.range.start > edit.old.start || edit.old.end > fragment.range.end {
            return false;
        }
        let delta = edit.new.len() as isize - edit.old.len() as isize;
        let shifted = |offset: usize| {
            offset
                .checked_add_signed(delta)
                .expect("valid projected splice")
        };
        let range = fragment.range.start..shifted(fragment.range.end);
        if range.end > projection.text.len()
            || projection.text[range.clone()].contains(['\n', '\u{fffc}'])
        {
            return false;
        }
        let table_index = projection
            .rich
            .tables
            .partition_point(|table| table.range.end <= range.start);
        if projection
            .rich
            .tables
            .get(table_index)
            .is_some_and(|table| table.range.start < range.end)
        {
            return false;
        }
        // The fragment must still be an entire physical paragraph. This also
        // excludes edits which remove a newline just outside its old range.
        if range.start > 0 && projection.text.as_bytes()[range.start - 1] != b'\n'
            || projection
                .text
                .as_bytes()
                .get(range.end)
                .is_some_and(|&c| c != b'\n')
        {
            return false;
        }
        if fragment.range.is_empty() || range.is_empty() {
            return false;
        }
        let heading = projection
            .rich
            .headings
            .iter()
            .find(|h| h.range.start <= range.start && range.start < h.range.end);
        let metrics = self.metrics.as_ref().expect("patch has layout metrics");
        let body_size = theme.typography.body_size * metrics.zoom;
        let expected_size = heading.map_or(body_size, |h| styles::heading_size(body_size, h.level));
        if style.bold != heading.is_some() || style.size != expected_size {
            return false;
        }
        let old_range = fragment.range.clone();
        let bounds = fragment.bounds;
        let style = *style;
        let metrics = self.metrics.as_ref().expect("patch has layout metrics");
        let mut part = RichLayout::default();
        let mut builder = Builder {
            layout: &mut part,
            previous: HashMap::new(),
            projection,
            tokens: theme.tokens,
            size: theme.typography.body_size * metrics.zoom,
            line_height: theme.typography.line_height,
            width: bounds.width,
            y: bounds.y,
            line_starts: Arc::clone(&projection.line_starts),
            base: metrics.base.as_deref(),
            images,
            zoom: metrics.zoom,
        };
        let height = builder.paragraph(
            range,
            bounds.position(),
            bounds.width,
            style.size,
            style.bold,
            style.alignment,
        );
        let dy = height - bounds.height;
        let first = self
            .rows
            .partition_point(|r| r.display_range.start < old_range.start);
        let end = self
            .rows
            .partition_point(|r| r.display_range.end <= old_range.end);
        let new_end = first + part.rows.len();
        self.rows.splice(first..end, part.rows);
        for row in &mut self.rows[new_end..] {
            row.display_range = shifted(row.display_range.start)..shifted(row.display_range.end);
            row.display_start = shifted(row.display_start);
            row.bounds.y += dy;
        }
        for fragment in &mut self.fragments[i + 1..] {
            fragment.range = shifted(fragment.range.start)..shifted(fragment.range.end);
            fragment.bounds.y += dy;
            if let FragmentKind::Text {
                origin,
                display_start,
                ..
            } = &mut fragment.kind
            {
                origin.y += dy;
                *display_start = shifted(*display_start);
            }
        }
        for (rect, _) in &mut self.decorations {
            if rect.y >= bounds.y + bounds.height - 0.01 {
                rect.y += dy;
            }
        }
        self.fragments[i] = part
            .fragments
            .pop()
            .expect("paragraph creates one fragment");
        // Rows and fragments already own the new paragraph's shared geometry.
        // Keep the last full-layout cache bounded: retaining every intermediate
        // shape would grow memory with each keystroke in a long paragraph.
        self.height += dy;
        true
    }

    #[cfg(test)]
    pub fn caret(&self, offset: usize) -> Rectangle {
        self.caret_at(offset, None)
    }

    #[cfg(test)]
    pub fn caret_at(&self, offset: usize, y: Option<f32>) -> Rectangle {
        let source = self.mapping.as_ref().map_or(offset, |mapping| {
            mapping.to_source(offset, Affinity::After, None)
        });
        self.source_caret(source, y)
    }

    fn rows_at(&self, offset: usize) -> &[VisualRow] {
        let first = self
            .rows
            .partition_point(|row| row.display_range.end < offset);
        let count = self.rows[first..].partition_point(|row| row.display_range.start <= offset);
        &self.rows[first..first + count]
    }

    pub fn source_caret(&self, source: usize, y: Option<f32>) -> Rectangle {
        let offset = self
            .mapping
            .as_ref()
            .map_or(source, |map| map.to_display(source));
        let rows = self.rows_at(offset);
        let row = y
            .and_then(|y| {
                rows.iter()
                    .min_by(|a, b| (a.bounds.y - y).abs().total_cmp(&(b.bounds.y - y).abs()))
            })
            .or_else(|| {
                rows.iter()
                    .find(|r| r.display_range.contains(&offset) || r.display_range.is_empty())
            })
            .or_else(|| rows.last())
            .or_else(|| {
                self.rows.get(
                    self.rows
                        .partition_point(|row| row.display_range.end <= offset)
                        .saturating_sub(1),
                )
            });
        row.map_or(Rectangle::new(Point::ORIGIN, Size::new(1.5, 24.0)), |row| {
            row.display_caret(offset)
        })
    }

    pub fn visible_fragments(&self, y: Range<f32>) -> impl Iterator<Item = &Fragment> {
        self.fragment_index.visible(y).map(|i| &self.fragments[i])
    }

    pub fn visible_decorations(
        &self,
        y: Range<f32>,
    ) -> impl Iterator<Item = &(Rectangle, Decoration)> {
        self.decoration_index
            .visible(y)
            .map(|i| &self.decorations[i])
    }

    /// Resolves wrap affinity independently of the previous row's vertical position.
    pub fn source_caret_on_side(&self, source: usize, upstream: bool) -> Rectangle {
        let offset = self
            .mapping
            .as_ref()
            .map_or(source, |map| map.to_display(source));
        let mut rows = self.rows_at(offset).iter();
        let row = if upstream {
            rows.next()
        } else {
            rows.next_back()
        };
        row.map_or_else(
            || self.source_caret(source, None),
            |row| row.display_caret(offset),
        )
    }

    pub fn upstream_affinity(&self, source: usize, y: f32) -> bool {
        y < self.source_caret_on_side(source, false).y - 0.5
    }

    /// Source position and document Y of the first visible row, for scroll anchoring.
    pub fn viewport_anchor(&self, scroll: f32) -> Option<(usize, f32)> {
        self.rows
            .iter()
            .find(|row| row.bounds.y + row.bounds.height > scroll)
            .map(|row| (self.source_offset(row.display_range.start), row.bounds.y))
    }

    fn hit_display(&self, point: Point) -> usize {
        let Some(fragment) = self
            .fragments
            .iter()
            .min_by(|a, b| distance(a.bounds, point).total_cmp(&distance(b.bounds, point)))
        else {
            return 0;
        };
        match &fragment.kind {
            FragmentKind::Text {
                paragraph,
                origin,
                display_start,
                ..
            } => paragraph
                .hit_test(point - (*origin - Point::ORIGIN))
                .map_or(*display_start, |hit| display_start + hit.cursor()),
            FragmentKind::Image { .. } => {
                if point.x < fragment.bounds.center_x() {
                    fragment.range.start
                } else {
                    fragment.range.end
                }
            }
        }
    }

    #[cfg(test)]
    pub fn hit(&self, point: Point) -> usize {
        self.hit_display(point)
    }

    fn source_offset(&self, offset: usize) -> usize {
        self.mapping.as_ref().map_or(offset, |mapping| {
            mapping.to_source(offset, Affinity::After, None)
        })
    }

    pub fn hit_source_with_row(&self, point: Point) -> (usize, f32) {
        let offset = self.hit_display(point);
        let y = self
            .rows_at(offset)
            .iter()
            .min_by(|a, b| {
                let distance = |row: &VisualRow| {
                    (row.bounds.y - point.y)
                        .max(point.y - row.bounds.y - row.bounds.height)
                        .max(0.0)
                };
                distance(a).total_cmp(&distance(b))
            })
            .map_or(0.0, |row| row.bounds.y);
        (self.source_offset(offset), y)
    }

    fn vertical_row(
        &self,
        caret: Rectangle,
        x: f32,
        direction: f32,
        page: Option<f32>,
    ) -> Option<&VisualRow> {
        let eligible = |row: &&VisualRow| {
            if direction < 0.0 {
                row.bounds.y < caret.y - 0.5
            } else {
                row.bounds.y > caret.y + 0.5
            }
        };
        let target = caret.y + direction * page.unwrap_or(1.0);
        let nearest = |a: &&VisualRow, b: &&VisualRow| {
            (a.bounds.y - target)
                .abs()
                .total_cmp(&(b.bounds.y - target).abs())
        };
        self.rows
            .iter()
            .filter(eligible)
            .filter(|row| row.bounds.x <= x && x <= row.bounds.x + row.bounds.width + 1.5)
            .min_by(nearest)
            .or_else(|| self.rows.iter().filter(eligible).min_by(nearest))
    }

    #[cfg(test)]
    pub fn vertical(
        &self,
        caret: Rectangle,
        x: f32,
        direction: f32,
        page: Option<f32>,
    ) -> Option<(usize, f32)> {
        self.vertical_row(caret, x, direction, page)
            .map(|row| (row.display_hit(x), row.bounds.y))
    }

    pub fn vertical_source(
        &self,
        caret: Rectangle,
        x: f32,
        direction: f32,
        page: Option<f32>,
    ) -> Option<(usize, f32)> {
        self.vertical_row(caret, x, direction, page)
            .map(|row| (self.source_offset(row.display_hit(x)), row.bounds.y))
    }

    pub fn source_line_edge(&self, source: usize, caret: Rectangle, end: bool) -> usize {
        let offset = self
            .mapping
            .as_ref()
            .map_or(source, |mapping| mapping.to_display(source));
        self.rows_at(offset)
            .iter()
            .find(|row| (row.bounds.y - caret.y).abs() < 0.5)
            .map_or(source, |row| {
                self.source_offset(if end {
                    row.display_range.end
                } else {
                    row.display_range.start
                })
            })
    }

    #[cfg(test)]
    pub fn line_edge(&self, offset: usize, caret: Rectangle, end: bool) -> usize {
        let Some(mapping) = &self.mapping else {
            return offset;
        };
        let source = mapping.to_source(offset, Affinity::After, None);
        mapping.to_display(self.source_line_edge(source, caret, end))
    }

    #[cfg(test)]
    pub fn selection_rects(&self, selection: Selection) -> Vec<Rectangle> {
        let Some(mapping) = &self.mapping else {
            return Vec::new();
        };
        let range = selection.range();
        self.source_selection_rects(Selection {
            anchor: mapping.to_source(range.start, Affinity::Before, None),
            head: mapping.to_source(range.end, Affinity::After, None),
        })
    }

    #[cfg(test)]
    pub fn source_selection_rects(&self, selection: Selection) -> Vec<Rectangle> {
        self.source_selection_rects_in(selection, f32::NEG_INFINITY..f32::INFINITY)
    }

    pub fn source_selection_rects_in(
        &self,
        selection: Selection,
        visible: Range<f32>,
    ) -> Vec<Rectangle> {
        let map = |offset| {
            self.mapping
                .as_ref()
                .map_or(offset, |mapping| mapping.to_display(offset))
        };
        let selected = selection.range();
        let range = map(selected.start)..map(selected.end);
        if range.is_empty() {
            return Vec::new();
        }
        let mut result = Vec::new();
        for index in self.row_index.visible(visible.clone()) {
            let row = &self.rows[index];
            if row.bounds.y + row.bounds.height < visible.start || row.bounds.y > visible.end {
                continue;
            }
            let start = row.display_range.start.max(range.start);
            let end = row.display_range.end.min(range.end);
            if start <= end
                && range.start <= row.display_range.end
                && range.end > row.display_range.start
            {
                let left = row.display_caret(start).x;
                let right = row.display_caret(end).x;
                result.push(Rectangle::new(
                    Point::new(left.min(right), row.bounds.y),
                    Size::new((right - left).abs().max(3.0), row.bounds.height),
                ));
            }
        }
        result
    }
}

fn distance(bounds: Rectangle, point: Point) -> f32 {
    let dx = (bounds.x - point.x)
        .max(point.x - bounds.x - bounds.width)
        .max(0.0);
    let dy = (bounds.y - point.y)
        .max(point.y - bounds.y - bounds.height)
        .max(0.0);
    // Prefer the nearest visual row before resolving the column within a table.
    dy * dy * 100.0 + dx * dx
}

struct Builder<'a> {
    layout: &'a mut RichLayout,
    previous: HashMap<ParagraphKey, Arc<CachedParagraph>>,
    projection: &'a Projection,
    tokens: Tokens,
    size: f32,
    line_height: f32,
    width: f32,
    y: f32,
    line_starts: Arc<[usize]>,
    base: Option<&'a Path>,
    images: &'a ImageStore,
    zoom: f32,
}

impl Builder<'_> {
    fn line(&mut self, range: Range<usize>, size: f32, bold: bool) {
        let height = self.flow(range, Point::new(0.0, self.y), self.width, size, bold, 0);
        self.y += height;
    }

    fn flow(
        &mut self,
        range: Range<usize>,
        position: Point,
        width: f32,
        size: f32,
        bold: bool,
        alignment: u8,
    ) -> f32 {
        let mut y = position.y;
        let mut part = range.start;
        let first = self
            .projection
            .rich
            .images
            .partition_point(|image| image.range.end <= range.start);
        for image in self.projection.rich.images[first..]
            .iter()
            .take_while(|image| image.range.end <= range.end)
        {
            if part < image.range.start {
                y += self.paragraph(
                    part..image.range.start,
                    Point::new(position.x, y),
                    width,
                    size,
                    bold,
                    alignment,
                );
            }
            let asset = self.images.get(&image.url, self.base);
            let (handle, dimensions, status) = match asset {
                Some(Asset::Ready(image)) => (Some(image.handle.clone()), Some(image.size), None),
                Some(Asset::Failed(error)) => (None, None, Some(error.as_str())),
                _ => (None, None, Some("Loading image…")),
            };
            let image_width = dimensions.map_or(width, |s| (s.width as f32).min(width));
            let height = dimensions
                .map_or(112.0 * self.zoom, |s| {
                    image_width * s.height as f32 / s.width.max(1) as f32
                })
                .min(520.0 * self.zoom);
            let image_width = dimensions.map_or(image_width, |s| {
                (height * s.width as f32 / s.height.max(1) as f32).min(image_width)
            });
            let bounds = Rectangle::new(
                Point::new(position.x + (width - image_width) / 2.0, y + 8.0),
                Size::new(image_width, height),
            );
            self.layout.rows.push(VisualRow {
                display_range: image.range.clone(),
                bounds: Rectangle::new(Point::new(position.x, bounds.y), Size::new(width, height)),
                display_stops: Arc::from([
                    (0, bounds.x),
                    (image.range.len(), bounds.x + bounds.width),
                ]),
                display_start: image.range.start,
                origin_x: 0.0,
            });
            self.layout.fragments.push(Fragment {
                range: image.range.clone(),
                bounds,
                kind: FragmentKind::Image {
                    handle,
                    label: format!(
                        "{}{}",
                        image.alt,
                        status.map_or_else(String::new, |status| format!("\n{status}"))
                    ),
                },
            });
            y += height + 16.0;
            part = image.range.end;
        }
        if part < range.end || part == range.start {
            y += self.paragraph(
                part..range.end,
                Point::new(position.x, y),
                width,
                size,
                bold,
                alignment,
            );
        }
        y - position.y
    }

    fn paragraph(
        &mut self,
        range: Range<usize>,
        position: Point,
        width: f32,
        size: f32,
        bold: bool,
        alignment: u8,
    ) -> f32 {
        let line_index = self
            .line_starts
            .partition_point(|&start| start <= range.start)
            .saturating_sub(1);
        let line_start = self.line_starts[line_index];
        let marks = self
            .projection
            .highlights
            .get(line_index)
            .into_iter()
            .flat_map(|marks| marks.iter())
            .filter_map(|(mark, style)| {
                let start = (mark.start + line_start).max(range.start);
                let end = (mark.end + line_start).min(range.end);
                if start >= end {
                    return None;
                }
                Some((start - range.start..end - range.start, *style))
            })
            .collect::<Vec<_>>();
        let size = if !range.is_empty()
            && marks.first().is_some_and(|(mark, _)| mark.start == 0)
            && marks
                .last()
                .is_some_and(|(mark, _)| mark.end == range.len())
            && marks.iter().all(|(_, style)| style.contains(Style::SYNTAX))
        {
            self.size * 0.8
        } else {
            size
        };
        let key = ParagraphKey {
            text: self.projection.text[range.clone()].to_owned(),
            marks,
            width: width.to_bits(),
            size: size.to_bits(),
            line_height: self.line_height.to_bits(),
            alignment,
            bold,
        };
        let cached = self
            .layout
            .cache
            .get(&key)
            .cloned()
            .or_else(|| self.previous.remove(&key))
            .unwrap_or_else(|| {
                let mut spans = Vec::<text::Span<'_>>::new();
                let mut start = 0;
                for (mark, style) in &key.marks {
                    if mark.start > start {
                        spans.push(text::Span::new(&key.text[start..mark.start]));
                    }
                    let format = styles::inline_format(
                        Style(style.0 | if bold { Style::BOLD } else { 0 }),
                        self.tokens,
                    );
                    spans.push(
                        text::Span::new(&key.text[mark.clone()])
                            .font_maybe(format.font)
                            .color_maybe(format.color)
                            .size(if style.contains(Style::SYNTAX) {
                                self.size * 0.8
                            } else {
                                size
                            }),
                    );
                    start = mark.end;
                }
                if start < key.text.len() || spans.is_empty() {
                    spans.push(text::Span::new(&key.text[start..]));
                }
                let paragraph = Paragraph::with_spans(text::Text {
                    content: spans.as_slice(),
                    bounds: Size::new(width.max(1.0), f32::INFINITY),
                    size: size.into(),
                    line_height: text::LineHeight::Relative(if bold {
                        1.3
                    } else {
                        self.line_height
                    }),
                    font: Font {
                        weight: if bold {
                            iced::font::Weight::Bold
                        } else {
                            iced::font::Weight::Normal
                        },
                        ..styles::BODY_FONT
                    },
                    align_x: match alignment {
                        1 => text::Alignment::Center,
                        2 => text::Alignment::Right,
                        _ => text::Alignment::Default,
                    },
                    align_y: iced::alignment::Vertical::Top,
                    shaping: text::Shaping::Advanced,
                    wrapping: text::Wrapping::WordOrGlyph,
                });
                let mut rows = Vec::new();
                for run in paragraph.buffer().layout_runs() {
                    let mut stops = Vec::new();
                    for glyph in run.glyphs {
                        let cluster = &key.text[glyph.start..glyph.end];
                        let graphemes = cluster
                            .grapheme_indices(true)
                            .map(|(n, _)| n)
                            .chain(std::iter::once(cluster.len()))
                            .collect::<Vec<_>>();
                        let count = graphemes.len().saturating_sub(1).max(1);
                        for (step, index) in graphemes.into_iter().enumerate() {
                            let fraction = step as f32 / count as f32;
                            let x = glyph.x
                                + glyph.w
                                    * if glyph.level.is_rtl() {
                                        1.0 - fraction
                                    } else {
                                        fraction
                                    };
                            stops.push((glyph.start + index, x));
                        }
                    }
                    if stops.is_empty() {
                        stops.push((0, 0.0));
                    }
                    stops.sort_by_key(|(index, _)| *index);
                    stops.dedup_by_key(|(index, _)| *index);
                    let display_range = stops.first().expect("row has a caret").0
                        ..stops.last().expect("row has a caret").0;
                    let display_stops = stops;
                    rows.push(CachedRow {
                        range: display_range,
                        stops: Arc::from(display_stops),
                        top: run.line_top,
                        height: run.line_height,
                    });
                }
                Arc::new(CachedParagraph { paragraph, rows })
            });
        let paragraph = &cached.paragraph;
        let height = paragraph
            .min_height()
            .max(size * if bold { 1.3 } else { self.line_height });
        let origin = Rectangle::new(position, Size::new(width, height)).anchor(
            paragraph.min_bounds(),
            paragraph.align_x(),
            paragraph.align_y(),
        );
        for row in &cached.rows {
            self.layout.rows.push(VisualRow {
                display_range: range.start + row.range.start..range.start + row.range.end,
                display_stops: Arc::clone(&row.stops),
                display_start: range.start,
                origin_x: origin.x,
                bounds: Rectangle::new(
                    Point::new(position.x, origin.y + row.top),
                    Size::new(width, row.height),
                ),
            });
        }
        self.layout.cache.insert(key, Arc::clone(&cached));
        self.layout.fragments.push(Fragment {
            range: range.clone(),
            bounds: Rectangle::new(position, Size::new(width, height)),
            kind: FragmentKind::Text {
                paragraph: paragraph.clone(),
                origin,
                display_start: range.start,
                style: ParagraphStyle {
                    size,
                    bold,
                    alignment,
                },
            },
        });
        height
    }

    fn table(&mut self, table: &crate::rich_document::Table) {
        let columns = table.alignment.len().max(1);
        let cell_width = self.width / columns as f32;
        let padding = (self.size * 0.65).min(cell_width / 4.0);
        for (row_index, row) in table.rows.iter().enumerate() {
            let y = self.y;
            let mut height = self.size * self.line_height + padding * 2.0;
            for (column, cell) in row.iter().enumerate() {
                let alignment = match table.alignment.get(column) {
                    Some(pulldown_cmark::Alignment::Center) => 1,
                    Some(pulldown_cmark::Alignment::Right) => 2,
                    _ => 0,
                };
                let h = self.flow(
                    cell.clone(),
                    Point::new(column as f32 * cell_width + padding, y + padding),
                    (cell_width - padding * 2.0).max(1.0),
                    self.size,
                    row_index == 0,
                    alignment,
                );
                height = height.max(h + padding * 2.0);
            }
            let bounds = Rectangle::new(Point::new(0.0, y), Size::new(self.width, height));
            if row_index == 0 {
                self.layout.decorations.push((bounds, Decoration::Header));
            }
            for column in 0..columns {
                self.layout.decorations.push((
                    Rectangle::new(
                        Point::new(column as f32 * cell_width, y),
                        Size::new(cell_width, height),
                    ),
                    Decoration::Border,
                ));
            }
            self.y += height;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_store::ImageStore;
    use crate::theme::Preset;

    fn layout(source: &str, width: f32) -> (Projection, RichLayout) {
        let projection = Projection::new(source);
        let mut layout = RichLayout::default();
        layout.rebuild(
            &projection,
            &Preset::Paper.resolve(),
            1.0,
            width,
            None,
            &ImageStore::default(),
        );
        (projection, layout)
    }

    #[test]
    fn headings_have_distinct_measured_sizes_and_pointer_hits() {
        let (projection, layout) = layout("# Large\n\n## Medium\n\nBody", 600.0);
        let large = layout.caret(0);
        let medium = layout.caret(projection.text.find("Medium").unwrap());
        let body = layout.caret(projection.text.find("Body").unwrap());
        assert!(large.height > medium.height && medium.height > body.height);
        for word in ["Large", "Medium", "Body"] {
            let offset = projection.text.find(word).unwrap() + 1;
            let caret = layout.caret(offset);
            assert_eq!(
                layout.hit(Point::new(caret.x + 0.1, caret.center_y())),
                offset
            );
        }
    }

    #[test]
    fn revealed_heading_keeps_its_size_and_makes_every_marker_reachable() {
        let source = "### Title\n\nBody";
        let projection = Projection::with_context(source, Some(4));
        let mut layout = RichLayout::default();
        layout.rebuild(
            &projection,
            &Preset::Paper.resolve(),
            1.0,
            600.0,
            None,
            &ImageStore::default(),
        );
        assert!(
            layout.caret(4).height > layout.caret(projection.text.find("Body").unwrap()).height
        );
        for offset in 0..9 {
            let caret = layout.caret(offset);
            assert_eq!(
                layout.hit(Point::new(caret.x + 0.1, caret.center_y())),
                offset
            );
        }
    }

    #[test]
    fn hidden_delimiters_and_long_destinations_occupy_no_layout_space() {
        let long = "long/destination/".repeat(100);
        let source =
            format!("# Title\n\nA **bold** *soft* `code` [link](https://example.com/{long}) end.");
        let equivalent = "# Title\n\nA **bold** *soft* `code` [link](x) end.";
        let (projection, layout) = layout(&source, 180.0);
        let (other, expected) = super::tests::layout(equivalent, 180.0);
        assert_eq!(projection.text, "Title\n\nA bold soft code link end.");
        assert_eq!(projection.text, other.text);
        assert_eq!(layout.height, expected.height);
        assert_eq!(layout.rows.len(), expected.rows.len());
        for (offset, _) in projection.text.char_indices() {
            assert_eq!(layout.caret(offset), expected.caret(offset));
        }
        assert_eq!(layout.caret(0).x, 0.0);
        assert_eq!(layout.caret(projection.text.find("A ").unwrap()).x, 0.0);
    }

    #[test]
    fn disclosure_reflows_locally_and_keeps_source_hit_testing() {
        let source = "Before\n\nA [label](https://example.com/long/destination/that/wraps) **bold** end.\n\nAfter";
        let (concealed, mut layout) = layout(source, 180.0);
        let before = layout.source_caret(0, None);
        let height = layout.height;
        let head = source.find("label").unwrap() + 2;
        let revealed = concealed.disclosing(concealed.disclosure_at(Some(head)));
        layout.rebuild(
            &revealed,
            &Preset::Paper.resolve(),
            1.0,
            180.0,
            None,
            &ImageStore::default(),
        );
        assert_eq!(layout.source_caret(0, None), before);
        assert!(layout.height > height);
        assert!(revealed.text.contains("[label](https://"));
        assert!(!revealed.text.contains("**"));
        for raw in [
            head,
            source.find("https").unwrap() + 3,
            source.find("After").unwrap() + 2,
        ] {
            let caret = layout.source_caret(raw, None);
            assert_eq!(
                layout
                    .hit_source_with_row(Point::new(caret.x + 0.1, caret.center_y()))
                    .0,
                raw
            );
        }
    }

    #[test]
    fn pointer_and_vertical_navigation_retain_source_positions_on_disclosed_setext() {
        let source = "Before\n\nTitle\n=====\n\nAfter";
        let projection = Projection::with_context(source, Some(source.find("Title").unwrap()));
        let mut layout = RichLayout::default();
        layout.rebuild(
            &projection,
            &Preset::Paper.resolve(),
            1.0,
            500.0,
            None,
            &ImageStore::default(),
        );
        for word in ["Title", "=====", "After"] {
            let raw = source.find(word).unwrap();
            let caret = layout.source_caret(raw, None);
            assert_eq!(
                layout
                    .hit_source_with_row(Point::new(caret.x + 0.1, caret.center_y()))
                    .0,
                raw
            );
            let above = layout
                .vertical_source(caret, caret.x, -1.0, None)
                .unwrap()
                .0;
            let above = layout.source_caret(above, None);
            assert_eq!(
                layout.vertical_source(above, caret.x, 1.0, None).unwrap().0,
                raw
            );
        }
    }

    #[test]
    fn table_cells_wrap_independently_and_vertical_motion_keeps_the_column() {
        let (projection, layout) = layout(
            "Before\n\n| Left | Right |\n| --- | --- |\n| Text that wraps across several lines | Second |\n\nAfter",
            260.0,
        );
        let table = &projection.rich.tables[0];
        let right = table.rows[0][1].start + 2;
        let cell = layout.caret(right);
        let down = layout.vertical(cell, cell.x, 1.0, None).unwrap().0;
        assert!(
            table.rows[1][1].contains(&down),
            "{down:?} in {:?}",
            table.rows[1][1]
        );
        let left = projection.rich.tables[0].rows[1][0].clone();
        let rows = layout
            .rows
            .iter()
            .filter(|row| {
                row.display_range.start >= left.start && row.display_range.end <= left.end
            })
            .count();
        assert!(rows > 1);
        let selection = Selection {
            anchor: 0,
            head: projection.text.len(),
        };
        assert!(layout.selection_rects(selection).len() > 7);
        let caret = layout.caret(down);
        let below = layout.vertical(caret, cell.x, 1.0, None).unwrap().0;
        assert!(
            below > table.rows[1][1].end,
            "Down must leave a short cell beside a wrapping cell"
        );
    }

    #[test]
    fn image_is_one_hit_tested_stop_between_text_blocks() {
        let (projection, layout) = layout("Before\n\n![Alt](missing.png)\n\nAfter", 500.0);
        let image = &projection.rich.images[0];
        let fragment = layout
            .fragments
            .iter()
            .find(|f| matches!(f.kind, FragmentKind::Image { .. }))
            .unwrap();
        assert_eq!(
            layout.hit(fragment.bounds.position() + iced::Vector::new(2.0, 10.0)),
            image.range.start
        );
        assert_eq!(
            layout.hit(Point::new(
                fragment.bounds.x + fragment.bounds.width - 2.0,
                fragment.bounds.center_y()
            )),
            image.range.end
        );
        assert!(
            layout
                .selection_rects(Selection {
                    anchor: 0,
                    head: projection.text.len()
                })
                .iter()
                .any(|rect| rect.height > 100.0)
        );
    }

    #[test]
    fn column_alignment_uses_the_same_origin_for_caret_and_pointer() {
        let (projection, layout) = layout(
            "| Left | Center | Right |\n|:---|:---:|---:|\n| x | y | z |",
            600.0,
        );
        let row = &projection.rich.tables[0].rows[1];
        let left = layout.caret(row[0].start).x;
        let center = layout.caret(row[1].start).x;
        let right = layout.caret(row[2].start).x;
        assert!(
            left < 30.0 && center > 280.0 && right > 550.0,
            "{left}, {center}, {right}"
        );
        for cell in row {
            let caret = layout.caret(cell.start);
            assert_eq!(
                layout.hit(Point::new(caret.x + 0.1, caret.center_y())),
                cell.start
            );
        }
    }

    #[test]
    fn visual_line_end_and_pointer_keep_the_correct_side_of_a_wrap() {
        let (projection, layout) = layout(
            "A long paragraph with words that wrap to several visual lines.",
            140.0,
        );
        assert!(layout.rows.len() > 2);
        let row = &layout.rows[0];
        let edge = layout.line_edge(0, row.display_caret(0), true);
        let caret = layout.caret_at(edge, Some(row.bounds.y));
        assert_eq!(caret.y, row.bounds.y);
        assert!(edge < projection.text.len());
        let (hit, y) = layout.hit_source_with_row(Point::new(139.0, row.bounds.center_y()));
        assert_eq!(y, row.bounds.y);
        assert_eq!(layout.caret_at(hit, Some(y)).y, row.bounds.y);
    }

    #[test]
    fn incremental_geometry_matches_fresh_layout_through_repeated_reflow() {
        let mut source = "# Heading words\n\nA paragraph with **strong words** and [label](https://example.com/long/path) that wraps.\n\n| A | B |\n|---|---|\n| Words | Other |\n\n![alt](image.png)\n\nAfter words".to_owned();
        let mut projection = Projection::new(&source);
        let mut actual = RichLayout::default();
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let mut patched = 0;
        for step in 0..270 {
            let head = if step == 0 {
                source.find("# Hea").unwrap() + 5
            } else {
                source.find("A par").unwrap() + 5
            };
            if step < 135 && step % 5 == 0 {
                let mut next = source.clone();
                next.insert(head, 'x');
                projection = projection
                    .updating(&next, Some(head + 1))
                    .unwrap_or_else(|| Projection::with_context(&next, Some(head + 1)));
                source = next;
            } else {
                let head = if step % 2 == 0 {
                    source.find("strong").unwrap() + 2
                } else {
                    if step % 7 == 0 {
                        source.find("# Hea").unwrap() + 5
                    } else {
                        source.find("label").unwrap() + 2
                    }
                };
                projection = projection.disclosing(projection.disclosure_at(Some(head)));
            }
            actual.rebuild(&projection, &theme, 1.0, 180.0, None, &images);
            patched += usize::from(actual.patches > 0);
            let mut expected = RichLayout::default();
            expected.rebuild(&projection, &theme, 1.0, 180.0, None, &images);
            assert!(
                (actual.height - expected.height).abs() < 0.01,
                "height at step {step}"
            );
            assert_eq!(actual.rows.len(), expected.rows.len());
            assert_eq!(actual.fragments.len(), expected.fragments.len());
            assert!(actual.cache.len() <= actual.fragments.len());
            for (offset, _) in projection.text.char_indices() {
                let a = actual.caret(offset);
                let b = expected.caret(offset);
                assert!(
                    (a.x - b.x).abs() < 0.01
                        && (a.y - b.y).abs() < 0.01
                        && (a.height - b.height).abs() < 0.01,
                    "caret {offset} at step {step}: {a:?} vs {b:?}"
                );
            }
            for (a, b) in actual.decorations.iter().zip(&expected.decorations) {
                assert!((a.0.y - b.0.y).abs() < 0.01);
            }
        }
        assert!(patched > 100);
    }

    #[test]
    fn clipped_selection_and_drawing_match_full_geometry_in_wrapping_tables() {
        let source = "Before\n\n| **Long header words** | Right |\n|---|---|\n| Many words that wrap across several rows | Other words |\n\n![alt](image.png)\n\nAfter";
        let (projection, layout) = layout(source, 180.0);
        let selection = Selection {
            anchor: 0,
            head: source.len(),
        };
        for top in [0.0, 40.0, 90.0, 180.0, 300.0] {
            let visible = top..top + 60.0;
            let full = layout
                .source_selection_rects(selection)
                .into_iter()
                .filter(|r| r.y + r.height >= visible.start && r.y <= visible.end)
                .collect::<Vec<_>>();
            assert_eq!(
                layout.source_selection_rects_in(selection, visible.clone()),
                full
            );
            let mut expected = layout
                .fragments
                .iter()
                .filter(|f| {
                    f.bounds.y + f.bounds.height >= visible.start && f.bounds.y <= visible.end
                })
                .map(|f| f.range.clone())
                .collect::<Vec<_>>();
            let mut actual = layout
                .visible_fragments(visible)
                .filter(|f| f.bounds.y + f.bounds.height >= top && f.bounds.y <= top + 60.0)
                .map(|f| f.range.clone())
                .collect::<Vec<_>>();
            expected.sort_by_key(|r| r.start);
            actual.sort_by_key(|r| r.start);
            assert_eq!(actual, expected);
        }
        assert!(projection.rich.tables.len() == 1);
    }

    #[test]
    #[ignore = "run with mise run bench"]
    fn performance_rich_layout_cache() {
        use std::time::Instant;
        let source = (0..1000).map(|i| format!("## Heading {i}\n\nA paragraph with **bold** and *italic* and enough text to measure.\n\n")).collect::<String>();
        let mut projection = Projection::new(&source);
        let mut layout = RichLayout::default();
        let theme = Preset::Paper.resolve();
        let images = ImageStore::default();
        let start = Instant::now();
        layout.rebuild(&projection, &theme, 1.0, 720.0, None, &images);
        let initial = start.elapsed();
        let start = Instant::now();
        for i in 0..100 {
            projection = projection.disclosing(projection.disclosure_at(Some(i * 100)));
            layout.rebuild(&projection, &theme, 1.0, 720.0, None, &images);
        }
        println!(
            "Rich 2000 blocks: 100 syntax disclosure + cached layouts={:?}, mean={:?}",
            start.elapsed(),
            start.elapsed() / 100
        );
        let start = Instant::now();
        for i in 0..100 {
            projection = Projection::new(&format!("{source}{i}"));
            layout.rebuild(&projection, &theme, 1.0, 720.0, None, &images);
        }
        println!(
            "Rich 2000 blocks: initial layout={initial:?}, 100 projection + cached layouts={:?}, mean={:?}",
            start.elapsed(),
            start.elapsed() / 100
        );
        assert!(layout.height > 10_000.0);
    }
}
