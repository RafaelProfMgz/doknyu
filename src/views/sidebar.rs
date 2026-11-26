//! Navigation column: Containers, Volumes, Networks and Images, collapsible to icons only, with
//! "Check for updates" and "Uninstall" at the bottom.

use iced::widget::{button, center, column, container, row, rule, space, text, tooltip};
use iced::{Alignment, Element, Length, Padding};

use rust_i18n::t;

use crate::actions::{self, ActionId};
use crate::app::{Message, Screen};
use crate::ui::components::pill;
use crate::ui::fonts::{SANS_MEDIUM, SANS_SEMIBOLD};
use crate::ui::icons::{Icon, icon};
use crate::ui::style;
use crate::ui::tokens::{
    BORDER_WIDTH, ROW_HEIGHT, SIDEBAR_COLLAPSED, SIDEBAR_WIDTH, SPACE_1, SPACE_2, SPACE_3, SPACE_4,
    TEXT, TEXT_LG, TEXT_SM,
};

/// The product name: a brand, the same in every language.
const APP_NAME: &str = "doknyu";

/// The app's name and tagline at the top of the column; only the mark when collapsed.
fn brand(collapsed: bool) -> Element<'static, Message> {
    let mark = icon(Icon::Container)
        .size(TEXT_LG + 4.0)
        .style(style::text::accent);
    if collapsed {
        return container(mark)
            .center_x(Length::Fill)
            .padding(Padding::from([SPACE_3, 0.0]))
            .into();
    }
    let name = column![
        text(APP_NAME).size(TEXT_LG).font(SANS_SEMIBOLD),
        text(t!("sidebar.tagline").into_owned())
            .size(TEXT_SM)
            .style(style::text::faint),
    ];
    container(row![mark, name].spacing(SPACE_3).align_y(Alignment::Center))
        .padding(Padding::from([SPACE_3, SPACE_2]))
        .into()
}

/// A small uppercase title over a group of entries; hidden when collapsed.
fn section(label: String, collapsed: bool) -> Element<'static, Message> {
    if collapsed {
        return space().height(SPACE_2).into();
    }
    container(
        text(label.to_uppercase())
            .size(TEXT_SM)
            .font(SANS_MEDIUM)
            .style(style::text::faint),
    )
    .padding(Padding::from([SPACE_2, SPACE_2]).bottom(SPACE_1))
    .into()
}

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
        let glyph = if active {
            icon(glyph).style(style::text::accent)
        } else {
            icon(glyph)
        };
        let mut line = row![
            glyph,
            text(label.clone())
                .size(TEXT)
                .font(SANS_MEDIUM)
                .width(Length::Fill),
        ]
        .spacing(SPACE_3)
        .align_y(Alignment::Center);
        if let Some(count) = count {
            line = line.push(if active {
                pill(count.to_string(), |p| p.accent)
            } else {
                pill(count.to_string(), |p| p.muted)
            });
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
            .spacing(SPACE_3)
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
    let entries = column![
        brand(collapsed),
        section(t!("sidebar.section_resources").into_owned(), collapsed),
        entries,
        space().height(Length::Fill),
        section(t!("sidebar.section_app").into_owned(), collapsed),
        footer
    ];
    let (width, padding) = if collapsed {
        (SIDEBAR_COLLAPSED, Padding::new(SPACE_1))
    } else {
        (
            SIDEBAR_WIDTH,
            Padding::from([SPACE_2, SPACE_2]).bottom(SPACE_4),
        )
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
