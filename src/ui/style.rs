//! Style functions for iced widgets, all derived from the active [`Palette`].

use iced::widget::{pane_grid, rule, scrollable, text_input};
use iced::{Background, Border, Color, Shadow, Theme, Vector, border};

use crate::ui::theme::palette;
use crate::ui::tokens::{
    ALPHA_HOVER, ALPHA_PRESSED, ALPHA_SCRIM, ALPHA_SELECTION, ALPHA_SOFT, BORDER_WIDTH, NO_BORDER,
    RADIUS, RADIUS_LG, SCROLLBAR_WIDTH, SHADOW_ALPHA, SHADOW_BLUR, SHADOW_OFFSET_Y,
};

fn rounded_border(color: Color, width: f32) -> Border {
    Border {
        color,
        width,
        radius: RADIUS.into(),
    }
}

fn shadow() -> Shadow {
    Shadow {
        color: Color::BLACK.scale_alpha(SHADOW_ALPHA),
        offset: Vector::new(0.0, SHADOW_OFFSET_Y),
        blur_radius: SHADOW_BLUR,
    }
}

pub mod button {
    use iced::widget::button::{Status, Style};
    use iced::{Background, Color, Shadow, Theme};

    use super::{ALPHA_HOVER, ALPHA_PRESSED, ALPHA_SOFT, NO_BORDER, rounded_border};
    use crate::ui::theme::palette;
    use crate::ui::tokens::{BORDER_WIDTH, Palette};

    fn base(text_color: Color) -> Style {
        Style {
            background: None,
            text_color,
            border: rounded_border(Color::TRANSPARENT, NO_BORDER),
            shadow: Shadow::default(),
            snap: true,
        }
    }

    fn filled(p: &Palette, color: Color, status: Status) -> Style {
        let color = match status {
            Status::Hovered => color.scale_alpha(ALPHA_HOVER),
            Status::Pressed => color.scale_alpha(ALPHA_PRESSED),
            Status::Active | Status::Disabled => color,
        };
        let mut style = base(p.bg);
        style.background = Some(Background::Color(color));
        if status == Status::Disabled {
            style.background = Some(Background::Color(color.scale_alpha(ALPHA_SOFT)));
            style.text_color = p.faint;
        }
        style
    }

    /// Transparent button; the background shows on hover. For icon buttons and row actions.
    pub fn ghost(t: &Theme, s: Status) -> Style {
        let p = palette(t);
        match s {
            Status::Active => base(p.muted),
            Status::Hovered => Style {
                background: Some(Background::Color(p.hover)),
                text_color: p.text,
                ..base(p.text)
            },
            Status::Pressed => Style {
                background: Some(Background::Color(p.selected)),
                text_color: p.text,
                ..base(p.text)
            },
            Status::Disabled => base(p.faint),
        }
    }

    /// Main action of a screen.
    pub fn primary(t: &Theme, s: Status) -> Style {
        let p = palette(t);
        filled(p, p.accent, s)
    }

    /// Destructive action.
    pub fn danger(t: &Theme, s: Status) -> Style {
        let p = palette(t);
        filled(p, p.danger, s)
    }

    /// Text that acts as a link (ports, "show details").
    pub fn link(t: &Theme, s: Status) -> Style {
        let p = palette(t);
        match s {
            Status::Active | Status::Pressed => base(p.accent),
            Status::Hovered => Style {
                background: Some(Background::Color(p.hover)),
                ..base(p.accent)
            },
            Status::Disabled => base(p.faint),
        }
    }

    /// Sidebar entry: a ghost button; the active one keeps the `selected` background.
    pub fn nav(active: bool) -> impl Fn(&Theme, Status) -> Style {
        move |t, s| {
            let mut style = ghost(t, s);
            if active && s != Status::Pressed {
                let p = palette(t);
                style.background = Some(Background::Color(p.selected));
                style.text_color = p.text;
            }
            style
        }
    }

    /// Filter toggle: accent border and soft tint when on.
    pub fn chip(active: bool) -> impl Fn(&Theme, Status) -> Style {
        move |t, s| {
            let p = palette(t);
            let (border_color, text_color, tint) = if active {
                (
                    p.accent,
                    p.text,
                    Some(Background::Color(p.accent.scale_alpha(ALPHA_SOFT))),
                )
            } else {
                (p.border, p.muted, None)
            };
            let background = match s {
                Status::Hovered | Status::Pressed if !active => Some(Background::Color(p.hover)),
                _ => tint,
            };
            Style {
                background,
                text_color: if s == Status::Disabled {
                    p.faint
                } else {
                    text_color
                },
                border: rounded_border(border_color, BORDER_WIDTH),
                ..base(text_color)
            }
        }
    }
}

pub mod container {
    use iced::widget::container::Style;
    use iced::{Background, Border, Color, Theme};

    use super::{ALPHA_SCRIM, RADIUS, RADIUS_LG, rounded_border, shadow};
    use crate::ui::theme::palette;
    use crate::ui::tokens::BORDER_WIDTH;

    /// The window background.
    pub fn bg(t: &Theme) -> Style {
        let p = palette(t);
        Style {
            text_color: Some(p.text),
            background: Some(Background::Color(p.bg)),
            ..Style::default()
        }
    }

    /// Sidebar, panel headers and status bar.
    pub fn surface(t: &Theme) -> Style {
        let p = palette(t);
        Style {
            text_color: Some(p.text),
            background: Some(Background::Color(p.surface)),
            ..Style::default()
        }
    }

    /// Floating things (modal, palette, toast, tooltip): border, large radius and shadow.
    pub fn elevated(t: &Theme) -> Style {
        let p = palette(t);
        Style {
            text_color: Some(p.text),
            background: Some(Background::Color(p.elevated)),
            border: Border {
                radius: RADIUS_LG.into(),
                ..rounded_border(p.border, BORDER_WIDTH)
            },
            shadow: shadow(),
            ..Style::default()
        }
    }

    /// Plain 1px-bordered box, no shadow.
    pub fn bordered(t: &Theme) -> Style {
        let p = palette(t);
        Style {
            text_color: Some(p.text),
            border: rounded_border(p.border, BORDER_WIDTH),
            ..Style::default()
        }
    }

    /// Dimmed backdrop behind a modal.
    pub fn scrim(_: &Theme) -> Style {
        Style {
            background: Some(Background::Color(Color::BLACK.scale_alpha(ALPHA_SCRIM))),
            ..Style::default()
        }
    }

    /// A solid shape, for the status dot and accent bars.
    pub fn swatch(color: Color, radius: f32) -> impl Fn(&Theme) -> Style {
        move |_| Style {
            background: Some(Background::Color(color)),
            border: Border {
                radius: radius.into(),
                ..Border::default()
            },
            ..Style::default()
        }
    }

    /// A list row: selected beats hovered; a resting row has no background.
    pub fn row(selected: bool, hovered: bool) -> impl Fn(&Theme) -> Style {
        move |t| {
            let p = palette(t);
            let background = if selected {
                Some(Background::Color(p.selected))
            } else if hovered {
                Some(Background::Color(p.hover))
            } else {
                None
            };
            Style {
                text_color: Some(p.text),
                background,
                border: Border {
                    radius: RADIUS.into(),
                    ..Border::default()
                },
                ..Style::default()
            }
        }
    }
}

pub mod text {
    use iced::Theme;
    use iced::widget::text::Style;

    use crate::ui::theme::palette;

    pub fn muted(t: &Theme) -> Style {
        Style {
            color: Some(palette(t).muted),
        }
    }

    pub fn faint(t: &Theme) -> Style {
        Style {
            color: Some(palette(t).faint),
        }
    }
}

/// Search box and other single-line inputs: no heavy frame, `accent` border while focused.
pub fn text_input_field(t: &Theme, s: text_input::Status) -> text_input::Style {
    let p = palette(t);
    let border_color = match s {
        text_input::Status::Focused { .. } => p.accent,
        text_input::Status::Hovered => p.muted,
        text_input::Status::Active | text_input::Status::Disabled => p.border,
    };
    text_input::Style {
        background: Background::Color(p.bg),
        border: rounded_border(border_color, BORDER_WIDTH),
        icon: p.muted,
        placeholder: p.faint,
        value: if s == text_input::Status::Disabled {
            p.faint
        } else {
            p.text
        },
        selection: p.accent.scale_alpha(ALPHA_SELECTION),
    }
}

/// Use with [`thin_scrollbar`] so the rail is as thin as the style suggests.
pub fn scrollable_thin(t: &Theme, s: scrollable::Status) -> scrollable::Style {
    let p = palette(t);
    let scroller_color = match s {
        scrollable::Status::Active { .. } => p.border,
        scrollable::Status::Hovered { .. } => p.faint,
        scrollable::Status::Dragged { .. } => p.muted,
    };
    let rail = scrollable::Rail {
        background: None,
        border: border::rounded(RADIUS),
        scroller: scrollable::Scroller {
            background: Background::Color(scroller_color),
            border: border::rounded(RADIUS),
        },
    };
    scrollable::Style {
        container: iced::widget::container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Background::Color(p.elevated),
            border: border::rounded(RADIUS_LG)
                .width(BORDER_WIDTH)
                .color(p.border),
            shadow: shadow(),
            icon: p.muted,
        },
    }
}

/// Vertical scrolling with a thin scrollbar.
pub fn thin_scrollbar() -> scrollable::Direction {
    scrollable::Direction::Vertical(
        scrollable::Scrollbar::new()
            .width(SCROLLBAR_WIDTH)
            .scroller_width(SCROLLBAR_WIDTH),
    )
}

/// Divider of the pane grid: a thin accent line while hovered or dragged, no region highlight.
pub fn pane_grid_style(t: &Theme) -> pane_grid::Style {
    let line = pane_grid::Line {
        color: palette(t).accent,
        width: BORDER_WIDTH,
    };
    pane_grid::Style {
        hovered_region: pane_grid::Highlight {
            background: Background::Color(Color::TRANSPARENT),
            border: rounded_border(Color::TRANSPARENT, 0.0),
        },
        picked_split: line,
        hovered_split: line,
    }
}

/// 1px divider in the border color.
pub fn rule_border(t: &Theme) -> rule::Style {
    rule::Style {
        color: palette(t).border,
        radius: border::Radius::default(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::theme::theme;
    use crate::ui::tokens::{DARK, LIGHT};
    use iced::widget::button::Status;

    #[test]
    fn active_nav_entry_keeps_the_selected_background() {
        let t = theme(&DARK);
        let active = button::nav(true)(&t, Status::Active);
        assert_eq!(active.background, Some(Background::Color(DARK.selected)));
        let idle = button::nav(false)(&t, Status::Active);
        assert_eq!(idle.background, None);
        let hovered = button::nav(false)(&t, Status::Hovered);
        assert_eq!(hovered.background, Some(Background::Color(DARK.hover)));
    }

    #[test]
    fn pane_grid_divider_is_accent_in_both_themes() {
        for p in [&DARK, &LIGHT] {
            let s = pane_grid_style(&theme(p));
            assert_eq!(s.hovered_split.color, p.accent);
            assert_eq!(s.picked_split.color, p.accent);
        }
    }
}
