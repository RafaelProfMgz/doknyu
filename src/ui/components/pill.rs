use iced::widget::{container, text};
use iced::{Color, Element, Padding};

use crate::ui::fonts::SANS_MEDIUM;
use crate::ui::style;
use crate::ui::tokens::{Palette, SPACE_1, SPACE_2, TEXT_SM};

/// A small rounded label in one of the palette's colors over a soft tint of it: counts in the
/// sidebar, ports, the state of a container. It never carries meaning by color alone: the text
/// says it too.
pub fn pill<'a, M: 'a>(label: String, color: fn(&Palette) -> Color) -> Element<'a, M> {
    container(text(label).size(TEXT_SM).font(SANS_MEDIUM))
        .padding(Padding::from([SPACE_1 / 2.0, SPACE_2]))
        .style(style::container::pill(color))
        .into()
}
