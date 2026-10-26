//! Declarative themes resolve once into semantic colors and an Iced palette.

use iced::theme::Palette;
use iced::{Color, Theme};
use serde::Deserialize;
use thiserror::Error;

/// Values describe roles, never particular widgets or color names.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub canvas: Color,
    pub surface: Color,
    pub elevated: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub border: Color,
    pub accent: Color,
    pub accent_soft: Color,
    pub selection: Color,
    pub code: Color,
    pub danger: Color,
    pub success: Color,
    pub brand_accent: Color,
    pub on_accent: Color,
    pub on_accent_soft: Color,
    pub accent_hover: Color,
    pub hover: Color,
    pub control_border: Color,
    pub disabled: Color,
    pub scrim: Color,
    pub shadow: Color,
}

#[derive(Debug, Clone)]
pub struct ResolvedTheme {
    pub name: String,
    pub tokens: Tokens,
    pub iced: Theme,
    pub typography: Typography,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Typography {
    pub body_size: f32,
    pub line_height: f32,
    pub content_width: f32,
}

impl Default for Typography {
    fn default() -> Self {
        Self {
            body_size: 18.0,
            line_height: 1.65,
            content_width: 720.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Paper,
    Midnight,
    Sage,
}

impl Preset {
    pub const ALL: [Self; 3] = [Self::Paper, Self::Midnight, Self::Sage];

    pub fn name(self) -> &'static str {
        match self {
            Self::Paper => "Paper",
            Self::Midnight => "Midnight",
            Self::Sage => "Sage",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Paper => "Warm paper & plum ink",
            Self::Midnight => "A softer light after dark",
            Self::Sage => "Quiet greens & warm linen",
        }
    }

    pub fn resolve(self) -> ResolvedTheme {
        // Built-in definitions are validated by tests; a malformed embedded file is an authoring error.
        let definition = match self {
            Self::Paper => include_str!("../../themes/paper.toml"),
            Self::Midnight => include_str!("../../themes/midnight.toml"),
            Self::Sage => include_str!("../../themes/sage.toml"),
        };
        ResolvedTheme::from_toml(definition).expect("built-in themes must pass validation")
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Definition {
    name: String,
    colors: Colors,
    #[serde(default)]
    typography: Typography,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Colors {
    canvas: String,
    surface: String,
    elevated: String,
    text: String,
    muted: String,
    faint: String,
    border: String,
    accent: String,
    accent_soft: String,
    selection: String,
    code: String,
    danger: String,
    success: String,
    #[serde(default)]
    brand_accent: Option<String>,
}

#[derive(Debug, Error)]
pub enum ThemeError {
    #[error("Invalid theme: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("{role} must be a color in #RRGGBB format (received {value:?})")]
    Color { role: &'static str, value: String },
    #[error("{0}")]
    Validation(&'static str),
}

impl ResolvedTheme {
    pub fn from_toml(source: &str) -> Result<Self, ThemeError> {
        let definition: Definition = toml::from_str(source)?;
        if definition.name.trim().is_empty() || definition.name.len() > 80 {
            return Err(ThemeError::Validation("Theme name must contain 1–80 bytes"));
        }
        let typography = definition.typography;
        if !(12.0..=32.0).contains(&typography.body_size)
            || !(1.2..=2.2).contains(&typography.line_height)
            || !(480.0..=1200.0).contains(&typography.content_width)
        {
            return Err(ThemeError::Validation(
                "Typography requires body_size 12–32, line_height 1.2–2.2, and content_width 480–1200",
            ));
        }
        let colors = definition.colors;
        let text = parse_color("text", &colors.text)?;
        let canvas = parse_color("canvas", &colors.canvas)?;
        let surface = parse_color("surface", &colors.surface)?;
        let accent = parse_color("accent", &colors.accent)?;
        let accent_soft = parse_color("accent_soft", &colors.accent_soft)?;
        let border = parse_color("border", &colors.border)?;
        let muted = parse_color("muted", &colors.muted)?;
        let on_accent = foreground(accent, text, canvas);
        let dark = luminance(canvas) < 0.25;
        let tokens = Tokens {
            canvas,
            surface,
            elevated: parse_color("elevated", &colors.elevated)?,
            text,
            muted,
            faint: parse_color("faint", &colors.faint)?,
            border,
            accent,
            accent_soft,
            selection: parse_color("selection", &colors.selection)?,
            code: parse_color("code", &colors.code)?,
            danger: parse_color("danger", &colors.danger)?,
            success: parse_color("success", &colors.success)?,
            brand_accent: colors
                .brand_accent
                .as_deref()
                .map(|value| parse_color("brand_accent", value))
                .transpose()?
                .unwrap_or(accent),
            on_accent,
            on_accent_soft: if contrast(accent, accent_soft) >= 4.5 {
                accent
            } else {
                foreground(accent_soft, text, canvas)
            },
            accent_hover: mix(
                accent,
                if luminance(on_accent) > 0.5 {
                    Color::BLACK
                } else {
                    Color::WHITE
                },
                0.10,
            ),
            hover: mix(surface, text, 0.055),
            control_border: mix(border, text, 0.46),
            disabled: mix(surface, muted, 0.48),
            scrim: Color::BLACK.scale_alpha(if dark { 0.54 } else { 0.26 }),
            shadow: Color::BLACK.scale_alpha(if dark { 0.32 } else { 0.13 }),
        };
        let iced = Theme::custom(
            definition.name.clone(),
            Palette {
                background: tokens.canvas,
                text: tokens.text,
                primary: tokens.accent,
                success: tokens.success,
                warning: tokens.muted,
                danger: tokens.danger,
            },
        );
        Ok(Self {
            name: definition.name,
            tokens,
            iced,
            typography,
        })
    }
}

fn mix(a: Color, b: Color, amount: f32) -> Color {
    Color::from_rgb(
        a.r + (b.r - a.r) * amount,
        a.g + (b.g - a.g) * amount,
        a.b + (b.b - a.b) * amount,
    )
}

fn luminance(color: Color) -> f32 {
    let linear = |value: f32| {
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(color.r) + 0.7152 * linear(color.g) + 0.0722 * linear(color.b)
}

pub(super) fn contrast(a: Color, b: Color) -> f32 {
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

fn foreground(background: Color, text: Color, canvas: Color) -> Color {
    let preferred = if contrast(text, background) >= contrast(canvas, background) {
        text
    } else {
        canvas
    };
    if contrast(preferred, background) >= 4.5 {
        preferred
    } else if contrast(Color::BLACK, background) >= contrast(Color::WHITE, background) {
        Color::BLACK
    } else {
        Color::WHITE
    }
}

fn parse_color(role: &'static str, value: &str) -> Result<Color, ThemeError> {
    let invalid = || ThemeError::Color {
        role,
        value: value.to_owned(),
    };
    let digits = value
        .strip_prefix('#')
        .filter(|s| s.len() == 6)
        .ok_or_else(invalid)?;
    let number = u32::from_str_radix(digits, 16).map_err(|_| invalid())?;
    Ok(Color::from_rgb8(
        (number >> 16) as u8,
        (number >> 8) as u8,
        number as u8,
    ))
}

#[cfg(test)]
mod tests {
    use super::{Preset, ResolvedTheme, contrast};

    #[test]
    fn presets_resolve_to_the_same_semantic_palette() {
        for preset in Preset::ALL {
            let theme = preset.resolve();
            assert_eq!(theme.iced.palette().background, theme.tokens.canvas);
            assert_eq!(theme.iced.palette().primary, theme.tokens.accent);
        }
    }

    #[test]
    fn rejects_invalid_colors_unknown_fields_and_unsafe_metrics() {
        let paper = include_str!("../../themes/paper.toml");
        for malformed in [
            paper.replace("#F8F5F0", "red"),
            paper.replace("body_size = 18", "body_size = 900"),
            paper.replace("canvas =", "typo ="),
        ] {
            assert!(ResolvedTheme::from_toml(&malformed).is_err());
        }
    }

    #[test]
    fn built_in_text_controls_and_selections_keep_readable_contrast() {
        for preset in Preset::ALL {
            let t = preset.resolve().tokens;
            for foreground in [t.text, t.muted, t.faint, t.accent, t.danger] {
                for background in [t.canvas, t.surface, t.elevated, t.code, t.accent_soft] {
                    assert!(contrast(foreground, background) >= 4.5, "{}", preset.name());
                }
            }
            assert!(contrast(t.text, t.selection) >= 4.5);
            assert!(contrast(t.on_accent, t.accent) >= 4.5);
            assert!(contrast(t.on_accent, t.accent_hover) >= 4.5);
            assert!(contrast(t.on_accent_soft, t.accent_soft) >= 4.5);
            assert!(contrast(t.control_border, t.canvas) >= 3.0);
        }
    }

    #[test]
    fn legacy_custom_themes_resolve_optional_brand_and_light_primary_colors() {
        let paper = include_str!("../../themes/paper.toml");
        let mut definition: toml::Value = toml::from_str(paper).unwrap();
        definition["colors"]
            .as_table_mut()
            .unwrap()
            .remove("brand_accent");
        for primary in ["#F4D9EC", "#302A35", "#8C7689"] {
            definition["colors"]["accent"] = primary.into();
            let theme = ResolvedTheme::from_toml(&toml::to_string(&definition).unwrap()).unwrap();
            let t = theme.tokens;
            assert_eq!(t.brand_accent, t.accent);
            assert!(contrast(t.on_accent, t.accent) >= 4.5);
            assert!(contrast(t.on_accent, t.accent_hover) >= 4.5);
            assert!(contrast(t.on_accent_soft, t.accent_soft) >= 4.5);
        }
        definition["colors"]
            .as_table_mut()
            .unwrap()
            .insert("brand_accent".into(), "invalid".into());
        assert!(ResolvedTheme::from_toml(&toml::to_string(&definition).unwrap()).is_err());
    }
}
