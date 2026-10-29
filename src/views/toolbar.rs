//! The bar above a list: search, filter chips and the item count.

use iced::widget::{column, container, row, rule, text};
use iced::{Alignment, Element, Length, Padding};
use rust_i18n::t;

use crate::ui::style;
use crate::ui::tokens::{BORDER_WIDTH, SPACE_1, SPACE_2, SPACE_3, TEXT_SM};

/// Key of the count text: "1 item" is singular, everything else plural.
fn count_key(count: usize) -> &'static str {
    if count == 1 {
        "containers.count_one"
    } else {
        "containers.count"
    }
}

/// `search` on the left, `chips` and the number of listed items on the right.
pub fn toolbar<'a, M: 'a>(
    search: Element<'a, M>,
    chips: Vec<Element<'a, M>>,
    count: usize,
) -> Element<'a, M> {
    let line = row![
        container(search).width(Length::Fill),
        row(chips).spacing(SPACE_1).align_y(Alignment::Center),
        text(t!(count_key(count), count = count).into_owned())
            .size(TEXT_SM)
            .style(style::text::faint),
    ]
    .spacing(SPACE_3)
    .align_y(Alignment::Center);
    column![
        container(line).padding(Padding::from([SPACE_2, SPACE_3])),
        rule::horizontal(BORDER_WIDTH).style(style::rule_border),
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toolbar_builds_with_and_without_chips() {
        let _: Element<'_, ()> = toolbar(text("search").into(), vec![], 0);
        let _: Element<'_, ()> = toolbar(
            text("search").into(),
            vec![text("a").into(), text("b").into()],
            8,
        );
    }

    #[test]
    fn count_text_is_singular_for_one_and_translated() {
        assert_eq!(count_key(1), "containers.count_one");
        assert_eq!(count_key(0), "containers.count");
        assert_eq!(count_key(8), "containers.count");
        for key in ["containers.count", "containers.count_one"] {
            for locale in ["en", "pt-BR"] {
                let text = t!(key, locale = locale, count = 8);
                assert_ne!(text, key, "missing {locale} translation for {key}");
                assert!(text.contains('8'), "{key} ({locale}) lacks the count");
            }
        }
    }
}
