use iced::widget::{center, container, mouse_area, opaque, stack};
use iced::{Element, Length};

use crate::ui::style;
use crate::ui::tokens::{MODAL_WIDTH, SPACE_6};

/// `card` over `base`, on a dimmed backdrop. A click outside the card sends `on_dismiss`;
/// everything under the backdrop stops receiving events.
pub fn modal<'a, M: Clone + 'a>(
    base: Element<'a, M>,
    card: Element<'a, M>,
    on_dismiss: M,
) -> Element<'a, M> {
    let card = container(card)
        .padding(SPACE_6)
        .max_width(MODAL_WIDTH)
        .style(style::container::elevated);
    let backdrop = opaque(
        mouse_area(
            center(opaque(card))
                .width(Length::Fill)
                .height(Length::Fill)
                .style(style::container::scrim),
        )
        .on_press(on_dismiss),
    );
    stack![base, backdrop].into()
}
