//! The bottom dock: header with the container name, tabs and window buttons, and the tab body.
//! The tab bodies are in `views::detail`.

use iced::widget::{column, container, row, text};
use iced::{Alignment, Element, Length};
use rust_i18n::t;

use crate::actions::{self, ActionId};
use crate::app::{Dock, DockTab, Message};
use crate::model::ContainerInfo;
use crate::ui::components::{chip, icon_button, panel_header};
use crate::ui::fonts::SANS_MEDIUM;
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::tokens::{SPACE_2, SPACE_4, TEXT};
use crate::views::detail::terminal::{self, TerminalState};
use crate::views::detail::{DetailState, logs, stats};

const TABS: [DockTab; 3] = [DockTab::Logs, DockTab::Stats, DockTab::Terminal];

fn tab_label(tab: DockTab) -> String {
    match tab {
        DockTab::Logs => t!("dock.tab.logs"),
        DockTab::Stats => t!("dock.tab.stats"),
        DockTab::Terminal => t!("dock.tab.terminal"),
    }
    .into_owned()
}

/// The dock for `dock`; `info` is the container, if it is still in the list, `detail` what the
/// tabs keep for it, `term` the shell session and `docker_cli` whether the `docker` command
/// exists (the session needs it).
pub fn view<'a>(
    dock: &'a Dock,
    info: Option<&'a ContainerInfo>,
    detail: Option<&'a DetailState>,
    term: &'a TerminalState,
    docker_cli: bool,
) -> Element<'a, Message> {
    let mut title = row![
        text(info.map_or(dock.container_id.as_str(), |c| c.name.as_str()))
            .size(TEXT)
            .font(SANS_MEDIUM),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center);
    for tab in TABS {
        title = title.push(chip(tab_label(tab), dock.tab == tab, Message::DockTab(tab)));
    }

    let (size_icon, size_tip) = if dock.maximized {
        (Icon::Minimize2, t!("dock.restore"))
    } else {
        (Icon::Maximize2, t!("dock.maximize"))
    };
    let running = info.is_some_and(|c| c.state.is_running());
    let mut actions = row![];
    if running {
        actions = actions.push(icon_button(
            Icon::SquareTerminal,
            Some(Message::Action(ActionId::OpenExternalTerminal)),
            actions::tip(ActionId::OpenExternalTerminal, &t!("dock.open_terminal")),
        ));
    }
    let actions = actions
        .push(icon_button(
            size_icon,
            Some(Message::ToggleMaximize),
            size_tip.into_owned(),
        ))
        .push(icon_button(
            Icon::X,
            // With the dock open, toggling it closes it.
            Some(Message::Action(ActionId::ToggleDock)),
            actions::tip(ActionId::ToggleDock, &t!("dock.close")),
        ));

    let body: Element<'a, Message> = match (dock.tab, detail) {
        (DockTab::Logs, Some(detail)) => container(logs::view(detail))
            .style(style::container::bg)
            .into(),
        (DockTab::Stats, Some(detail)) => container(stats::view(detail, running))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(style::container::bg)
            .into(),
        (DockTab::Terminal, _) => terminal::view(term, running, docker_cli),
        _ => container(text(t!("dock.empty_tab").into_owned()).style(style::text::muted))
            .center(Length::Fill)
            .padding(SPACE_4)
            .style(style::container::bg)
            .into(),
    };
    column![panel_header(title.into(), actions.into()), body].into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ContainerState;

    #[test]
    fn view_builds_for_every_tab_and_size() {
        let term = TerminalState::default();
        for docker_cli in [true, false] {
            for tab in TABS {
                for maximized in [false, true] {
                    let dock = Dock {
                        container_id: "web-id".into(),
                        tab,
                        maximized,
                    };
                    let detail = DetailState::new("web-id".into());
                    for state in [ContainerState::Running, ContainerState::Exited { code: 0 }] {
                        let info = ContainerInfo {
                            id: "web-id".into(),
                            name: "web".into(),
                            image: "nginx".into(),
                            state,
                            status_text: String::new(),
                            ports: vec![],
                        };
                        let _: Element<'_, Message> =
                            view(&dock, Some(&info), Some(&detail), &term, docker_cli);
                    }
                    let _: Element<'_, Message> = view(&dock, None, None, &term, docker_cli);
                }
            }
        }
    }
}
