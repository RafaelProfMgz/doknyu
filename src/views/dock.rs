//! The bottom dock: header with the container name, tabs and window buttons, and the tab body.
//! The Logs, Stats and Terminal tabs are filled in by phases 3 and 5.

use iced::widget::{column, container, row, text};
use iced::{Alignment, Element, Length};
use rust_i18n::t;

use crate::app::{Dock, DockTab, Message};
use crate::ui::components::{chip, icon_button, panel_header};
use crate::ui::fonts::SANS_MEDIUM;
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::tokens::{SPACE_2, SPACE_4, TEXT};

const TABS: [DockTab; 3] = [DockTab::Logs, DockTab::Stats, DockTab::Terminal];

fn tab_label(tab: DockTab) -> String {
    match tab {
        DockTab::Logs => t!("dock.tab.logs"),
        DockTab::Stats => t!("dock.tab.stats"),
        DockTab::Terminal => t!("dock.tab.terminal"),
    }
    .into_owned()
}

/// The dock for `dock`; `name` is the container's name, if it is still in the list.
pub fn view<'a>(dock: &'a Dock, name: Option<&'a str>) -> Element<'a, Message> {
    let mut title = row![
        text(name.unwrap_or(&dock.container_id))
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
    let actions = row![
        icon_button(
            size_icon,
            Some(Message::ToggleMaximize),
            size_tip.into_owned()
        ),
        icon_button(
            Icon::X,
            Some(Message::CloseDock),
            t!("dock.close").into_owned()
        ),
    ];

    let body = container(text(t!("dock.empty_tab").into_owned()).style(style::text::muted))
        .center(Length::Fill)
        .padding(SPACE_4)
        .style(style::container::bg);
    column![panel_header(title.into(), actions.into()), body].into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_builds_for_every_tab_and_size() {
        for tab in TABS {
            for maximized in [false, true] {
                let dock = Dock {
                    container_id: "web-id".into(),
                    tab,
                    maximized,
                };
                let _: Element<'_, Message> = view(&dock, Some("web"));
                let _: Element<'_, Message> = view(&dock, None);
            }
        }
    }
}
