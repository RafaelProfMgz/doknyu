use iced::widget::{button, text};
use iced::{Element, Padding};

use crate::ui::style;
use crate::ui::tokens::{SPACE_1, SPACE_2, TEXT_SM};

/// A filter that switches on and off.
pub fn chip<'a, M: Clone + 'a>(label: String, active: bool, on_press: M) -> Element<'a, M> {
    button(text(label).size(TEXT_SM))
        .padding(Padding::from([SPACE_1, SPACE_2]))
        .style(style::button::chip(active))
        .on_press(on_press)
        .into()
}
