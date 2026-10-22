//! All component styling consumes the same resolved semantic tokens.

use iced::widget::{button, container, markdown, text_editor, text_input};
use iced::{Background, Border, Color, Font, Theme};

use crate::theme::{ResolvedTheme, Tokens};

pub const BODY_FONT: Font = Font {
    family: iced::font::Family::Name("Newsreader"),
    ..Font::DEFAULT
};

pub fn heading_size(body_size: f32, level: u8) -> f32 {
    body_size
        * match level {
            1 => 2.22,
            2 => 1.67,
            3 => 1.3,
            4 => 1.15,
            5 => 1.05,
            _ => 1.0,
        }
}

#[derive(Debug, Clone, Copy)]
pub enum Button {
    Quiet,
    Selected,
    Primary,
    Danger,
}

pub fn button_style(
    tokens: Tokens,
    kind: Button,
) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_, status| {
        let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
        let (background, color) = match kind {
            Button::Primary => (
                Some(if hovered {
                    tokens.accent.scale_alpha(0.85)
                } else {
                    tokens.accent
                }),
                tokens.canvas,
            ),
            Button::Selected => (Some(tokens.accent_soft), tokens.accent),
            Button::Danger => (
                hovered.then_some(tokens.danger.scale_alpha(0.1)),
                tokens.danger,
            ),
            Button::Quiet => (
                hovered.then_some(tokens.border.scale_alpha(0.5)),
                tokens.muted,
            ),
        };
        button::Style {
            background: background.map(Background::Color),
            text_color: color,
            border: Border {
                radius: 6.0.into(),
                ..Border::default()
            },
            ..button::Style::default()
        }
    }
}

pub fn panel(background: Color) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(background.into()),
        ..container::Style::default()
    }
}

pub fn card(tokens: Tokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(tokens.elevated.into()),
        text_color: Some(tokens.text),
        border: Border {
            color: tokens.border,
            width: 1.0,
            radius: 12.0.into(),
        },
        shadow: iced::Shadow {
            color: Color::BLACK.scale_alpha(0.1),
            offset: iced::Vector::new(0.0, 12.0),
            blur_radius: 40.0,
        },
        ..container::Style::default()
    }
}

pub fn editor(tokens: Tokens) -> impl Fn(&Theme, text_editor::Status) -> text_editor::Style {
    move |_, _| text_editor::Style {
        background: Color::TRANSPARENT.into(),
        border: Border::default(),
        placeholder: tokens.faint,
        value: tokens.text,
        selection: tokens.selection,
    }
}

pub fn input(tokens: Tokens) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
    move |_, status| text_input::Style {
        background: tokens.canvas.into(),
        border: Border {
            color: if matches!(status, text_input::Status::Focused { .. }) {
                tokens.accent
            } else {
                tokens.border
            },
            width: 1.0,
            radius: 5.0.into(),
        },
        icon: tokens.muted,
        placeholder: tokens.faint,
        value: tokens.text,
        selection: tokens.selection,
    }
}

pub fn markdown_settings(theme: &ResolvedTheme) -> markdown::Settings {
    let tokens = theme.tokens;
    let mut style = markdown::Style::from(&theme.iced);
    style.font = BODY_FONT;
    style.link_color = tokens.accent;
    style.inline_code_color = tokens.accent;
    style.inline_code_highlight = markdown::Highlight {
        background: tokens.code.into(),
        border: Border {
            radius: 3.0.into(),
            ..Border::default()
        },
    };
    style.inline_code_padding = iced::Padding::from([2, 4]);
    let mut settings = markdown::Settings::with_text_size(theme.typography.body_size, style);
    settings.h1_size = 40.0.into();
    settings.h2_size = 27.0.into();
    settings.h3_size = 21.0.into();
    settings.code_size = 14.0.into();
    settings.spacing = 12.0.into();
    settings
}

pub fn inline_format(
    style: crate::document::projection::Style,
    tokens: Tokens,
) -> iced::advanced::text::highlighter::Format<Font> {
    use crate::document::projection::Style;
    use iced::advanced::text::highlighter::Format;
    let base = if style.contains(Style::CODE) || style.contains(Style::SYNTAX) {
        Font::MONOSPACE
    } else {
        BODY_FONT
    };
    Format {
        font: Some(Font {
            weight: if style.contains(Style::BOLD) {
                iced::font::Weight::Bold
            } else {
                iced::font::Weight::Normal
            },
            style: if style.contains(Style::ITALIC) {
                iced::font::Style::Italic
            } else {
                iced::font::Style::Normal
            },
            ..base
        }),
        color: Some(if style.contains(Style::SYNTAX) {
            tokens.muted
        } else if style.contains(Style::LINK) || style.contains(Style::CODE) {
            tokens.accent
        } else if style.contains(Style::MUTED) {
            tokens.muted
        } else {
            tokens.text
        }),
    }
}
