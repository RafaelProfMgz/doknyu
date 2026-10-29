//! Key presses to actions. A pure function, so the table can be tested without a window.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

use crate::actions::ActionId;

/// The action a key press stands for, if any.
///
/// `captured` is true when a widget (a `text_input`) already handled the event: keys that only
/// type something, such as `j` or `/`, must then not trigger anything. Shortcuts with Ctrl and
/// Esc always work.
pub fn resolve(key: &Key, mods: Modifiers, captured: bool) -> Option<ActionId> {
    if *key == Key::Named(Named::Escape) {
        return Some(ActionId::Escape);
    }
    if mods.control() {
        return with_ctrl(key, mods);
    }
    if captured {
        return None;
    }
    plain(key, mods)
}

/// Ctrl+key. Alt and the logo key are not part of any shortcut, so they cancel the match.
fn with_ctrl(key: &Key, mods: Modifiers) -> Option<ActionId> {
    if mods.alt() || mods.logo() {
        return None;
    }
    let Key::Character(c) = key else {
        return None;
    };
    match (c.to_lowercase().as_str(), mods.shift()) {
        ("p", true) => Some(ActionId::OpenPalette),
        ("1", false) => Some(ActionId::GoContainers),
        ("2", false) => Some(ActionId::GoVolumes),
        ("3", false) => Some(ActionId::GoNetworks),
        ("b", false) => Some(ActionId::ToggleSidebar),
        ("j", false) => Some(ActionId::ToggleDock),
        ("f", false) => Some(ActionId::FocusSearch),
        _ => None,
    }
}

/// Keys without Ctrl, only meant for when nothing is being typed.
fn plain(key: &Key, mods: Modifiers) -> Option<ActionId> {
    if mods.alt() || mods.logo() {
        return None;
    }
    match key {
        // `/` may need Shift on some layouts.
        Key::Character(c) if c.as_str() == "/" => Some(ActionId::FocusSearch),
        // A capital letter (Shift or Caps Lock) is not the shortcut: `S` could stop a container
        // by accident.
        Key::Character(c) if !mods.shift() => match c.as_str() {
            "j" => Some(ActionId::SelectNext),
            "k" => Some(ActionId::SelectPrev),
            "s" => Some(ActionId::StartStopSelected),
            "r" => Some(ActionId::RestartSelected),
            _ => None,
        },
        Key::Named(named) if !mods.shift() => match named {
            Named::ArrowDown => Some(ActionId::SelectNext),
            Named::ArrowUp => Some(ActionId::SelectPrev),
            Named::Enter => Some(ActionId::OpenSelected),
            Named::Delete => Some(ActionId::RemoveSelected),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }

    fn named(n: Named) -> Key {
        Key::Named(n)
    }

    #[test]
    fn keymap_table() {
        let ctrl = Modifiers::CTRL;
        let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
        let none = Modifiers::empty();
        let cases = [
            (ch("p"), ctrl_shift, Some(ActionId::OpenPalette)),
            (ch("P"), ctrl_shift, Some(ActionId::OpenPalette)),
            (ch("p"), ctrl, None),
            (ch("1"), ctrl, Some(ActionId::GoContainers)),
            (ch("2"), ctrl, Some(ActionId::GoVolumes)),
            (ch("3"), ctrl, Some(ActionId::GoNetworks)),
            (ch("b"), ctrl, Some(ActionId::ToggleSidebar)),
            (ch("j"), ctrl, Some(ActionId::ToggleDock)),
            (ch("f"), ctrl, Some(ActionId::FocusSearch)),
            (ch("/"), none, Some(ActionId::FocusSearch)),
            (ch("j"), none, Some(ActionId::SelectNext)),
            (ch("k"), none, Some(ActionId::SelectPrev)),
            (named(Named::ArrowDown), none, Some(ActionId::SelectNext)),
            (named(Named::ArrowUp), none, Some(ActionId::SelectPrev)),
            (named(Named::Enter), none, Some(ActionId::OpenSelected)),
            (ch("s"), none, Some(ActionId::StartStopSelected)),
            (ch("r"), none, Some(ActionId::RestartSelected)),
            (named(Named::Delete), none, Some(ActionId::RemoveSelected)),
            (named(Named::Escape), none, Some(ActionId::Escape)),
            // Not shortcuts.
            (ch("x"), none, None),
            (ch("S"), Modifiers::SHIFT, None),
            (ch("s"), Modifiers::ALT, None),
            (ch("1"), none, None),
            (ch("b"), Modifiers::CTRL | Modifiers::ALT, None),
            (named(Named::Tab), none, None),
        ];
        for (key, mods, expected) in cases {
            assert_eq!(resolve(&key, mods, false), expected, "{key:?} {mods:?}");
        }
    }

    #[test]
    fn letters_ignored_while_typing() {
        let none = Modifiers::empty();
        for key in [
            ch("j"),
            ch("k"),
            ch("s"),
            ch("r"),
            ch("/"),
            named(Named::Enter),
            named(Named::Delete),
            named(Named::ArrowDown),
        ] {
            assert_eq!(resolve(&key, none, true), None, "{key:?}");
        }
        assert_eq!(
            resolve(&ch("b"), Modifiers::CTRL, true),
            Some(ActionId::ToggleSidebar)
        );
        assert_eq!(
            resolve(&named(Named::Escape), none, true),
            Some(ActionId::Escape)
        );
    }
}
