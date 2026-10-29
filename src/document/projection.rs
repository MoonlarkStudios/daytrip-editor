//! A lossless source/display map with contextual syntax disclosure.
//!
//! Hidden delimiters occupy no display space. Source offsets remain authoritative
//! when the active heading or inline span changes the projected editing buffer.

use std::ops::Range;
use std::sync::Arc;

use pulldown_cmark::{Event, Parser, Tag};

use crate::document::block_parser::MARKDOWN_OPTIONS;
use crate::document::rich_document::RichDocument;
use crate::document::selection::Selection;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Style(pub u8);

impl Style {
    pub const BOLD: u8 = 1;
    pub const ITALIC: u8 = 2;
    pub const CODE: u8 = 4;
    pub const LINK: u8 = 8;
    pub const MUTED: u8 = 16;
    pub const SYNTAX: u8 = 32;
    pub const CODE_BLOCK: u8 = 64;
    pub fn contains(self, flag: u8) -> bool {
        self.0 & flag != 0
    }
}

pub type HighlightLines = Vec<Arc<[(Range<usize>, Style)]>>;

#[path = "projection_blocks.rs"]
mod block_update;
#[path = "projection_disclosure.rs"]
mod disclosure_update;
#[path = "projection_update.rs"]
mod incremental;

#[derive(Debug)]
pub struct ProjectionChange {
    pub previous: Arc<HighlightLines>,
    pub regions: Vec<TextChange>,
}

#[derive(Debug)]
pub struct Projection {
    pub text: String,
    pub highlights: Arc<HighlightLines>,
    pub rich: RichDocument,
    pub disclosure: Disclosure,
    pub source: String,
    /// Source identity is independent of text ownership; layout retains only this key.
    pub source_id: Arc<()>,
    pub mapping: Arc<OffsetMap>,
    pub change: Option<ProjectionChange>,
    pub line_starts: Arc<[usize]>,
    parsed: Parsed,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Disclosure(Vec<Range<usize>>);

#[derive(Debug)]
struct ProjectedText {
    pub text: String,
    pub highlights: Arc<HighlightLines>,
    pub rich: RichDocument,
    pub mapping: Arc<OffsetMap>,
}

#[derive(Debug)]
pub struct OffsetMap {
    pieces: Vec<Piece>,
    source_length: usize,
    display_length: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct Parsed {
    decorations: Vec<Decoration>,
    marks: Vec<Mark>,
    rich: RichDocument,
    pairs: Vec<Pair>,
    block_prefixes: Vec<Range<usize>>,
    safe_text: Vec<Range<usize>>,
    global_dependencies: bool,
}

#[derive(Debug, Clone)]
struct Piece {
    source: Range<usize>,
    display: Range<usize>,
    identity: bool,
    separator: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct Pair {
    open: Range<usize>,
    body: Range<usize>,
    close: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
struct Decoration {
    range: Range<usize>,
    text: String,
    kind: Concealment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Concealment {
    Syntax,
    Heading,
    CodeFence,
    Task,
    Structural,
}
#[derive(Debug, Clone, PartialEq)]
struct Mark {
    range: Range<usize>,
    style: u8,
}

#[derive(Debug, Clone, Copy)]
pub enum Affinity {
    Before,
    After,
}

pub struct ProjectedEdit {
    pub range: Range<usize>,
    pub text: String,
    pub selection: Selection,
}

/// The smallest UTF-8-safe splice between two text snapshots.
#[derive(Debug, Clone)]
pub struct TextChange {
    pub old: Range<usize>,
    pub new: Range<usize>,
}

impl TextChange {
    fn previous_offset(&self, offset: usize) -> usize {
        if offset < self.new.start {
            offset
        } else if offset <= self.new.end {
            self.old.start + (offset - self.new.start).min(self.old.len())
        } else {
            offset - self.new.end + self.old.end
        }
    }
}

pub fn difference(before: &str, after: &str) -> TextChange {
    // Compare equal chunks with memcmp instead of decoding every unchanged
    // character in a large document. Only splice boundaries need UTF-8 checks.
    let common = before.len().min(after.len());
    let chunks = before
        .as_bytes()
        .chunks_exact(64)
        .zip(after.as_bytes().chunks_exact(64))
        .take_while(|(a, b)| a == b)
        .count()
        * 64;
    let mut prefix = chunks
        + before.as_bytes()[chunks..common]
            .iter()
            .zip(&after.as_bytes()[chunks..common])
            .take_while(|(a, b)| a == b)
            .count();
    while !before.is_char_boundary(prefix) || !after.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let a = &before.as_bytes()[prefix..];
    let b = &after.as_bytes()[prefix..];
    let chunks = a
        .rchunks_exact(64)
        .zip(b.rchunks_exact(64))
        .take_while(|(a, b)| a == b)
        .count()
        * 64;
    let mut suffix = chunks
        + a[..a.len() - chunks]
            .iter()
            .rev()
            .zip(b[..b.len() - chunks].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
    while !before.is_char_boundary(before.len() - suffix)
        || !after.is_char_boundary(after.len() - suffix)
    {
        suffix -= 1;
    }
    TextChange {
        old: prefix..before.len() - suffix,
        new: prefix..after.len() - suffix,
    }
}

impl Projection {
    pub fn new(source: &str) -> Self {
        Self::with_context(source, None)
    }

    pub fn with_context(source: &str, caret: Option<usize>) -> Self {
        let parsed = Self::parse(source);
        let disclosure = Self::resolve_disclosure(source, &parsed, caret);
        Self::from_parsed(source.to_owned(), disclosure, parsed)
    }

    fn with_disclosure(source: &str, disclosure: Disclosure) -> Self {
        Self::from_parsed(source.to_owned(), disclosure, Self::parse(source))
    }

    fn parse(source: &str) -> Parsed {
        let mut decorations = Vec::new();
        let mut marks = Vec::new();
        let mut pairs = Vec::new();
        let mut in_code_block = false;
        let mut link_depth = 0_usize;
        let mut safe_text = Vec::new();
        let mut block_prefixes = Vec::new();
        let mut rich = RichDocument::default();
        let hide = |range: Range<usize>, decorations: &mut Vec<Decoration>| {
            if !range.is_empty() {
                decorations.push(Decoration {
                    range,
                    text: String::new(),
                    kind: Concealment::Syntax,
                });
            }
        };
        let parser = Parser::new_ext(source, MARKDOWN_OPTIONS);
        let mut global_dependencies = parser.reference_definitions().iter().next().is_some();
        for (event, range) in parser.into_offset_iter() {
            global_dependencies |= matches!(
                &event,
                Event::Start(Tag::FootnoteDefinition(_)) | Event::FootnoteReference(_)
            );
            rich.observe(&event, range.clone(), source);
            let raw = &source[range.clone()];
            match event {
                Event::Start(Tag::Strong | Tag::Emphasis | Tag::Strikethrough) => {
                    let (width, style) = match &event {
                        Event::Start(Tag::Strong) => (2, Style::BOLD),
                        Event::Start(Tag::Emphasis) => (1, Style::ITALIC),
                        _ => (2, Style::MUTED),
                    };
                    if range.len() >= width * 2 {
                        let open = range.start..range.start + width;
                        let close = range.end - width..range.end;
                        hide(open.clone(), &mut decorations);
                        hide(close.clone(), &mut decorations);
                        marks.push(Mark {
                            range: range.clone(),
                            style,
                        });
                        pairs.push(Pair {
                            body: open.end..close.start,
                            open,
                            close,
                        });
                    }
                }
                Event::Code(_) => {
                    let width = raw.bytes().take_while(|&byte| byte == b'`').count();
                    if width > 0 && range.len() >= 2 * width {
                        let open = range.start..range.start + width;
                        let close = range.end - width..range.end;
                        hide(open.clone(), &mut decorations);
                        hide(close.clone(), &mut decorations);
                        pairs.push(Pair {
                            body: open.end..close.start,
                            open,
                            close,
                        });
                    }
                    marks.push(Mark {
                        range,
                        style: Style::CODE,
                    });
                }
                Event::Start(Tag::Link { .. }) => {
                    link_depth += 1;
                    if raw.starts_with('<') && raw.ends_with('>') {
                        let open = range.start..range.start + 1;
                        let close = range.end - 1..range.end;
                        hide(open.clone(), &mut decorations);
                        hide(close.clone(), &mut decorations);
                        pairs.push(Pair {
                            body: open.end..close.start,
                            open,
                            close,
                        });
                    } else if raw.starts_with('[')
                        && let Some(close) = closing_bracket(raw)
                    {
                        let open = range.start..range.start + 1;
                        let close = range.start + close..range.end;
                        hide(open.clone(), &mut decorations);
                        hide(close.clone(), &mut decorations);
                        pairs.push(Pair {
                            body: open.end..close.start,
                            open,
                            close,
                        });
                    }
                    marks.push(Mark {
                        range,
                        style: Style::LINK,
                    });
                }
                Event::Start(Tag::Heading { .. }) => {
                    let indent = raw.bytes().take_while(|&byte| byte == b' ').count();
                    let hashes = raw[indent..]
                        .bytes()
                        .take_while(|&byte| byte == b'#')
                        .count();
                    let first_decoration = decorations.len();
                    if (1..=6).contains(&hashes)
                        && raw
                            .as_bytes()
                            .get(indent + hashes)
                            .is_none_or(u8::is_ascii_whitespace)
                    {
                        let prefix = indent + hashes;
                        let spaces = raw[prefix..]
                            .bytes()
                            .take_while(|byte| matches!(byte, b' ' | b'\t'))
                            .count();
                        block_prefixes.push(range.start..range.start + prefix + spaces);
                        hide(range.start..range.start + prefix + spaces, &mut decorations);
                        let trimmed = raw.trim_end_matches(['\r', '\n', ' ', '\t']);
                        let closing = trimmed
                            .bytes()
                            .rev()
                            .take_while(|&byte| byte == b'#')
                            .count();
                        if closing > 0
                            && trimmed.len() > closing
                            && trimmed.as_bytes()[trimmed.len() - closing - 1].is_ascii_whitespace()
                        {
                            hide(
                                range.start + trimmed.len() - closing..range.start + trimmed.len(),
                                &mut decorations,
                            );
                        }
                    } else if let Some(newline) = raw.trim_end_matches(['\r', '\n']).rfind('\n') {
                        hide(
                            range.start + newline
                                ..range.start + raw.trim_end_matches(['\r', '\n']).len(),
                            &mut decorations,
                        );
                    }
                    for decoration in &mut decorations[first_decoration..] {
                        decoration.kind = Concealment::Heading;
                    }
                    marks.push(Mark {
                        range,
                        style: Style::BOLD,
                    });
                }
                Event::Start(Tag::Item) => {
                    let indent = raw
                        .bytes()
                        .take_while(|byte| matches!(byte, b' ' | b'\t'))
                        .count();
                    if raw
                        .as_bytes()
                        .get(indent)
                        .is_some_and(|byte| matches!(byte, b'-' | b'+' | b'*'))
                    {
                        decorations.push(Decoration {
                            range: range.start + indent..range.start + indent + 1,
                            text: "•".into(),
                            kind: Concealment::Syntax,
                        });
                    }
                }
                Event::TaskListMarker(done) => decorations.push(Decoration {
                    range,
                    text: if done { "☑" } else { "☐" }.into(),
                    kind: Concealment::Task,
                }),
                Event::Start(Tag::BlockQuote(_)) => {
                    let mut offset = range.start;
                    for line in raw.split_inclusive('\n') {
                        let indent = line.bytes().take_while(|&byte| byte == b' ').count();
                        if line.as_bytes().get(indent) == Some(&b'>') {
                            decorations.push(Decoration {
                                range: offset + indent..offset + indent + 1,
                                text: "│".into(),
                                kind: Concealment::Syntax,
                            });
                        }
                        offset += line.len();
                    }
                    marks.push(Mark {
                        range,
                        style: Style::MUTED,
                    });
                }
                Event::Start(Tag::CodeBlock(_)) => {
                    in_code_block = true;
                    marks.push(Mark {
                        range,
                        style: Style::CODE_BLOCK,
                    });
                }
                Event::Rule => decorations.push(Decoration {
                    range: range.start..range.start + raw.trim_end_matches(['\r', '\n']).len(),
                    text: "────────────".into(),
                    kind: Concealment::Syntax,
                }),
                Event::End(pulldown_cmark::TagEnd::CodeBlock) => {
                    in_code_block = false;
                    if let Some(block) = rich.code_blocks.last().filter(|block| block.fenced) {
                        for fence in [
                            block.source_range.start..block.source_body.start,
                            block.source_body.end..block.source_range.end,
                        ] {
                            let end = fence.start
                                + source[fence.clone()].trim_end_matches(['\r', '\n']).len();
                            if fence.start < end {
                                decorations.push(Decoration {
                                    range: fence.start..end,
                                    text: String::new(),
                                    kind: Concealment::CodeFence,
                                });
                            }
                        }
                    }
                }
                Event::Start(Tag::Image { .. }) => link_depth += 1,
                Event::End(pulldown_cmark::TagEnd::Link | pulldown_cmark::TagEnd::Image) => {
                    link_depth = link_depth.saturating_sub(1)
                }
                Event::Text(_) if !in_code_block => {
                    if link_depth == 0 {
                        safe_text.push(range.clone());
                    }
                    for (index, character) in raw.char_indices() {
                        let position = range.start + index;
                        if character.is_ascii_punctuation() {
                            let backslashes = source[..position]
                                .bytes()
                                .rev()
                                .take_while(|&byte| byte == b'\\')
                                .count();
                            if backslashes % 2 == 1 {
                                hide(position - 1..position, &mut decorations);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for (index, _) in source.match_indices("\r\n") {
            decorations.push(Decoration {
                range: index..index + 1,
                text: String::new(),
                kind: Concealment::Structural,
            });
        }
        decorations.extend(
            rich.decorations(source)
                .into_iter()
                .map(|(range, text)| Decoration {
                    range,
                    text,
                    kind: Concealment::Structural,
                }),
        );
        decorations.sort_by(|a, b| {
            a.range
                .start
                .cmp(&b.range.start)
                .then(b.range.end.cmp(&a.range.end))
        });
        Parsed {
            decorations,
            marks,
            rich,
            pairs,
            block_prefixes,
            safe_text,
            global_dependencies,
        }
    }

    /// Cached parser markers in source order, independent of syntax disclosure.
    pub fn task_markers(&self, range: Range<usize>) -> impl Iterator<Item = Range<usize>> {
        let first = self
            .parsed
            .decorations
            .partition_point(|decoration| decoration.range.start < range.start);
        self.parsed.decorations[first..]
            .iter()
            .take_while(move |decoration| decoration.range.start < range.end)
            .filter(|decoration| decoration.kind == Concealment::Task)
            .map(|decoration| decoration.range.clone())
    }

    /// A task command acts on the caret's physical source line, never a sibling
    /// item or a continuation line which has no checkbox of its own.
    pub fn task_marker_at(&self, offset: usize) -> Option<Range<usize>> {
        let before = self.source.get(..offset)?;
        let start = before.rfind('\n').map_or(0, |newline| newline + 1);
        let end = self.source[offset..]
            .find('\n')
            .map_or(self.source.len(), |newline| offset + newline);
        self.task_markers(start..end).next()
    }

    /// Resolves the active heading and just the innermost paired inline span.
    /// The end of a completed span belongs to the surrounding prose.
    pub fn disclosure_at(&self, caret: Option<usize>) -> Disclosure {
        Self::resolve_disclosure(&self.source, &self.parsed, caret)
    }

    fn resolve_disclosure(source: &str, parsed: &Parsed, caret: Option<usize>) -> Disclosure {
        let Some(caret) = caret else {
            return Disclosure::default();
        };
        let mut ranges = Vec::new();
        ranges.extend(
            parsed
                .decorations
                .iter()
                .filter(|decoration| {
                    decoration.kind == Concealment::CodeFence
                        && decoration.range.start <= caret
                        && caret <= decoration.range.end
                        && !parsed
                            .rich
                            .source_code_at(caret)
                            .is_some_and(|block| block.fenced && block.source_body.end == caret)
                })
                .map(|decoration| decoration.range.clone()),
        );
        let heading = parsed.rich.headings.iter().find(|heading| {
            let end = heading.range.start
                + source[heading.range.clone()]
                    .trim_end_matches(['\r', '\n'])
                    .len();
            heading.range.start <= caret && caret <= end
        });
        if let Some(heading) = heading {
            ranges.extend(
                parsed
                    .decorations
                    .iter()
                    .filter(|decoration| {
                        decoration.kind == Concealment::Heading
                            && heading.range.start <= decoration.range.start
                            && decoration.range.end <= heading.range.end
                    })
                    .map(|decoration| decoration.range.clone()),
            );
        }
        if let Some(pair) = parsed
            .pairs
            .iter()
            .filter(|pair| pair.open.start <= caret && caret < pair.close.end)
            .min_by_key(|pair| pair.close.end - pair.open.start)
        {
            ranges.extend([pair.open.clone(), pair.close.clone()]);
        }
        ranges.sort_by_key(|range| range.start);
        Disclosure(ranges)
    }

    /// Changes disclosure in place, retaining source text and parsed metadata.
    pub fn set_disclosure(&mut self, disclosure: Disclosure) {
        if self.set_disclosure_lines(&disclosure) {
            return;
        }
        let visible = project(&self.source, &self.parsed, true, |decoration| {
            decoration.kind == Concealment::Structural
                || !disclosure.0.iter().any(|range| {
                    decoration.range.start < range.end && range.start < decoration.range.end
                })
        });
        let regions = self.disclosure_regions(&disclosure, &visible.mapping);
        self.change = Some(ProjectionChange {
            previous: std::mem::replace(&mut self.highlights, visible.highlights),
            regions,
        });
        self.text = visible.text;
        self.mapping = visible.mapping;
        self.rich = visible.rich;
        self.disclosure = disclosure;
        self.line_starts = std::iter::once(0)
            .chain(self.text.match_indices('\n').map(|(n, _)| n + 1))
            .collect();
    }

    /// Keep distant and multiline disclosure changes separate. Whole source
    /// lines contain every affected replacement, including a Setext underline.
    fn disclosure_regions(&self, disclosure: &Disclosure, mapping: &OffsetMap) -> Vec<TextChange> {
        let mut lines = self
            .disclosure
            .0
            .iter()
            .chain(&disclosure.0)
            .map(|range| {
                let start = self.source[..range.start].rfind('\n').map_or(0, |n| n + 1);
                let end = self.source[range.end..]
                    .find('\n')
                    .map_or(self.source.len(), |n| range.end + n);
                start..end
            })
            .collect::<Vec<_>>();
        lines.sort_by_key(|range| range.start);
        let mut merged: Vec<Range<usize>> = Vec::new();
        for range in lines {
            if let Some(last) = merged.last_mut()
                && range.start <= last.end
            {
                last.end = last.end.max(range.end);
            } else {
                merged.push(range);
            }
        }
        merged
            .into_iter()
            .map(|range| TextChange {
                old: self.to_display(range.start)..self.to_display(range.end),
                new: mapping.to_display(range.start)..mapping.to_display(range.end),
            })
            .collect()
    }

    fn from_parsed(source: String, disclosure: Disclosure, parsed: Parsed) -> Self {
        let visible = project(&source, &parsed, true, |decoration| {
            decoration.kind == Concealment::Structural
                || !disclosure.0.iter().any(|range| {
                    decoration.range.start < range.end && range.start < decoration.range.end
                })
        });
        Self {
            line_starts: std::iter::once(0)
                .chain(visible.text.match_indices('\n').map(|(n, _)| n + 1))
                .collect(),
            change: None,
            text: visible.text,
            highlights: visible.highlights,
            rich: visible.rich,
            disclosure,
            source,
            source_id: Arc::new(()),
            mapping: visible.mapping,
            parsed,
        }
    }

    fn concealed_pair(&self, pair: &Pair) -> bool {
        self.to_display(pair.open.start) == self.to_display(pair.open.end)
            && self.to_display(pair.close.start) == self.to_display(pair.close.end)
    }

    fn disclosure_after(&self, edit: &Range<usize>, length: usize) -> Disclosure {
        Disclosure(
            self.disclosure
                .0
                .iter()
                .map(|line| {
                    let shift = |offset: usize, end: bool| {
                        if offset < edit.start || offset == edit.start && !end {
                            offset
                        } else if offset >= edit.end {
                            offset - edit.len() + length
                        } else {
                            edit.start + if end { length } else { 0 }
                        }
                    };
                    shift(line.start, false)..shift(line.end, true)
                })
                .collect(),
        )
    }

    pub fn to_display(&self, offset: usize) -> usize {
        self.mapping.to_display(offset)
    }

    pub fn to_source(&self, offset: usize, affinity: Affinity, hint: Option<usize>) -> usize {
        self.mapping.to_source(offset, affinity, hint)
    }

    pub fn display_selection(&self, source: Selection) -> Selection {
        Selection {
            anchor: self.to_display(source.anchor),
            head: self.to_display(source.head),
        }
    }

    pub fn normalize_caret(&self, mut selection: Selection) -> Selection {
        if selection.is_caret() {
            let display = self.to_display(selection.head);
            let table_index = self
                .rich
                .tables
                .partition_point(|table| table.source_range.end <= selection.head);
            if let Some(table) = self.rich.tables.get(table_index)
                && table.source_range.start <= selection.head
            {
                let row_index = table
                    .rows
                    .partition_point(|row| row.last().is_some_and(|cell| cell.end < display));
                for (cell, source) in table
                    .rows
                    .get(row_index)
                    .into_iter()
                    .flatten()
                    .zip(table.source_rows.get(row_index).into_iter().flatten())
                {
                    if cell.start == display && selection.head < source.start {
                        selection = Selection::caret(source.start);
                    } else if cell.end == display && selection.head > source.end {
                        selection = Selection::caret(source.end);
                    }
                }
            }
            let prefix_index = self
                .parsed
                .block_prefixes
                .partition_point(|prefix| prefix.end <= selection.head);
            if let Some(prefix) = self.parsed.block_prefixes.get(prefix_index)
                && prefix.contains(&selection.head)
                && self.to_display(prefix.start) == self.to_display(prefix.end)
            {
                selection = Selection::caret(prefix.end);
            }
            loop {
                let index = self
                    .parsed
                    .pairs
                    .partition_point(|pair| pair.open.start <= selection.head);
                if let Some(pair) = index
                    .checked_sub(1)
                    .and_then(|index| self.parsed.pairs.get(index))
                    && pair.open.contains(&selection.head)
                    && self.concealed_pair(pair)
                {
                    selection = Selection::caret(pair.open.end);
                } else {
                    break;
                }
            }
        }
        selection
    }

    pub fn source_selection(&self, display: Selection, previous: Selection) -> Selection {
        let previous_display = self.display_selection(previous);
        let map = |offset, old_offset, hint| {
            self.to_source(
                offset,
                if offset >= old_offset {
                    Affinity::Before
                } else {
                    Affinity::After
                },
                Some(hint),
            )
        };
        if display.is_caret() {
            return Selection::caret(map(display.head, previous_display.head, previous.head));
        }
        Selection {
            anchor: map(display.anchor, previous_display.anchor, previous.anchor),
            head: map(display.head, previous_display.head, previous.head),
        }
    }

    pub fn source_range(&self, display: Range<usize>) -> Range<usize> {
        let mut range = self.to_source(display.start, Affinity::After, None)
            ..self.to_source(display.end, Affinity::Before, None);
        for piece in &self.mapping.pieces {
            if !piece.identity
                && piece.display.start < display.end
                && piece.display.end > display.start
            {
                range.start = range.start.min(piece.source.start);
                range.end = range.end.max(piece.source.end);
            }
        }
        if self.text[display.clone()].contains('\n') {
            for prefix in &self.parsed.block_prefixes {
                if prefix.start >= range.start
                    && prefix.start <= range.end
                    && self.to_display(prefix.end) == display.end
                {
                    range.end = range.end.max(prefix.end);
                }
            }
        }
        range.end = range.end.max(range.start);
        range
    }

    /// Applies an inline mark independently to each selected line, preserving
    /// block prefixes, nested source formatting, and selection direction.
    pub fn format_inline(
        &self,
        source: &str,
        selection: Selection,
        marker: &str,
    ) -> Option<ProjectedEdit> {
        if selection.is_caret() {
            return None;
        }
        let selected = selection.range();
        let mut ranges = Vec::new();
        let mut line_start = 0;
        for segment in self.text.split_inclusive(['\n', '\t', '\u{fffc}']) {
            let line = segment.trim_end_matches(['\n', '\t', '\u{fffc}']);
            let line_end = line_start + line.len();
            let start = line_start.max(selected.start);
            let end = line_end.min(selected.end);
            if start < end {
                let slice = &self.text[start..end];
                let left = slice.len() - slice.trim_start().len();
                let right = slice.trim_end().len();
                if left < right {
                    let mut start = start + left;
                    // Bullet/quote glyphs and task boxes are structural content.
                    if start == line_start {
                        for prefix in ["• ", "│ ", "☐ ", "☑ "] {
                            if self.text[start..end].starts_with(prefix) {
                                start += prefix.len();
                            }
                        }
                    }
                    if start < (end - slice.len() + right) {
                        ranges.push(self.source_range(start..end - slice.len() + right));
                    }
                }
            }
            line_start += segment.len();
        }
        if ranges.is_empty() {
            return None;
        }
        let pairs = ranges
            .iter()
            .map(|range| {
                self.parsed
                    .pairs
                    .iter()
                    .find(|pair| pair.body == *range && &source[pair.open.clone()] == marker)
            })
            .collect::<Vec<_>>();
        let unwrap = pairs.iter().all(Option::is_some);
        let edits = ranges
            .into_iter()
            .zip(pairs)
            .map(|(range, pair)| {
                if unwrap {
                    let pair = pair.expect("all selected lines have the mark");
                    (
                        pair.open.start..pair.close.end,
                        source[pair.body.clone()].to_owned(),
                    )
                } else if pair.is_some() {
                    (range.clone(), source[range].to_owned())
                } else {
                    (range.clone(), format!("{marker}{}{marker}", &source[range]))
                }
            })
            .collect::<Vec<_>>();
        let range = edits.first()?.0.start..edits.last()?.0.end;
        let mut replacement = source[range.clone()].to_owned();
        for (edit, text) in edits.into_iter().rev() {
            replacement.replace_range(edit.start - range.start..edit.end - range.start, &text);
        }
        let mut result = source.to_owned();
        result.replace_range(range.clone(), &replacement);
        let projected = Self::new(&result);
        let forward = selection.anchor <= selection.head;
        let restored = Selection {
            anchor: projected.to_source(
                selection.anchor,
                if forward {
                    Affinity::After
                } else {
                    Affinity::Before
                },
                None,
            ),
            head: projected.to_source(
                selection.head,
                if forward {
                    Affinity::Before
                } else {
                    Affinity::After
                },
                None,
            ),
        };
        Some(ProjectedEdit {
            range,
            text: replacement,
            selection: restored,
        })
    }

    pub fn edit(
        &self,
        source: &str,
        after: &str,
        previous: Selection,
        cursor: Selection,
    ) -> ProjectedEdit {
        let mut diff = difference(&self.text, after);
        let selected = self.display_selection(previous).range();
        // A replacement must honor the actual selection, even when the new text
        // happens to share a prefix/suffix with what was selected.
        if !previous.is_caret()
            && after.len() >= selected.start + self.text.len() - selected.end
            && after.starts_with(&self.text[..selected.start])
            && after.ends_with(&self.text[selected.end..])
        {
            diff = TextChange {
                old: selected.clone(),
                new: selected.start..after.len() - (self.text.len() - selected.end),
            };
        }
        let inserted = &after[diff.new.clone()];
        let mut range = if previous.range() == (0..source.len()) && !previous.is_caret() {
            0..source.len()
        } else if diff.old.is_empty() {
            let point = self.to_source(diff.old.start, Affinity::After, Some(previous.head));
            point..point
        } else {
            self.source_range(diff.old.clone())
        };
        {
            loop {
                let before = range.clone();
                for pair in &self.parsed.pairs {
                    if pair.open.start > range.end {
                        break;
                    }
                    if pair.close.end < range.start {
                        continue;
                    }
                    if range.start <= pair.body.start
                        && range.end >= pair.body.end
                        && (inserted.is_empty()
                            || range.start <= pair.open.start
                            || range.end >= pair.close.end)
                        && self.concealed_pair(pair)
                    {
                        range.start = range.start.min(pair.open.start);
                        range.end = range.end.max(pair.close.end);
                    }
                }
                if before == range {
                    break;
                }
            }
        }
        let mut closers = Vec::new();
        let mut openers = Vec::new();
        for pair in &self.parsed.pairs {
            if pair.open.start > range.end {
                break;
            }
            if pair.close.end < range.start {
                continue;
            }
            if range.start >= pair.open.end
                && range.start < pair.close.start
                && range.end >= pair.close.end
                && self.concealed_pair(pair)
            {
                closers.push(pair.close.clone());
            }
            if range.start <= pair.open.start
                && range.end >= pair.open.end
                && range.end > pair.open.end
                && range.end <= pair.close.start
                && self.concealed_pair(pair)
            {
                openers.push(pair.open.clone());
            }
        }
        let had_repairs = !closers.is_empty() || !openers.is_empty();
        closers.sort_by_key(|range| range.start);
        openers.sort_by_key(|range| range.start);
        let core_length = if closers.is_empty() {
            inserted.len()
        } else {
            inserted.trim_end().len()
        };
        let mut prefix_space = String::new();
        if !closers.is_empty() && core_length == 0 {
            let body_start = self
                .parsed
                .pairs
                .iter()
                .filter(|pair| closers.contains(&pair.close))
                .map(|pair| pair.body.start)
                .max()
                .unwrap_or(range.start);
            let prefix = &source[body_start..range.start];
            let spaces = prefix.len() - prefix.trim_end().len();
            prefix_space = source[range.start - spaces..range.start].to_owned();
            range.start -= spaces;
        }
        let closer_length = closers.iter().map(Range::len).sum::<usize>();
        let mut replacement = inserted[..core_length].to_owned();
        for delimiter in closers {
            replacement.push_str(&source[delimiter]);
        }
        replacement.push_str(&prefix_space);
        replacement.push_str(&inserted[core_length..]);
        if !openers.is_empty() {
            let body_end = self
                .parsed
                .pairs
                .iter()
                .filter(|pair| openers.contains(&pair.open))
                .map(|pair| pair.body.end)
                .min()
                .unwrap_or(range.end);
            let suffix = &source[range.end..body_end];
            let spaces = suffix.len() - suffix.trim_start().len();
            replacement.push_str(&source[range.end..range.end + spaces]);
            range.end += spaces;
            for delimiter in openers {
                replacement.push_str(&source[delimiter]);
            }
        }
        if had_repairs
            && !inserted
                .chars()
                .any(|c| matches!(c, '*' | '_' | '`' | '[' | ']' | '<' | '>' | '~' | '\\'))
        {
            let mut candidate = source.to_owned();
            candidate.replace_range(range.clone(), &replacement);
            if Self::with_disclosure(&candidate, self.disclosure_after(&range, replacement.len()))
                .text
                != after
            {
                // CommonMark cannot represent every rich-text join (for example,
                // a delimiter between a letter and emoji). Preserve visible text
                // by flattening only the affected inline spans in that case.
                let mut expanded = range.clone();
                loop {
                    let previous = expanded.clone();
                    for pair in self
                        .parsed
                        .pairs
                        .iter()
                        .filter(|pair| self.concealed_pair(pair))
                    {
                        if pair.open.start < expanded.end && pair.close.end > expanded.start {
                            expanded.start = expanded.start.min(pair.open.start);
                            expanded.end = expanded.end.max(pair.close.end);
                        }
                    }
                    if previous == expanded {
                        break;
                    }
                }
                let start = self.to_display(expanded.start);
                let end = self.to_display(expanded.end);
                let visible = format!(
                    "{}{}{}",
                    &self.text[start..diff.old.start],
                    inserted,
                    &self.text[diff.old.end..end]
                );
                let mut escaped = String::with_capacity(visible.len());
                for character in visible.chars() {
                    if character.is_ascii_punctuation() {
                        escaped.push('\\');
                    }
                    escaped.push(character);
                }
                candidate = source.to_owned();
                candidate.replace_range(expanded.clone(), &escaped);
                let projection = Self::with_disclosure(
                    &candidate,
                    self.disclosure_after(&expanded, escaped.len()),
                );
                return ProjectedEdit {
                    range: expanded,
                    text: escaped,
                    selection: Selection {
                        anchor: projection.to_source(cursor.anchor, Affinity::After, None),
                        head: projection.to_source(cursor.head, Affinity::After, None),
                    },
                };
            }
        }
        let map = |offset: usize| {
            if offset < diff.new.start {
                self.to_source(offset, Affinity::Before, None)
            } else if offset <= diff.new.end {
                let inserted_offset = offset - diff.new.start;
                range.start
                    + inserted_offset
                    + if core_length == 0 {
                        closer_length + prefix_space.len()
                    } else if inserted_offset > core_length {
                        closer_length
                    } else {
                        0
                    }
            } else {
                let old_offset = offset - diff.new.len() + diff.old.len();
                let original = self.to_source(old_offset, Affinity::After, None);
                original.saturating_sub(range.len()) + replacement.len()
            }
        };
        let selection = Selection {
            anchor: map(cursor.anchor),
            head: map(cursor.head),
        };
        ProjectedEdit {
            range,
            text: replacement,
            selection,
        }
    }
}

impl OffsetMap {
    pub fn to_display(&self, offset: usize) -> usize {
        let index = self
            .pieces
            .partition_point(|piece| piece.source.end <= offset);
        match self.pieces.get(index) {
            Some(piece) if offset > piece.source.start && piece.identity => {
                piece.display.start + offset - piece.source.start
            }
            Some(piece) if offset > piece.source.start && piece.separator => piece.display.end,
            Some(piece) => piece.display.start,
            None => self.display_length,
        }
    }

    pub fn to_source(&self, offset: usize, affinity: Affinity, hint: Option<usize>) -> usize {
        let offset = offset.min(self.display_length);
        let index = self
            .pieces
            .partition_point(|piece| piece.display.end <= offset);
        if let Some(piece) = self.pieces.get(index)
            && offset > piece.display.start
        {
            return if piece.identity {
                piece.source.start + offset - piece.display.start
            } else {
                match affinity {
                    Affinity::Before => piece.source.start,
                    Affinity::After => piece.source.end,
                }
            };
        }
        let before = index
            .checked_sub(1)
            .map_or(0, |i| self.pieces[i].source.end);
        let after = self
            .pieces
            .get(index)
            .map_or(self.source_length, |piece| piece.source.start);
        if let Some(hint) = hint
            && (before..=after).contains(&hint)
        {
            return hint;
        }
        match affinity {
            Affinity::Before => before,
            Affinity::After => after,
        }
    }
}

fn project(
    source: &str,
    parsed: &Parsed,
    paint: bool,
    keep: impl Fn(&Decoration) -> bool,
) -> ProjectedText {
    let mut text = String::with_capacity(source.len());
    let mut pieces = Vec::new();
    let mut marks = if paint {
        parsed.marks.clone()
    } else {
        Vec::new()
    };
    let mut covered = 0;
    let mut push = |range: Range<usize>, replacement: Option<&str>| {
        let content = replacement.unwrap_or(&source[range.clone()]);
        if !content.is_empty() {
            let start = text.len();
            text.push_str(content);
            pieces.push(Piece {
                source: range,
                display: start..text.len(),
                identity: replacement.is_none(),
                separator: matches!(replacement, Some("\t" | "\n")),
            });
        }
    };
    for decoration in &parsed.decorations {
        if !keep(decoration) {
            if paint {
                marks.push(Mark {
                    range: decoration.range.clone(),
                    style: Style::SYNTAX,
                });
            }
            continue;
        }
        if decoration.range.start < covered {
            continue;
        }
        push(covered..decoration.range.start, None);
        push(decoration.range.clone(), Some(&decoration.text));
        covered = decoration.range.end;
    }
    push(covered..source.len(), None);
    let mapping = Arc::new(OffsetMap {
        pieces,
        source_length: source.len(),
        display_length: text.len(),
    });
    let highlights = Arc::new(if paint {
        highlight_lines(&text, &mapping, marks)
    } else {
        Vec::new()
    });
    let mut rich = parsed.rich.clone();
    rich.map_ranges(|offset| mapping.to_display(offset));
    ProjectedText {
        text,
        mapping,
        highlights,
        rich,
    }
}

fn highlight_lines(text: &str, mapping: &OffsetMap, marks: Vec<Mark>) -> HighlightLines {
    let mut events = Vec::new();
    for mark in marks {
        let start = mapping.to_display(mark.range.start);
        let end = mapping.to_display(mark.range.end);
        events.push((start, mark.style.trailing_zeros() as usize, 1_i32));
        events.push((end, mark.style.trailing_zeros() as usize, -1_i32));
    }
    events.sort_by_key(|event| event.0);
    let mut runs = Vec::new();
    let mut counters = [0_i32; u8::BITS as usize];
    let mut last = 0;
    for (position, kind, delta) in events {
        let style = Style(counters.iter().enumerate().fold(0, |flags, (i, &count)| {
            flags | if count > 0 { 1 << i } else { 0 }
        }));
        if position > last && style.0 != 0 {
            runs.push((last..position, style));
        }
        counters[kind] += delta;
        last = position;
    }
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in text.split('\n') {
        let end = offset + line.len();
        let first = runs.partition_point(|(range, _)| range.end <= offset);
        lines.push(
            runs[first..]
                .iter()
                .take_while(|(range, _)| range.start < end)
                .map(|(range, style)| {
                    (
                        range.start.max(offset) - offset..range.end.min(end) - offset,
                        *style,
                    )
                })
                .collect::<Vec<_>>()
                .into(),
        );
        offset = end + 1;
    }
    lines
}

fn closing_bracket(text: &str) -> Option<usize> {
    let mut depth = 0;
    let mut escaped = false;
    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' => escaped = true,
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{Affinity, Projection, Style};
    use crate::document::selection::Selection;

    fn edit(source: &str, after: &str, before: Selection, caret: usize) -> (String, Selection) {
        let projection = Projection::new(source);
        let edit = projection.edit(source, after, before, Selection::caret(caret));
        let mut source = source.to_owned();
        source.replace_range(edit.range, &edit.text);
        (source, edit.selection)
    }

    #[test]
    fn task_marker_queries_use_parser_metadata_after_disclosure_and_word_rebasing() {
        let source = include_str!("../../tests/fixtures/live-tasks.md").replace('\n', "\r\n");
        let mut projection = Projection::new(&source);
        let markers = projection.task_markers(0..source.len()).collect::<Vec<_>>();
        assert_eq!(markers.len(), 6);
        for marker in &markers {
            assert!(matches!(&source[marker.clone()], "[ ]" | "[x]" | "[X]"));
            assert_eq!(
                projection.task_marker_at(marker.start),
                Some(marker.clone())
            );
        }
        for text in ["which has", "This is literal", "A literal"] {
            assert!(
                projection
                    .task_marker_at(source.find(text).unwrap())
                    .is_none()
            );
        }
        let caret = source.find("café").unwrap();
        projection.set_disclosure(projection.disclosure_at(Some(caret)));
        assert_eq!(
            projection.task_markers(0..source.len()).collect::<Vec<_>>(),
            markers
        );
        let changed = source.replacen("Write", "Wriite", 1);
        assert!(projection.update_words(&changed, Some(caret + 1)));
        let fresh = Projection::with_context(&changed, Some(caret + 1));
        assert_eq!(
            projection
                .task_markers(0..changed.len())
                .collect::<Vec<_>>(),
            fresh.task_markers(0..changed.len()).collect::<Vec<_>>()
        );
        assert_eq!(projection.text, fresh.text);
    }

    #[test]
    fn parser_upgrade_keeps_wikilinks_and_superscripts_literal() {
        let source = include_str!("../../tests/fixtures/parser-compatibility.md").trim_end();
        let projection = Projection::new(source);
        assert_eq!(projection.text, source);
        assert!(projection.rich.images.is_empty());
        for offset in source
            .char_indices()
            .map(|(offset, _)| offset)
            .chain([source.len()])
        {
            assert_eq!(projection.to_display(offset), offset);
            assert_eq!(projection.to_source(offset, Affinity::Before, None), offset);
            assert_eq!(projection.to_source(offset, Affinity::After, None), offset);
        }
    }

    #[test]
    fn completed_markup_disappears_and_incomplete_markup_remains() {
        let projection = Projection::new(
            "# Title\n\n**bold** and *soft*, `code`, [link](https://example.com).\n\n**unfinished",
        );
        assert_eq!(
            projection.text,
            "Title\n\nbold and soft, code, link.\n\n**unfinished"
        );
    }

    #[test]
    fn active_heading_and_inline_span_retain_semantic_styles() {
        let source = "### **Title**\n\n**Body**";
        let projection = Projection::with_context(source, Some(6));
        assert_eq!(projection.text, "### **Title**\n\nBody");
        assert_eq!(projection.rich.headings[0].level, 3);
        let styles = &projection.highlights[0];
        assert!(
            styles
                .iter()
                .any(|(range, style)| range.contains(&0) && style.contains(Style::SYNTAX))
        );
        assert!(styles.iter().any(|(range, style)| range.contains(&6)
            && style.contains(Style::BOLD)
            && !style.contains(Style::SYNTAX)));
        for offset in 0..=13 {
            assert_eq!(projection.to_display(offset), offset);
            assert_eq!(
                projection.normalize_caret(Selection::caret(offset)),
                Selection::caret(offset)
            );
        }
    }

    #[test]
    fn only_the_innermost_span_under_the_caret_is_disclosed() {
        let source = "## A **bold *nested* word** and [link](https://example.com) `code`";
        for (word, expected) in [
            ("A ", "## A bold nested word and link code"),
            ("bold", "## A **bold nested word** and link code"),
            ("nested", "## A bold *nested* word and link code"),
            (
                "link",
                "## A bold nested word and [link](https://example.com) code",
            ),
            ("code", "## A bold nested word and link `code`"),
        ] {
            let projection = Projection::with_context(source, Some(source.find(word).unwrap()));
            assert_eq!(projection.text, expected, "at {word}");
            for (display, _) in projection.text.char_indices() {
                let raw = projection.to_source(display, Affinity::After, None);
                assert!(source.is_char_boundary(raw));
                assert_eq!(projection.to_display(raw), display);
            }
        }
    }

    #[test]
    fn completed_span_conceals_at_its_end_and_autolinks_disclose() {
        let source = "**x** <https://example.com>";
        for head in 0..5 {
            assert_eq!(
                Projection::with_context(source, Some(head)).text,
                "**x** https://example.com"
            );
        }
        assert_eq!(
            Projection::with_context(source, Some(5)).text,
            "x https://example.com"
        );
        assert_eq!(
            Projection::with_context(source, Some(8)).text,
            "x <https://example.com>"
        );
        assert_eq!(
            Projection::with_context(source, Some(source.len())).text,
            "x https://example.com"
        );
    }

    #[test]
    fn active_table_rows_and_images_retain_their_rich_structure() {
        let source = "| **A** | B |\n|---|---|\n| x | y |\n\n![alt](room.png)";
        let concealed = Projection::new(source);
        let projection = Projection::with_context(source, Some(6));
        let table = &projection.rich.tables[0];
        assert_eq!(table.alignment.len(), 2);
        assert_eq!(&projection.text[table.rows[0][0].clone()], "**A**");
        assert_eq!(&projection.text[table.rows[1][0].clone()], "x");
        let start = source.find("![").unwrap();
        let projection = Projection::with_context(source, Some(start));
        assert_eq!(projection.text, concealed.text);
        assert_eq!(projection.rich.images.len(), 1);
    }

    #[test]
    fn nested_styles_links_and_unicode_map_without_phantom_cursor_positions() {
        let source = "# 日本語\n\n**A *🦀* word** [link **label**](https://example.com)";
        let projection = Projection::new(source);
        assert_eq!(projection.text, "日本語\n\nA 🦀 word link label");
        for (display, _) in projection.text.char_indices() {
            let raw = projection.to_source(display, Affinity::After, None);
            assert!(source.is_char_boundary(raw));
            assert_eq!(projection.to_display(raw), display);
        }
    }

    #[test]
    fn typing_after_a_completed_span_stays_outside_but_typing_inside_inherits_style() {
        let (source, caret) = edit("**bold", "**bold**", Selection::caret(6), 8);
        assert_eq!(source, "**bold**");
        let (source, _) = edit(&source, "bold!", caret, 5);
        assert_eq!(source, "**bold**!");
        let (source, _) = edit("**bold**", "bold!", Selection::caret(6), 5);
        assert_eq!(source, "**bold!**");
    }

    #[test]
    fn deleting_last_styled_character_removes_empty_delimiters() {
        let (source, _) = edit("**x**", "", Selection::caret(3), 0);
        assert_eq!(source, "");
    }

    #[test]
    fn cross_span_deletion_repairs_surviving_formatting() {
        let (source, _) = edit(
            "**hello world** end",
            "hello X",
            Selection {
                anchor: 8,
                head: 19,
            },
            7,
        );
        assert_eq!(source, "**hello X**");
        let (source, _) = edit(
            "start **hello world**",
            "X world",
            Selection {
                anchor: 0,
                head: 13,
            },
            1,
        );
        assert_eq!(source, "X **world**");
    }

    #[test]
    fn full_document_selection_replaces_hidden_headings_and_delimiters() {
        let source = "# Title\n\n**body**";
        let (source, _) = edit(
            source,
            "New",
            Selection {
                anchor: 0,
                head: source.len(),
            },
            3,
        );
        assert_eq!(source, "New");
    }

    #[test]
    fn block_boundaries_are_normal_editable_newlines() {
        let source = "# First\n\nSecond\n";
        let (source, _) = edit(source, "FirstSecond\n", Selection { anchor: 7, head: 9 }, 5);
        assert_eq!(source, "# FirstSecond\n");
    }

    #[test]
    fn crlf_fences_and_literal_escapes_keep_lossless_source_mapping() {
        let source = "# Hi\r\n\r\n\\*literal\\*\r\n\r\n```rust\r\nlet x = 1;\r\n```\r\n";
        let projection = Projection::new(source);
        assert_eq!(projection.text, "Hi\n\n*literal*\n\n\nlet x = 1;\n\n");
        assert_eq!(projection.to_source(0, Affinity::After, None), 2);
    }
    #[test]
    fn literal_backslashes_and_fenced_code_are_not_accidentally_concealed() {
        assert_eq!(Projection::new(r"\\*literal").text, r"\*literal");
        assert_eq!(
            Projection::new("```\n\\*literal\\*\n```\n").text,
            "\n\\*literal\\*\n\n"
        );
    }

    #[test]
    fn inline_formatting_preserves_blocks_and_toggles_exact_marks() {
        let source = "# Title\n\nBody\n\n- Item";
        let projection = Projection::new(source);
        let selection = Selection {
            anchor: 0,
            head: projection.text.len(),
        };
        let edit = projection
            .format_inline(source, selection, "**")
            .expect("nonempty selection");
        let mut result = source.to_owned();
        result.replace_range(edit.range, &edit.text);
        assert_eq!(result, "# **Title**\n\n**Body**\n\n- **Item**");
        let projection = Projection::new(&result);
        let edit = projection
            .format_inline(&result, selection, "**")
            .expect("marked selection");
        result.replace_range(edit.range, &edit.text);
        assert_eq!(result, source);
    }

    #[test]
    fn replacements_over_every_visible_range_preserve_the_unselected_text() {
        for source in [
            "**hello *world*** end",
            "# 日本語\n\n**café 🦀**",
            "before [link](https://example.com) after",
            "`code` and text",
        ] {
            let projection = Projection::new(source);
            let boundaries = projection
                .text
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(projection.text.len()))
                .collect::<Vec<_>>();
            for &start in &boundaries {
                for &end in boundaries.iter().filter(|&&end| end > start) {
                    let selection = Selection {
                        anchor: projection.to_source(start, Affinity::After, None),
                        head: projection.to_source(end, Affinity::Before, None),
                    };
                    let after =
                        format!("{}X{}", &projection.text[..start], &projection.text[end..]);
                    let edit =
                        projection.edit(source, &after, selection, Selection::caret(start + 1));
                    let mut result = source.to_owned();
                    result.replace_range(edit.range, &edit.text);
                    assert_eq!(
                        Projection::new(&result).text,
                        after,
                        "source={source:?}, selected={start}..{end}, result={result:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn rich_table_hides_structure_and_maps_every_cell_to_its_source() {
        let source = "# Title\n\n| Name | **Value** |\n| :--- | ---: |\n| café | 42 |\n| empty |  |\n\nAfter";
        let projection = Projection::new(source);
        assert_eq!(
            projection.text,
            "Title\n\nName\tValue\ncafé\t42\nempty\t\n\nAfter"
        );
        let table = &projection.rich.tables[0];
        assert_eq!(table.rows.len(), 3);
        for (row, expected) in
            table
                .rows
                .iter()
                .zip([["Name", "Value"], ["café", "42"], ["empty", ""]])
        {
            for (cell, expected) in row.iter().zip(expected) {
                assert_eq!(&projection.text[cell.clone()], expected);
                assert!(source.is_char_boundary(projection.to_source(
                    cell.start,
                    Affinity::After,
                    None
                )));
            }
        }
    }

    #[test]
    fn images_are_atomic_and_deletion_and_replacement_preserve_neighbors() {
        let source = "Before\n\n![A **quiet** place](images/room.png \"Room\")\n\nAfter";
        let projection = Projection::new(source);
        assert_eq!(projection.text, "Before\n\n\u{fffc}\n\nAfter");
        assert_eq!(projection.rich.images[0].alt, "A quiet place");
        let image = &projection.rich.images[0];
        let selected = projection.source_selection(
            Selection {
                anchor: image.range.start,
                head: image.range.end,
            },
            Selection::default(),
        );
        let after = projection.text.replace('\u{fffc}', "");
        let edit = projection.edit(
            source,
            &after,
            selected,
            Selection::caret(image.range.start),
        );
        let mut result = source.to_owned();
        result.replace_range(edit.range, &edit.text);
        assert_eq!(result, "Before\n\n\n\nAfter");
    }

    #[test]
    fn tables_accept_empty_ragged_escaped_and_nested_cells() {
        for source in [
            "| A | B |\n|---|---|\n| one |\n",
            "A|B\n--|--\n|\n",
            "| | |\n|---|---|\n| | |\n",
            "| A | B |\r\n|---|---|\r\n| a\\|b | **c** |\r\n",
            "> | A | B |\n> |---|---|\n> | x | y |\n",
        ] {
            let projection = Projection::new(source);
            assert_eq!(projection.rich.tables.len(), 1, "{source:?}");
            for table in &projection.rich.tables {
                for cell in table.rows.iter().flatten() {
                    assert!(
                        cell.start <= cell.end && cell.end <= projection.text.len(),
                        "{source:?}: {cell:?}"
                    );
                }
            }
        }
    }
}
