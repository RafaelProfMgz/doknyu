use iced::widget::{column, container, row, rule, space};
use iced::{Alignment, Element, Length, Padding};

use crate::ui::style;
use crate::ui::tokens::{BORDER_WIDTH, ROW_HEIGHT, SPACE_2};

/// Top bar of the dock and of sections: title on the left, actions on the right.
pub fn panel_header<'a, M: 'a>(title: Element<'a, M>, actions: Element<'a, M>) -> Element<'a, M> {
    column![
        container(
            row![title, space().width(Length::Fill), actions]
                .spacing(SPACE_2)
                .align_y(Alignment::Center),
        )
        .padding(Padding::from([0.0, SPACE_2]))
        .height(ROW_HEIGHT)
        .width(Length::Fill)
        .align_y(Alignment::Center)
        .style(style::container::surface),
        rule::horizontal(BORDER_WIDTH).style(style::rule_border),
    ]
    .into()
}
