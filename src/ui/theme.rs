//! Light/dark themes built from the design tokens.

use iced::Theme;
use iced::theme::{self, Mode};

use crate::ui::tokens::{DARK, LIGHT, Palette};

/// Which theme the user wants; `System` follows the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    System,
    Dark,
    Light,
}

const DARK_NAME: &str = "doknyu dark";
const LIGHT_NAME: &str = "doknyu light";

/// Picks the palette. A system without preference (`Mode::None`) gets the dark one.
pub fn resolve(mode: ThemeMode, system: Mode) -> &'static Palette {
    match mode {
        ThemeMode::Dark => &DARK,
        ThemeMode::Light => &LIGHT,
        ThemeMode::System => match system {
            Mode::Light => &LIGHT,
            Mode::Dark | Mode::None => &DARK,
        },
    }
}

/// An iced theme for the palette; the base palette keeps the widgets that still use the default
/// style in the right colors.
pub fn theme(p: &'static Palette) -> Theme {
    let name = if *p == LIGHT { LIGHT_NAME } else { DARK_NAME };
    Theme::custom(
        name,
        theme::Palette {
            background: p.bg,
            text: p.text,
            primary: p.accent,
            success: p.success,
            warning: p.warning,
            danger: p.danger,
        },
    )
}

/// The palette of a theme made by [`theme`]; any other theme gets the dark one.
pub fn palette(theme: &Theme) -> &'static Palette {
    match theme {
        Theme::Custom(custom) if custom.to_string() == LIGHT_NAME => &LIGHT,
        _ => &DARK,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::tokens::{DARK, LIGHT};
    use iced::Color;
    use iced::theme::Mode;

    #[test]
    fn resolve_follows_system() {
        assert_eq!(resolve(ThemeMode::System, Mode::Light), &LIGHT);
        assert_eq!(resolve(ThemeMode::System, Mode::Dark), &DARK);
        assert_eq!(resolve(ThemeMode::System, Mode::None), &DARK);
    }

    #[test]
    fn forced_mode_ignores_system() {
        assert_eq!(resolve(ThemeMode::Light, Mode::Dark), &LIGHT);
        assert_eq!(resolve(ThemeMode::Dark, Mode::Light), &DARK);
    }

    #[test]
    fn palette_roundtrips_through_theme() {
        assert_eq!(palette(&theme(&LIGHT)), &LIGHT);
        assert_eq!(palette(&theme(&DARK)), &DARK);
        assert_eq!(palette(&iced::Theme::Light), &DARK);
    }

    #[test]
    fn dark_palette_matches_spec() {
        assert_eq!(DARK.bg, Color::from_rgb8(0x28, 0x2c, 0x33));
        assert_eq!(DARK.accent, Color::from_rgb8(0x74, 0xad, 0xe8));
        assert_eq!(LIGHT.accent, Color::from_rgb8(0x5c, 0x78, 0xe2));
    }
}
