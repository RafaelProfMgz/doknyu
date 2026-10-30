//! Volume list: search, selection, per-row removal and the "remove unused" dialog.

use std::collections::HashSet;

use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Id, button, center, column, container, row, scrollable, sensor, space, text, tooltip,
};
use iced::{Alignment, Element, Length, Padding};
use rust_i18n::t;

use crate::actions::{self, ActionId};
use crate::app::Message;
use crate::error::DockerError;
use crate::model::VolumeInfo;
use crate::ui::components::{
    RowEvents, Shortcut, icon_button, kbd, list_header, list_row, search_field,
};
use crate::ui::fonts::{MONO, SANS_MEDIUM, SANS_SEMIBOLD};
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::tokens::{
    DIALOG_LIST_HEIGHT, ICON_BUTTON_SIZE, ROW_HEIGHT, SPACE_1, SPACE_2, SPACE_4, SPACE_6, TEXT,
    TEXT_LG, TEXT_SM,
};
use crate::views::toolbar::toolbar;

/// Widget id of the search box, so a shortcut can focus it.
pub const SEARCH_ID: &str = "volumes-search";

/// Widget id of the scrollable with the rows, so the keyboard selection can scroll it.
pub const LIST_ID: &str = "volumes-list";

/// What the open dialog asks the user to confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VolumeConfirm {
    /// Remove this volume (by name).
    Remove(String),
    /// Remove every volume nobody uses; the names are the ones unused when the dialog opened.
    Prune(Vec<String>),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VolumesState {
    pub items: Vec<VolumeInfo>,
    pub query: String,
    pub confirm: Option<VolumeConfirm>,
    /// Volumes with a removal in flight.
    pub pending: HashSet<String>,
    /// The highlighted row, by volume name.
    pub selected: Option<String>,
    /// The row under the cursor, by volume name.
    pub hovered: Option<String>,
    /// Scroll position of the rows and the height of the area showing them, as last reported.
    pub scroll_y: f32,
    pub viewport_h: f32,
}

/// Messages of the volume list.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A list reload finished; the number identifies which reload it answers.
    Loaded(u64, Result<Vec<VolumeInfo>, DockerError>),
    Query(String),
    /// A click on a row.
    Select(String),
    Hover(String),
    Leave(String),
    AskRemove(String),
    AskPrune,
    /// The dialog's main button (or Enter).
    Confirm,
    Cancel,
    Removed(String, Result<(), DockerError>),
    /// The listed volumes were removed one by one: which went, and which Docker refused.
    Pruned {
        removed: Vec<String>,
        failed: Vec<(String, DockerError)>,
    },
    Scrolled {
        offset: f32,
        height: f32,
    },
    Resized(f32),
}

impl VolumesState {
    /// Items matching the search (name or driver, case-insensitive), by name.
    pub fn visible(&self) -> Vec<&VolumeInfo> {
        let query = self.query.trim().to_lowercase();
        let mut items: Vec<&VolumeInfo> = self
            .items
            .iter()
            .filter(|v| {
                query.is_empty()
                    || v.name.to_lowercase().contains(&query)
                    || v.driver.to_lowercase().contains(&query)
            })
            .collect();
        items.sort_by_cached_key(|v| v.name.to_lowercase());
        items
    }

    /// After the search changed: a selection that is no longer in [`Self::visible`] moves to
    /// the first visible row, or to nothing if the list is empty.
    pub fn keep_selection_visible(&mut self) {
        let visible = self.visible();
        let hidden = self
            .selected
            .as_deref()
            .is_some_and(|name| !visible.iter().any(|v| v.name == name));
        if hidden {
            self.selected = visible.first().map(|v| v.name.clone());
        }
    }

    /// Replaces the list after a reload. A selected volume that is gone hands the selection to
    /// the one now at its position in [`Self::visible`] (or the last one, or nothing).
    pub fn set_items(&mut self, items: Vec<VolumeInfo>) {
        let old_position = self
            .selected
            .as_deref()
            .and_then(|name| self.visible().iter().position(|v| v.name == name));
        self.items = items;
        let exists = |name: &str| self.items.iter().any(|v| v.name == name);
        if self.selected.as_deref().is_some_and(|name| !exists(name)) {
            let visible = self.visible();
            let position = old_position
                .unwrap_or(usize::MAX)
                .min(visible.len().saturating_sub(1));
            self.selected = visible.get(position).map(|v| v.name.clone());
        }
        if self.hovered.as_deref().is_some_and(|name| !exists(name)) {
            self.hovered = None;
        }
        // The volume to remove is gone: nothing left to confirm.
        if matches!(&self.confirm, Some(VolumeConfirm::Remove(name)) if !exists(name)) {
            self.confirm = None;
        }
        // What the prune dialog lists is what will be removed, so it follows the list.
        if matches!(self.confirm, Some(VolumeConfirm::Prune(_))) {
            self.confirm = Some(VolumeConfirm::Prune(self.unused()));
        }
    }

    /// Names of the volumes no container uses and nobody is removing, by name.
    pub fn unused(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .items
            .iter()
            .filter(|v| v.used_by.is_empty() && !self.pending.contains(&v.name))
            .map(|v| v.name.clone())
            .collect();
        names.sort_by_cached_key(|n| n.to_lowercase());
        names
    }

    /// Whether the main button of the open dialog does something: removing always does,
    /// pruning only if there is something to prune.
    pub fn can_confirm(&self) -> bool {
        match &self.confirm {
            Some(VolumeConfirm::Remove(_)) => true,
            Some(VolumeConfirm::Prune(names)) => !names.is_empty(),
            None => false,
        }
    }
}

fn msg(m: Msg) -> Message {
    Message::Volumes(m)
}

/// Key of the text shown instead of the list, if there is nothing to list.
fn empty_key(state: &VolumesState) -> Option<&'static str> {
    if state.items.is_empty() {
        Some("volumes.empty")
    } else if state.visible().is_empty() {
        Some("volumes.empty_filtered")
    } else {
        None
    }
}

// Column widths, shared by the header and the rows so they line up.
const NAME_WIDTH: Length = Length::FillPortion(3);
const DRIVER_WIDTH: Length = Length::FillPortion(1);
const MOUNTPOINT_WIDTH: Length = Length::FillPortion(4);
const USED_BY_WIDTH: Length = Length::FillPortion(2);
/// Room for the remove button and the spacer in front of it in `row_actions`.
const ACTIONS_WIDTH: f32 = ICON_BUTTON_SIZE + SPACE_1;

/// A single line of text that is cut, not wrapped, when the column is too narrow.
fn cell<'a>(content: impl Into<Element<'a, Message>>, width: Length) -> Element<'a, Message> {
    container(content).width(width).clip(true).into()
}

/// Names of the containers using a volume, or the dash when nobody does.
fn used_by_text(v: &VolumeInfo) -> String {
    if v.used_by.is_empty() {
        t!("volumes.not_used").into_owned()
    } else {
        v.used_by.join(", ")
    }
}

fn row_actions<'a>(v: &VolumeInfo, busy: bool, selected: bool) -> Element<'a, Message> {
    let in_use = !v.used_by.is_empty();
    // A volume in use is not offered for removal: Docker would refuse; the tooltip says why.
    let tip = if in_use {
        t!("volumes.in_use", names = v.used_by.join(", ")).into_owned()
    } else if selected {
        actions::tip(ActionId::RemoveSelected, &t!("volumes.remove"))
    } else {
        t!("volumes.remove").into_owned()
    };
    let on_press = (!in_use && !busy).then(|| msg(Msg::AskRemove(v.name.clone())));
    container(
        row![
            space().width(Length::Fill),
            icon_button(Icon::Trash2, on_press, tip)
        ]
        .spacing(SPACE_1)
        .align_y(Alignment::Center),
    )
    .width(ACTIONS_WIDTH)
    .into()
}

fn row_view<'a>(
    v: &'a VolumeInfo,
    selected: bool,
    hovered: bool,
    busy: bool,
) -> Element<'a, Message> {
    let one_line = |t: iced::widget::Text<'a>| t.wrapping(Wrapping::None);

    // The mountpoint is long: one line, the whole path in the tooltip.
    let mountpoint_tip = container(text(v.mountpoint.as_str()).size(TEXT_SM).font(MONO))
        .padding(Padding::from([SPACE_1, SPACE_2]))
        .style(style::container::elevated);
    let mountpoint = tooltip(
        one_line(
            text(v.mountpoint.as_str())
                .size(TEXT_SM)
                .font(MONO)
                .style(style::text::muted),
        ),
        mountpoint_tip,
        tooltip::Position::Bottom,
    )
    .gap(SPACE_1);

    let mut cells = row![
        cell(
            one_line(text(v.name.as_str()).size(TEXT).font(SANS_MEDIUM)),
            NAME_WIDTH
        ),
        cell(
            one_line(
                text(v.driver.as_str())
                    .size(TEXT_SM)
                    .font(MONO)
                    .style(style::text::muted)
            ),
            DRIVER_WIDTH
        ),
        cell(mountpoint, MOUNTPOINT_WIDTH),
        cell(
            one_line(
                text(used_by_text(v))
                    .size(TEXT_SM)
                    .style(style::text::faint)
            ),
            USED_BY_WIDTH
        ),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center);
    if !(selected || hovered) {
        // Keeps the columns where they are when the actions appear.
        cells = cells.push(space().width(ACTIONS_WIDTH));
    }

    // There is nothing to open in a volume: a double click only selects.
    let events = RowEvents {
        on_select: msg(Msg::Select(v.name.clone())),
        on_open: msg(Msg::Select(v.name.clone())),
        on_enter: msg(Msg::Hover(v.name.clone())),
        on_exit: msg(Msg::Leave(v.name.clone())),
    };
    list_row(
        cells.width(Length::Fill).into(),
        Some(row_actions(v, busy, selected)),
        selected,
        hovered,
        events,
    )
}

/// The content of the dialog; [`crate::ui::components::modal`] frames it. `None` while nothing
/// is being confirmed.
pub fn dialog(state: &VolumesState) -> Option<Element<'static, Message>> {
    let confirm = state.confirm.as_ref()?;
    let (title, body, label) = match confirm {
        VolumeConfirm::Remove(name) => (
            t!("volumes.remove_dialog.title"),
            column![text(
                t!("volumes.remove_dialog.body", name = name.as_str()).into_owned()
            )],
            t!("volumes.remove_dialog.confirm"),
        ),
        VolumeConfirm::Prune(names) => (
            t!("volumes.prune_dialog.title"),
            prune_body(names),
            t!("volumes.prune_dialog.confirm"),
        ),
    };
    let confirm_button = button(
        row![
            text(label.into_owned()),
            kbd(&Shortcut {
                ctrl: false,
                shift: false,
                key: "Enter",
            }),
        ]
        .spacing(SPACE_2)
        .align_y(Alignment::Center),
    )
    .style(style::button::danger)
    .on_press_maybe(state.can_confirm().then(|| msg(Msg::Confirm)));
    let card = column![
        text(title.into_owned()).size(TEXT_LG).font(SANS_SEMIBOLD),
        body,
        row![
            space().width(Length::Fill),
            button(text(t!("common.cancel").into_owned()))
                .style(style::button::ghost)
                .on_press(msg(Msg::Cancel)),
            confirm_button,
        ]
        .spacing(SPACE_2),
    ]
    .spacing(SPACE_4);
    Some(card.into())
}

/// What the prune dialog says: the volumes that will go, or that there are none.
fn prune_body(names: &[String]) -> Column<'static, Message> {
    if names.is_empty() {
        return column![text(t!("volumes.prune_dialog.empty").into_owned())];
    }
    // Rows of a fixed height, so the dialog's height is known before it scrolls.
    let list = Column::with_children(names.iter().map(|n| {
        container(text(n.clone()).size(TEXT_SM).font(MONO))
            .height(ROW_HEIGHT)
            .center_y(ROW_HEIGHT)
            .into()
    }));
    // A long list scrolls inside the dialog instead of growing past the window.
    let list: Element<'static, Message> = if names.len() > DIALOG_LIST_ROWS {
        scrollable(list)
            .direction(style::thin_scrollbar())
            .style(style::scrollable_thin)
            .height(DIALOG_LIST_HEIGHT)
            .into()
    } else {
        list.into()
    };
    column![
        text(t!("volumes.prune_dialog.body").into_owned()),
        container(list).padding(Padding::from([0.0, SPACE_2])),
    ]
    .spacing(SPACE_2)
}

/// Names the prune dialog lists before it starts to scroll.
const DIALOG_LIST_ROWS: usize = 8;

/// The Volumes screen. The dialog is drawn by `App::view` over the whole window.
pub fn view(state: &VolumesState) -> Element<'_, Message> {
    let visible = state.visible();
    let prune = button(text(t!("volumes.prune").into_owned()).size(TEXT_SM))
        .style(style::button::ghost)
        .on_press(msg(Msg::AskPrune));
    let bar = toolbar(
        search_field(
            Id::new(SEARCH_ID),
            t!("volumes.search").into_owned(),
            &state.query,
            |q| msg(Msg::Query(q)),
        ),
        vec![prune.into()],
        visible.len(),
    );

    let body: Element<'_, Message> = match empty_key(state) {
        Some(key) => center(
            text(t!(key).into_owned())
                .style(style::text::muted)
                .align_x(Alignment::Center),
        )
        .padding(SPACE_6)
        .into(),
        None => {
            let header = list_header(vec![
                (t!("volumes.col.name").into_owned(), NAME_WIDTH),
                (t!("volumes.col.driver").into_owned(), DRIVER_WIDTH),
                (t!("volumes.col.mountpoint").into_owned(), MOUNTPOINT_WIDTH),
                (t!("volumes.col.used_by").into_owned(), USED_BY_WIDTH),
                (String::new(), Length::Fixed(ACTIONS_WIDTH)),
            ]);
            let rows = Column::with_children(visible.into_iter().map(|v| {
                row_view(
                    v,
                    state.selected.as_deref() == Some(v.name.as_str()),
                    state.hovered.as_deref() == Some(v.name.as_str()),
                    state.pending.contains(&v.name),
                )
            }));
            // The sensor reports the height of the area the rows scroll in.
            let list = sensor(
                scrollable(rows)
                    .id(Id::new(LIST_ID))
                    .direction(style::thin_scrollbar())
                    .style(style::scrollable_thin)
                    .on_scroll(|v| {
                        msg(Msg::Scrolled {
                            offset: v.absolute_offset().y,
                            height: v.bounds().height,
                        })
                    })
                    .height(Length::Fill),
            )
            .on_show(|size| msg(Msg::Resized(size.height)))
            .on_resize(|size| msg(Msg::Resized(size.height)));
            column![header, list].into()
        }
    };

    column![bar, body]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vol(name: &str, driver: &str, used_by: &[&str]) -> VolumeInfo {
        VolumeInfo {
            name: name.into(),
            driver: driver.into(),
            mountpoint: format!("/var/lib/docker/volumes/{name}/_data"),
            used_by: used_by.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn state_with(items: Vec<VolumeInfo>) -> VolumesState {
        VolumesState {
            items,
            ..VolumesState::default()
        }
    }

    fn names(state: &VolumesState) -> Vec<&str> {
        state.visible().iter().map(|v| v.name.as_str()).collect()
    }

    #[test]
    fn visible_filters_by_name_or_driver_and_sorts_by_name() {
        let mut s = state_with(vec![
            vol("db_data", "local", &["db"]),
            vol("Cache", "local", &[]),
            vol("nfs_share", "nfs", &[]),
        ]);
        assert_eq!(names(&s), vec!["Cache", "db_data", "nfs_share"]);
        s.query = " DB ".into();
        assert_eq!(names(&s), vec!["db_data"]);
        s.query = "nfs".into();
        assert_eq!(names(&s), vec!["nfs_share"]);
        s.query = "zzz".into();
        assert!(s.visible().is_empty());
    }

    #[test]
    fn unused_lists_only_volumes_nobody_uses() {
        let s = state_with(vec![
            vol("db_data", "local", &["db"]),
            vol("cache", "local", &[]),
            vol("tmp", "local", &[]),
        ]);
        assert_eq!(s.unused(), vec!["cache", "tmp"]);
    }

    #[test]
    fn selection_moves_to_neighbor_when_removed() {
        let abc = || {
            vec![
                vol("a", "local", &[]),
                vol("b", "local", &[]),
                vol("c", "local", &[]),
            ]
        };
        let mut s = state_with(abc());
        s.selected = Some("b".into());
        s.set_items(vec![abc().remove(0), abc().remove(2)]);
        assert_eq!(s.selected.as_deref(), Some("c"));

        let mut s = state_with(abc());
        s.selected = Some("c".into());
        s.set_items(abc()[..2].to_vec());
        assert_eq!(s.selected.as_deref(), Some("b"));

        let mut s = state_with(abc());
        s.selected = Some("b".into());
        s.set_items(abc());
        assert_eq!(s.selected.as_deref(), Some("b"));
        s.set_items(vec![]);
        assert_eq!(s.selected, None);
    }

    #[test]
    fn hover_is_cleared_when_the_volume_is_gone() {
        let mut s = state_with(vec![vol("a", "local", &[])]);
        s.hovered = Some("a".into());
        s.set_items(vec![]);
        assert_eq!(s.hovered, None);
    }

    #[test]
    fn selection_moves_to_first_visible_when_the_search_hides_it() {
        let mut s = state_with(vec![vol("alpha", "local", &[]), vol("beta", "local", &[])]);
        s.selected = Some("beta".into());
        s.query = "alp".into();
        s.keep_selection_visible();
        assert_eq!(s.selected.as_deref(), Some("alpha"));
        s.query = "zzz".into();
        s.keep_selection_visible();
        assert_eq!(s.selected, None);
    }

    #[test]
    fn set_items_drops_a_dialog_about_a_vanished_volume() {
        let mut s = state_with(vec![vol("a", "local", &[])]);
        s.confirm = Some(VolumeConfirm::Remove("a".into()));
        s.set_items(vec![]);
        assert_eq!(s.confirm, None);
    }

    #[test]
    fn set_items_refreshes_the_prune_list() {
        let mut s = state_with(vec![vol("a", "local", &[]), vol("b", "local", &["web"])]);
        s.confirm = Some(VolumeConfirm::Prune(s.unused()));
        // `b` is free now, `a` was taken.
        s.set_items(vec![vol("a", "local", &["db"]), vol("b", "local", &[])]);
        assert_eq!(s.confirm, Some(VolumeConfirm::Prune(vec!["b".into()])));
        s.set_items(vec![vol("a", "local", &["db"])]);
        assert_eq!(s.confirm, Some(VolumeConfirm::Prune(vec![])));
        assert!(!s.can_confirm());
    }

    #[test]
    fn prune_dialog_with_nothing_to_remove_cannot_be_confirmed() {
        let mut s = state_with(vec![]);
        assert!(!s.can_confirm(), "no dialog");
        s.confirm = Some(VolumeConfirm::Prune(vec![]));
        assert!(!s.can_confirm());
        s.confirm = Some(VolumeConfirm::Prune(vec!["x".into()]));
        assert!(s.can_confirm());
        s.confirm = Some(VolumeConfirm::Remove("x".into()));
        assert!(s.can_confirm());
    }

    #[test]
    fn view_and_dialog_build_in_every_state() {
        let mut long = vol(&"n".repeat(200), "local", &["a", "b"]);
        long.mountpoint = "/var/lib/docker/volumes/".repeat(20);
        let mut s = state_with(vec![
            vol("db_data", "local", &["db", "web"]),
            vol("cache", "local", &[]),
            long,
        ]);
        let _ = view(&s);
        s.selected = Some("cache".into());
        s.hovered = Some("db_data".into());
        s.pending.insert("cache".into());
        let _ = view(&s);
        for confirm in [
            VolumeConfirm::Remove("cache".into()),
            VolumeConfirm::Prune(vec![]),
            VolumeConfirm::Prune((0..50).map(|i| format!("vol{i}")).collect()),
        ] {
            s.confirm = Some(confirm);
            assert!(dialog(&s).is_some());
            let _ = view(&s);
        }
        s.confirm = None;
        assert!(dialog(&s).is_none());
        let _ = view(&VolumesState::default());
        s.query = "zzz".into();
        let _ = view(&s);
    }

    #[test]
    fn volumes_texts_are_translated() {
        use rust_i18n::t;
        for key in [
            "volumes.search",
            "volumes.empty",
            "volumes.empty_filtered",
            "volumes.col.name",
            "volumes.col.driver",
            "volumes.col.mountpoint",
            "volumes.col.used_by",
            "volumes.not_used",
            "volumes.remove",
            "volumes.in_use",
            "volumes.remove_dialog.title",
            "volumes.remove_dialog.body",
            "volumes.remove_dialog.confirm",
            "volumes.prune",
            "volumes.prune_dialog.title",
            "volumes.prune_dialog.body",
            "volumes.prune_dialog.empty",
            "volumes.prune_dialog.confirm",
            "volumes.pruned",
            "volumes.pruned_one",
            "actions.prune_volumes",
        ] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(
                    t!(key, locale = locale, name = "x", names = "x", count = 2),
                    key,
                    "missing {locale} translation for {key}"
                );
            }
        }
    }
}
