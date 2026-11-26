//! Design tokens: colors, spacing, radii, type sizes and fixed dimensions.
//! Views never use literals for any of these.

use iced::Color;

/// Colors of one theme: Catppuccin Mocha (dark) and Latte (light), the palette of portkyu and
/// of the Ghostty theme. Latte's accents are darkened so text in them passes WCAG AA (4.5:1)
/// on every background of the theme; the `contrast` tests below hold that line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// The content area.
    pub bg: Color,
    /// Sidebar, panel headers and cards (Catppuccin "mantle").
    pub surface: Color,
    /// The status bar, the darkest band (Catppuccin "crust").
    pub chrome: Color,
    pub elevated: Color,
    pub border: Color,
    pub hover: Color,
    pub selected: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    /// Mauve: selection, focus, primary actions.
    pub accent: Color,
    /// Blue: links and informational notices.
    pub info: Color,
    /// Teal: the second data series of the charts and the ANSI cyan.
    pub cyan: Color,
    /// Peach: ports and other numbers worth spotting.
    pub orange: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
}

const fn rgb(hex: u32) -> Color {
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Catppuccin Mocha.
pub const DARK: Palette = Palette {
    bg: rgb(0x1e1e2e),
    surface: rgb(0x181825),
    chrome: rgb(0x11111b),
    elevated: rgb(0x181825),
    border: rgb(0x313244),
    hover: rgb(0x292a3b),
    selected: rgb(0x332d47),
    text: rgb(0xcdd6f4),
    muted: rgb(0xa6adc8),
    faint: rgb(0x9399b2),
    accent: rgb(0xcba6f7),
    info: rgb(0x89b4fa),
    cyan: rgb(0x94e2d5),
    orange: rgb(0xfab387),
    success: rgb(0xa6e3a1),
    warning: rgb(0xf9e2af),
    danger: rgb(0xf38ba8),
};

/// Catppuccin Latte, accents darkened for contrast.
pub const LIGHT: Palette = Palette {
    bg: rgb(0xeff1f5),
    surface: rgb(0xe6e9ef),
    chrome: rgb(0xdce0e8),
    elevated: rgb(0xeff1f5),
    border: rgb(0xccd0da),
    hover: rgb(0xe1e4eb),
    selected: rgb(0xe6ddf6),
    text: rgb(0x4c4f69),
    muted: rgb(0x55586f),
    faint: rgb(0x5e6174),
    accent: rgb(0x7e28ee),
    info: rgb(0x0a54e8),
    cyan: rgb(0x116c71),
    orange: rgb(0xaa3f01),
    success: rgb(0x2d701e),
    warning: rgb(0x895712),
    danger: rgb(0xc10e34),
};

pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_6: f32 = 24.0;

pub const RADIUS: f32 = 6.0;
/// Modal, command palette and cards.
pub const RADIUS_LG: f32 = 10.0;
/// Pills (counts, ports, state badges): fully rounded at their height.
pub const RADIUS_PILL: f32 = 10.0;

/// The smallest text; nothing in the interface goes below it.
pub const TEXT_SM: f32 = 12.0;
pub const TEXT: f32 = 14.0;
pub const TEXT_LG: f32 = 16.0;
/// Big numbers of the Stats cards and the app name in the sidebar.
pub const TEXT_XL: f32 = 20.0;

pub const ROW_HEIGHT: f32 = 32.0;
pub const STATUS_BAR_HEIGHT: f32 = 30.0;
/// Fixed height of one line of the log view, which is what lets it draw only the visible ones.
pub const LOG_LINE_HEIGHT: f32 = 20.0;
/// Height of a sparkline and the width of its line.
pub const SPARKLINE_HEIGHT: f32 = 56.0;
/// Horizontal guides behind a sparkline.
pub const SPARKLINE_GRID_LINES: usize = 3;
pub const SPARKLINE_STROKE: f32 = 2.0;
pub const SIDEBAR_WIDTH: f32 = 220.0;
pub const SIDEBAR_COLLAPSED: f32 = 48.0;

pub const BORDER_WIDTH: f32 = 1.0;
pub const NO_BORDER: f32 = 0.0;
/// Left bar of the selected row and of toasts.
pub const ACCENT_BAR_WIDTH: f32 = 3.0;
/// Container status dot.
pub const DOT_SIZE: f32 = 8.0;
/// Square icon button.
pub const ICON_BUTTON_SIZE: f32 = 28.0;
pub const MODAL_WIDTH: f32 = 460.0;
/// Tallest list inside a dialog; longer ones scroll.
pub const DIALOG_LIST_HEIGHT: f32 = 8.0 * ROW_HEIGHT;
/// Widest content card, for example the onboarding.
pub const CARD_MAX_WIDTH: f32 = 520.0;
pub const TOAST_WIDTH: f32 = 380.0;
pub const SCROLLBAR_WIDTH: f32 = 8.0;
/// Space between a scrollbar and the content it scrolls.
pub const SCROLLBAR_GAP: f32 = 2.0;

/// Tint of an accent/status color used as a soft background (active chip, selection).
pub const ALPHA_SOFT: f32 = 0.15;
/// Filled buttons while hovered / pressed.
pub const ALPHA_HOVER: f32 = 0.85;
pub const ALPHA_PRESSED: f32 = 0.7;
/// Text selection highlight.
pub const ALPHA_SELECTION: f32 = 0.3;
/// Dimming behind a modal.
pub const ALPHA_SCRIM: f32 = 0.5;
/// Drop shadow of elevated surfaces.
pub const SHADOW_ALPHA: f32 = 0.3;
pub const SHADOW_BLUR: f32 = 12.0;
pub const SHADOW_OFFSET_Y: f32 = 4.0;

/// Terminal tab: line height as a multiple of the font size (the emulator's cell height).
pub const TERM_LINE_SCALE: f32 = 1.3;
/// How far the "bright" ANSI colors move toward the text color, and the "dim" ones toward the
/// background.
pub const TERM_BRIGHT_MIX: f32 = 0.25;
pub const TERM_DIM_MIX: f32 = 0.4;
/// Width of the keyboard focus ring of inputs: thicker than a border, so focus does not depend
/// on telling two border colors apart.
pub const FOCUS_RING_WIDTH: f32 = 2.0;

#[cfg(test)]
mod contrast {
    use super::*;

    fn luminance(c: Color) -> f32 {
        let lin = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
    }

    /// WCAG 2 contrast ratio.
    pub(crate) fn ratio(a: Color, b: Color) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    /// Every color used for text passes AA (4.5:1) on every background it can sit on.
    #[test]
    fn text_colors_pass_wcag_aa() {
        for (name, p) in [("dark", &DARK), ("light", &LIGHT)] {
            let backgrounds = [
                ("bg", p.bg),
                ("surface", p.surface),
                ("chrome", p.chrome),
                ("hover", p.hover),
                ("selected", p.selected),
            ];
            let texts = [
                ("text", p.text),
                ("muted", p.muted),
                ("faint", p.faint),
                ("accent", p.accent),
                ("info", p.info),
                ("cyan", p.cyan),
                ("orange", p.orange),
                ("success", p.success),
                ("warning", p.warning),
                ("danger", p.danger),
            ];
            for (bn, b) in backgrounds {
                for (tn, t) in texts {
                    let r = ratio(t, b);
                    assert!(r >= 4.5, "{name}: {tn} on {bn} is {r:.2}:1");
                }
            }
        }
    }

    /// Text on a filled button (primary, danger) uses the background color.
    #[test]
    fn filled_buttons_pass_wcag_aa() {
        for p in [&DARK, &LIGHT] {
            for fill in [p.accent, p.danger] {
                assert!(ratio(p.bg, fill) >= 4.5);
            }
        }
    }

    /// Borders and the focus ring are visible against what they frame (WCAG 1.4.11: 3:1 for
    /// the focus ring; the borders only separate, so they stay subtle).
    #[test]
    fn focus_ring_is_visible() {
        for p in [&DARK, &LIGHT] {
            assert!(ratio(p.accent, p.bg) >= 3.0);
            assert!(ratio(p.accent, p.surface) >= 3.0);
        }
    }
}
