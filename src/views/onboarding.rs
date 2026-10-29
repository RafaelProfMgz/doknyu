//! Screens shown while Docker cannot be reached (not installed, stopped, no permission).

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length};
use rust_i18n::t;

use crate::app::Message;
use crate::error::DockerError;
use crate::views::widgets::friendly_error;

pub const INSTALL_URL: &str = "https://docs.docker.com/engine/install/";
pub const START_COMMAND: &str = "sudo systemctl start docker";
pub const GROUP_COMMAND: &str = "sudo usermod -aG docker $USER";

/// A shell command in a monospace box with a "Copy" button.
fn command_block(command: &'static str) -> Element<'static, Message> {
    container(
        row![
            container(text(command).font(iced::Font::MONOSPACE))
                .padding(8)
                .width(Length::Fill)
                .style(container::bordered_box),
            button(text(t!("onboarding.copy").into_owned()))
                .style(button::secondary)
                .on_press(Message::Copy(command.to_owned())),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .max_width(560)
    .into()
}

/// Explains the connection problem and how to fix it.
pub fn view(err: &DockerError) -> Element<'_, Message> {
    let mut body = column![].spacing(12).max_width(560);
    match err {
        DockerError::NotInstalled => {
            body = body
                .push(text(t!("onboarding.not_installed.title").into_owned()).size(24))
                .push(text(t!("onboarding.not_installed.body").into_owned()))
                .push(
                    button(text(t!("onboarding.not_installed.button").into_owned()))
                        .on_press(Message::OpenUrl(INSTALL_URL.to_owned())),
                );
        }
        DockerError::NotRunning => {
            body = body
                .push(text(t!("onboarding.not_running.title").into_owned()).size(24))
                .push(text(t!("onboarding.not_running.body").into_owned()))
                .push(command_block(START_COMMAND));
        }
        DockerError::PermissionDenied => {
            body = body
                .push(text(t!("onboarding.permission.title").into_owned()).size(24))
                .push(text(t!("onboarding.permission.body").into_owned()))
                .push(command_block(GROUP_COMMAND))
                .push(text(t!("onboarding.permission.warning").into_owned()));
        }
        other => {
            body = body
                .push(text(t!("onboarding.other.title").into_owned()).size(24))
                .push(text(friendly_error(other)));
        }
    }
    body = body
        .push(
            button(text(t!("onboarding.retry").into_owned()))
                .style(button::primary)
                .on_press(Message::Retry),
        )
        .push(
            text(t!("onboarding.auto_retry").into_owned())
                .size(12)
                .style(text::secondary),
        );
    container(body).center(Length::Fill).padding(24).into()
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
