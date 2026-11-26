//! The bottom dock: header with the container name, tabs and window buttons, and the tab body.
//! The tab bodies are in `views::detail`.

use iced::widget::{button, column, container, row, rule, text};
use iced::{Alignment, Element, Length, Padding};
use rust_i18n::t;

use crate::actions::{self, ActionId};
use crate::app::{Dock, DockTab, Message};
use crate::model::ContainerInfo;
use crate::ui::components::{icon_button, panel_header, status_dot};
use crate::ui::fonts::{MONO, SANS_MEDIUM, SANS_SEMIBOLD};
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::tokens::{BORDER_WIDTH, SPACE_1, SPACE_2, SPACE_3, SPACE_4, TEXT, TEXT_SM};

/// One tab of the dock: the active one is tinted in the accent.
fn tab<'a>(label: String, active: bool, on_press: Message) -> Element<'a, Message> {
    button(text(label).size(TEXT_SM).font(SANS_MEDIUM))
        .padding(Padding::from([SPACE_1, SPACE_3]))
        .style(style::button::tab(active))
        .on_press(on_press)
        .into()
}
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
/// tabs keep for it, and `term` the shell session.
pub fn view<'a>(
    dock: &'a Dock,
    info: Option<&'a ContainerInfo>,
    detail: Option<&'a DetailState>,
    term: &'a TerminalState,
) -> Element<'a, Message> {
    let mut title = row![].spacing(SPACE_2).align_y(Alignment::Center);
    if let Some(c) = info {
        title = title.push(status_dot(&c.state));
    }
    title = title.push(
        text(info.map_or(dock.container_id.as_str(), |c| c.name.as_str()))
            .size(TEXT)
            .font(SANS_SEMIBOLD),
    );
    if let Some(c) = info {
        title = title.push(
            text(c.image.as_str())
                .size(TEXT_SM)
                .font(MONO)
                .style(style::text::faint),
        );
    }
    let mut tabs = row![].spacing(SPACE_1).align_y(Alignment::Center);
    for t in TABS {
        tabs = tabs.push(tab(tab_label(t), dock.tab == t, Message::DockTab(t)));
    }
    let title = title
        .push(container(rule::vertical(BORDER_WIDTH).style(style::rule_border)).height(TEXT))
        .push(tabs);

    let (size_icon, size_tip) = if dock.maximized {
        (Icon::Minimize2, t!("dock.restore"))
    } else {
        (Icon::Maximize2, t!("dock.maximize"))
    };
    let running = info.is_some_and(|c| c.state.is_running());
    let mut actions = row![];
    if running && !crate::sandbox::in_flatpak() {
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
        (DockTab::Terminal, _) => terminal::view(term, running),
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
        {
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
                            view(&dock, Some(&info), Some(&detail), &term);
                    }
                    let _: Element<'_, Message> = view(&dock, None, None, &term);
                }
            }
        }
    }
}
