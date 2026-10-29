//! Embedded fonts. The family names are the ones inside the TTF files (see
//! `assets/fonts/LICENSES.md`); the weight picks the face of IBM Plex Sans.

use iced::Font;
use iced::font::Weight;

const SANS_FAMILY: &str = "IBM Plex Sans";

pub const SANS: Font = Font::with_name(SANS_FAMILY);
pub const SANS_MEDIUM: Font = Font {
    weight: Weight::Medium,
    ..Font::with_name(SANS_FAMILY)
};
pub const SANS_SEMIBOLD: Font = Font {
    weight: Weight::Semibold,
    ..Font::with_name(SANS_FAMILY)
};
pub const MONO: Font = Font::with_name("IBM Plex Mono");
pub const ICONS: Font = Font::with_name("lucide");

/// Every font file, to be registered with `Application::font`.
pub fn all_bytes() -> [&'static [u8]; 5] {
    [
        include_bytes!("../../assets/fonts/IBMPlexSans-Regular.ttf"),
        include_bytes!("../../assets/fonts/IBMPlexSans-Medium.ttf"),
        include_bytes!("../../assets/fonts/IBMPlexSans-SemiBold.ttf"),
        include_bytes!("../../assets/fonts/IBMPlexMono-Regular.ttf"),
        include_bytes!("../../assets/fonts/lucide.ttf"),
    ]
}
