use iced::widget::{button, column, container, row, space, text};
use iced::{Alignment, Element, Length, Padding};
use rust_i18n::t;

use crate::ui::components::icon_button;
use crate::ui::fonts::MONO;
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::theme::palette;
use crate::ui::tokens::{ACCENT_BAR_WIDTH, SPACE_1, SPACE_2, SPACE_3, TEXT_SM, TOAST_WIDTH};

/// What a toast is about; sets the color of its left bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Error,
    Warning,
    Info,
}

/// Compact notice with a colored left bar. `details` is the technical text plus whether it is
/// open; `on_toggle` shows or hides it.
pub fn toast<'a, M: Clone + 'a>(
    kind: ToastKind,
    message: String,
    details: Option<(String, bool)>,
    on_toggle: M,
    on_dismiss: M,
) -> Element<'a, M> {
    let mut body = column![text(message)].spacing(SPACE_1);
    if let Some((details, open)) = details {
        let label = if open {
            t!("toast.hide_details")
        } else {
            t!("toast.show_details")
        };
        body = body.push(
            button(text(label.into_owned()).size(TEXT_SM))
                .padding(0)
                .style(style::button::link)
                .on_press(on_toggle),
        );
        if open {
            body = body.push(
                text(details)
                    .font(MONO)
                    .size(TEXT_SM)
                    .style(style::text::faint),
            );
        }
    }
    let bar = container(space().width(ACCENT_BAR_WIDTH).height(Length::Fill)).style(move |t| {
        let p = palette(t);
        let color = match kind {
            ToastKind::Error => p.danger,
            ToastKind::Warning => p.warning,
            ToastKind::Info => p.accent,
        };
        style::container::swatch(color, 0.0)(t)
    });
    container(
        row![
            bar,
            container(
                row![
                    body.width(Length::Fill),
                    icon_button(Icon::X, Some(on_dismiss), t!("common.close").into_owned()),
                ]
                .spacing(SPACE_2)
                .align_y(Alignment::Start),
            )
            .padding(Padding::from([SPACE_2, SPACE_3])),
        ]
        .height(Length::Shrink),
    )
    .width(TOAST_WIDTH)
    .style(style::container::elevated)
    .into()
}
