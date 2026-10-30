//! The registry of everything the user can do. Buttons, shortcuts and (later) the command
//! palette all end in `Message::Action(ActionId)`, which `App::run_action` carries out.

use rust_i18n::t;

use crate::app::{App, Connection, DockTab, Screen};
use crate::ui::components::Shortcut;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ActionId {
    GoContainers,
    GoVolumes,
    GoNetworks,
    ToggleSidebar,
    ToggleDock,
    FocusSearch,
    /// Ctrl+F while the dock shows the logs; `App::on_key` swaps it in for `FocusSearch`.
    FocusLogSearch,
    /// Shows the logs of the selected container in the dock.
    DockLogs,
    /// Shows the stats of the selected container in the dock.
    DockStats,
    ToggleTimestamps,
    CopyLogs,
    OpenPalette,
    SelectNext,
    SelectPrev,
    OpenSelected,
    StartStopSelected,
    RestartSelected,
    RemoveSelected,
    ThemeSystem,
    ThemeDark,
    ThemeLight,
    /// Removes the volumes nobody uses (asks first); from any screen it goes to Volumes.
    PruneVolumes,
    Retry,
    Escape,
}

/// How an action is presented: its translated label, its shortcut hint and whether the
/// command palette lists it.
#[derive(Debug, Clone, Copy)]
pub struct ActionDef {
    pub id: ActionId,
    /// Key of the label in the locale files.
    pub label_key: &'static str,
    pub shortcut: Option<Shortcut>,
    pub in_palette: bool,
}

const fn key(key: &'static str) -> Option<Shortcut> {
    Some(Shortcut {
        ctrl: false,
        shift: false,
        key,
    })
}

const fn ctrl(key: &'static str) -> Option<Shortcut> {
    Some(Shortcut {
        ctrl: true,
        shift: false,
        key,
    })
}

const fn def_of(
    id: ActionId,
    label_key: &'static str,
    shortcut: Option<Shortcut>,
    in_palette: bool,
) -> ActionDef {
    ActionDef {
        id,
        label_key,
        shortcut,
        in_palette,
    }
}

/// Every action, in the order the palette lists them when nothing else ranks them.
static ALL: [ActionDef; 24] = [
    def_of(
        ActionId::GoContainers,
        "actions.go_containers",
        ctrl("1"),
        true,
    ),
    def_of(ActionId::GoVolumes, "actions.go_volumes", ctrl("2"), true),
    def_of(ActionId::GoNetworks, "actions.go_networks", ctrl("3"), true),
    def_of(
        ActionId::ToggleSidebar,
        "actions.toggle_sidebar",
        ctrl("B"),
        true,
    ),
    def_of(ActionId::ToggleDock, "actions.toggle_dock", ctrl("J"), true),
    def_of(
        ActionId::FocusSearch,
        "actions.focus_search",
        ctrl("F"),
        true,
    ),
    // No shortcut of its own: Ctrl+F is routed here by the app when the logs are on screen.
    def_of(
        ActionId::FocusLogSearch,
        "actions.focus_log_search",
        None,
        true,
    ),
    def_of(
        ActionId::DockLogs,
        "actions.dock_logs",
        Some(Shortcut {
            ctrl: true,
            shift: true,
            key: "L",
        }),
        true,
    ),
    def_of(
        ActionId::DockStats,
        "actions.dock_stats",
        Some(Shortcut {
            ctrl: true,
            shift: true,
            key: "S",
        }),
        true,
    ),
    def_of(
        ActionId::ToggleTimestamps,
        "actions.toggle_timestamps",
        None,
        true,
    ),
    def_of(ActionId::CopyLogs, "actions.copy_logs", None, true),
    def_of(
        ActionId::OpenPalette,
        "actions.open_palette",
        Some(Shortcut {
            ctrl: true,
            shift: true,
            key: "P",
        }),
        false,
    ),
    def_of(ActionId::SelectNext, "actions.select_next", key("J"), false),
    def_of(ActionId::SelectPrev, "actions.select_prev", key("K"), false),
    def_of(
        ActionId::OpenSelected,
        "actions.open_selected",
        key("Enter"),
        true,
    ),
    def_of(
        ActionId::StartStopSelected,
        "actions.start_stop_selected",
        key("S"),
        true,
    ),
    def_of(
        ActionId::RestartSelected,
        "actions.restart_selected",
        key("R"),
        true,
    ),
    def_of(
        ActionId::RemoveSelected,
        "actions.remove_selected",
        key("Delete"),
        true,
    ),
    def_of(ActionId::ThemeSystem, "actions.theme_system", None, true),
    def_of(ActionId::ThemeDark, "actions.theme_dark", None, true),
    def_of(ActionId::ThemeLight, "actions.theme_light", None, true),
    def_of(ActionId::PruneVolumes, "actions.prune_volumes", None, true),
    def_of(ActionId::Retry, "actions.retry", None, true),
    def_of(ActionId::Escape, "actions.escape", key("Esc"), false),
];

pub fn all() -> &'static [ActionDef] {
    &ALL
}

pub fn def(id: ActionId) -> &'static ActionDef {
    ALL.iter()
        .find(|d| d.id == id)
        .expect("every ActionId has an entry in ALL")
}

/// Whether the action makes sense right now. `App::run_action` does nothing for an action that
/// is not available, and the palette hides it.
pub fn available(app: &App, id: ActionId) -> bool {
    use ActionId::*;
    let connected = app.conn == Connection::Ok;
    // A list can be used: Docker answers, it is on screen (not hidden by the maximized dock)
    // and no dialog covers it. `on_containers`, `on_volumes` and `on_networks` say which screen it is.
    let dialog_open = app.containers.confirm_remove.is_some()
        || app.volumes.confirm.is_some()
        || app.networks.confirm_remove.is_some();
    let list_usable = connected
        && matches!(
            app.screen,
            Screen::Containers | Screen::Volumes | Screen::Networks
        )
        && !dialog_open
        && !app.dock.as_ref().is_some_and(|d| d.maximized);
    let on_containers = list_usable && app.screen == Screen::Containers;
    let on_volumes = list_usable && app.screen == Screen::Volumes;
    let on_networks = list_usable && app.screen == Screen::Networks;
    let has_selection = || {
        app.containers
            .selected
            .as_deref()
            .is_some_and(|id| app.containers.items.iter().any(|c| c.id == id))
    };
    // Docker refuses to remove a volume in use, so it is not offered; nor one being removed.
    let volume_removable = || {
        app.volumes.selected.as_deref().is_some_and(|name| {
            app.volumes
                .items
                .iter()
                .any(|v| v.name == name && v.used_by.is_empty())
                && !app.volumes.pending.contains(name)
        })
    };
    // Docker's default networks are never offered, nor one being removed. One with containers
    // connected is: the dialog warns and Docker has the last word.
    let network_removable = || {
        app.networks
            .selected
            .as_deref()
            .is_some_and(|id| app.networks.removable(id))
    };
    // The Logs tab is on screen: its search, timestamps and copy only make sense then.
    let logs_open = connected
        && app.dock.as_ref().is_some_and(|d| d.tab == DockTab::Logs)
        && app.detail.is_some();
    match id {
        GoContainers | GoVolumes | GoNetworks | ToggleSidebar | OpenPalette | ThemeSystem
        | ThemeDark | ThemeLight | Escape => true,
        ToggleDock | PruneVolumes => connected,
        // Works from the maximized dock too: it does not need the list, only the selection.
        DockLogs | DockStats => {
            connected && app.containers.confirm_remove.is_none() && has_selection()
        }
        ToggleTimestamps | FocusLogSearch => logs_open,
        CopyLogs => logs_open && app.detail.as_ref().is_some_and(|d| !d.logs.is_empty()),
        Retry => matches!(app.conn, Connection::Failed(_)),
        FocusSearch | SelectNext | SelectPrev => list_usable,
        // Volumes and networks have nothing to open, start or restart.
        OpenSelected | StartStopSelected | RestartSelected => on_containers && has_selection(),
        RemoveSelected => {
            (on_containers && has_selection())
                || (on_volumes && volume_removable())
                || (on_networks && network_removable())
        }
    }
}

/// The translated label of an action.
pub fn label(id: ActionId) -> String {
    t!(def(id).label_key).into_owned()
}

/// A tooltip text: the label of a button followed by the shortcut of its action, if any.
pub fn tip(id: ActionId, label: &str) -> String {
    match def(id).shortcut {
        Some(shortcut) => format!("{label} ({})", shortcut.label()),
        None => label.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_label_in_both_locales() {
        for d in all() {
            for locale in ["en", "pt-BR"] {
                let text = t!(d.label_key, locale = locale);
                assert_ne!(text, d.label_key, "missing {locale} label for {:?}", d.id);
            }
        }
    }

    #[test]
    fn every_action_id_is_registered_once() {
        for d in all() {
            assert_eq!(all().iter().filter(|o| o.id == d.id).count(), 1);
            assert_eq!(def(d.id).id, d.id);
        }
    }

    #[test]
    fn log_actions_are_in_the_palette_and_open_logs_has_its_shortcut() {
        for id in [
            ActionId::DockLogs,
            ActionId::ToggleTimestamps,
            ActionId::CopyLogs,
            ActionId::FocusLogSearch,
        ] {
            assert!(def(id).in_palette, "{id:?}");
        }
        assert_eq!(tip(ActionId::DockLogs, "Logs"), "Logs (Ctrl ⇧ L)");
    }

    #[test]
    fn dock_stats_is_in_the_palette_with_its_shortcut() {
        assert!(def(ActionId::DockStats).in_palette);
        assert_eq!(tip(ActionId::DockStats, "Stats"), "Stats (Ctrl ⇧ S)");
    }

    #[test]
    fn tip_appends_the_shortcut() {
        assert_eq!(tip(ActionId::ToggleDock, "Panel"), "Panel (Ctrl J)");
        assert_eq!(tip(ActionId::Retry, "Retry"), "Retry");
    }
}
