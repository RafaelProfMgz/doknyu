use iced::widget::{container, text};
use iced::{Element, Padding};

use crate::ui::style;
use crate::ui::tokens::{SPACE_1, TEXT_SM};

/// A keyboard shortcut as shown to the user. The keymap of the later phases reuses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub key: &'static str,
}

impl Shortcut {
    /// Text of the hint, for example `Ctrl ⇧ P`.
    pub fn label(&self) -> String {
        let mut parts = Vec::new();
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.shift {
            parts.push("⇧");
        }
        parts.push(self.key);
        parts.join(" ")
    }
}

/// Small bordered hint with the shortcut.
pub fn kbd<'a, M: 'a>(shortcut: &Shortcut) -> Element<'a, M> {
    container(
        text(shortcut.label())
            .size(TEXT_SM)
            .style(style::text::faint),
    )
    .padding(Padding::from([0.0, SPACE_1]))
    .style(style::container::bordered)
    .into()
}
