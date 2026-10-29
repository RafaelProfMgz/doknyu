//! Reusable pieces: status badge, confirmation dialog, toasts and friendly error texts.

use iced::widget::{button, column, container, opaque, row, space, text};
use iced::{Alignment, Border, Color, Element, Length, Theme};
use rust_i18n::t;

use crate::app::{Message, Toast};
use crate::error::DockerError;
use crate::model::ContainerState;

/// Plain-language text for an error; the technical details go in `Toast::details`.
pub fn friendly_error(e: &DockerError) -> String {
    match e {
        DockerError::NotInstalled => t!("error.not_installed"),
        DockerError::NotRunning => t!("error.not_running"),
        DockerError::PermissionDenied => t!("error.permission_denied"),
        DockerError::NotFound(_) => t!("error.not_found"),
        DockerError::Conflict(_) => t!("error.conflict"),
        DockerError::Api { .. } => t!("error.api"),
        DockerError::Other(_) => t!("error.other"),
    }
    .into_owned()
}

fn state_label(state: &ContainerState) -> String {
    match state {
        ContainerState::Running => t!("state.running"),
        ContainerState::Paused => t!("state.paused"),
        ContainerState::Restarting => t!("state.restarting"),
        ContainerState::Created => t!("state.created"),
        ContainerState::Exited { code } => t!("state.exited", code = code),
        ContainerState::Dead => t!("state.dead"),
    }
    .into_owned()
}

fn state_color(theme: &Theme, state: &ContainerState) -> Color {
    let p = theme.extended_palette();
    match state {
        ContainerState::Running => p.success.base.color,
        ContainerState::Paused | ContainerState::Restarting => p.warning.base.color,
        s if s.is_error() => p.danger.base.color,
        _ => p.secondary.strong.color,
    }
}

/// Colored dot plus the translated state name.
pub fn status_badge<'a, M: 'a>(state: &ContainerState) -> Element<'a, M> {
    let state = state.clone();
    let label = state_label(&state);
    let dot =
        container(space().width(10).height(10)).style(move |theme: &Theme| container::Style {
            background: Some(state_color(theme, &state).into()),
            border: Border::default().rounded(5),
            ..container::Style::default()
        });
    row![dot, text(label)]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

/// Modal confirmation card over a dimmed backdrop. Layer it with `stack!`.
pub fn confirm_dialog<'a, M: Clone + 'a>(
    title: String,
    body: String,
    confirm_label: String,
    on_confirm: M,
    on_cancel: M,
) -> Element<'a, M> {
    let card = container(
        column![
            text(title).size(20),
            text(body),
            row![
                space().width(Length::Fill),
                button(text(t!("common.cancel").into_owned()))
                    .style(button::secondary)
                    .on_press(on_cancel),
                button(text(confirm_label))
                    .style(button::danger)
                    .on_press(on_confirm),
            ]
            .spacing(8),
        ]
        .spacing(16),
    )
    .padding(24)
    .max_width(420)
    .style(container::rounded_box);

    opaque(
        container(opaque(card))
            .center(Length::Fill)
            .style(|_| container::background(Color::from_rgba(0.0, 0.0, 0.0, 0.5))),
    )
}

/// Toasts stacked in the bottom-right corner; each one can be dismissed.
pub fn toast_stack(toasts: &[Toast]) -> Element<'_, Message> {
    let items = toasts.iter().enumerate().map(|(i, toast)| {
        let mut body = column![text(toast.text.clone())].spacing(4);
        if let Some(details) = &toast.details {
            let label = if toast.expanded {
                t!("toast.hide_details")
            } else {
                t!("toast.show_details")
            };
            body = body.push(
                button(text(label.into_owned()).size(12))
                    .padding(0)
                    .style(button::text)
                    .on_press(Message::ToggleToast(i)),
            );
            if toast.expanded {
                body = body.push(text(details.clone()).size(12).style(text::secondary));
            }
        }
        container(
            row![
                body.width(Length::Fill),
                button(text("×"))
                    .style(button::text)
                    .on_press(Message::DismissToast(i)),
            ]
            .spacing(8)
            .align_y(Alignment::Start),
        )
        .padding(12)
        .width(360)
        .style(container::rounded_box)
        .into()
    });
    container(column(items).spacing(8))
        .padding(16)
        .align_right(Length::Fill)
        .align_bottom(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn created_looks_stopped_not_like_a_warning() {
        let theme = Theme::Light;
        let created = state_color(&theme, &ContainerState::Created);
        assert_eq!(
            created,
            state_color(&theme, &ContainerState::Exited { code: 0 })
        );
        assert_ne!(created, state_color(&theme, &ContainerState::Paused));
    }

    #[test]
    fn badge_and_dialog_build_for_every_state() {
        let states = [
            ContainerState::Running,
            ContainerState::Paused,
            ContainerState::Restarting,
            ContainerState::Created,
            ContainerState::Exited { code: 0 },
            ContainerState::Exited { code: 137 },
            ContainerState::Dead,
        ];
        for s in states {
            let _: Element<'_, Message> = status_badge(&s);
        }
        let _: Element<'_, Message> = confirm_dialog(
            "t".into(),
            "b".into(),
            "ok".into(),
            Message::Retry,
            Message::Retry,
        );
    }

    #[test]
    fn exited_label_includes_the_exit_code() {
        assert!(state_label(&ContainerState::Exited { code: 137 }).contains("137"));
    }
}
