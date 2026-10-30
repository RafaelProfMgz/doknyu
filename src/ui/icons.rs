//! Lucide icons. The codepoints come from `font/info.json` of `lucide-static` 1.64.0.

use iced::widget::{Text, text};

use crate::ui::fonts::ICONS;
use crate::ui::tokens::TEXT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Box,
    Database,
    Network,
    Play,
    Square,
    RotateCw,
    Trash2,
    Search,
    PanelLeft,
    PanelBottom,
    Maximize2,
    Minimize2,
    X,
    ExternalLink,
    Command,
    Circle,
    TriangleAlert,
    Copy,
    ChevronUp,
    ChevronDown,
    ArrowDownToLine,
    Lock,
    SquareTerminal,
}

impl Icon {
    pub const ALL: &'static [Icon] = &[
        Icon::Box,
        Icon::Database,
        Icon::Network,
        Icon::Play,
        Icon::Square,
        Icon::RotateCw,
        Icon::Trash2,
        Icon::Search,
        Icon::PanelLeft,
        Icon::PanelBottom,
        Icon::Maximize2,
        Icon::Minimize2,
        Icon::X,
        Icon::ExternalLink,
        Icon::Command,
        Icon::Circle,
        Icon::TriangleAlert,
        Icon::Copy,
        Icon::ChevronUp,
        Icon::ChevronDown,
        Icon::ArrowDownToLine,
        Icon::Lock,
        Icon::SquareTerminal,
    ];

    /// Glyph in the Lucide font (private use area).
    pub fn codepoint(self) -> char {
        match self {
            Icon::Box => '\u{e061}',
            Icon::Database => '\u{e0ad}',
            Icon::Network => '\u{e125}',
            Icon::Play => '\u{e13c}',
            Icon::Square => '\u{e167}',
            Icon::RotateCw => '\u{e149}',
            Icon::Trash2 => '\u{e18e}',
            Icon::Search => '\u{e151}',
            Icon::PanelLeft => '\u{e12a}',
            Icon::PanelBottom => '\u{e42c}',
            Icon::Maximize2 => '\u{e113}',
            Icon::Minimize2 => '\u{e11b}',
            Icon::X => '\u{e1b2}',
            Icon::ExternalLink => '\u{e0b9}',
            Icon::Command => '\u{e09a}',
            Icon::Circle => '\u{e076}',
            Icon::TriangleAlert => '\u{e193}',
            Icon::Copy => '\u{e09e}',
            Icon::ChevronUp => '\u{e070}',
            Icon::ChevronDown => '\u{e06d}',
            Icon::ArrowDownToLine => '\u{e455}',
            Icon::Lock => '\u{e10b}',
            Icon::SquareTerminal => '\u{e20a}',
        }
    }
}

/// The icon as text in the Lucide font.
pub fn icon<'a>(i: Icon) -> Text<'a> {
    text(i.codepoint()).font(ICONS).size(TEXT + 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_codepoints_are_unique_and_private_use() {
        let mut seen = std::collections::HashSet::new();
        for i in Icon::ALL {
            let c = i.codepoint();
            assert!(('\u{E000}'..='\u{F8FF}').contains(&c), "{i:?} -> {c:?}");
            assert!(seen.insert(c), "{i:?} repeats {c:?}");
        }
        assert_eq!(seen.len(), 23);
    }
}
