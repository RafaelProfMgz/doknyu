//! Stateless building blocks of every screen. Generic in the message type: they know nothing
//! about `App`. Views compose these and never style widgets by hand.

mod chip;
mod human_bytes;
mod icon_button;
mod kbd;
mod list;
mod modal;
mod panel_header;
mod search_field;
mod sparkline;
mod status_dot;
mod toast;

pub use chip::chip;
pub use human_bytes::human_bytes;
pub use icon_button::icon_button;
pub use kbd::{Shortcut, kbd};
pub use list::{RowEvents, list_header, list_row};
pub use modal::modal;
pub use panel_header::panel_header;
pub use search_field::search_field;
pub use sparkline::sparkline;
pub use status_dot::{dot_color, status_dot};
pub use toast::{ToastKind, toast};

#[cfg(test)]
mod tests {
    use iced::widget::{button, scrollable, text_input};
    use iced::{Background, Element, Length};

    use crate::model::ContainerState;
    use crate::ui::components::*;
    use crate::ui::icons::Icon;
    use crate::ui::style;
    use crate::ui::theme::theme;
    use crate::ui::tokens::{DARK, LIGHT};

    const BUTTON_STATUSES: [button::Status; 4] = [
        button::Status::Active,
        button::Status::Hovered,
        button::Status::Pressed,
        button::Status::Disabled,
    ];

    #[test]
    fn components_build_in_both_themes() {
        for p in [&DARK, &LIGHT] {
            let t = theme(p);
            for s in BUTTON_STATUSES {
                let _ = style::button::ghost(&t, s);
                let _ = style::button::primary(&t, s);
                let _ = style::button::danger(&t, s);
                let _ = style::button::link(&t, s);
                let _ = style::button::chip(true)(&t, s);
                let _ = style::button::chip(false)(&t, s);
            }
            let _ = style::container::bg(&t);
            let _ = style::container::surface(&t);
            let _ = style::container::elevated(&t);
            let _ = style::container::scrim(&t);
            for (selected, hovered) in [(false, false), (false, true), (true, false), (true, true)]
            {
                let _ = style::container::row(selected, hovered)(&t);
            }
            for s in [
                text_input::Status::Active,
                text_input::Status::Hovered,
                text_input::Status::Focused { is_hovered: false },
                text_input::Status::Disabled,
            ] {
                let _ = style::text_input_field(&t, s);
            }
            for s in [
                scrollable::Status::Active {
                    is_horizontal_scrollbar_disabled: false,
                    is_vertical_scrollbar_disabled: false,
                },
                scrollable::Status::Hovered {
                    is_horizontal_scrollbar_hovered: false,
                    is_vertical_scrollbar_hovered: true,
                    is_horizontal_scrollbar_disabled: false,
                    is_vertical_scrollbar_disabled: false,
                },
            ] {
                let _ = style::scrollable_thin(&t, s);
            }
            let _ = style::rule_border(&t);
        }

        let _: Element<'_, ()> = icon_button(Icon::Play, Some(()), "tip".into());
        let _: Element<'_, ()> = icon_button(Icon::Play, None, "tip".into());
        let events = || RowEvents {
            on_select: (),
            on_open: (),
            on_enter: (),
            on_exit: (),
        };
        for (selected, hovered) in [(false, false), (false, true), (true, false)] {
            let _: Element<'_, ()> = list_row(
                iced::widget::text("cells").into(),
                Some(iced::widget::text("actions").into()),
                selected,
                hovered,
                events(),
            );
        }
        let _: Element<'_, ()> = list_row(
            iced::widget::text("cells").into(),
            None,
            true,
            true,
            events(),
        );
        let _: Element<'_, ()> = list_header(vec![
            ("Name".into(), Length::Fill),
            ("Image".into(), Length::FillPortion(2)),
        ]);
        for state in [
            ContainerState::Running,
            ContainerState::Paused,
            ContainerState::Restarting,
            ContainerState::Created,
            ContainerState::Exited { code: 0 },
            ContainerState::Exited { code: 1 },
            ContainerState::Dead,
        ] {
            let _: Element<'_, ()> = status_dot(&state);
        }
        let _: Element<'_, ()> = search_field(
            iced::widget::Id::new("search"),
            "Search".into(),
            "value",
            |_| (),
        );
        let _: Element<'_, ()> = chip("Running".into(), true, ());
        let _: Element<'_, ()> = chip("Stopped".into(), false, ());
        let _: Element<'_, ()> = kbd(&Shortcut {
            ctrl: true,
            shift: true,
            key: "P",
        });
        let _: Element<'_, ()> = panel_header(
            iced::widget::text("title").into(),
            iced::widget::text("actions").into(),
        );
        let _: Element<'_, ()> = modal(
            iced::widget::text("base").into(),
            iced::widget::text("card").into(),
            (),
        );
        for kind in [ToastKind::Error, ToastKind::Warning, ToastKind::Info] {
            let _: Element<'_, ()> = toast(kind, "text".into(), None, (), ());
            let _: Element<'_, ()> = toast(kind, "text".into(), Some(("d".into(), false)), (), ());
            let _: Element<'_, ()> = toast(kind, "text".into(), Some(("d".into(), true)), (), ());
        }
    }

    #[test]
    fn status_dot_colors() {
        let p = &DARK;
        assert_eq!(dot_color(p, &ContainerState::Running), p.success);
        assert_eq!(dot_color(p, &ContainerState::Paused), p.warning);
        assert_eq!(dot_color(p, &ContainerState::Restarting), p.warning);
        assert_eq!(dot_color(p, &ContainerState::Exited { code: 1 }), p.danger);
        assert_eq!(dot_color(p, &ContainerState::Dead), p.danger);
        assert_eq!(dot_color(p, &ContainerState::Created), p.faint);
        assert_eq!(dot_color(p, &ContainerState::Exited { code: 0 }), p.faint);
    }

    #[test]
    fn row_style_selected_beats_hovered() {
        let t = theme(&DARK);
        let bg = |s: iced::widget::container::Style| s.background;
        assert_eq!(
            bg(style::container::row(true, true)(&t)),
            Some(Background::Color(DARK.selected))
        );
        assert_eq!(
            bg(style::container::row(false, true)(&t)),
            Some(Background::Color(DARK.hover))
        );
        assert_eq!(bg(style::container::row(false, false)(&t)), None);
    }

    #[test]
    fn chip_active_uses_accent_border() {
        let t = theme(&LIGHT);
        let active = style::button::chip(true)(&t, button::Status::Active);
        let inactive = style::button::chip(false)(&t, button::Status::Active);
        assert_eq!(active.border.color, LIGHT.accent);
        assert_eq!(inactive.border.color, LIGHT.border);
    }

    #[test]
    fn kbd_label() {
        let s = Shortcut {
            ctrl: true,
            shift: true,
            key: "P",
        };
        assert_eq!(s.label(), "Ctrl ⇧ P");
        let s = Shortcut {
            ctrl: true,
            shift: false,
            key: "B",
        };
        assert_eq!(s.label(), "Ctrl B");
        let s = Shortcut {
            ctrl: false,
            shift: false,
            key: "Esc",
        };
        assert_eq!(s.label(), "Esc");
    }

    #[test]
    fn component_texts_are_translated() {
        for key in ["common.close", "toast.show_details", "toast.hide_details"] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(
                    rust_i18n::t!(key, locale = locale),
                    key,
                    "missing {locale} translation for {key}"
                );
            }
        }
    }
}
