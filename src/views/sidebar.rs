//! Navigation column: Containers, Volumes and Networks, collapsible to icons only.

use iced::widget::button::{Status, Style};
use iced::widget::{button, center, column, container, row, rule, text, tooltip};
use iced::{Alignment, Background, Element, Length, Padding, Theme};

use rust_i18n::t;

use crate::app::{Message, Screen};
use crate::ui::fonts::SANS_MEDIUM;
use crate::ui::icons::{Icon, icon};
use crate::ui::style;
use crate::ui::theme::palette;
use crate::ui::tokens::{
    BORDER_WIDTH, ROW_HEIGHT, SIDEBAR_COLLAPSED, SIDEBAR_WIDTH, SPACE_1, SPACE_2, TEXT, TEXT_SM,
};

/// A ghost button; the active one keeps the `selected` background.
fn nav_style(active: bool) -> impl Fn(&Theme, Status) -> Style {
    move |t, s| {
        let mut style = style::button::ghost(t, s);
        if active && s != Status::Pressed {
            let p = palette(t);
            style.background = Some(Background::Color(p.selected));
            style.text_color = p.text;
        }
        style
    }
}

fn entry(
    glyph: Icon,
    label: String,
    count: Option<usize>,
    target: Screen,
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
        line.into()
    };
    let item = button(content)
        .width(Length::Fill)
        .height(ROW_HEIGHT)
        .padding(Padding::from([0.0, SPACE_2]))
        .style(nav_style(active))
        .on_press(Message::Navigate(target));
    if !collapsed {
        return item.into();
    }
    let tip = container(text(label).size(TEXT_SM))
        .padding(Padding::from([SPACE_1, SPACE_2]))
        .style(style::container::elevated);
    tooltip(item, tip, tooltip::Position::Right)
        .gap(SPACE_1)
        .into()
}

/// Navigation column. `containers` is the number shown beside Containers; Volumes and Networks
/// have no count until phase 4.
pub fn view(current: &Screen, collapsed: bool, containers: Option<usize>) -> Element<'_, Message> {
    let entries = column![
        entry(
            Icon::Box,
            t!("sidebar.containers").into_owned(),
            containers,
            Screen::Containers,
            *current == Screen::Containers,
            collapsed,
        ),
        entry(
            Icon::Database,
            t!("sidebar.volumes").into_owned(),
            None,
            Screen::Volumes,
            *current == Screen::Volumes,
            collapsed,
        ),
        entry(
            Icon::Network,
            t!("sidebar.networks").into_owned(),
            None,
            Screen::Networks,
            *current == Screen::Networks,
            collapsed,
        ),
    ]
    .spacing(SPACE_1);
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
        for screen in [Screen::Containers, Screen::Volumes, Screen::Networks] {
            for collapsed in [false, true] {
                for count in [None, Some(0), Some(12)] {
                    let _: Element<'_, Message> = view(&screen, collapsed, count);
                }
            }
        }
    }

    #[test]
    fn active_entry_keeps_the_selected_background() {
        use crate::ui::theme::theme;
        use crate::ui::tokens::DARK;
        let t = theme(&DARK);
        let active = nav_style(true)(&t, Status::Active);
        assert_eq!(active.background, Some(Background::Color(DARK.selected)));
        let idle = nav_style(false)(&t, Status::Active);
        assert_eq!(idle.background, None);
        let hovered = nav_style(false)(&t, Status::Hovered);
        assert_eq!(hovered.background, Some(Background::Color(DARK.hover)));
    }
}
