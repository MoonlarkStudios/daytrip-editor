//! Structural metadata collected during the projection's Markdown parse.

use std::ops::Range;
use std::sync::Arc;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Tag, TagEnd};

#[derive(Debug, Clone, PartialEq)]
pub struct Heading {
    pub range: Range<usize>,
    pub level: u8,
}

#[cfg(test)]
mod tests {
    use crate::document::projection::Projection;

    #[test]
    fn code_metadata_preserves_normalized_content_and_literal_shorter_fences() {
        for (source, expected) in [
            ("```rust\r\nlet café = 1;\r\n```\r\n", "let café = 1;\n"),
            ("````text\n```\n````\n", "```\n"),
            (
                "~~~unknown extra-info\n<literal> & **raw**\n~~~\n",
                "<literal> & **raw**\n",
            ),
            ("```\n```\n", ""),
            ("```rust\nunfinished", "unfinished"),
            ("    indented\n    code\n", "indented\ncode\n"),
            ("> ```rust\n> let x = 1;\n> ```\n", "let x = 1;\n"),
        ] {
            let projection = Projection::new(source);
            assert_eq!(projection.rich.code_blocks.len(), 1, "{source:?}");
            let block = &projection.rich.code_blocks[0];
            assert_eq!(block.text.as_str(), expected, "{source:?}");
            assert!(source.get(block.source_body.clone()).is_some());
            assert!(projection.text.get(block.body.clone()).is_some());
        }
        let projection = Projection::new("````\n```\n");
        assert!(
            projection.text.contains("```"),
            "shorter fences are code content"
        );
    }
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

#[derive(Debug, Clone, PartialEq)]
pub struct CodeBlock {
    pub range: Range<usize>,
    pub source_range: Range<usize>,
    pub body: Range<usize>,
    pub source_body: Range<usize>,
    pub language: String,
    /// Parser-normalized content, without fences or container indentation.
    pub text: Arc<String>,
    pub fenced: bool,
}

impl CodeBlock {
    pub fn last_line(&self, line: &Range<usize>) -> bool {
        if self.fenced && self.body.is_empty() {
            line.start == self.body.start
        } else {
            line.end >= self.range.end.saturating_sub(1)
        }
    }

    fn new(kind: &CodeBlockKind<'_>, range: Range<usize>, source: &str) -> Self {
        let raw = &source[range.clone()];
        let fenced = matches!(kind, CodeBlockKind::Fenced(_));
        let mut body = range.clone();
        if fenced {
            body.start = range.start + raw.find('\n').map_or(raw.len(), |i| i + 1);
            // Text events extend this range. Their offsets distinguish real
            // closing fences from shorter runs and fences inside containers.
            body.end = body.start;
        }
        Self {
            source_range: range.clone(),
            range,
            source_body: body.clone(),
            body,
            language: match kind {
                CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or("").into(),
                CodeBlockKind::Indented => String::new(),
            },
            text: Arc::default(),
            fenced,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct RichDocument {
    pub headings: Vec<Heading>,
    pub tables: Vec<Table>,
    pub images: Vec<Image>,
    pub code_blocks: Vec<CodeBlock>,
    image_depth: usize,
    in_table: bool,
    in_code: bool,
}

impl RichDocument {
    pub fn observe(&mut self, event: &Event<'_>, range: Range<usize>, source: &str) {
        match event {
            Event::Start(Tag::CodeBlock(kind)) => {
                self.in_code = true;
                self.code_blocks.push(CodeBlock::new(kind, range, source));
            }
            Event::Text(text) if self.in_code => {
                if let Some(block) = self.code_blocks.last_mut() {
                    Arc::make_mut(&mut block.text).push_str(text);
                    if block.fenced {
                        block.body.end = range.end;
                        block.source_body.end = range.end;
                    }
                }
            }
            Event::End(TagEnd::CodeBlock) => self.in_code = false,
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
        for block in &mut self.code_blocks {
            range(&mut block.range);
            range(&mut block.body);
        }
    }

    pub fn code_line(&self, line: &Range<usize>) -> Option<(usize, &CodeBlock)> {
        let index = self.code_blocks.partition_point(|block| {
            block.range.end <= line.start
                && !block.range.is_empty()
                && !(block.fenced && block.body.is_empty() && block.body.start == line.start)
        });
        self.code_blocks
            .get(index)
            .filter(|block| block.range.start <= line.end)
            .map(|block| (index, block))
    }

    pub fn source_code_at(&self, point: usize) -> Option<&CodeBlock> {
        let index = self
            .code_blocks
            .partition_point(|block| block.source_range.end < point);
        self.code_blocks
            .get(index)
            .filter(|block| block.source_range.start <= point && point <= block.source_range.end)
    }
}
