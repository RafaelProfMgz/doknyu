//! Navigation column: Containers, Volumes, Networks and Images, collapsible to icons only, with
//! "Check for updates" and "Uninstall" at the bottom.

use iced::widget::{button, center, column, container, row, rule, space, text, tooltip};
use iced::{Alignment, Element, Length, Padding};

use rust_i18n::t;

use crate::actions::{self, ActionId};
use crate::app::{Message, Screen};
use crate::ui::fonts::SANS_MEDIUM;
use crate::ui::icons::{Icon, icon};
use crate::ui::style;
use crate::ui::tokens::{
    BORDER_WIDTH, ROW_HEIGHT, SIDEBAR_COLLAPSED, SIDEBAR_WIDTH, SPACE_1, SPACE_2, TEXT, TEXT_SM,
};

fn entry(
    glyph: Icon,
    label: String,
    count: Option<usize>,
    action: ActionId,
    active: bool,
    collapsed: bool,
) -> Element<'static, Message> {
    let content: Element<'static, Message> = if collapsed {
        center(icon(glyph)).into()
    } else {
        let mut line = row![
            icon(glyph),
            text(label.clone())
                .size(TEXT)
                .font(SANS_MEDIUM)
                .width(Length::Fill),
        ]
        .spacing(SPACE_2)
        .align_y(Alignment::Center);
        if let Some(count) = count {
            line = line.push(
                text(count.to_string())
                    .size(TEXT_SM)
                    .style(style::text::faint),
            );
        }
        // A fixed-height button keeps its content at the top; center it like the collapsed icon.
        container(line).center_y(Length::Fill).into()
    };
    let item = button(content)
        .width(Length::Fill)
        .height(ROW_HEIGHT)
        .padding(Padding::from([0.0, SPACE_2]))
        .style(style::button::nav(active))
        .on_press(Message::Action(action));
    let tip = container(text(actions::tip(action, &label)).size(TEXT_SM))
        .padding(Padding::from([SPACE_1, SPACE_2]))
        .style(style::container::elevated);
    tooltip(item, tip, tooltip::Position::Right)
        .gap(SPACE_1)
        .into()
}

/// A quiet entry at the bottom of the column (update, uninstall): muted text, no count.
fn footer_entry(
    glyph: Icon,
    label: String,
    action: ActionId,
    collapsed: bool,
) -> Element<'static, Message> {
    let content: Element<'static, Message> = if collapsed {
        center(icon(glyph).style(style::text::muted)).into()
    } else {
        container(
            row![
                icon(glyph).style(style::text::muted),
                text(label.clone())
                    .size(TEXT_SM)
                    .style(style::text::muted)
                    .width(Length::Fill),
            ]
            .spacing(SPACE_2)
            .align_y(Alignment::Center),
        )
        .center_y(Length::Fill)
        .into()
    };
    let item = button(content)
        .width(Length::Fill)
        .height(ROW_HEIGHT)
        .padding(Padding::from([0.0, SPACE_2]))
        .style(style::button::nav(false))
        .on_press(Message::Action(action));
    let tip = container(text(label).size(TEXT_SM))
        .padding(Padding::from([SPACE_1, SPACE_2]))
        .style(style::container::elevated);
    tooltip(item, tip, tooltip::Position::Right)
        .gap(SPACE_1)
        .into()
}

/// Navigation column. `containers`, `volumes`, `networks` and `images` are the numbers shown
/// beside their entries.
pub fn view(
    current: &Screen,
    collapsed: bool,
    containers: Option<usize>,
    volumes: Option<usize>,
    networks: Option<usize>,
    images: Option<usize>,
) -> Element<'_, Message> {
    let entries = column![
        entry(
            Icon::Box,
            t!("sidebar.containers").into_owned(),
            containers,
            ActionId::GoContainers,
            *current == Screen::Containers,
            collapsed,
        ),
        entry(
            Icon::Database,
            t!("sidebar.volumes").into_owned(),
            volumes,
            ActionId::GoVolumes,
            *current == Screen::Volumes,
            collapsed,
        ),
        entry(
            Icon::Network,
            t!("sidebar.networks").into_owned(),
            networks,
            ActionId::GoNetworks,
            *current == Screen::Networks,
            collapsed,
        ),
        entry(
            Icon::Image,
            t!("sidebar.images").into_owned(),
            images,
            ActionId::GoImages,
            *current == Screen::Images,
            collapsed,
        ),
    ]
    .spacing(SPACE_1);
    let footer = column![
        footer_entry(
            Icon::RotateCw,
            t!("sidebar.check_update").into_owned(),
            ActionId::CheckUpdate,
            collapsed,
        ),
        footer_entry(
            Icon::Trash2,
            t!("sidebar.uninstall").into_owned(),
            ActionId::Uninstall,
            collapsed,
        ),
    ]
    .spacing(SPACE_1);
    let entries = column![entries, space().height(Length::Fill), footer];
    let (width, padding) = if collapsed {
        (SIDEBAR_COLLAPSED, SPACE_1)
    } else {
        (SIDEBAR_WIDTH, SPACE_2)
    };
    row![
        container(entries)
            .padding(padding)
            .width(width)
            .height(Length::Fill)
            .style(style::container::surface),
        rule::vertical(BORDER_WIDTH).style(style::rule_border),
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_builds_collapsed_and_expanded() {
        for screen in [
            Screen::Containers,
            Screen::Volumes,
            Screen::Networks,
            Screen::Images,
        ] {
            for collapsed in [false, true] {
                for count in [None, Some(0), Some(12)] {
                    let _: Element<'_, Message> =
                        view(&screen, collapsed, count, count, count, count);
                }
            }
        }
    }
}
