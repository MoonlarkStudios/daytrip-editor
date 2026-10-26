//! Portable Markdown for writing tools. Dialog targets are stable, checked source
//! ranges; they never authorize replacing text that changed while a dialog was open.

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::document::block_parser::MARKDOWN_OPTIONS;
use crate::document::collaboration::{Collaboration, StableSelection};
use crate::document::selection::Selection;

#[derive(Debug, Clone)]
pub struct Target {
    owner: u64,
    selection: StableSelection,
    expected: String,
}
impl Target {
    pub fn new(document: &Collaboration, source: &str, range: Range<usize>) -> Self {
        Self {
            owner: document.replica_id(),
            selection: document.selection(Selection {
                anchor: range.start,
                head: range.end,
            }),
            expected: source[range].to_owned(),
        }
    }
    pub fn resolve(&self, document: &Collaboration, source: &str) -> Result<Range<usize>, String> {
        if self.owner != document.replica_id() {
            return Err("This edit belongs to an earlier document session.".into());
        }
        let selection = document
            .resolve_selection(&self.selection)
            .ok_or("This passage is no longer available.")?;
        let range = selection.range();
        if source.get(range.clone()) != Some(self.expected.as_str()) {
            return Err("This passage changed while its details were open. Close and reopen the details to keep both edits.".into());
        }
        Ok(range)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Details {
    pub range: Range<usize>,
    pub label: String,
    pub destination: String,
    pub title: String,
    pub original_label: Option<(String, String)>,
    pub caption: String,
    pub credit: String,
    pub credit_url: String,
}

/// Parse only when a writing command is invoked, never during layout or typing.
pub fn inline_at(source: &str, offset: usize, image: bool) -> Option<Details> {
    let mut current: Option<Details> = None;
    let mut label_end = 0;
    let mut image_depth = 0;
    for (event, range) in Parser::new_ext(source, MARKDOWN_OPTIONS).into_offset_iter() {
        let ending = matches!(&event, Event::End(TagEnd::Link) if !image)
            || matches!(&event, Event::End(TagEnd::Image) if image && image_depth == 1);
        if current.is_some() && !ending {
            label_end = label_end.max(range.end);
        }
        match event {
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) if image => {
                image_depth += 1;
                if image_depth > 1 {
                    continue;
                }
                label_end = range.start + 2;
                current = Some(Details {
                    range,
                    destination: dest_url.into_string(),
                    title: title.into_string(),
                    ..Details::default()
                });
            }
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) if !image => {
                label_end = range.start + 1;
                current = Some(Details {
                    range,
                    destination: dest_url.into_string(),
                    title: title.into_string(),
                    ..Details::default()
                });
            }
            Event::Text(text) | Event::Code(text) => {
                if let Some(details) = &mut current {
                    details.label.push_str(&text);
                }
            }
            Event::End(TagEnd::Image) if image => {
                image_depth -= 1;
                if image_depth > 0 {
                    continue;
                }
                if let Some(mut details) = current.take() {
                    remember_label(source, &mut details, label_end, true);
                    let details = figure_details(source, details);
                    if details.range.start <= offset && offset <= details.range.end {
                        return Some(details);
                    }
                    if offset > details.range.end
                        && offset - details.range.end <= 4
                        && source
                            .get(details.range.end..offset)
                            .is_some_and(|gap| gap.trim().is_empty())
                    {
                        return Some(details);
                    }
                }
            }
            Event::End(TagEnd::Link) if !image => {
                if let Some(mut details) = current.take()
                    && (details.range.start <= offset && offset <= details.range.end)
                {
                    remember_label(source, &mut details, label_end, false);
                    return Some(details);
                }
            }
            _ => {}
        }
    }
    None
}

fn remember_label(source: &str, details: &mut Details, end: usize, image: bool) {
    let prefix = if image { "![" } else { "[" };
    if source[details.range.clone()].starts_with(prefix)
        && end < details.range.end
        && let Some(raw) = source.get(details.range.start + prefix.len()..end)
    {
        details.original_label = Some((details.label.clone(), raw.into()));
    }
}

/// Destination-only edits retain the existing label's inline formatting and title.
pub fn details_link(details: &Details) -> String {
    let raw = details
        .original_label
        .as_ref()
        .filter(|(label, _)| label == &details.label)
        .map(|(_, raw)| raw.as_str());
    let mut result = link(&details.label, &details.destination);
    if let Some(raw) = raw {
        let escaped = escape(&details.label);
        result.replace_range(1..1 + escaped.len(), raw);
    }
    if !details.title.is_empty() {
        result.pop();
        let title = details
            .title
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace(['\r', '\n'], " ");
        result.push_str(&format!(" \"{title}\")"));
    }
    result
}

fn figure_details(source: &str, mut details: Details) -> Details {
    // Only consume the exact standalone conventions emitted by these controls.
    // Ordinary prose following an image remains outside the edit target.
    let tail = &source[details.range.end..];
    let newline = if tail.starts_with("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let gap = newline.repeat(2);
    let mut consumed = 0;
    if let Some(rest) = tail.strip_prefix(&gap) {
        let line = rest.split(newline).next().unwrap_or("");
        if line.starts_with('*') && line.ends_with('*') && line.len() > 2 && !line.starts_with("**")
        {
            details.caption = unescape(&line[1..line.len() - 1]);
            consumed = gap.len() + line.len();
        }
    }
    if let Some(rest) = tail[consumed..].strip_prefix(&gap) {
        let line = rest.split(newline).next().unwrap_or("");
        if let Some(credit) = line.strip_prefix("Photo: ") {
            if let Some(link) = inline_at(credit, 0, false)
                && link.range.end == credit.len()
            {
                details.credit = link.label;
                details.credit_url = link.destination;
            } else {
                details.credit = unescape(credit);
            }
            consumed += gap.len() + line.len();
        }
    }
    details.range.end += consumed;
    details
}

pub fn safe_destination(destination: &str) -> bool {
    let value = destination.trim();
    !value.is_empty()
        && !value.chars().any(char::is_control)
        && (!value.contains(':')
            || value.starts_with("https://")
            || value.starts_with("http://")
            || value.starts_with("mailto:")
            || value.starts_with("file://"))
}

pub fn escape(text: &str) -> String {
    let mut result = String::new();
    for character in text.chars() {
        if "\\`*_{}[]<>()!#|".contains(character) {
            result.push('\\');
        }
        if character == '\n' || character == '\r' {
            result.push(' ');
        } else {
            result.push(character);
        }
    }
    result
}
fn unescape(text: &str) -> String {
    // Parse text escapes using the same Markdown implementation as the editor.
    Parser::new(text)
        .filter_map(|event| match event {
            Event::Text(text) | Event::Code(text) => Some(text.into_string()),
            _ => None,
        })
        .collect()
}
pub fn link(label: &str, destination: &str) -> String {
    let destination = destination
        .trim()
        .replace('\\', "%5C")
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace(' ', "%20");
    format!("[{}](<{destination}>)", escape(label))
}
pub fn figure(details: &Details, newline: &str) -> String {
    let mut result = format!("!{}", details_link(details));
    if !details.caption.trim().is_empty() {
        result.push_str(&format!(
            "{newline}{newline}*{}*",
            escape(details.caption.trim())
        ));
    }
    if !details.credit.trim().is_empty() {
        let credit = if details.credit_url.trim().is_empty() {
            escape(details.credit.trim())
        } else {
            link(details.credit.trim(), &details.credit_url)
        };
        result.push_str(&format!("{newline}{newline}Photo: {credit}"));
    }
    result
}
pub fn quote(body: &str, author: &str, title: &str, destination: &str, newline: &str) -> String {
    let mut lines: Vec<String> = body.lines().map(|line| format!("> {line}")).collect();
    let source = if destination.trim().is_empty() {
        escape(title.trim())
    } else {
        link(
            if title.trim().is_empty() {
                destination.trim()
            } else {
                title.trim()
            },
            destination,
        )
    };
    let author = escape(author.trim());
    let credit = [author, source]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    if !credit.is_empty() {
        lines.push(">".into());
        lines.push(format!("> — {credit}"));
    }
    lines.join(newline)
}
pub fn newline(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// Existing block quotations stay editable through the same attribution tool.
pub fn quote_at(source: &str, offset: usize) -> Option<(Details, String)> {
    for (event, mut range) in Parser::new_ext(source, MARKDOWN_OPTIONS).into_offset_iter() {
        if matches!(event, Event::Start(Tag::BlockQuote(_))) && range.contains(&offset) {
            let raw = source[range.clone()].trim_end_matches(['\r', '\n']);
            range.end = range.start + raw.len();
            let mut lines: Vec<String> = raw
                .lines()
                .map(|line| {
                    line.strip_prefix("> ")
                        .or_else(|| line.strip_prefix('>'))
                        .unwrap_or(line)
                        .to_owned()
                })
                .collect();
            let mut details = Details {
                range,
                ..Details::default()
            };
            if let Some(credit) = lines
                .last()
                .and_then(|line| line.strip_prefix("— "))
                .map(str::to_owned)
            {
                if let Some(start) = credit
                    .find('[')
                    .filter(|start| *start == 0 || credit[..*start].ends_with(", "))
                    && let Some(link) = inline_at(&credit, start, false)
                    && link.range.end == credit.len()
                {
                    details.credit = unescape(credit[..start].trim_end_matches(", "));
                    details.label = link.label;
                    details.credit_url = link.destination;
                } else {
                    details.credit = unescape(&credit);
                }
                lines.pop();
                if lines.last().is_some_and(|line| line.is_empty()) {
                    lines.pop();
                }
            }
            return Some((details, lines.join(newline(source))));
        }
    }
    None
}

pub fn heading_slug(label: &str) -> String {
    label
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '-' || *c == '_')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Document;
    use crate::document::collaboration::{EditOrigin, SourceEdit};

    #[test]
    fn portable_figures_and_links_round_trip_unicode_and_delimiters() {
        let details = Details {
            label: "海 [dawn] 🦀".into(),
            destination: "note.assets/a b.png".into(),
            caption: "A *quiet* coast.".into(),
            credit: "A [name]".into(),
            credit_url: "https://example.com/?a=(b)".into(),
            ..Details::default()
        };
        let source = format!("{}\r\n\r\nOrdinary prose.", figure(&details, "\r\n"));
        let found = inline_at(&source, 3, true).unwrap();
        assert_eq!(found.label, details.label);
        assert_eq!(found.destination, "note.assets/a%20b.png");
        assert_eq!(found.caption, details.caption);
        assert_eq!(found.credit, details.credit);
        assert_eq!(&source[found.range.end..], "\r\n\r\nOrdinary prose.");
        assert!(inline_at("`[code](https://example.com)`", 4, false).is_none());
        assert!(!safe_destination("javascript:alert(1)"));
        assert!(!safe_destination("https://example.com\nmalicious"));
    }

    #[test]
    fn changing_a_destination_retains_formatted_labels_and_markdown_titles() {
        for source in [
            "[A **bold** `]` label](<https://old.example> \"Helpful title\")",
            "![A *coast*](<image.png> \"At dawn\")",
        ] {
            let image = source.starts_with('!');
            let mut details = inline_at(source, 4, image).unwrap();
            details.destination = "https://new.example/".into();
            let new = if image {
                figure(&details, "\n")
            } else {
                details_link(&details)
            };
            assert_eq!(
                new,
                source
                    .replace("https://old.example", "https://new.example/")
                    .replace("image.png", "https://new.example/")
            );
        }
    }
    #[test]
    fn dialog_targets_rebase_unrelated_changes_and_reject_overlap_and_reopen() {
        let mut doc = Document::new("# Title\n\nA [link](https://example.com).".into());
        let link = inline_at(doc.source(), 13, false).unwrap();
        let target = Target::new(&doc.collaboration, doc.source(), link.range.clone());
        doc.transact(
            &[SourceEdit {
                range: 0..0,
                text: "Intro\n\n".into(),
            }],
            EditOrigin::Human,
            None,
            true,
        )
        .unwrap();
        let rebased = target.resolve(&doc.collaboration, doc.source()).unwrap();
        assert_eq!(
            &doc.source()[rebased.clone()],
            "[link](https://example.com)"
        );
        doc.transact(
            &[SourceEdit {
                range: rebased.start + 1..rebased.start + 5,
                text: "new".into(),
            }],
            EditOrigin::Agent,
            None,
            true,
        )
        .unwrap();
        assert!(target.resolve(&doc.collaboration, doc.source()).is_err());
        let reopened = Document::from_state(&doc.collaboration.state()).unwrap();
        assert!(
            target
                .resolve(&reopened.collaboration, reopened.source())
                .is_err()
        );
    }
}
