//! The command palette: a search box and the actions that match it.

use std::cmp::Reverse;

use iced::widget::{
    Column, Id, button, column, container, mouse_area, opaque, row, rule, scrollable, text,
};
use iced::{Alignment, Element, Length, Padding};
use rust_i18n::t;

use crate::actions::{self, ActionId};
use crate::app::{App, Message};
use crate::fuzzy;
use crate::ui::components::{kbd, search_field};
use crate::ui::style;
use crate::ui::tokens::{BORDER_WIDTH, ROW_HEIGHT, SPACE_1, SPACE_2, TEXT};

/// Widget id of the palette's search box, so it can take the focus when it opens.
pub const PALETTE_INPUT_ID: &str = "palette-input";

/// Widget id of the scrollable with the results, so the keyboard cursor can scroll it.
pub const PALETTE_LIST_ID: &str = "palette-list";

const PALETTE_WIDTH: f32 = 560.0;
/// Distance between the top of the window and the palette.
const PALETTE_TOP_MARGIN: f32 = 64.0;
/// Most results on screen at once; the rest scroll.
const MAX_VISIBLE: usize = 12;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PaletteState {
    pub query: String,
    /// Index of the highlighted result.
    pub cursor: usize,
}

/// The actions the palette lists for `query`: meant for the palette, available right now and
/// matching the translated label, best match first. Equal scores keep the registry order.
pub fn matches(app: &App, query: &str) -> Vec<ActionId> {
    let mut found: Vec<(u32, ActionId)> = actions::all()
        .iter()
        .filter(|d| d.in_palette && actions::available(app, d.id))
        .filter_map(|d| fuzzy::score(query, &actions::label(d.id)).map(|score| (score, d.id)))
        .collect();
    // The sort is stable.
    found.sort_by_key(|(score, _)| Reverse(*score));
    found.into_iter().map(|(_, id)| id).collect()
}

/// Height of the results area for `count` results.
pub fn list_height(count: usize) -> f32 {
    count.min(MAX_VISIBLE) as f32 * ROW_HEIGHT
}

fn item<'a>(id: ActionId, highlighted: bool) -> Element<'a, Message> {
    let mut content = row![text(actions::label(id)).size(TEXT).width(Length::Fill),]
        .spacing(SPACE_2)
        .align_y(Alignment::Center);
    if let Some(shortcut) = &actions::def(id).shortcut {
        content = content.push(kbd(shortcut));
    }
    // A fixed-height button keeps its content at the top.
    button(container(content).center_y(Length::Fill))
        .on_press(Message::PaletteRun(Some(id)))
        .width(Length::Fill)
        .height(ROW_HEIGHT)
        .padding(Padding::from([0.0, SPACE_2]))
        .style(style::button::nav(highlighted))
        .into()
}

/// The palette over a dimmed window; a click outside closes it.
pub fn view<'a>(app: &'a App, state: &'a PaletteState) -> Element<'a, Message> {
    let results = matches(app, &state.query);
    let input = search_field(
        Id::new(PALETTE_INPUT_ID),
        t!("palette.placeholder").into_owned(),
        &state.query,
        Message::PaletteQuery,
    );
    let list: Element<'a, Message> = if results.is_empty() {
        container(text(t!("palette.empty").into_owned()).style(style::text::muted))
            .padding(Padding::from([SPACE_2, SPACE_2]))
            .into()
    } else {
        let height = list_height(results.len());
        let items = Column::with_children(
            results
                .iter()
                .enumerate()
                .map(|(i, id)| item(*id, i == state.cursor)),
        );
        scrollable(items)
            .id(Id::new(PALETTE_LIST_ID))
            .direction(style::thin_scrollbar())
            .style(style::scrollable_thin)
            .on_scroll(|v| Message::PaletteScrolled(v.absolute_offset().y))
            .height(height)
            .into()
    };
    let card = container(
        column![
            input,
            rule::horizontal(BORDER_WIDTH).style(style::rule_border),
            list,
        ]
        .spacing(SPACE_1),
    )
    .padding(SPACE_2)
    .width(PALETTE_WIDTH)
    .style(style::container::elevated);
    opaque(
        mouse_area(
            container(opaque(card))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(Alignment::Center)
                .padding(Padding::default().top(PALETTE_TOP_MARGIN))
                .style(style::container::scrim),
        )
        .on_press(Message::Action(ActionId::Escape)),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::actions;
    use crate::app::test_support::settle;
    use crate::app::{Connection, Message};
    use crate::docker::FakeClient;
    use crate::error::DockerError;

    async fn connected_app() -> App {
        let fake = Arc::new(FakeClient::with_sample_data());
        let mut app = App::new(fake).0;
        settle(&mut app, Message::Pinged(Ok(()))).await;
        app
    }

    #[tokio::test]
    async fn palette_hides_unavailable_actions() {
        let mut app = connected_app().await;
        app.containers.selected = None;
        let all = matches(&app, "");
        assert!(!all.contains(&ActionId::StartStopSelected));
        assert!(!all.contains(&ActionId::RemoveSelected));
        assert!(!all.contains(&ActionId::Retry), "Retry needs a failure");
        assert!(all.contains(&ActionId::GoVolumes));

        app.containers.selected = Some("web-id".into());
        assert!(matches(&app, "").contains(&ActionId::StartStopSelected));

        app.conn = Connection::Failed(DockerError::NotRunning);
        let failed = matches(&app, "");
        assert!(failed.contains(&ActionId::Retry));
        assert!(
            !failed.contains(&ActionId::StartStopSelected),
            "no connection"
        );
    }

    #[tokio::test]
    async fn palette_lists_only_actions_meant_for_it_in_registry_order() {
        let app = connected_app().await;
        let listed = matches(&app, "");
        assert!(!listed.contains(&ActionId::OpenPalette));
        assert!(!listed.contains(&ActionId::Escape));
        assert!(!listed.contains(&ActionId::SelectNext));
        let expected: Vec<_> = actions::all()
            .iter()
            .filter(|d| d.in_palette && actions::available(&app, d.id))
            .map(|d| d.id)
            .collect();
        assert_eq!(listed, expected, "an empty query keeps the registry order");
    }

    #[tokio::test]
    async fn palette_ranks_by_score_and_drops_non_matches() {
        let app = connected_app().await;
        let found = matches(&app, "volumes");
        assert_eq!(found.first(), Some(&ActionId::GoVolumes));
        assert!(!found.contains(&ActionId::GoNetworks));
        assert!(matches(&app, "zzzzzz").is_empty());
    }

    #[tokio::test]
    async fn palette_matches_translated_labels() {
        let app = connected_app().await;
        // The labels come from the active locale, `en` unless something sets another.
        assert!(matches(&app, "dark").contains(&ActionId::ThemeDark));
    }

    #[tokio::test]
    async fn view_builds_for_every_state() {
        let mut app = connected_app().await;
        let states = [
            PaletteState::default(),
            PaletteState {
                query: "zzzz".into(),
                cursor: 0,
            },
            PaletteState {
                query: "a".into(),
                cursor: 99,
            },
        ];
        for conn in [Connection::Ok, Connection::Failed(DockerError::NotRunning)] {
            app.conn = conn;
            for state in &states {
                let _ = view(&app, state);
            }
        }
    }
}
