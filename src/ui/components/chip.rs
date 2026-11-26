use iced::widget::{button, row, text};
use iced::{Alignment, Element, Padding};

use crate::ui::icons::{Icon, icon};
use crate::ui::style;
use crate::ui::tokens::{SPACE_1, SPACE_2, SPACE_3, TEXT_SM};

/// A filter that switches on and off. An active chip shows a check mark besides its tint, so the
/// state does not rest on color alone.
pub fn chip<'a, M: Clone + 'a>(label: String, active: bool, on_press: M) -> Element<'a, M> {
    let mut content = row![].spacing(SPACE_1).align_y(Alignment::Center);
    if active {
        content = content.push(icon(Icon::Check).size(TEXT_SM));
    }
    content = content.push(text(label).size(TEXT_SM));
    button(content)
        .padding(Padding::from([SPACE_1, SPACE_3]).left(if active { SPACE_2 } else { SPACE_3 }))
        .style(style::button::chip(active))
        .on_press(on_press)
        .into()
}
