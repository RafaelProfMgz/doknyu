use iced::widget::{button, center, container, text, tooltip};
use iced::{Element, Padding};

use crate::ui::icons::{Icon, icon};
use crate::ui::style;
use crate::ui::tokens::{ICON_BUTTON_SIZE, SPACE_1, SPACE_2, TEXT_SM};

/// A square icon button with a tooltip; without `on_press` it is disabled.
pub fn icon_button<'a, M: Clone + 'a>(
    glyph: Icon,
    on_press: Option<M>,
    tip: String,
) -> Element<'a, M> {
    let button = button(center(icon(glyph)))
        .width(ICON_BUTTON_SIZE)
        .height(ICON_BUTTON_SIZE)
        .padding(0)
        .style(style::button::ghost)
        .on_press_maybe(on_press);
    let tip = container(text(tip).size(TEXT_SM))
        .padding(Padding::from([SPACE_1, SPACE_2]))
        .style(style::container::elevated);
    tooltip(button, tip, tooltip::Position::Bottom)
        .gap(SPACE_1)
        .into()
}
