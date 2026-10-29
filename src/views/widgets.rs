//! Pieces tied to the app messages: toast stack, state names and friendly error texts.

use iced::widget::{column, container};
use iced::{Element, Length, Padding};
use rust_i18n::t;

use crate::app::{Message, Toast};
use crate::error::DockerError;
use crate::model::ContainerState;
use crate::ui::components::toast;
use crate::ui::tokens::{SPACE_2, SPACE_4, STATUS_BAR_HEIGHT};

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

/// Translated state name, for example "Exited (137)".
pub fn state_label(state: &ContainerState) -> String {
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

/// Toasts stacked in the bottom-right corner, above the status bar; each one can be dismissed.
pub fn toast_stack(toasts: &[Toast]) -> Element<'_, Message> {
    let items = toasts.iter().enumerate().map(|(i, t)| {
        toast(
            t.kind,
            t.text.clone(),
            t.details.clone().map(|d| (d, t.expanded)),
            Message::ToggleToast(i),
            Message::DismissToast(i),
        )
    });
    container(column(items).spacing(SPACE_2))
        .padding(Padding {
            bottom: SPACE_4 + STATUS_BAR_HEIGHT,
            ..Padding::from(SPACE_4)
        })
        .align_right(Length::Fill)
        .align_bottom(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::components::ToastKind;

    #[test]
    fn toast_stack_builds_for_every_kind() {
        let toasts: Vec<Toast> = [ToastKind::Error, ToastKind::Warning, ToastKind::Info]
            .into_iter()
            .map(|kind| Toast {
                kind,
                text: "text".into(),
                details: Some("details".into()),
                expanded: true,
            })
            .collect();
        let _: Element<'_, Message> = toast_stack(&toasts);
    }

    #[test]
    fn exited_label_includes_the_exit_code() {
        assert!(state_label(&ContainerState::Exited { code: 137 }).contains("137"));
    }
}
