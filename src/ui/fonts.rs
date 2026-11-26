//! Embedded fonts. The family name is the one inside the TTF files (see
//! `assets/fonts/LICENSES.md`); the weight picks the face of JetBrainsMonoNL Nerd Font.
//!
//! The whole interface uses the one monospaced family (as the terminal and portkyu do), so
//! `SANS*` and `MONO` are roles, not different typefaces: `MONO` marks text that must line up
//! (IDs, ports, sizes, logs) and stays put if the UI font ever changes again.

use iced::Font;
use iced::font::Weight;

const FAMILY: &str = "JetBrainsMonoNL Nerd Font";

pub const SANS: Font = Font::with_name(FAMILY);
pub const SANS_MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..Font::with_name(FAMILY)
};
pub const SANS_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..Font::with_name(FAMILY)
};
pub const MONO: Font = Font::with_name(FAMILY);
pub const ICONS: Font = Font::with_name("lucide");

/// Every font file, to be registered with `Application::font`.
pub fn all_bytes() -> [&'static [u8]; 4] {
    [
        include_bytes!("../../assets/fonts/JetBrainsMonoNLNerdFont-Regular.ttf"),
        include_bytes!("../../assets/fonts/JetBrainsMonoNLNerdFont-Medium.ttf"),
        include_bytes!("../../assets/fonts/JetBrainsMonoNLNerdFont-SemiBold.ttf"),
        include_bytes!("../../assets/fonts/lucide.ttf"),
    ]
}
