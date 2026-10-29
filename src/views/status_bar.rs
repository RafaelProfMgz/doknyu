//! Bottom bar: connection, container counts and the layout and palette buttons.

use iced::widget::{button, container, row, space, text, tooltip};
use iced::{Alignment, Color, Element, Length, Padding};
use rust_i18n::t;

use crate::app::{Connection, Message};
use crate::model::ContainerInfo;
use crate::ui::components::{Shortcut, icon_button, kbd};
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::theme::palette;
use crate::ui::tokens::{DOT_SIZE, Palette, SPACE_1, SPACE_2, SPACE_3, STATUS_BAR_HEIGHT, TEXT_SM};
use crate::views::widgets::friendly_error;

/// Hint of the command palette button. The palette itself arrives in phase 2.6.
const PALETTE_SHORTCUT: Shortcut = Shortcut {
    ctrl: true,
    shift: true,
    key: "P",
};

/// Containers that are up (running, paused or restarting) and the rest.
pub fn counts(items: &[ContainerInfo]) -> (usize, usize) {
    use crate::model::ContainerState::{Paused, Restarting, Running};
    let running = items
        .iter()
        .filter(|c| matches!(c.state, Running | Paused | Restarting))
        .count();
    (running, items.len() - running)
}

/// Color of the connection dot: green when connected, yellow while checking, red when failed.
fn connection_color(p: &Palette, conn: &Connection) -> Color {
    match conn {
        Connection::Ok => p.success,
        Connection::Checking => p.warning,
        Connection::Failed(_) => p.danger,
    }
}

fn connection_dot<'a>(conn: &Connection) -> Element<'a, Message> {
    let conn = conn.clone();
    container(space().width(DOT_SIZE).height(DOT_SIZE))
        .style(move |t| {
            style::container::swatch(connection_color(palette(t), &conn), DOT_SIZE / 2.0)(t)
        })
        .into()
}

fn label<'a>(content: String) -> Element<'a, Message> {
    text(content).size(TEXT_SM).style(style::text::muted).into()
}

/// The bar. The counts only show while connected.
pub fn view<'a>(conn: &Connection, items: &[ContainerInfo]) -> Element<'a, Message> {
    let status = match conn {
        Connection::Ok => t!("status.connected").into_owned(),
        Connection::Checking => t!("status.checking").into_owned(),
        Connection::Failed(e) => friendly_error(e),
    };
    let mut left = row![connection_dot(conn), label(status)]
        .spacing(SPACE_2)
        .align_y(Alignment::Center);
    if *conn == Connection::Ok {
        let (running, stopped) = counts(items);
        left = left.push(label(
            t!("status.counts", running = running, stopped = stopped).into_owned(),
        ));
    }

    // Without an action yet: phase 2.6 turns it into the button that opens the palette.
    let palette_hint = tooltip(
        button(kbd(&PALETTE_SHORTCUT))
            .padding(Padding::from([0.0, SPACE_1]))
            .style(style::button::ghost),
        container(text(t!("status.palette").into_owned()).size(TEXT_SM))
            .padding(Padding::from([SPACE_1, SPACE_2]))
            .style(style::container::elevated),
        tooltip::Position::Top,
    )
    .gap(SPACE_1);

    container(
        row![
            left.spacing(SPACE_3),
            space().width(Length::Fill),
            icon_button(
                Icon::PanelLeft,
                Some(Message::ToggleSidebar),
                t!("status.toggle_sidebar").into_owned(),
            ),
            icon_button(
                Icon::PanelBottom,
                // The dock needs Docker: it cannot be opened from the onboarding.
                (*conn == Connection::Ok).then_some(Message::ToggleDock),
                t!("status.toggle_dock").into_owned(),
            ),
            palette_hint,
        ]
        .spacing(SPACE_1)
        .align_y(Alignment::Center),
    )
    .padding(Padding::from([0.0, SPACE_2]))
    .width(Length::Fill)
    .height(STATUS_BAR_HEIGHT)
    .align_y(Alignment::Center)
    .style(style::container::surface)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::DockerError;
    use crate::model::ContainerState;
    use crate::ui::tokens::{DARK, LIGHT};

    fn info(state: ContainerState) -> ContainerInfo {
        ContainerInfo {
            id: "id".into(),
            name: "name".into(),
            image: "image".into(),
            state,
            status_text: String::new(),
            ports: vec![],
        }
    }

    #[test]
    fn counts_treat_paused_and_restarting_as_running() {
        let items = [
            info(ContainerState::Running),
            info(ContainerState::Paused),
            info(ContainerState::Restarting),
            info(ContainerState::Created),
            info(ContainerState::Exited { code: 1 }),
            info(ContainerState::Dead),
        ];
        assert_eq!(counts(&items), (3, 3));
        assert_eq!(counts(&[]), (0, 0));
    }

    #[test]
    fn connection_dot_colors() {
        for p in [&DARK, &LIGHT] {
            assert_eq!(connection_color(p, &Connection::Ok), p.success);
            assert_eq!(connection_color(p, &Connection::Checking), p.warning);
            let failed = Connection::Failed(DockerError::NotRunning);
            assert_eq!(connection_color(p, &failed), p.danger);
        }
    }

    #[test]
    fn view_builds_for_every_connection() {
        let items = [info(ContainerState::Running)];
        for conn in [
            Connection::Checking,
            Connection::Ok,
            Connection::Failed(DockerError::PermissionDenied),
        ] {
            let _: Element<'_, Message> = view(&conn, &items);
        }
    }
}
