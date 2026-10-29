use iced::widget::{button, column, container, text};
use iced::{Element, Length};
use rust_i18n::t;

use crate::app::{Message, Screen};

fn entry(label: String, target: Screen, active: bool) -> Element<'static, Message> {
    button(text(label))
        .width(Length::Fill)
        .style(if active {
            button::primary
        } else {
            button::text
        })
        .on_press(Message::Navigate(target))
        .into()
}

/// Navigation column. The container detail counts as part of the Containers section.
pub fn view(current: &Screen) -> Element<'_, Message> {
    let containers = matches!(current, Screen::Containers | Screen::Detail(_));
    container(
        column![
            entry(
                t!("sidebar.containers").into_owned(),
                Screen::Containers,
                containers
            ),
            entry(
                t!("sidebar.volumes").into_owned(),
                Screen::Volumes,
                *current == Screen::Volumes
            ),
            entry(
                t!("sidebar.networks").into_owned(),
                Screen::Networks,
                *current == Screen::Networks
            ),
        ]
        .spacing(4),
    )
    .padding(12)
    .width(200)
    .height(Length::Fill)
    .style(container::secondary)
    .into()
}
