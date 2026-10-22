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

    pub fn resolve(self) -> ResolvedTheme {
        // These embedded files are validated by tests. Fall back safely if edited incorrectly.
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
        let tokens = Tokens {
            canvas: parse_color("canvas", &colors.canvas)?,
            surface: parse_color("surface", &colors.surface)?,
            elevated: parse_color("elevated", &colors.elevated)?,
            text: parse_color("text", &colors.text)?,
            muted: parse_color("muted", &colors.muted)?,
            faint: parse_color("faint", &colors.faint)?,
            border: parse_color("border", &colors.border)?,
            accent: parse_color("accent", &colors.accent)?,
            accent_soft: parse_color("accent_soft", &colors.accent_soft)?,
            selection: parse_color("selection", &colors.selection)?,
            code: parse_color("code", &colors.code)?,
            danger: parse_color("danger", &colors.danger)?,
            success: parse_color("success", &colors.success)?,
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
    use super::{Preset, ResolvedTheme};

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
            paper.replace("#FAF9F6", "red"),
            paper.replace("body_size = 18", "body_size = 900"),
            paper.replace("canvas =", "typo ="),
        ] {
            assert!(ResolvedTheme::from_toml(&malformed).is_err());
        }
    }
}
