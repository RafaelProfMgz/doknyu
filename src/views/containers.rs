//! Container list: search, filters, per-row actions and the removal dialog.

use std::collections::HashSet;

use iced::widget::{
    Column, button, checkbox, column, container, row, scrollable, space, text, text_input,
};
use iced::{Alignment, Element, Length};
use rust_i18n::t;

use crate::app::Message;
use crate::error::DockerError;
use crate::model::{ContainerInfo, ContainerState};
use crate::ui::components::{modal, status_dot};
use crate::ui::fonts::{MONO, SANS_MEDIUM, SANS_SEMIBOLD};
use crate::ui::style;
use crate::ui::tokens::{SPACE_1, SPACE_2, SPACE_3, SPACE_4, SPACE_6, TEXT, TEXT_LG, TEXT_SM};
use crate::views::widgets::state_label;

#[derive(Debug, Clone, PartialEq)]
pub struct ContainersState {
    pub items: Vec<ContainerInfo>,
    pub query: String,
    pub show_stopped: bool,
    pub pending: HashSet<String>,
    pub confirm_remove: Option<String>,
}

impl Default for ContainersState {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            query: String::new(),
            show_stopped: true,
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
    ToggleStopped(bool),
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
    Open(String),
}

/// Containers that are not running and not on their way to running.
fn is_stopped(state: &ContainerState) -> bool {
    matches!(
        state,
        ContainerState::Exited { .. } | ContainerState::Created | ContainerState::Dead
    )
}

impl ContainersState {
    /// Items matching the search (name or image, case-insensitive) and the stopped filter;
    /// running first, then by name.
    pub fn visible(&self) -> Vec<&ContainerInfo> {
        let query = self.query.trim().to_lowercase();
        let mut items: Vec<&ContainerInfo> = self
            .items
            .iter()
            .filter(|c| self.show_stopped || !is_stopped(&c.state))
            .filter(|c| {
                query.is_empty()
                    || c.name.to_lowercase().contains(&query)
                    || c.image.to_lowercase().contains(&query)
            })
            .collect();
        items.sort_by_cached_key(|c| (!c.state.is_running(), c.name.to_lowercase()));
        items
    }
}

fn msg(m: Msg) -> Message {
    Message::Containers(m)
}

fn action_button<'a>(label: String, on_press: Option<Message>) -> Element<'a, Message> {
    button(text(label).size(TEXT))
        .style(style::button::ghost)
        .on_press_maybe(on_press)
        .into()
}

fn row_view<'a>(c: &'a ContainerInfo, busy: bool) -> Element<'a, Message> {
    let id = &c.id;
    let enabled = |m: Msg| (!busy).then(|| msg(m));

    let mut ports = row![].spacing(SPACE_1);
    for p in c.ports.iter().filter_map(|p| p.public) {
        ports = ports.push(
            button(text(format!("localhost:{p}")).size(TEXT).font(MONO))
                .style(style::button::link)
                .on_press(msg(Msg::OpenPort(p))),
        );
    }

    let mut actions = row![].spacing(SPACE_2).align_y(Alignment::Center);
    if busy {
        actions = actions.push(text(t!("containers.working").into_owned()).size(TEXT));
    }
    if c.state.is_running() {
        actions = actions
            .push(action_button(
                t!("containers.stop").into_owned(),
                enabled(Msg::Stop(id.clone())),
            ))
            .push(action_button(
                t!("containers.restart").into_owned(),
                enabled(Msg::Restart(id.clone())),
            ));
    } else {
        actions = actions.push(action_button(
            t!("containers.start").into_owned(),
            enabled(Msg::Start(id.clone())),
        ));
    }
    actions = actions.push(
        button(text(t!("containers.remove").into_owned()).size(TEXT))
            .style(style::button::danger)
            .on_press_maybe(enabled(Msg::AskRemove(id.clone()))),
    );

    container(
        row![
            button(
                column![
                    text(c.name.as_str()).size(TEXT_LG).font(SANS_MEDIUM),
                    text(c.image.as_str())
                        .size(TEXT_SM)
                        .font(MONO)
                        .style(style::text::muted),
                ]
                .spacing(SPACE_1)
            )
            .style(style::button::ghost)
            .width(Length::FillPortion(3))
            .on_press(msg(Msg::Open(id.clone()))),
            column![
                row![status_dot(&c.state), text(state_label(&c.state))]
                    .spacing(SPACE_2)
                    .align_y(Alignment::Center),
                text(c.status_text.as_str())
                    .size(TEXT_SM)
                    .style(style::text::faint),
            ]
            .spacing(SPACE_1)
            .width(Length::FillPortion(2)),
            container(ports).width(Length::FillPortion(2)),
            actions,
        ]
        .spacing(SPACE_3)
        .align_y(Alignment::Center),
    )
    .padding(SPACE_2)
    .style(style::container::bordered)
    .into()
}

/// The content of the removal dialog; [`modal`] frames it.
fn remove_card(state: &ContainersState, id: &str) -> Element<'static, Message> {
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
    card.into()
}

/// The Containers screen.
pub fn view(state: &ContainersState) -> Element<'_, Message> {
    let toolbar = row![
        text_input(&t!("containers.search"), &state.query)
            .on_input(|q| msg(Msg::Query(q)))
            .style(style::text_input_field)
            .width(Length::Fill),
        checkbox(state.show_stopped)
            .label(t!("containers.show_stopped").into_owned())
            .on_toggle(|v| msg(Msg::ToggleStopped(v))),
    ]
    .spacing(SPACE_4)
    .align_y(Alignment::Center);

    let visible = state.visible();
    let list: Element<'_, Message> = if visible.is_empty() {
        let key = if state.items.is_empty() {
            t!("containers.empty")
        } else {
            t!("containers.no_match")
        };
        container(text(key.into_owned()).style(style::text::muted))
            .center_x(Length::Fill)
            .padding(SPACE_6)
            .into()
    } else {
        scrollable(
            Column::with_children(
                visible
                    .into_iter()
                    .map(|c| row_view(c, state.pending.contains(&c.id))),
            )
            .spacing(SPACE_2)
            .padding([0.0, SPACE_3]),
        )
        .direction(style::thin_scrollbar())
        .style(style::scrollable_thin)
        .height(Length::Fill)
        .into()
    };

    let page = container(
        column![
            text(t!("containers.title").into_owned())
                .size(TEXT_LG)
                .font(SANS_SEMIBOLD),
            toolbar,
            list,
        ]
        .spacing(SPACE_3),
    )
    .padding(SPACE_6)
    .width(Length::Fill)
    .height(Length::Fill);

    match &state.confirm_remove {
        Some(id) => modal(page.into(), remove_card(state, id), msg(Msg::CancelRemove)),
        None => page.into(),
    }
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
    fn visible_hides_stopped_when_toggled_off() {
        let mut s = state_with(vec![
            info("1", "web", "nginx", ContainerState::Running),
            info("2", "worker", "w", ContainerState::Exited { code: 1 }),
            info("3", "new", "n", ContainerState::Created),
            info("4", "dead", "d", ContainerState::Dead),
        ]);
        assert_eq!(s.visible().len(), 4);
        s.show_stopped = false;
        assert_eq!(names(&s), vec!["web"]);
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
