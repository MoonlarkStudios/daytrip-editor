//! CommonMark block boundaries and conservative, independently parseable islands.

use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag};

/// Proof that one isolated source window was reparsed for this predecessor.
#[derive(Debug)]
pub(crate) struct BlockChange {
    pub previous_revision: u64,
    pub old: Range<usize>,
    pub new: Range<usize>,
}

pub(super) struct Span {
    pub range: Range<usize>,
    pub independent: bool,
}

pub(super) struct Scan {
    pub spans: Vec<Span>,
    pub global_dependencies: bool,
}

pub(super) fn scan(source: &str) -> Scan {
    let parser = Parser::new_ext(source, Options::all());
    let mut global_dependencies = parser.reference_definitions().iter().next().is_some();
    let mut ranges = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    let mut independent = false;
    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(tag) => {
                global_dependencies |= matches!(tag, Tag::FootnoteDefinition(_));
                if depth == 0 {
                    start = range.start;
                    independent =
                        matches!(tag, Tag::Paragraph | Tag::Heading { .. } | Tag::Table(_))
                            && unindented(source, start);
                }
                depth += 1;
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    ranges.push(Span {
                        range: start..range.end,
                        independent,
                    });
                }
            }
            Event::Rule | Event::Html(_) if depth == 0 => ranges.push(Span {
                independent: matches!(event, Event::Rule) && unindented(source, range.start),
                range,
            }),
            Event::FootnoteReference(_) => global_dependencies = true,
            _ => {}
        }
    }
    // Malformed definitions can produce overlapping top-level offset ranges.
    // Keep their union intact, never certifying it as an independent island.
    ranges.sort_by_key(|span| span.range.start);
    let mut disjoint: Vec<Span> = Vec::new();
    for span in ranges {
        if let Some(last) = disjoint.last_mut()
            && span.range.start < last.range.end
        {
            last.range.end = last.range.end.max(span.range.end);
            last.independent = false;
        } else {
            disjoint.push(span);
        }
    }
    // Invisible source (definitions, for example) remains an editable block.
    let mut covered = 0;
    let mut spans = Vec::new();
    for span in disjoint {
        if !source[covered..span.range.start].trim().is_empty() {
            spans.push(Span {
                range: covered..span.range.start,
                independent: false,
            });
        }
        covered = span.range.end;
        spans.push(span);
    }
    if !source[covered..].trim().is_empty() || spans.is_empty() {
        spans.push(Span {
            range: covered..source.len(),
            independent: false,
        });
    }
    Scan {
        spans,
        global_dependencies,
    }
}

fn at_line_start(source: &str, offset: usize) -> bool {
    offset == 0 || source.as_bytes()[offset - 1] == b'\n'
}

fn unindented(source: &str, offset: usize) -> bool {
    at_line_start(source, offset)
        && source
            .as_bytes()
            .get(offset)
            .is_some_and(|byte| !matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b':'))
}

/// Blank lines isolate non-container blocks. Indentation, definitions, HTML,
/// fences and lists need the full parser: their effects can cross this window.
pub(super) fn island(source: &str, range: &Range<usize>) -> Option<Range<usize>> {
    if !at_line_start(source, range.start) {
        return None;
    }
    if range.start > 0 {
        let previous = source[..range.start]
            .strip_suffix('\n')?
            .rsplit('\n')
            .next()?;
        if !blank(previous) {
            return None;
        }
    }
    let ending = &source[range.end..];
    let end = if ending.starts_with("\r\n") {
        range.end + 2
    } else if ending.starts_with('\n') {
        range.end + 1
    } else if ending.is_empty() {
        range.end
    } else {
        return None;
    };
    if end < source.len() && !blank(source[end..].split('\n').next()?) {
        return None;
    }
    Some(range.start..end)
}

fn blank(line: &str) -> bool {
    line.bytes()
        .all(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
}
