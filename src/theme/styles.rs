//! All component styling consumes the same resolved semantic tokens.

use iced::widget::{button, container, markdown, scrollable, text_editor, text_input};
use iced::{Background, Border, Color, Font, Theme};

use crate::theme::{ResolvedTheme, Tokens};

pub const BODY_FONT: Font = Font {
    family: iced::font::Family::Name("Newsreader"),
    ..Font::DEFAULT
};

pub const CODE_LINE_HEIGHT: f32 = 1.5;

pub fn code_size(body_size: f32) -> f32 {
    body_size * 0.82
}

pub fn code_surface(tokens: Tokens) -> container::Style {
    container::Style {
        background: Some(tokens.code.into()),
        text_color: Some(tokens.text),
        border: Border {
            color: tokens.border,
            width: 1.0,
            radius: 8.0.into(),
        },
        ..container::Style::default()
    }
}

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

/// A blank Markdown line separates paragraphs without a full extra text row.
/// Use the same metric for measured layout and virtual height estimates.
pub fn line_size(body_size: f32, heading: Option<u8>, empty: bool) -> f32 {
    heading.map_or(if empty { body_size * 0.65 } else { body_size }, |level| {
        heading_size(body_size, level)
    })
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
                    tokens.accent_hover
                } else {
                    tokens.accent
                }),
                tokens.on_accent,
            ),
            Button::Selected => (Some(tokens.accent_soft), tokens.on_accent_soft),
            Button::Danger => (
                hovered.then_some(tokens.danger.scale_alpha(0.1)),
                tokens.danger,
            ),
            Button::Quiet => (
                hovered.then_some(tokens.hover),
                if hovered { tokens.text } else { tokens.muted },
            ),
        };
        // A selected file without an action is still the current file, not an
        // unavailable command. Preserve its selection treatment.
        let disabled = status == button::Status::Disabled && !matches!(kind, Button::Selected);
        button::Style {
            background: if disabled {
                None
            } else {
                background.map(Background::Color)
            },
            text_color: if disabled { tokens.disabled } else { color },
            border: Border {
                color: if status == button::Status::Pressed {
                    color
                } else {
                    Color::TRANSPARENT
                },
                width: if status == button::Status::Pressed {
                    1.0
                } else {
                    0.0
                },
                radius: 7.0.into(),
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
            radius: 16.0.into(),
        },
        shadow: iced::Shadow {
            color: tokens.shadow,
            offset: iced::Vector::new(0.0, 14.0),
            blur_radius: 36.0,
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
                tokens.control_border
            },
            width: if matches!(status, text_input::Status::Focused { .. }) {
                2.0
            } else {
                1.0
            },
            radius: 7.0.into(),
        },
        icon: tokens.muted,
        placeholder: tokens.faint,
        value: tokens.text,
        selection: tokens.selection,
    }
}

pub fn scrollbar(tokens: Tokens) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
    move |theme, status| {
        let mut style = scrollable::default(theme, status);
        let active = !matches!(status, scrollable::Status::Active { .. });
        for rail in [&mut style.vertical_rail, &mut style.horizontal_rail] {
            rail.background = None;
            rail.border = Border::default();
            rail.scroller.background = if active { tokens.muted } else { tokens.border }.into();
            rail.scroller.border = Border {
                radius: 8.0.into(),
                ..Border::default()
            };
        }
        style
    }
}

pub fn inset(tokens: Tokens) -> impl Fn(&Theme) -> container::Style {
    move |_| container::Style {
        background: Some(tokens.canvas.into()),
        border: Border {
            color: tokens.border,
            width: 1.0,
            radius: 9.0.into(),
        },
        ..container::Style::default()
    }
}

pub fn markdown_settings(theme: &ResolvedTheme) -> markdown::Settings {
    let tokens = theme.tokens;
    let mut style = markdown::Style::from(&theme.iced);
    style.font = BODY_FONT;
    style.link_color = tokens.accent;
    style.inline_code_color = tokens.text;
    style.inline_code_highlight = markdown::Highlight {
        background: tokens.code.into(),
        border: Border {
            radius: 3.0.into(),
            ..Border::default()
        },
    };
    style.inline_code_padding = iced::Padding::from([2, 4]);
    let mut settings = markdown::Settings::with_text_size(theme.typography.body_size, style);
    settings.h1_size = heading_size(theme.typography.body_size, 1).into();
    settings.h2_size = heading_size(theme.typography.body_size, 2).into();
    settings.h3_size = heading_size(theme.typography.body_size, 3).into();
    settings.h4_size = heading_size(theme.typography.body_size, 4).into();
    settings.h5_size = heading_size(theme.typography.body_size, 5).into();
    settings.h6_size = heading_size(theme.typography.body_size, 6).into();
    settings.code_size = code_size(theme.typography.body_size).into();
    settings.spacing = 12.0.into();
    settings
}

pub fn inline_format(
    style: crate::document::projection::Style,
    tokens: Tokens,
) -> iced::advanced::text::highlighter::Format<Font> {
    use crate::document::projection::Style;
    use iced::advanced::text::highlighter::Format;
    let base = if style.contains(Style::CODE)
        || style.contains(Style::CODE_BLOCK)
        || style.contains(Style::SYNTAX)
    {
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
        } else if style.contains(Style::LINK) {
            tokens.accent
        } else if style.contains(Style::MUTED) {
            tokens.muted
        } else {
            tokens.text
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Preset;
    use crate::theme::definition::contrast;

    #[test]
    fn interactive_states_remain_legible_on_their_surfaces() {
        for preset in Preset::ALL {
            let theme = preset.resolve();
            let t = theme.tokens;
            for kind in [
                Button::Quiet,
                Button::Selected,
                Button::Primary,
                Button::Danger,
            ] {
                let style = button_style(t, kind);
                for status in [
                    button::Status::Active,
                    button::Status::Hovered,
                    button::Status::Pressed,
                ] {
                    let state = style(&theme.iced, status);
                    for surface in [t.canvas, t.surface, t.elevated] {
                        let background = match state.background {
                            Some(Background::Color(c)) => Color::from_rgb(
                                c.r * c.a + surface.r * (1.0 - c.a),
                                c.g * c.a + surface.g * (1.0 - c.a),
                                c.b * c.a + surface.b * (1.0 - c.a),
                            ),
                            _ => surface,
                        };
                        assert!(
                            contrast(state.text_color, background) >= 4.5,
                            "{} {kind:?} {status:?}",
                            preset.name()
                        );
                    }
                }
                if !matches!(kind, Button::Selected) {
                    assert_eq!(
                        style(&theme.iced, button::Status::Disabled).text_color,
                        t.disabled
                    );
                }
            }
        }
    }
}
