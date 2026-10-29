use iced::widget::{container, space};
use iced::{Color, Element};

use crate::model::ContainerState;
use crate::ui::style;
use crate::ui::theme::palette;
use crate::ui::tokens::{DOT_SIZE, Palette};

/// Color of the dot: running is green, paused/restarting yellow, failed red, the rest grey.
pub fn dot_color(p: &Palette, state: &ContainerState) -> Color {
    match state {
        ContainerState::Running => p.success,
        ContainerState::Paused | ContainerState::Restarting => p.warning,
        s if s.is_error() => p.danger,
        _ => p.faint,
    }
}

/// Round state indicator.
pub fn status_dot<'a, M: 'a>(state: &ContainerState) -> Element<'a, M> {
    let state = state.clone();
    container(space().width(DOT_SIZE).height(DOT_SIZE))
        .style(move |t| style::container::swatch(dot_color(palette(t), &state), DOT_SIZE / 2.0)(t))
        .into()
}
