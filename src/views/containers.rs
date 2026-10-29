//! Container list: search, filters, selection, per-row actions and the removal dialog.

use std::collections::HashSet;

use iced::widget::text::Wrapping;
use iced::widget::{Column, Id, button, center, column, container, row, scrollable, space, text};
use iced::{Alignment, Element, Length};
use rust_i18n::t;

use crate::app::Message;
use crate::error::DockerError;
use crate::model::{ContainerInfo, ContainerState};
use crate::ui::components::{
    RowEvents, chip, icon_button, list_header, list_row, search_field, status_dot,
};
use crate::ui::fonts::{MONO, SANS_MEDIUM, SANS_SEMIBOLD};
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::tokens::{
    DOT_SIZE, ICON_BUTTON_SIZE, SPACE_1, SPACE_2, SPACE_4, SPACE_6, TEXT, TEXT_LG, TEXT_SM,
};
use crate::views::toolbar::toolbar;

/// Widget id of the search box, so a shortcut can focus it.
pub const SEARCH_ID: &str = "containers-search";

#[derive(Debug, Clone, PartialEq)]
pub struct ContainersState {
    pub items: Vec<ContainerInfo>,
    pub query: String,
    /// The "Running" chip: running, paused and restarting containers.
    pub show_running: bool,
    /// The "Stopped" chip: everything else.
    pub show_stopped: bool,
    /// The highlighted row, by container id.
    pub selected: Option<String>,
    /// The row under the cursor, by container id.
    pub hovered: Option<String>,
    pub pending: HashSet<String>,
    pub confirm_remove: Option<String>,
}

impl Default for ContainersState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            query: String::new(),
            show_running: true,
            show_stopped: true,
            selected: None,
            hovered: None,
            pending: HashSet::new(),
            confirm_remove: None,
        }
    }
}

/// Messages of the container list.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A list reload finished; the number identifies which reload it answers.
    Loaded(u64, Result<Vec<ContainerInfo>, DockerError>),
    Query(String),
    ToggleRunning,
    ToggleStopped,
    /// A click on a row.
    Select(String),
    /// The cursor entered a row.
    Hover(String),
    /// The cursor left a row. Ignored if another row has been entered since, because the
    /// enter of the new row can be reported before the exit of the old one.
    Leave(String),
    Start(String),
    Stop(String),
    Restart(String),
    AskRemove(String),
    ConfirmRemove {
        force: bool,
    },
    CancelRemove,
    ActionDone(String, Result<(), DockerError>),
    OpenPort(u16),
    /// A double click on a row: shows the container in the dock.
    Open(String),
}

/// Containers that count for the "Stopped" chip: everything the "Running" chip does not take.
fn is_stopped(state: &ContainerState) -> bool {
    !matches!(
        state,
        ContainerState::Running | ContainerState::Paused | ContainerState::Restarting
    )
}

impl ContainersState {
    /// Items matching the search (name or image, case-insensitive) and the chips; running
    /// first, then by name.
    pub fn visible(&self) -> Vec<&ContainerInfo> {
        let query = self.query.trim().to_lowercase();
        let mut items: Vec<&ContainerInfo> = self
            .items
            .iter()
            .filter(|c| {
                if is_stopped(&c.state) {
                    self.show_stopped
                } else {
                    self.show_running
                }
            })
            .filter(|c| {
                query.is_empty()
                    || c.name.to_lowercase().contains(&query)
                    || c.image.to_lowercase().contains(&query)
            })
            .collect();
        items.sort_by_cached_key(|c| (!c.state.is_running(), c.name.to_lowercase()));
        items
    }

    /// Replaces the list after a reload. A selected container that is gone hands the selection
    /// to the one now at its position in [`Self::visible`] (or the last one, or nothing).
    pub fn set_items(&mut self, items: Vec<ContainerInfo>) {
        let old_position = self
            .selected
            .as_deref()
            .and_then(|id| self.visible().iter().position(|c| c.id == id));
        self.items = items;
        let exists = |id: &str| self.items.iter().any(|c| c.id == id);
        if self.selected.as_deref().is_some_and(|id| !exists(id)) {
            let visible = self.visible();
            let position = old_position
                .unwrap_or(usize::MAX)
                .min(visible.len().saturating_sub(1));
            self.selected = visible.get(position).map(|c| c.id.clone());
        }
        if self.hovered.as_deref().is_some_and(|id| !exists(id)) {
            self.hovered = None;
        }
    }
}

/// Key of the text shown instead of the list, if there is nothing to list.
fn empty_key(state: &ContainersState) -> Option<&'static str> {
    if state.items.is_empty() {
        Some("containers.empty")
    } else if state.visible().is_empty() {
        Some("containers.empty_filtered")
    } else {
        None
    }
}

fn msg(m: Msg) -> Message {
    Message::Containers(m)
}

// Column widths, shared by the header and the rows so they line up.
const NAME_WIDTH: Length = Length::FillPortion(3);
const IMAGE_WIDTH: Length = Length::FillPortion(3);
const PORTS_WIDTH: Length = Length::FillPortion(2);
const STATUS_WIDTH: f32 = 8.0 * SPACE_4;
/// Room for the widest set of actions (stop, restart, remove).
const ACTIONS_WIDTH: f32 = 3.0 * ICON_BUTTON_SIZE + 2.0 * SPACE_1;

/// Public ports of the container, each once (Docker lists IPv4 and IPv6 separately).
fn public_ports(c: &ContainerInfo) -> Vec<u16> {
    let mut ports = Vec::new();
    for p in c.ports.iter().filter_map(|p| p.public) {
        if !ports.contains(&p) {
            ports.push(p);
        }
    }
    ports
}

/// A single line of text that is cut, not wrapped, when the column is too narrow.
fn cell<'a>(content: impl Into<Element<'a, Message>>, width: Length) -> Element<'a, Message> {
    container(content).width(width).clip(true).into()
}

fn row_actions<'a>(c: &ContainerInfo, busy: bool) -> Element<'a, Message> {
    let id = &c.id;
    let enabled = |m: Msg| (!busy).then(|| msg(m));
    let mut actions = row![space().width(Length::Fill)]
        .spacing(SPACE_1)
        .align_y(Alignment::Center);
    if c.state.is_running() {
        actions = actions
            .push(icon_button(
                Icon::Square,
                enabled(Msg::Stop(id.clone())),
                t!("containers.stop").into_owned(),
            ))
            .push(icon_button(
                Icon::RotateCw,
                enabled(Msg::Restart(id.clone())),
                t!("containers.restart").into_owned(),
            ));
    } else {
        actions = actions.push(icon_button(
            Icon::Play,
            enabled(Msg::Start(id.clone())),
            t!("containers.start").into_owned(),
        ));
    }
    actions = actions.push(icon_button(
        Icon::Trash2,
        enabled(Msg::AskRemove(id.clone())),
        t!("containers.remove").into_owned(),
    ));
    container(actions).width(ACTIONS_WIDTH).into()
}

fn row_view<'a>(
    c: &'a ContainerInfo,
    selected: bool,
    hovered: bool,
    busy: bool,
) -> Element<'a, Message> {
    let one_line = |t: iced::widget::Text<'a>| t.wrapping(Wrapping::None);

    let mut ports = row![].spacing(SPACE_2);
    for p in public_ports(c) {
        ports = ports.push(
            button(one_line(text(p.to_string()).size(TEXT_SM).font(MONO)))
                .padding(0)
                .style(style::button::link)
                .on_press(msg(Msg::OpenPort(p))),
        );
    }

    let status = if busy {
        t!("containers.working").into_owned()
    } else {
        c.status_text.clone()
    };

    let show_actions = selected || hovered;
    let mut cells = row![
        status_dot(&c.state),
        cell(
            one_line(text(c.name.as_str()).size(TEXT).font(SANS_MEDIUM)),
            NAME_WIDTH
        ),
        cell(
            one_line(
                text(c.image.as_str())
                    .size(TEXT_SM)
                    .font(MONO)
                    .style(style::text::muted)
            ),
            IMAGE_WIDTH
        ),
        cell(ports, PORTS_WIDTH),
        cell(
            one_line(text(status).size(TEXT_SM).style(style::text::faint)),
            Length::Fixed(STATUS_WIDTH)
        ),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center);
    if !show_actions {
        // Keeps the columns where they are when the actions appear.
        cells = cells.push(space().width(ACTIONS_WIDTH));
    }

    let events = RowEvents {
        on_select: msg(Msg::Select(c.id.clone())),
        on_open: msg(Msg::Open(c.id.clone())),
        on_enter: msg(Msg::Hover(c.id.clone())),
        on_exit: msg(Msg::Leave(c.id.clone())),
    };
    list_row(
        cells.width(Length::Fill).into(),
        Some(row_actions(c, busy)),
        selected,
        hovered,
        events,
    )
}

/// The content of the removal dialog; [`crate::ui::components::modal`] frames it. `None` while
/// no removal is being confirmed.
pub fn remove_dialog(state: &ContainersState) -> Option<Element<'static, Message>> {
    let id = state.confirm_remove.as_deref()?;
    let found = state.items.iter().find(|c| c.id == id);
    let name = found.map_or(id, |c| c.name.as_str());
    let running = found.is_some_and(|c| c.state.is_running());
    let (body, label) = if running {
        (
            t!("containers.remove_running.body", name = name),
            t!("containers.remove_running.confirm"),
        )
    } else {
        (
            t!("containers.remove_dialog.body", name = name),
            t!("containers.remove_dialog.confirm"),
        )
    };
    let card = column![
        text(t!("containers.remove_dialog.title").into_owned())
            .size(TEXT_LG)
            .font(SANS_SEMIBOLD),
        text(body.into_owned()),
        row![
            space().width(Length::Fill),
            button(text(t!("common.cancel").into_owned()))
                .style(style::button::ghost)
                .on_press(msg(Msg::CancelRemove)),
            button(text(label.into_owned()))
                .style(style::button::danger)
                .on_press(msg(Msg::ConfirmRemove { force: running })),
        ]
        .spacing(SPACE_2),
    ]
    .spacing(SPACE_4);
    Some(card.into())
}

/// The Containers screen. The removal dialog is drawn by `App::view` over the whole window.
pub fn view(state: &ContainersState) -> Element<'_, Message> {
    let visible = state.visible();
    let bar = toolbar(
        search_field(
            Id::new(SEARCH_ID),
            t!("containers.search").into_owned(),
            &state.query,
            |q| msg(Msg::Query(q)),
        ),
        vec![
            chip(
                t!("containers.filter_running").into_owned(),
                state.show_running,
                msg(Msg::ToggleRunning),
            ),
            chip(
                t!("containers.filter_stopped").into_owned(),
                state.show_stopped,
                msg(Msg::ToggleStopped),
            ),
        ],
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
                (String::new(), Length::Fixed(DOT_SIZE)),
                (t!("containers.col.name").into_owned(), NAME_WIDTH),
                (t!("containers.col.image").into_owned(), IMAGE_WIDTH),
                (t!("containers.col.ports").into_owned(), PORTS_WIDTH),
                (
                    t!("containers.col.status").into_owned(),
                    Length::Fixed(STATUS_WIDTH),
                ),
                (String::new(), Length::Fixed(ACTIONS_WIDTH)),
            ]);
            let rows = Column::with_children(visible.into_iter().map(|c| {
                row_view(
                    c,
                    state.selected.as_deref() == Some(c.id.as_str()),
                    state.hovered.as_deref() == Some(c.id.as_str()),
                    state.pending.contains(&c.id),
                )
            }));
            column![
                header,
                scrollable(rows)
                    .direction(style::thin_scrollbar())
                    .style(style::scrollable_thin)
                    .height(Length::Fill),
            ]
            .into()
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
    use crate::model::ContainerState;

    fn info(id: &str, name: &str, image: &str, state: ContainerState) -> ContainerInfo {
        ContainerInfo {
            id: id.into(),
            name: name.into(),
            image: image.into(),
            state,
            status_text: String::new(),
            ports: vec![],
        }
    }

    fn state_with(items: Vec<ContainerInfo>) -> ContainersState {
        ContainersState {
            items,
            ..ContainersState::default()
        }
    }

    fn names(state: &ContainersState) -> Vec<&str> {
        state.visible().iter().map(|c| c.name.as_str()).collect()
    }

    #[test]
    fn visible_filters_by_name_or_image() {
        let mut s = state_with(vec![
            info("1", "web", "nginx", ContainerState::Running),
            info("2", "db", "postgres:16", ContainerState::Running),
            info("3", "POSTGRES-admin", "pgadmin", ContainerState::Running),
        ]);
        s.query = "POSTGRES".into();
        assert_eq!(names(&s), vec!["db", "POSTGRES-admin"]);
        s.query = "web".into();
        assert_eq!(names(&s), vec!["web"]);
        s.query = "nope".into();
        assert!(s.visible().is_empty());
    }

    #[test]
    fn chips_filter_running_and_stopped() {
        let mut s = state_with(vec![
            info("1", "web", "nginx", ContainerState::Running),
            info("2", "worker", "w", ContainerState::Exited { code: 1 }),
            info("3", "new", "n", ContainerState::Created),
            info("4", "dead", "d", ContainerState::Dead),
        ]);
        assert!(s.show_running && s.show_stopped, "both chips start on");
        assert_eq!(s.visible().len(), 4);
        s.show_running = false;
        assert_eq!(names(&s), vec!["dead", "new", "worker"]);
        s.show_running = true;
        s.show_stopped = false;
        assert_eq!(names(&s), vec!["web"]);
        s.show_running = false;
        assert!(s.visible().is_empty());
    }

    #[test]
    fn paused_counts_as_running_chip() {
        let mut s = state_with(vec![
            info("1", "paused", "p", ContainerState::Paused),
            info("2", "loop", "l", ContainerState::Restarting),
            info("3", "off", "o", ContainerState::Exited { code: 0 }),
        ]);
        s.show_stopped = false;
        assert_eq!(names(&s), vec!["loop", "paused"]);
        s.show_stopped = true;
        s.show_running = false;
        assert_eq!(names(&s), vec!["off"]);
    }

    #[test]
    fn empty_filtered_state_when_nothing_matches() {
        let mut s = state_with(vec![info("1", "web", "nginx", ContainerState::Running)]);
        assert_eq!(empty_key(&s), None);
        s.show_running = false;
        s.show_stopped = false;
        assert_eq!(empty_key(&s), Some("containers.empty_filtered"));
        s.show_running = true;
        s.show_stopped = true;
        s.query = "zzz".into();
        assert_eq!(empty_key(&s), Some("containers.empty_filtered"));
        assert_eq!(
            empty_key(&ContainersState::default()),
            Some("containers.empty")
        );
        // The view builds for each of the three cases.
        let _ = view(&s);
        let _ = view(&ContainersState::default());
    }

    #[test]
    fn selection_moves_to_neighbor_when_removed() {
        let abc = || {
            vec![
                info("a", "a", "i", ContainerState::Running),
                info("b", "b", "i", ContainerState::Running),
                info("c", "c", "i", ContainerState::Running),
            ]
        };
        let mut s = state_with(abc());
        s.selected = Some("b".into());
        s.set_items(vec![abc().remove(0), abc().remove(2)]);
        assert_eq!(s.selected.as_deref(), Some("c"));

        // Was the last one: the new last one is selected.
        let mut s = state_with(abc());
        s.selected = Some("c".into());
        s.set_items(abc()[..2].to_vec());
        assert_eq!(s.selected.as_deref(), Some("b"));

        // Still there: kept. Nothing left: none.
        let mut s = state_with(abc());
        s.selected = Some("b".into());
        s.set_items(abc());
        assert_eq!(s.selected.as_deref(), Some("b"));
        s.set_items(vec![]);
        assert_eq!(s.selected, None);
    }

    #[test]
    fn neighbor_follows_the_visible_order_not_the_item_order() {
        // Visible order is running first, then by name: x (running), a, b.
        let items = |with_a: bool| {
            let mut v = vec![
                info("x", "x", "i", ContainerState::Running),
                info("b", "b", "i", ContainerState::Created),
            ];
            if with_a {
                v.push(info("a", "a", "i", ContainerState::Created));
            }
            v
        };
        let mut s = state_with(items(true));
        s.selected = Some("a".into());
        s.set_items(items(false));
        assert_eq!(s.selected.as_deref(), Some("b"));
    }

    #[test]
    fn hover_is_cleared_when_the_container_is_gone() {
        let mut s = state_with(vec![info("a", "a", "i", ContainerState::Running)]);
        s.hovered = Some("a".into());
        s.set_items(vec![]);
        assert_eq!(s.hovered, None);
    }

    #[test]
    fn long_names_do_not_panic() {
        let mut long = info(
            "long",
            &"n".repeat(200),
            &"registry.example.com/team/image:".repeat(8),
            ContainerState::Running,
        );
        long.status_text = "Up ".repeat(100);
        long.ports = (1..=40)
            .map(|p| crate::model::PortMapping {
                private: p,
                public: Some(8000 + p),
                protocol: "tcp".into(),
            })
            .collect();
        let mut s = state_with(vec![long]);
        for (selected, hovered, busy) in [(false, false, false), (true, true, true)] {
            s.selected = selected.then(|| "long".to_string());
            s.hovered = hovered.then(|| "long".to_string());
            s.pending = busy.then(|| "long".to_string()).into_iter().collect();
            let _ = view(&s);
        }
    }

    #[test]
    fn duplicate_ports_are_shown_once() {
        let mut c = info("1", "web", "nginx", ContainerState::Running);
        let mapping = |public| crate::model::PortMapping {
            private: 80,
            public: Some(public),
            protocol: "tcp".into(),
        };
        c.ports = vec![mapping(8080), mapping(8080), mapping(9090)];
        assert_eq!(public_ports(&c), vec![8080, 9090]);
    }

    #[test]
    fn containers_texts_are_translated() {
        for key in [
            "containers.filter_running",
            "containers.filter_stopped",
            "containers.empty",
            "containers.empty_filtered",
            "containers.col.name",
            "containers.col.image",
            "containers.col.ports",
            "containers.col.status",
        ] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(
                    rust_i18n::t!(key, locale = locale),
                    key,
                    "missing {locale} translation for {key}"
                );
            }
        }
    }

    #[test]
    fn visible_sorts_running_first() {
        let s = state_with(vec![
            info("1", "alpha", "a", ContainerState::Exited { code: 0 }),
            info("2", "zeta", "z", ContainerState::Running),
            info("3", "beta", "b", ContainerState::Running),
            info("4", "Charlie", "c", ContainerState::Created),
        ]);
        assert_eq!(names(&s), vec!["beta", "zeta", "alpha", "Charlie"]);
    }
}
