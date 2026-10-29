use iced::widget::text_input::{self, Icon as InputIcon};
use iced::widget::{Id, text_input as input};
use iced::{Element, Padding};

use crate::ui::fonts::ICONS;
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::tokens::{SPACE_1, SPACE_2, TEXT};

/// Search box with a magnifier; the border turns `accent` while it has focus.
pub fn search_field<'a, M: Clone + 'a>(
    id: Id,
    placeholder: String,
    value: &str,
    on_input: impl Fn(String) -> M + 'a,
) -> Element<'a, M> {
    input(&placeholder, value)
        .id(id)
        .on_input(on_input)
        .size(TEXT)
        .padding(Padding::from([SPACE_1, SPACE_2]))
        .icon(InputIcon {
            font: ICONS,
            code_point: Icon::Search.codepoint(),
            size: Some(TEXT.into()),
            spacing: SPACE_2,
            side: text_input::Side::Left,
        })
        .style(style::text_input_field)
        .into()
}
