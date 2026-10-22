//! Structural metadata collected during the projection's Markdown parse.

use std::ops::Range;

use pulldown_cmark::{Alignment, Event, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub range: Range<usize>,
    pub level: u8,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Table {
    pub range: Range<usize>,
    pub source_range: Range<usize>,
    pub alignment: Vec<Alignment>,
    pub rows: Vec<Vec<Range<usize>>>,
    pub source_rows: Vec<Vec<Range<usize>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    pub range: Range<usize>,
    pub url: String,
    pub alt: String,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct RichDocument {
    pub headings: Vec<Heading>,
    pub tables: Vec<Table>,
    pub images: Vec<Image>,
    image_depth: usize,
    in_table: bool,
}

impl RichDocument {
    pub fn observe(&mut self, event: &Event<'_>, range: Range<usize>, source: &str) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => self.headings.push(Heading {
                range,
                level: *level as u8,
            }),
            Event::Start(Tag::Table(alignment)) => {
                self.in_table = true;
                self.tables.push(Table {
                    source_range: range.clone(),
                    range,
                    alignment: alignment.clone(),
                    rows: Vec::new(),
                    source_rows: Vec::new(),
                });
            }
            Event::Start(Tag::TableHead | Tag::TableRow) if self.in_table => {
                if let Some(table) = self.tables.last_mut() {
                    table.rows.push(Vec::new());
                }
            }
            Event::Start(Tag::TableCell) if self.in_table => {
                if let Some(row) = self
                    .tables
                    .last_mut()
                    .and_then(|table| table.rows.last_mut())
                {
                    let raw = &source[range.clone()];
                    let start = range.start + raw.len() - raw.trim_start().len();
                    let end = (range.start + raw.trim_end().len()).max(start);
                    row.push(start..end);
                }
            }
            Event::End(TagEnd::Table) => self.in_table = false,
            // An image is one selectable object in the editing buffer. Its
            // source, including the alt text and destination, stays lossless.
            Event::Start(Tag::Image { dest_url, .. }) => {
                if self.image_depth == 0 {
                    self.images.push(Image {
                        range,
                        url: dest_url.to_string(),
                        alt: String::new(),
                    });
                }
                self.image_depth += 1;
            }
            Event::Text(text) | Event::Code(text) if self.image_depth > 0 => {
                if let Some(image) = self.images.last_mut() {
                    image.alt.push_str(text);
                }
            }
            Event::SoftBreak | Event::HardBreak if self.image_depth > 0 => {
                if let Some(image) = self.images.last_mut() {
                    image.alt.push(' ');
                }
            }
            Event::End(TagEnd::Image) => self.image_depth = self.image_depth.saturating_sub(1),
            _ => {}
        }
    }

    /// Table separators become single editable navigation stops. The newline
    /// after the header also covers the source-only alignment row.
    pub fn decorations(&self, source: &str) -> Vec<(Range<usize>, String)> {
        let mut result = self
            .images
            .iter()
            .map(|image| (image.range.clone(), "\u{fffc}".into()))
            .collect::<Vec<_>>();
        for table in &self.tables {
            let cells = table.rows.iter().flatten().collect::<Vec<_>>();
            let Some(first) = cells.first() else { continue };
            result.push((table.range.start..first.start, String::new()));
            for (row_index, row) in table.rows.iter().enumerate() {
                for pair in row.windows(2) {
                    result.push((pair[0].end..pair[1].start, "\t".into()));
                }
                if let (Some(last), Some(next)) = (
                    row.last(),
                    table.rows.get(row_index + 1).and_then(|row| row.first()),
                ) {
                    result.push((last.end..next.start, "\n".into()));
                }
            }
            if let Some(last) = cells.last() {
                let tail = &source[last.end..table.range.end];
                result.push((
                    last.end..table.range.end,
                    if tail.ends_with('\n') { "\n" } else { "" }.into(),
                ));
            }
        }
        result
    }

    pub fn map_ranges(&mut self, map: impl Fn(usize) -> usize) {
        let range = |range: &mut Range<usize>| *range = map(range.start)..map(range.end);
        for heading in &mut self.headings {
            range(&mut heading.range);
        }
        for table in &mut self.tables {
            table.source_rows = table.rows.clone();
            range(&mut table.range);
            for cell in table.rows.iter_mut().flatten() {
                range(cell);
            }
        }
        for image in &mut self.images {
            range(&mut image.range);
        }
    }
}
