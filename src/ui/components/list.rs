use iced::widget::{column, container, mouse_area, row, rule, space, text};
use iced::{Alignment, Element, Length, Padding};

use crate::ui::style;
use crate::ui::theme::palette;
use crate::ui::tokens::{ACCENT_BAR_WIDTH, BORDER_WIDTH, ROW_HEIGHT, SPACE_2, TEXT_SM};

/// Distance from the left edge of a row to where its content starts: the accent bar and the
/// padding. The column titles of [`list_header`] start at the same place.
#[cfg(test)]
const CONTENT_OFFSET: f32 = ACCENT_BAR_WIDTH + SPACE_2;

/// `bar` and then `content`, laid out so that `content` starts at [`CONTENT_OFFSET`].
fn framed<'a, M: 'a>(bar: Element<'a, M>, content: Element<'a, M>) -> Element<'a, M> {
    row![
        bar,
        container(content)
            .padding(Padding::from([0.0, SPACE_2]))
            .align_y(Alignment::Center)
            .width(Length::Fill)
            .height(Length::Fill),
    ]
    .into()
}

/// What a row reports to the app.
#[derive(Debug, Clone)]
pub struct RowEvents<M> {
    pub on_select: M,
    pub on_open: M,
    pub on_enter: M,
    pub on_exit: M,
}

/// A fixed-height list row. A click selects, a double click opens, entering and leaving report
/// the hover. The selected row gets an `accent` bar on its left; `actions` only show while the
/// row is selected or hovered.
pub fn list_row<'a, M: Clone + 'a>(
    cells: Element<'a, M>,
    actions: Option<Element<'a, M>>,
    selected: bool,
    hovered: bool,
    events: RowEvents<M>,
) -> Element<'a, M> {
    let bar = container(space().width(ACCENT_BAR_WIDTH).height(Length::Fill)).style(move |t| {
        let color = if selected {
            palette(t).accent
        } else {
            iced::Color::TRANSPARENT
        };
        style::container::swatch(color, 0.0)(t)
    });
    let mut content = row![cells]
        .spacing(SPACE_2)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    if (selected || hovered)
        && let Some(actions) = actions
    {
        content = content.push(actions);
    }
    let body = framed(bar.into(), content.into());
    mouse_area(
        container(body)
            .width(Length::Fill)
            .height(ROW_HEIGHT)
            .style(style::container::row(selected, hovered)),
    )
    .on_press(events.on_select)
    .on_double_click(events.on_open)
    .on_enter(events.on_enter)
    .on_exit(events.on_exit)
    .into()
}

/// Discreet column titles above the rows.
pub fn list_header<'a, M: 'a>(columns: Vec<(String, Length)>) -> Element<'a, M> {
    let mut titles = row![]
        .spacing(SPACE_2)
        .align_y(Alignment::Center)
        .width(Length::Fill);
    for (title, width) in columns {
        titles = titles.push(
            text(title)
                .size(TEXT_SM)
                .style(style::text::faint)
                .width(width),
        );
    }
    let bar = space().width(ACCENT_BAR_WIDTH).into();
    column![
        container(framed(bar, titles.into()))
            .width(Length::Fill)
            .height(ROW_HEIGHT),
        rule::horizontal(BORDER_WIDTH).style(style::rule_border),
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_starts_after_the_bar_and_padding_in_rows_and_header() {
        // Both are built by `framed`, so the titles line up with the cells.
        assert_eq!(CONTENT_OFFSET, ACCENT_BAR_WIDTH + SPACE_2);
        let bar = || space().width(ACCENT_BAR_WIDTH).into();
        let _: Element<'_, ()> = framed(bar(), text("x").into());
        let _: Element<'_, ()> = list_header(vec![("a".into(), Length::Fill)]);
    }
}
