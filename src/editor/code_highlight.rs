//! Bounded, theme-aware syntax runs shared by live and reading code surfaces.

use crate::document::projection::difference;
use crate::theme::Tokens;
use iced::advanced::text::Highlighter as _;
use iced::{Color, Font};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct Run {
    pub range: Range<usize>,
    pub color: [u8; 4],
    pub font: Font,
}

#[derive(Default)]
pub(super) struct Cache {
    entries: Vec<Block>,
    bytes: usize,
}

struct Block {
    language: String,
    code: String,
    starts: Vec<usize>,
    parser: iced::highlighter::Highlighter,
    lines: Vec<Vec<Run>>,
}

impl Cache {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.bytes = 0;
    }

    pub fn prepare(&mut self, language: &str, code: &str, tokens: Tokens) -> Option<usize> {
        // Bound regex input independently of document admission. Oversized code
        // remains selectable and editable with the semantic monospace style.
        if language.is_empty()
            || code.len() > 256 * 1024
            || code.lines().any(|line| line.len() > 8192)
        {
            return None;
        }
        if let Some(index) = self
            .entries
            .iter()
            .position(|block| block.language == language && block.code == code)
        {
            return Some(index);
        }
        // Reuse a nearby revision, rewinding to its preceding parser snapshot.
        // Later styles are invalidated even when their text is unchanged: a
        // multiline comment, for example, can change that entire lexical context.
        for (index, block) in self.entries.iter_mut().enumerate() {
            if block.language != language {
                continue;
            }
            let change = difference(&block.code, code);
            if change.old.len() + change.new.len() > 1024
                || self.bytes - block.code.len() + code.len() > 512 * 1024
            {
                continue;
            }
            let line = block.code[..change.old.start]
                .bytes()
                .filter(|&b| b == b'\n')
                .count();
            if line < block.parser.current_line() {
                block.parser.change_line(line);
            }
            block.lines.truncate(block.parser.current_line());
            self.bytes = self.bytes - block.code.len() + code.len();
            block.code.replace_range(change.old, &code[change.new]);
            block.starts = line_starts(code);
            return Some(index);
        }
        if self.entries.len() >= 32 || self.bytes + code.len() > 512 * 1024 {
            self.clear();
        }
        self.bytes += code.len();
        self.entries.push(Block {
            language: language.into(),
            code: code.into(),
            starts: line_starts(code),
            parser: iced::highlighter::Highlighter::new(&settings(language, tokens)),
            lines: Vec::new(),
        });
        Some(self.entries.len() - 1)
    }

    pub fn line(&mut self, entry: usize, index: usize, tokens: Tokens) -> Vec<Run> {
        let block = &mut self.entries[entry];
        // Stop at the requested resident line. Snapshots and prior styles survive
        // scrolling and small edits; offscreen suffixes wait until needed.
        while block.parser.current_line() <= index {
            let current = block.parser.current_line();
            let Some(&start) = block.starts.get(current) else {
                break;
            };
            let end = block
                .starts
                .get(current + 1)
                .map_or(block.code.len(), |at| at - 1);
            let runs = block
                .parser
                .highlight_line(&block.code[start..end])
                .map(|(range, highlight)| format_run(range, highlight, tokens))
                .collect();
            block.lines.push(runs);
        }
        block.lines.get(index).cloned().unwrap_or_default()
    }
}

fn line_starts(code: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(code.match_indices('\n').map(|(at, _)| at + 1))
        .collect()
}

fn settings(language: &str, tokens: Tokens) -> iced::highlighter::Settings {
    iced::highlighter::Settings {
        theme: if luminance(tokens.code) < 0.4 {
            iced::highlighter::Theme::Base16Ocean
        } else {
            iced::highlighter::Theme::InspiredGitHub
        },
        token: match language.to_ascii_lowercase().as_str() {
            "javascript" | "jsx" => "js".into(),
            "typescript" | "tsx" => "ts".into(),
            "shell" | "sh" => "bash".into(),
            "yml" => "yaml".into(),
            "py" => "python".into(),
            other => other.into(),
        },
    }
}

fn format_run(range: Range<usize>, highlight: iced::highlighter::Highlight, tokens: Tokens) -> Run {
    Run {
        range,
        color: legible(highlight.color().unwrap_or(tokens.text), tokens).into_rgba8(),
        font: highlight.font().unwrap_or(Font::MONOSPACE),
    }
}

#[cfg(test)]
fn highlight(language: &str, code: &str, tokens: Tokens) -> Vec<Vec<Run>> {
    let mut highlighter = iced::highlighter::Stream::new(&settings(language, tokens));
    code.split('\n')
        .map(|line| {
            let runs = highlighter
                .highlight_line(line)
                .map(|(range, highlight)| format_run(range, highlight, tokens))
                .collect();
            highlighter.commit();
            runs
        })
        .collect()
}

fn luminance(color: Color) -> f32 {
    let channel = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

pub(super) fn legible(mut color: Color, tokens: Tokens) -> Color {
    let background = luminance(tokens.code);
    for _ in 0..12 {
        let value = luminance(color);
        if (value.max(background) + 0.05) / (value.min(background) + 0.05) >= 4.5 {
            return color;
        }
        color.r += (tokens.text.r - color.r) * 0.25;
        color.g += (tokens.text.g - color.g) * 0.25;
        color.b += (tokens.text.b - color.b) * 0.25;
    }
    tokens.text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Preset;

    #[test]
    fn cached_grammar_matches_fresh_runs_after_edits_across_snapshots() {
        let tokens = Preset::Paper.resolve().tokens;
        let original = (0..150)
            .map(|i| format!("let café_{i} = \"🦀\";\n"))
            .collect::<String>();
        for (line, insertion) in [(0, "/*\n"), (49, "/*"), (50, "//"), (51, "\n"), (120, "/*")] {
            let mut cache = Cache::default();
            let entry = cache.prepare("rust", &original, tokens).unwrap();
            cache.line(entry, 60, tokens);
            assert_eq!(
                cache.entries[entry].parser.current_line(),
                61,
                "offscreen suffix stays unparsed"
            );
            let mut changed = original.clone();
            let at = changed.find(&format!("let café_{line} ")).unwrap();
            changed.insert_str(at, insertion);
            let entry = cache.prepare("rust", &changed, tokens).unwrap();
            let full = highlight("rust", &changed, tokens);
            for index in [0, 48, 49, 50, 51, 60, 100, 120, 149] {
                assert_eq!(
                    cache.line(entry, index, tokens),
                    full[index],
                    "line {index} after {line}: {insertion:?}"
                );
            }
        }
        let mut cache = Cache::default();
        assert!(cache.prepare("rust", &"x".repeat(8193), tokens).is_none());
        assert!(
            cache
                .prepare("rust", &"x\n".repeat(140_000), tokens)
                .is_none()
        );
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn multiline_syntax_is_utf8_safe_and_contrasts_with_each_surface() {
        for preset in Preset::ALL {
            let tokens = preset.resolve().tokens;
            let code = "/* comment\nstill comment */\nlet café = \"🦀\";";
            let lines = highlight("rust", code, tokens);
            assert_eq!(lines.len(), 3);
            for (line, runs) in code.split('\n').zip(lines.iter()) {
                for run in runs {
                    assert!(line.get(run.range.clone()).is_some());
                    let [r, g, b, a] = run.color;
                    let value = luminance(Color::from_rgba8(r, g, b, a as f32 / 255.0));
                    let bg = luminance(tokens.code);
                    assert!((value.max(bg) + 0.05) / (value.min(bg) + 0.05) >= 4.4);
                }
            }
            assert_eq!(lines[0][0].color, lines[1][0].color);
        }
    }
}
