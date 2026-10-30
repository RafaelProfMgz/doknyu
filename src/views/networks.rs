//! Network list: search, selection and per-row removal. The Docker defaults are protected.

use std::collections::HashSet;

use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Id, button, center, column, container, row, scrollable, sensor, space, text,
};
use iced::{Alignment, Element, Length};
use rust_i18n::t;

use crate::actions::{self, ActionId};
use crate::app::Message;
use crate::error::DockerError;
use crate::model::NetworkInfo;
use crate::ui::components::{
    RowEvents, Shortcut, icon_button, kbd, list_header, list_row, search_field,
};
use crate::ui::fonts::{MONO, SANS_MEDIUM, SANS_SEMIBOLD};
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::tokens::{
    ICON_BUTTON_SIZE, SPACE_1, SPACE_2, SPACE_4, SPACE_6, TEXT, TEXT_LG, TEXT_SM,
};
use crate::views::toolbar::toolbar;

/// Widget id of the search box, so a shortcut can focus it.
pub const SEARCH_ID: &str = "networks-search";

/// Widget id of the scrollable with the rows, so the keyboard selection can scroll it.
pub const LIST_ID: &str = "networks-list";

/// The networks Docker creates itself; it refuses to remove them, so the app never offers to.
pub const PROTECTED_NETWORKS: [&str; 3] = ["bridge", "host", "none"];

/// Whether this is one of Docker's default networks (by name).
pub fn is_protected(n: &NetworkInfo) -> bool {
    PROTECTED_NETWORKS.contains(&n.name.as_str())
}

/// Networks are told apart by id (what `remove_network` takes), not by name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NetworksState {
    pub items: Vec<NetworkInfo>,
    pub query: String,
    /// The network whose removal the open dialog asks about.
    pub confirm_remove: Option<String>,
    /// Networks with a removal in flight.
    pub pending: HashSet<String>,
    /// The highlighted row.
    pub selected: Option<String>,
    /// The row under the cursor.
    pub hovered: Option<String>,
    /// Scroll position of the rows and the height of the area showing them, as last reported.
    pub scroll_y: f32,
    pub viewport_h: f32,
}

/// Messages of the network list. Ids are network ids.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A list reload finished; the number identifies which reload it answers.
    Loaded(u64, Result<Vec<NetworkInfo>, DockerError>),
    Query(String),
    /// A click on a row.
    Select(String),
    Hover(String),
    Leave(String),
    AskRemove(String),
    /// The dialog's main button (or Enter).
    Confirm,
    Cancel,
    Removed(String, Result<(), DockerError>),
    Scrolled {
        offset: f32,
        height: f32,
    },
    Resized(f32),
}

impl NetworksState {
    /// Items matching the search (name, driver or subnet, case-insensitive), by name.
    pub fn visible(&self) -> Vec<&NetworkInfo> {
        let query = self.query.trim().to_lowercase();
        let mut items: Vec<&NetworkInfo> = self
            .items
            .iter()
            .filter(|n| {
                query.is_empty()
                    || n.name.to_lowercase().contains(&query)
                    || n.driver.to_lowercase().contains(&query)
                    || n.subnet
                        .as_deref()
                        .is_some_and(|s| s.to_lowercase().contains(&query))
            })
            .collect();
        items.sort_by_cached_key(|n| n.name.to_lowercase());
        items
    }

    /// A network by id.
    pub fn find(&self, id: &str) -> Option<&NetworkInfo> {
        self.items.iter().find(|n| n.id == id)
    }

    /// Whether the app offers to remove this network: it exists, is not one of Docker's
    /// defaults and has no removal running. One with containers connected is offered: the
    /// dialog warns and Docker has the last word.
    pub fn removable(&self, id: &str) -> bool {
        self.find(id).is_some_and(|n| !is_protected(n)) && !self.pending.contains(id)
    }

    /// After the search changed: a selection that is no longer in [`Self::visible`] moves to
    /// the first visible row, or to nothing if the list is empty.
    pub fn keep_selection_visible(&mut self) {
        let visible = self.visible();
        let hidden = self
            .selected
            .as_deref()
            .is_some_and(|id| !visible.iter().any(|n| n.id == id));
        if hidden {
            self.selected = visible.first().map(|n| n.id.clone());
        }
    }

    /// Replaces the list after a reload. A selected network that is gone hands the selection to
    /// the one now at its position in [`Self::visible`] (or the last one, or nothing).
    pub fn set_items(&mut self, items: Vec<NetworkInfo>) {
        let old_position = self
            .selected
            .as_deref()
            .and_then(|id| self.visible().iter().position(|n| n.id == id));
        self.items = items;
        let exists = |id: &str| self.items.iter().any(|n| n.id == id);
        if self.selected.as_deref().is_some_and(|id| !exists(id)) {
            let visible = self.visible();
            let position = old_position
                .unwrap_or(usize::MAX)
                .min(visible.len().saturating_sub(1));
            self.selected = visible.get(position).map(|n| n.id.clone());
        }
        if self.hovered.as_deref().is_some_and(|id| !exists(id)) {
            self.hovered = None;
        }
        // The network to remove is gone: nothing left to confirm.
        if self.confirm_remove.as_deref().is_some_and(|id| !exists(id)) {
            self.confirm_remove = None;
        }
    }
}

fn msg(m: Msg) -> Message {
    Message::Networks(m)
}

/// Key of the text shown instead of the list, if there is nothing to list.
fn empty_key(state: &NetworksState) -> Option<&'static str> {
    if state.items.is_empty() {
        Some("networks.empty")
    } else if state.visible().is_empty() {
        Some("networks.empty_filtered")
    } else {
        None
    }
}

// Column widths, shared by the header and the rows so they line up.
const NAME_WIDTH: Length = Length::FillPortion(3);
const DRIVER_WIDTH: Length = Length::FillPortion(1);
const SUBNET_WIDTH: Length = Length::FillPortion(2);
const CONTAINERS_WIDTH: Length = Length::FillPortion(3);
/// Room for the remove button and the spacer in front of it in `row_actions`.
const ACTIONS_WIDTH: f32 = ICON_BUTTON_SIZE + SPACE_1;

/// A single line of text that is cut, not wrapped, when the column is too narrow.
fn cell<'a>(content: impl Into<Element<'a, Message>>, width: Length) -> Element<'a, Message> {
    container(content).width(width).clip(true).into()
}

/// The text of a column that may be empty: the dash stands for "nothing".
fn or_dash(text: Option<String>) -> String {
    text.unwrap_or_else(|| t!("networks.none").into_owned())
}

fn containers_text(n: &NetworkInfo) -> String {
    or_dash((!n.containers.is_empty()).then(|| n.containers.join(", ")))
}

fn row_actions<'a>(n: &NetworkInfo, busy: bool, selected: bool) -> Element<'a, Message> {
    let button = if is_protected(n) {
        // The lock says why there is nothing to press.
        icon_button(Icon::Lock, None, t!("networks.protected").into_owned())
    } else {
        let tip = if selected {
            actions::tip(ActionId::RemoveSelected, &t!("networks.remove"))
        } else {
            t!("networks.remove").into_owned()
        };
        let on_press = (!busy).then(|| msg(Msg::AskRemove(n.id.clone())));
        icon_button(Icon::Trash2, on_press, tip)
    };
    container(
        row![space().width(Length::Fill), button]
            .spacing(SPACE_1)
            .align_y(Alignment::Center),
    )
    .width(ACTIONS_WIDTH)
    .into()
}

fn row_view<'a>(
    n: &'a NetworkInfo,
    selected: bool,
    hovered: bool,
    busy: bool,
) -> Element<'a, Message> {
    let one_line = |t: iced::widget::Text<'a>| t.wrapping(Wrapping::None);
    let mut cells = row![
        cell(
            one_line(text(n.name.as_str()).size(TEXT).font(SANS_MEDIUM)),
            NAME_WIDTH
        ),
        cell(
            one_line(
                text(n.driver.as_str())
                    .size(TEXT_SM)
                    .font(MONO)
                    .style(style::text::muted)
            ),
            DRIVER_WIDTH
        ),
        cell(
            one_line(
                text(or_dash(n.subnet.clone()))
                    .size(TEXT_SM)
                    .font(MONO)
                    .style(style::text::muted)
            ),
            SUBNET_WIDTH
        ),
        cell(
            one_line(
                text(containers_text(n))
                    .size(TEXT_SM)
                    .style(style::text::faint)
            ),
            CONTAINERS_WIDTH
        ),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center);
    if !(selected || hovered) {
        // Keeps the columns where they are when the actions appear.
        cells = cells.push(space().width(ACTIONS_WIDTH));
    }

    // There is nothing to open in a network: a double click only selects.
    let events = RowEvents {
        on_select: msg(Msg::Select(n.id.clone())),
        on_open: msg(Msg::Select(n.id.clone())),
        on_enter: msg(Msg::Hover(n.id.clone())),
        on_exit: msg(Msg::Leave(n.id.clone())),
    };
    list_row(
        cells.width(Length::Fill).into(),
        Some(row_actions(n, busy, selected)),
        selected,
        hovered,
        events,
    )
}

/// The content of the dialog; [`crate::ui::components::modal`] frames it. `None` while nothing
/// is being confirmed. If containers are still connected it says so: Docker will refuse.
pub fn dialog(state: &NetworksState) -> Option<Element<'static, Message>> {
    let network = state.find(state.confirm_remove.as_deref()?)?;
    let mut body = column![text(
        t!("networks.remove_dialog.body", name = network.name.as_str()).into_owned()
    )]
    .spacing(SPACE_2);
    if !network.containers.is_empty() {
        body = body.push(
            text(
                t!(
                    "networks.remove_dialog.in_use",
                    names = network.containers.join(", ")
                )
                .into_owned(),
            )
            .size(TEXT_SM)
            .style(style::text::muted),
        );
    }
    let confirm_button = button(
        row![
            text(t!("networks.remove_dialog.confirm").into_owned()),
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
    .on_press(msg(Msg::Confirm));
    let card = column![
        text(t!("networks.remove_dialog.title").into_owned())
            .size(TEXT_LG)
            .font(SANS_SEMIBOLD),
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

/// The Networks screen. The dialog is drawn by `App::view` over the whole window.
pub fn view(state: &NetworksState) -> Element<'_, Message> {
    let visible = state.visible();
    let bar = toolbar(
        search_field(
            Id::new(SEARCH_ID),
            t!("networks.search").into_owned(),
            &state.query,
            |q| msg(Msg::Query(q)),
        ),
        vec![],
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
                (t!("networks.col.name").into_owned(), NAME_WIDTH),
                (t!("networks.col.driver").into_owned(), DRIVER_WIDTH),
                (t!("networks.col.subnet").into_owned(), SUBNET_WIDTH),
                (t!("networks.col.containers").into_owned(), CONTAINERS_WIDTH),
                (String::new(), Length::Fixed(ACTIONS_WIDTH)),
            ]);
            let rows = Column::with_children(visible.into_iter().map(|n| {
                row_view(
                    n,
                    state.selected.as_deref() == Some(n.id.as_str()),
                    state.hovered.as_deref() == Some(n.id.as_str()),
                    state.pending.contains(&n.id),
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

    fn net(name: &str, driver: &str, subnet: Option<&str>, containers: &[&str]) -> NetworkInfo {
        NetworkInfo {
            id: format!("{name}-id"),
            name: name.into(),
            driver: driver.into(),
            subnet: subnet.map(String::from),
            containers: containers.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn state_with(items: Vec<NetworkInfo>) -> NetworksState {
        NetworksState {
            items,
            ..NetworksState::default()
        }
    }

    fn names(state: &NetworksState) -> Vec<&str> {
        state.visible().iter().map(|n| n.name.as_str()).collect()
    }

    #[test]
    fn default_networks_are_protected() {
        for name in ["bridge", "host", "none"] {
            assert!(is_protected(&net(name, "x", None, &[])), "{name}");
        }
        assert_eq!(PROTECTED_NETWORKS, ["bridge", "host", "none"]);
        // The name decides, not the driver: a user network on the bridge driver is not default.
        assert!(!is_protected(&net("app_net", "bridge", None, &[])));
        assert!(!is_protected(&net("bridge2", "bridge", None, &[])));
        assert!(!is_protected(&net("Bridge", "bridge", None, &[])));
    }

    #[test]
    fn visible_filters_by_name_driver_or_subnet_and_sorts_by_name() {
        let mut s = state_with(vec![
            net("host", "host", None, &[]),
            net("app_net", "bridge", Some("172.20.0.0/16"), &["web"]),
            net("Backend", "overlay", Some("10.0.0.0/24"), &[]),
        ]);
        assert_eq!(names(&s), vec!["app_net", "Backend", "host"]);
        s.query = " BACK ".into();
        assert_eq!(names(&s), vec!["Backend"]);
        s.query = "overlay".into();
        assert_eq!(names(&s), vec!["Backend"]);
        s.query = "172.20".into();
        assert_eq!(names(&s), vec!["app_net"]);
        s.query = "zzz".into();
        assert!(s.visible().is_empty());
    }

    #[test]
    fn removable_excludes_protected_busy_and_unknown_networks() {
        let mut s = state_with(vec![
            net("bridge", "bridge", None, &[]),
            net("app_net", "bridge", None, &["web"]),
            net("scratch", "bridge", None, &[]),
        ]);
        assert!(!s.removable("bridge-id"));
        assert!(s.removable("app_net-id"), "in use: Docker decides");
        assert!(s.removable("scratch-id"));
        assert!(!s.removable("gone-id"));
        s.pending.insert("scratch-id".into());
        assert!(!s.removable("scratch-id"));
    }

    #[test]
    fn selection_moves_to_neighbor_when_removed() {
        let abc = || {
            vec![
                net("a", "bridge", None, &[]),
                net("b", "bridge", None, &[]),
                net("c", "bridge", None, &[]),
            ]
        };
        let mut s = state_with(abc());
        s.selected = Some("b-id".into());
        s.set_items(vec![abc().remove(0), abc().remove(2)]);
        assert_eq!(s.selected.as_deref(), Some("c-id"));

        let mut s = state_with(abc());
        s.selected = Some("c-id".into());
        s.set_items(abc()[..2].to_vec());
        assert_eq!(s.selected.as_deref(), Some("b-id"));

        let mut s = state_with(abc());
        s.selected = Some("b-id".into());
        s.set_items(abc());
        assert_eq!(s.selected.as_deref(), Some("b-id"));
        s.set_items(vec![]);
        assert_eq!(s.selected, None);
    }

    #[test]
    fn hover_and_dialog_are_cleared_when_the_network_is_gone() {
        let mut s = state_with(vec![net("a", "bridge", None, &[])]);
        s.hovered = Some("a-id".into());
        s.confirm_remove = Some("a-id".into());
        s.set_items(vec![net("a", "bridge", None, &[])]);
        assert_eq!(s.hovered.as_deref(), Some("a-id"));
        assert_eq!(s.confirm_remove.as_deref(), Some("a-id"));
        s.set_items(vec![]);
        assert_eq!(s.hovered, None);
        assert_eq!(s.confirm_remove, None);
    }

    #[test]
    fn selection_moves_to_first_visible_when_the_search_hides_it() {
        let mut s = state_with(vec![
            net("alpha", "bridge", None, &[]),
            net("beta", "bridge", None, &[]),
        ]);
        s.selected = Some("beta-id".into());
        s.query = "alp".into();
        s.keep_selection_visible();
        assert_eq!(s.selected.as_deref(), Some("alpha-id"));
        s.query = "zzz".into();
        s.keep_selection_visible();
        assert_eq!(s.selected, None);
    }

    #[test]
    fn view_and_dialog_build_in_every_state() {
        let mut s = state_with(vec![
            net("bridge", "bridge", Some("172.17.0.0/16"), &[]),
            net("app_net", "bridge", None, &["web", "db"]),
            net(
                &"n".repeat(200),
                "bridge",
                Some(&"1".repeat(100)),
                &["a"; 30],
            ),
            net("scratch", "bridge", None, &[]),
        ]);
        let _ = view(&s);
        s.selected = Some("scratch-id".into());
        s.hovered = Some("bridge-id".into());
        s.pending.insert("scratch-id".into());
        let _ = view(&s);
        for id in ["scratch-id", "app_net-id", "gone-id"] {
            s.confirm_remove = Some(id.into());
            assert_eq!(dialog(&s).is_some(), id != "gone-id", "{id}");
            let _ = view(&s);
        }
        s.confirm_remove = None;
        assert!(dialog(&s).is_none());
        let _ = view(&NetworksState::default());
        s.query = "zzz".into();
        let _ = view(&s);
    }

    #[test]
    fn networks_texts_are_translated() {
        use rust_i18n::t;
        for key in [
            "networks.search",
            "networks.empty",
            "networks.empty_filtered",
            "networks.col.name",
            "networks.col.driver",
            "networks.col.subnet",
            "networks.col.containers",
            "networks.none",
            "networks.remove",
            "networks.protected",
            "networks.remove_dialog.title",
            "networks.remove_dialog.body",
            "networks.remove_dialog.in_use",
            "networks.remove_dialog.confirm",
        ] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(
                    t!(key, locale = locale, name = "x", names = "x"),
                    key,
                    "missing {locale} translation for {key}"
                );
            }
        }
    }
}
