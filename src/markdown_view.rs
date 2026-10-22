//! The Markdown renderer uses the same theme as every other component.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use iced::widget::{checkbox, column, container, image, markdown, rich_text, row, rule, text};
use iced::{Alignment, Border, Element, Fill, Font};

use crate::app::Message;
use crate::styles;
use crate::theme::Tokens;

pub struct MarkdownView {
    pub tokens: Tokens,
    pub block: usize,
    pub base: Option<PathBuf>,
    pub line_height: f32,
    pub task: Cell<usize>,
}

impl<'a> markdown::Viewer<'a, Message> for MarkdownView {
    fn on_link_click(url: String) -> Message {
        Message::Link(url)
    }

    fn paragraph(
        &self,
        settings: markdown::Settings,
        content: &markdown::Text,
    ) -> Element<'a, Message> {
        rich_text(content.spans(settings.style))
            .size(settings.text_size)
            .line_height(iced::widget::text::LineHeight::Relative(self.line_height))
            .on_link_click(Message::Link)
            .into()
    }

    fn quote(
        &self,
        settings: markdown::Settings,
        contents: &'a [markdown::Item],
    ) -> Element<'a, Message> {
        let tokens = self.tokens;
        row![
            rule::vertical(2).style(move |_| rule::Style {
                color: tokens.accent,
                snap: true,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full
            }),
            container(markdown::view_with(contents, settings, self))
                .padding([6, 18])
                .style(move |_| container::Style {
                    text_color: Some(tokens.muted),
                    ..container::Style::default()
                })
        ]
        .into()
    }

    fn code_block(
        &self,
        _settings: markdown::Settings,
        language: Option<&'a str>,
        code: &'a str,
        _lines: &'a [markdown::Text],
    ) -> Element<'a, Message> {
        let tokens = self.tokens;
        // Highlighting is cached by the Markdown parser. The source editor also
        // supplies interactive syntax highlighting; a semantic monochrome code
        // surface keeps custom themes fully in control of rendered contrast.
        container(
            column![
                text(language.unwrap_or("code"))
                    .size(10)
                    .color(tokens.muted),
                text(code.trim_end_matches('\n'))
                    .font(Font::MONOSPACE)
                    .size(14)
                    .color(tokens.text)
            ]
            .spacing(12),
        )
        .padding(20)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(tokens.code.into()),
            border: Border {
                radius: 7.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        })
        .into()
    }

    fn image(
        &self,
        settings: markdown::Settings,
        url: &'a String,
        _title: &'a str,
        alt: &markdown::Text,
    ) -> Element<'a, Message> {
        let path = Path::new(url);
        let local = !url.contains("://") && !url.starts_with("data:");
        if local && let Some(base) = &self.base {
            let resolved = base.join(path);
            return image(resolved)
                .width(Fill)
                .content_fit(iced::ContentFit::Contain)
                .into();
        }
        let tokens = self.tokens;
        container(
            column![
                rich_text(alt.spans(settings.style)).size(15),
                text(if local {
                    "Save this document to resolve relative images."
                } else {
                    "Remote image · open the link to view"
                })
                .size(11)
                .color(tokens.muted)
            ]
            .spacing(8),
        )
        .padding(20)
        .width(Fill)
        .style(styles::panel(tokens.code))
        .into()
    }

    fn unordered_list(
        &self,
        settings: markdown::Settings,
        bullets: &'a [markdown::Bullet],
    ) -> Element<'a, Message> {
        self.list(settings, None, bullets)
    }

    fn ordered_list(
        &self,
        settings: markdown::Settings,
        start: u64,
        bullets: &'a [markdown::Bullet],
    ) -> Element<'a, Message> {
        self.list(settings, Some(start), bullets)
    }
}

impl MarkdownView {
    fn list<'a>(
        &self,
        settings: markdown::Settings,
        start: Option<u64>,
        bullets: &'a [markdown::Bullet],
    ) -> Element<'a, Message> {
        let mut rows = column![].spacing(8);
        for (index, bullet) in bullets.iter().enumerate() {
            let (marker, items): (Element<'a, Message>, _) =
                match bullet {
                    markdown::Bullet::Point { items } => (
                        text(start.map_or_else(
                            || "•".into(),
                            |start| format!("{}.", start + index as u64),
                        ))
                        .font(styles::BODY_FONT)
                        .size(settings.text_size)
                        .width(20)
                        .into(),
                        items,
                    ),
                    markdown::Bullet::Task { items, done } => {
                        let task = self.task.get();
                        self.task.set(task + 1);
                        let block = self.block;
                        (
                            checkbox(*done)
                                .on_toggle(move |_| Message::ToggleTask { block, task })
                                .size(15)
                                .into(),
                            items,
                        )
                    }
                };
            rows = rows.push(
                row![
                    container(marker).padding([6, 0]),
                    markdown::view_with(items, settings, self)
                ]
                .spacing(10)
                .align_y(Alignment::Start),
            );
        }
        container(rows).padding([0, 10]).into()
    }
}
