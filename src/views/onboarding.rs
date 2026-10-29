//! Screens shown while Docker cannot be reached (not installed, stopped, no permission).

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length, Padding};
use rust_i18n::t;

use crate::app::Message;
use crate::error::DockerError;
use crate::ui::fonts::{MONO, SANS_SEMIBOLD};
use crate::ui::style;
use crate::ui::tokens::{CARD_MAX_WIDTH, SPACE_2, SPACE_3, SPACE_6, TEXT, TEXT_LG, TEXT_SM};
use crate::views::widgets::friendly_error;

pub const INSTALL_URL: &str = "https://docs.docker.com/engine/install/";
pub const START_COMMAND: &str = "sudo systemctl start docker";
pub const GROUP_COMMAND: &str = "sudo usermod -aG docker $USER";

fn title<'a>(key: String) -> Element<'a, Message> {
    text(key).size(TEXT_LG).font(SANS_SEMIBOLD).into()
}

fn body<'a>(content: String) -> Element<'a, Message> {
    text(content).size(TEXT).into()
}

/// A button with the label sized like the rest of the card.
fn action<'a>(
    label: String,
    on_press: Message,
    look: fn(&iced::Theme, button::Status) -> button::Style,
) -> Element<'a, Message> {
    button(text(label).size(TEXT))
        .padding(Padding::from([SPACE_2, SPACE_3]))
        .style(look)
        .on_press(on_press)
        .into()
}

/// A shell command in a monospace box with a "Copy" button.
fn command_block(command: &'static str) -> Element<'static, Message> {
    row![
        container(text(command).size(TEXT).font(MONO))
            .padding(SPACE_2)
            .width(Length::Fill)
            .style(style::container::bordered),
        action(
            t!("onboarding.copy").into_owned(),
            Message::Copy(command.to_owned()),
            style::button::ghost,
        ),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center)
    .into()
}

/// Explains the connection problem and how to fix it, on a card in the middle of the area.
pub fn view(err: &DockerError) -> Element<'_, Message> {
    let mut card = column![].spacing(SPACE_3);
    match err {
        DockerError::NotInstalled => {
            card = card
                .push(title(t!("onboarding.not_installed.title").into_owned()))
                .push(body(t!("onboarding.not_installed.body").into_owned()))
                .push(action(
                    t!("onboarding.not_installed.button").into_owned(),
                    Message::OpenUrl(INSTALL_URL.to_owned()),
                    style::button::ghost,
                ));
        }
        DockerError::NotRunning => {
            card = card
                .push(title(t!("onboarding.not_running.title").into_owned()))
                .push(body(t!("onboarding.not_running.body").into_owned()))
                .push(command_block(START_COMMAND));
        }
        DockerError::PermissionDenied => {
            card = card
                .push(title(t!("onboarding.permission.title").into_owned()))
                .push(body(t!("onboarding.permission.body").into_owned()))
                .push(command_block(GROUP_COMMAND))
                .push(body(t!("onboarding.permission.warning").into_owned()));
        }
        other => {
            card = card
                .push(title(t!("onboarding.other.title").into_owned()))
                .push(body(friendly_error(other)));
        }
    }
    card = card
        .push(action(
            t!("onboarding.retry").into_owned(),
            Message::Retry,
            style::button::primary,
        ))
        .push(
            text(t!("onboarding.auto_retry").into_owned())
                .size(TEXT_SM)
                .style(style::text::faint),
        );
    container(
        container(card)
            .padding(SPACE_6)
            .max_width(CARD_MAX_WIDTH)
            .style(style::container::elevated),
    )
    .center(Length::Fill)
    .padding(SPACE_6)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_builds_for_every_error() {
        let all = [
            DockerError::NotInstalled,
            DockerError::NotRunning,
            DockerError::PermissionDenied,
            DockerError::Other("x".into()),
        ];
        for e in &all {
            let _: Element<'_, Message> = view(e);
        }
    }

    #[test]
    fn onboarding_texts_are_translated() {
        for key in [
            "onboarding.not_installed.title",
            "onboarding.not_installed.body",
            "onboarding.not_installed.button",
            "onboarding.not_running.title",
            "onboarding.not_running.body",
            "onboarding.permission.title",
            "onboarding.permission.body",
            "onboarding.permission.warning",
            "onboarding.other.title",
            "onboarding.retry",
            "onboarding.copy",
            "onboarding.auto_retry",
        ] {
            for locale in ["en", "pt-BR"] {
                let text = t!(key, locale = locale);
                assert_ne!(text, key, "missing {locale} translation for {key}");
            }
        }
    }

    #[test]
    fn command_blocks_match_the_spec() {
        assert_eq!(START_COMMAND, "sudo systemctl start docker");
        assert_eq!(GROUP_COMMAND, "sudo usermod -aG docker $USER");
    }
}
