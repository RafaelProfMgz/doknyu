//! Design tokens: colors, spacing, radii, type sizes and fixed dimensions.
//! Views never use literals for any of these.

use iced::Color;

/// Colors of one theme (One Dark / One Light, as in Zed).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub bg: Color,
    pub surface: Color,
    pub elevated: Color,
    pub border: Color,
    pub hover: Color,
    pub selected: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
}

const fn rgb(hex: u32) -> Color {
    Color::from_rgb8((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

pub const DARK: Palette = Palette {
    bg: rgb(0x282c33),
    surface: rgb(0x2f343e),
    elevated: rgb(0x2f343e),
    border: rgb(0x363c46),
    hover: rgb(0x363c46),
    selected: rgb(0x454a56),
    text: rgb(0xdce0e5),
    muted: rgb(0xa9afbc),
    faint: rgb(0x878a98),
    accent: rgb(0x74ade8),
    success: rgb(0xa1c181),
    warning: rgb(0xdec184),
    danger: rgb(0xd07277),
};

pub const LIGHT: Palette = Palette {
    bg: rgb(0xfafafa),
    surface: rgb(0xebebec),
    elevated: rgb(0xfafafa),
    border: rgb(0xdcdcdd),
    hover: rgb(0xdfdfe0),
    selected: rgb(0xd3d3d4),
    text: rgb(0x242529),
    muted: rgb(0x58585a),
    faint: rgb(0x7e7e83),
    accent: rgb(0x5c78e2),
    success: rgb(0x669f59),
    warning: rgb(0xa48819),
    danger: rgb(0xd36151),
};

pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 8.0;
pub const SPACE_3: f32 = 12.0;
pub const SPACE_4: f32 = 16.0;
pub const SPACE_6: f32 = 24.0;

pub const RADIUS: f32 = 4.0;
/// Modal and command palette.
pub const RADIUS_LG: f32 = 6.0;

pub const TEXT_SM: f32 = 12.0;
pub const TEXT: f32 = 13.0;
pub const TEXT_LG: f32 = 15.0;

pub const ROW_HEIGHT: f32 = 28.0;
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
pub const SIDEBAR_WIDTH: f32 = 200.0;
pub const SIDEBAR_COLLAPSED: f32 = 40.0;

pub const BORDER_WIDTH: f32 = 1.0;
pub const NO_BORDER: f32 = 0.0;
/// Left bar of the selected row and of toasts.
pub const ACCENT_BAR_WIDTH: f32 = 2.0;
/// Container status dot.
pub const DOT_SIZE: f32 = 8.0;
/// Square icon button.
pub const ICON_BUTTON_SIZE: f32 = 24.0;
pub const MODAL_WIDTH: f32 = 420.0;
pub const TOAST_WIDTH: f32 = 360.0;
pub const SCROLLBAR_WIDTH: f32 = 6.0;

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
