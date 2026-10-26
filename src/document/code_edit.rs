//! Source-coordinate code editing. Fences are never included in indentation edits.

use iced::widget::text_editor::Edit;

use super::projection::ProjectedEdit;
use super::rich_document::RichDocument;
use super::selection::Selection;

/// A caret immediately before a closing fence needs a content row. Keep the
/// closing fence on its own line when typing, pasting, or committing an IME.
pub fn insert_at_fence(
    source: &str,
    rich: &RichDocument,
    selection: Selection,
    action: &Edit,
) -> Option<ProjectedEdit> {
    let block = rich.source_code_at(selection.head).filter(|block| {
        block.fenced
            && block.source_body.end == selection.head
            && block.source_body.end < block.source_range.end
            && selection.is_caret()
    })?;
    let text = match action {
        Edit::Insert(c) => c.to_string(),
        Edit::Paste(text) => text.as_ref().clone(),
        Edit::Indent => indentation(&block.text, &block.language),
        Edit::Enter => {
            if source.contains("\r\n") {
                "\r\n".into()
            } else {
                "\n".into()
            }
        }
        _ => return None,
    };
    let head = selection.head + text.len();
    if text.is_empty() {
        return None;
    }
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    Some(ProjectedEdit {
        range: selection.range(),
        text: if text.ends_with('\n') {
            text
        } else {
            format!("{text}{newline}")
        },
        selection: Selection::caret(head),
    })
}

pub fn edit(
    source: &str,
    rich: &RichDocument,
    selection: Selection,
    action: &Edit,
) -> Option<ProjectedEdit> {
    let selected = selection.range();
    let block = rich.source_code_at(selected.start).filter(|block| {
        block.source_body.start <= selected.start
            && selected.end <= block.source_body.end
            && selected.start < block.source_range.end
    })?;
    let line_start = source[..selected.start].rfind('\n').map_or(0, |i| i + 1);
    if line_start < block.source_body.start {
        return None;
    }
    let unit = indentation(&block.text, &block.language);
    let prefix = &source[line_start..selected.start];
    // Container markers need their own continuation rules. Keep their native
    // edit path rather than interpreting a quote marker as code indentation.
    if prefix.trim_start().starts_with('>') {
        return None;
    }
    let indent = &prefix[..prefix
        .bytes()
        .take_while(|b| matches!(b, b' ' | b'\t'))
        .count()];
    let replace = |range: std::ops::Range<usize>, text: String| ProjectedEdit {
        selection: Selection::caret(range.start + text.len()),
        range,
        text,
    };
    match action {
        Edit::Enter => {
            let newline = if source[..selected.start].ends_with('\r') || source.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            let last = prefix.trim_end().chars().last();
            let opens = matches!(last, Some('{' | '[' | '('))
                || last == Some(':')
                    && matches!(block.language.as_str(), "python" | "py" | "yaml" | "yml");
            let mut text = format!("{newline}{indent}{}", if opens { &unit } else { "" });
            let head = selected.start + text.len();
            let closing = source[selected.end..].chars().next();
            if matches!(
                (last, closing),
                (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')'))
            ) {
                text.push_str(&format!("{newline}{indent}"));
            }
            Some(ProjectedEdit {
                range: selected,
                text,
                selection: Selection::caret(head),
            })
        }
        Edit::Indent if selection.is_caret() && selected.start < block.source_body.end => {
            let count = if unit == "\t" {
                1
            } else {
                unit.len() - prefix.chars().count() % unit.len()
            };
            Some(replace(
                selected,
                if unit == "\t" {
                    unit
                } else {
                    " ".repeat(count)
                },
            ))
        }
        Edit::Indent | Edit::Unindent => {
            let last = if selected.end > selected.start && source[..selected.end].ends_with('\n') {
                selected.end - 1
            } else {
                selected.end
            };
            let line_end = source[last..].find('\n').map_or(source.len(), |i| last + i);
            if line_end > block.source_body.end || last >= block.source_body.end {
                return None;
            }
            let mut text = String::new();
            let mut edits = Vec::new();
            let mut offset = line_start;
            for line in source[line_start..line_end].split_inclusive('\n') {
                if matches!(action, Edit::Indent) {
                    edits.push((offset, 0, unit.len()));
                    text.push_str(&unit);
                    text.push_str(line);
                } else {
                    let removed = if line.starts_with('\t') {
                        1
                    } else {
                        line.bytes()
                            .take(unit.len())
                            .take_while(|&b| b == b' ')
                            .count()
                    };
                    edits.push((offset, removed, 0));
                    text.push_str(&line[removed..]);
                }
                offset += line.len();
            }
            let map = |point: usize| {
                let mut mapped = point;
                for &(at, removed, added) in &edits {
                    if point >= at {
                        mapped = mapped - removed.min(point - at) + added;
                    }
                }
                mapped
            };
            Some(ProjectedEdit {
                range: line_start..line_end,
                text,
                selection: Selection {
                    anchor: map(selection.anchor),
                    head: map(selection.head),
                },
            })
        }
        Edit::Backspace
            if selection.is_caret()
                && !prefix.is_empty()
                && prefix == indent
                && prefix.ends_with(' ') =>
        {
            let width = if unit == "\t" { 4 } else { unit.len() };
            let count = (prefix.len() - 1) % width + 1;
            let spaces = prefix.bytes().rev().take_while(|&b| b == b' ').count();
            Some(replace(
                selected.start - count.min(spaces)..selected.start,
                String::new(),
            ))
        }
        _ => None,
    }
}

fn indentation(code: &str, language: &str) -> String {
    let mut spaces = usize::MAX;
    for line in code.lines().filter(|line| !line.trim().is_empty()) {
        if line.starts_with('\t') {
            return "\t".into();
        }
        let count = line.bytes().take_while(|&b| b == b' ').count();
        if count > 0 {
            spaces = spaces.min(count);
        }
    }
    let fallback = if matches!(
        language,
        "js" | "javascript" | "ts" | "typescript" | "json" | "yaml" | "yml" | "html" | "css"
    ) {
        2
    } else {
        4
    };
    " ".repeat(if spaces <= 8 { spaces } else { fallback })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::projection::Projection;

    #[test]
    fn enter_indents_pairs_and_preserves_crlf() {
        let source = "```rust\r\nfn main() {}\r\n```\r\n";
        let projection = Projection::new(source);
        let at = source.find('}').unwrap();
        let result = edit(source, &projection.rich, Selection::caret(at), &Edit::Enter).unwrap();
        assert_eq!(result.text, "\r\n    \r\n");
        assert_eq!(result.selection, Selection::caret(at + 6));
    }

    #[test]
    fn block_indent_preserves_reverse_utf8_selection_and_excludes_end_line() {
        let source = "```python\nα = 1\nβ = 2\n```\nTail";
        let projection = Projection::new(source);
        let start = source.find('α').unwrap();
        let end = source.rfind("```").unwrap();
        let before = Selection {
            anchor: end,
            head: start,
        };
        let result = edit(source, &projection.rich, before, &Edit::Indent).unwrap();
        assert_eq!(result.text, "    α = 1\n    β = 2");
        assert!(result.selection.anchor > result.selection.head);
        let mut after = source.to_owned();
        after.replace_range(result.range, &result.text);
        assert!(after.ends_with("\n```\nTail"));
        let projection = Projection::new(&after);
        let restored = edit(&after, &projection.rich, result.selection, &Edit::Unindent).unwrap();
        after.replace_range(restored.range, &restored.text);
        assert_eq!(after, source);
        assert_eq!(restored.selection.head, start);
        assert_eq!(restored.selection.anchor, end);
    }
}
