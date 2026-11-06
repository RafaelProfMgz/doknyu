//! Image list: one row per tag, search, safe removal and the "remove untagged" dialog.

use std::collections::HashSet;

use iced::widget::text::Wrapping;
use iced::widget::{
    Column, Id, button, center, column, container, row, scrollable, sensor, space, text,
};
use iced::{Alignment, Element, Length, Padding};
use rust_i18n::t;

use crate::actions::{self, ActionId};
use crate::app::Message;
use crate::error::DockerError;
use crate::model::{ImageInfo, PruneReport};
use crate::ui::components::{
    RowEvents, Shortcut, icon_button, kbd, list_header, list_row, search_field,
};
use crate::ui::fonts::{MONO, SANS_MEDIUM, SANS_SEMIBOLD};
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::tokens::{
    DIALOG_LIST_HEIGHT, ICON_BUTTON_SIZE, ROW_HEIGHT, SPACE_1, SPACE_2, SPACE_4, SPACE_6, TEXT,
    TEXT_LG, TEXT_SM,
};
use crate::views::toolbar::toolbar;

/// Widget id of the search box, so a shortcut can focus it.
pub const SEARCH_ID: &str = "images-search";

/// Widget id of the scrollable with the rows, so the keyboard selection can scroll it.
pub const LIST_ID: &str = "images-list";

/// What Docker's list shows for an image without a tag.
const NO_TAG: &str = "<none>:<none>";

/// Characters of the image id shown, as `docker images` does.
const SHORT_ID_LEN: usize = 12;

/// One line of the list: an image has one for each of its tags, or a single one if it has none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageRow {
    /// What to hand to `remove_image`: `repo:tag`, or the id of an untagged image. Also the
    /// key of the row (selection, hover, pending).
    pub reference: String,
    /// What the row shows: `repo:tag`, or `<none>:<none>`.
    pub label: String,
    /// Full `sha256:...` id.
    pub id: String,
    pub size: u64,
    /// Unix seconds.
    pub created: i64,
    pub used_by: Vec<String>,
    /// Tags of the same image that have their own rows.
    pub other_tags: usize,
}

impl ImageRow {
    /// Untagged, so removed by id.
    pub fn is_dangling(&self) -> bool {
        self.reference == self.id
    }
}

/// The rows of the list: one per tag.
pub fn rows(items: &[ImageInfo]) -> Vec<ImageRow> {
    let row_of =
        |image: &ImageInfo, reference: String, label: String, other_tags: usize| ImageRow {
            reference,
            label,
            id: image.id.clone(),
            size: image.size,
            created: image.created,
            used_by: image.used_by.clone(),
            other_tags,
        };
    items
        .iter()
        .flat_map(|image| {
            if image.repo_tags.is_empty() {
                return vec![row_of(image, image.id.clone(), NO_TAG.to_owned(), 0)];
            }
            let others = image.repo_tags.len() - 1;
            image
                .repo_tags
                .iter()
                .map(|tag| row_of(image, tag.clone(), tag.clone(), others))
                .collect()
        })
        .collect()
}

/// The first twelve characters of an id, without the `sha256:` prefix.
pub fn short_id(id: &str) -> &str {
    let id = id.strip_prefix("sha256:").unwrap_or(id);
    match id.char_indices().nth(SHORT_ID_LEN) {
        Some((end, _)) => &id[..end],
        None => id,
    }
}

/// A size in bytes for people, in base 1000 like Docker: `512 B`, `1.2 MB`, `3.4 GB`.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    // 999.96 KB rounds to "1000.0 KB": that is "1.0 MB".
    if (value * 10.0).round() / 10.0 >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// How long ago an image was created, in the largest whole unit: `3 days`.
pub fn format_age(created: i64, now: i64) -> String {
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    let secs = now.saturating_sub(created);
    let plural = |n: i64, one: &str, many: &str| {
        if n == 1 {
            t!(one, count = n)
        } else {
            t!(many, count = n)
        }
        .into_owned()
    };
    match secs {
        s if s < MINUTE => t!("images.age.now").into_owned(),
        s if s < HOUR => t!("images.age.minutes", count = s / MINUTE).into_owned(),
        s if s < DAY => t!("images.age.hours", count = s / HOUR).into_owned(),
        s if s < 30 * DAY => plural(s / DAY, "images.age.day", "images.age.days"),
        s if s < 365 * DAY => plural(s / (30 * DAY), "images.age.month", "images.age.months"),
        s => plural(s / (365 * DAY), "images.age.year", "images.age.years"),
    }
}

/// Seconds since the Unix epoch.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// What the open dialog asks the user to confirm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageConfirm {
    /// Remove this reference (`repo:tag`, or the id of an untagged image).
    Remove(String),
    /// Remove the untagged images nobody uses: the (id, size) of each one when the dialog
    /// opened or was last refreshed.
    Prune(Vec<(String, u64)>),
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImagesState {
    pub items: Vec<ImageInfo>,
    pub query: String,
    pub confirm: Option<ImageConfirm>,
    /// References with a removal in flight.
    pub pending: HashSet<String>,
    /// The highlighted row, by reference.
    pub selected: Option<String>,
    /// The row under the cursor, by reference.
    pub hovered: Option<String>,
    /// Scroll position of the rows and the height of the area showing them, as last reported.
    pub scroll_y: f32,
    pub viewport_h: f32,
}

/// Messages of the image list.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A list reload finished; the number identifies which reload it answers.
    Loaded(u64, Result<Vec<ImageInfo>, DockerError>),
    Query(String),
    /// A click on a row (by reference).
    Select(String),
    Hover(String),
    Leave(String),
    AskRemove(String),
    AskPrune,
    /// The dialog's main button (or Enter).
    Confirm,
    Cancel,
    Removed(String, Result<(), DockerError>),
    Pruned(Result<PruneReport, DockerError>),
    Scrolled {
        offset: f32,
        height: f32,
    },
    Resized(f32),
}

impl ImagesState {
    /// Rows matching the search (`repo:tag` or id, case-insensitive), by tag with the untagged
    /// ones last.
    pub fn visible(&self) -> Vec<ImageRow> {
        let query = self.query.trim().to_lowercase();
        let mut rows: Vec<ImageRow> = rows(&self.items)
            .into_iter()
            .filter(|r| {
                query.is_empty()
                    || r.label.to_lowercase().contains(&query)
                    || r.id.to_lowercase().contains(&query)
            })
            .collect();
        rows.sort_by_cached_key(|r| (r.is_dangling(), r.label.to_lowercase(), r.reference.clone()));
        rows
    }

    /// After the search changed: a selection that is no longer in [`Self::visible`] moves to
    /// the first visible row, or to nothing if the list is empty.
    pub fn keep_selection_visible(&mut self) {
        let visible = self.visible();
        let hidden = self
            .selected
            .as_deref()
            .is_some_and(|reference| !visible.iter().any(|r| r.reference == reference));
        if hidden {
            self.selected = visible.first().map(|r| r.reference.clone());
        }
    }

    /// Replaces the list after a reload. A selected row that is gone hands the selection to
    /// the one now at its position in [`Self::visible`] (or the last one, or nothing).
    pub fn set_items(&mut self, items: Vec<ImageInfo>) {
        let old_position = self
            .selected
            .as_deref()
            .and_then(|reference| self.visible().iter().position(|r| r.reference == reference));
        self.items = items;
        let exists = |this: &Self, reference: &str| {
            rows(&this.items).iter().any(|r| r.reference == reference)
        };
        if self.selected.as_deref().is_some_and(|r| !exists(self, r)) {
            let visible = self.visible();
            let position = old_position
                .unwrap_or(usize::MAX)
                .min(visible.len().saturating_sub(1));
            self.selected = visible.get(position).map(|r| r.reference.clone());
        }
        if self.hovered.as_deref().is_some_and(|r| !exists(self, r)) {
            self.hovered = None;
        }
        // The row to remove is gone: nothing left to confirm.
        if matches!(&self.confirm, Some(ImageConfirm::Remove(r)) if !exists(self, r)) {
            self.confirm = None;
        }
        // What the prune dialog lists is what will be removed, so it follows the list.
        if matches!(self.confirm, Some(ImageConfirm::Prune(_))) {
            self.confirm = Some(ImageConfirm::Prune(self.dangling()));
        }
    }

    /// The row with this reference, if listed.
    pub fn row(&self, reference: &str) -> Option<ImageRow> {
        rows(&self.items)
            .into_iter()
            .find(|r| r.reference == reference)
    }

    /// Whether the row can be removed now: Docker refuses an image a container uses, so it is
    /// not offered, nor is one that is being removed already.
    pub fn removable(&self, reference: &str) -> bool {
        !self.pending.contains(reference)
            && self.row(reference).is_some_and(|r| r.used_by.is_empty())
    }

    /// The (id, size) of the untagged images no container uses and nobody is removing, by id.
    pub fn dangling(&self) -> Vec<(String, u64)> {
        let mut list: Vec<(String, u64)> = self
            .items
            .iter()
            .filter(|i| {
                i.repo_tags.is_empty() && i.used_by.is_empty() && !self.pending.contains(&i.id)
            })
            .map(|i| (i.id.clone(), i.size))
            .collect();
        list.sort();
        list
    }

    /// Whether the main button of the open dialog does something: removing always does,
    /// pruning only if there is something to prune.
    pub fn can_confirm(&self) -> bool {
        match &self.confirm {
            Some(ImageConfirm::Remove(_)) => true,
            Some(ImageConfirm::Prune(list)) => !list.is_empty(),
            None => false,
        }
    }
}

fn msg(m: Msg) -> Message {
    Message::Images(m)
}

/// Key of the text shown instead of the list, if there is nothing to list.
fn empty_key(state: &ImagesState) -> Option<&'static str> {
    if state.items.is_empty() {
        Some("images.empty")
    } else if state.visible().is_empty() {
        Some("images.empty_filtered")
    } else {
        None
    }
}

// Column widths, shared by the header and the rows so they line up.
const NAME_WIDTH: Length = Length::FillPortion(4);
const ID_WIDTH: Length = Length::FillPortion(2);
const SIZE_WIDTH: Length = Length::FillPortion(1);
const AGE_WIDTH: Length = Length::FillPortion(1);
const USED_BY_WIDTH: Length = Length::FillPortion(3);
/// Room for the remove button and the spacer in front of it in `row_actions`.
const ACTIONS_WIDTH: f32 = ICON_BUTTON_SIZE + SPACE_1;

/// A single line of text that is cut, not wrapped, when the column is too narrow.
fn cell<'a>(content: impl Into<Element<'a, Message>>, width: Length) -> Element<'a, Message> {
    container(content).width(width).clip(true).into()
}

/// Names of the containers using an image, or the dash when nobody does.
fn used_by_text(r: &ImageRow) -> String {
    if r.used_by.is_empty() {
        t!("images.not_used").into_owned()
    } else {
        r.used_by.join(", ")
    }
}

/// How the dialogs call a row: its tag, or the id of an untagged one.
fn display_name(r: &ImageRow) -> String {
    if r.is_dangling() {
        format!("{} ({})", r.label, short_id(&r.id))
    } else {
        r.label.clone()
    }
}

fn row_actions<'a>(r: &ImageRow, busy: bool, selected: bool) -> Element<'a, Message> {
    let in_use = !r.used_by.is_empty();
    // An image in use is not offered for removal: Docker would refuse; the tooltip says why.
    let tip = if in_use {
        t!("images.in_use", names = r.used_by.join(", ")).into_owned()
    } else if selected {
        actions::tip(ActionId::RemoveSelected, &t!("images.remove"))
    } else {
        t!("images.remove").into_owned()
    };
    let on_press = (!in_use && !busy).then(|| msg(Msg::AskRemove(r.reference.clone())));
    container(
        row![
            space().width(Length::Fill),
            icon_button(Icon::Trash2, on_press, tip)
        ]
        .spacing(SPACE_1)
        .align_y(Alignment::Center),
    )
    .width(ACTIONS_WIDTH)
    .into()
}

fn row_view<'a>(
    r: &ImageRow,
    now: i64,
    selected: bool,
    hovered: bool,
    busy: bool,
) -> Element<'a, Message> {
    let one_line = |t: iced::widget::Text<'a>| t.wrapping(Wrapping::None);

    let mut cells = row![
        cell(
            one_line(text(r.label.clone()).size(TEXT).font(SANS_MEDIUM)),
            NAME_WIDTH
        ),
        cell(
            one_line(
                text(short_id(&r.id).to_owned())
                    .size(TEXT_SM)
                    .font(MONO)
                    .style(style::text::muted)
            ),
            ID_WIDTH
        ),
        cell(
            one_line(
                text(format_size(r.size))
                    .size(TEXT_SM)
                    .font(MONO)
                    .style(style::text::muted)
            ),
            SIZE_WIDTH
        ),
        cell(
            one_line(
                text(format_age(r.created, now))
                    .size(TEXT_SM)
                    .style(style::text::muted)
            ),
            AGE_WIDTH
        ),
        cell(
            one_line(
                text(used_by_text(r))
                    .size(TEXT_SM)
                    .style(style::text::faint)
            ),
            USED_BY_WIDTH
        ),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center);
    if !(selected || hovered) {
        // Keeps the columns where they are when the actions appear.
        cells = cells.push(space().width(ACTIONS_WIDTH));
    }

    // There is nothing to open in an image: a double click only selects.
    let events = RowEvents {
        on_select: msg(Msg::Select(r.reference.clone())),
        on_open: msg(Msg::Select(r.reference.clone())),
        on_enter: msg(Msg::Hover(r.reference.clone())),
        on_exit: msg(Msg::Leave(r.reference.clone())),
    };
    list_row(
        cells.width(Length::Fill).into(),
        Some(row_actions(r, busy, selected)),
        selected,
        hovered,
        events,
    )
}

/// The content of the dialog; [`crate::ui::components::modal`] frames it. `None` while nothing
/// is being confirmed.
pub fn dialog(state: &ImagesState) -> Option<Element<'static, Message>> {
    let confirm = state.confirm.as_ref()?;
    let (title, body, label) = match confirm {
        ImageConfirm::Remove(reference) => (
            t!("images.remove_dialog.title"),
            remove_body(state, reference),
            t!("images.remove_dialog.confirm"),
        ),
        ImageConfirm::Prune(list) => (
            t!("images.prune_dialog.title"),
            prune_body(list),
            t!("images.prune_dialog.confirm"),
        ),
    };
    let confirm_button = button(
        row![
            text(label.into_owned()),
            kbd(&Shortcut {
                ctrl: false,
                shift: false,
                key: "Enter",
            }),
        ]
        .spacing(SPACE_2)
        .align_y(Alignment::Center),
    )
    .style(style::button::danger)
    .on_press_maybe(state.can_confirm().then(|| msg(Msg::Confirm)));
    let card = column![
        text(title.into_owned()).size(TEXT_LG).font(SANS_SEMIBOLD),
        body,
        row![
            space().width(Length::Fill),
            button(text(t!("common.cancel").into_owned()))
                .style(style::button::ghost)
                .on_press(msg(Msg::Cancel)),
            confirm_button,
        ]
        .spacing(SPACE_2),
    ]
    .spacing(SPACE_4);
    Some(card.into())
}

/// What the remove dialog says: the reference, and that the image stays if it has other tags.
fn remove_body(state: &ImagesState, reference: &str) -> Column<'static, Message> {
    let row = state.row(reference);
    let name = row
        .as_ref()
        .map_or_else(|| reference.to_owned(), display_name);
    let mut body = column![text(
        t!("images.remove_dialog.body", name = name).into_owned()
    )]
    .spacing(SPACE_2);
    if let Some(other_tags) = row.map(|r| r.other_tags).filter(|n| *n > 0) {
        let key = if other_tags == 1 {
            "images.remove_dialog.other_tags_one"
        } else {
            "images.remove_dialog.other_tags"
        };
        body = body.push(
            text(t!(key, count = other_tags).into_owned())
                .size(TEXT_SM)
                .style(style::text::muted),
        );
    }
    body
}

/// What the prune dialog says: the images that will go and their total, or that there are none.
fn prune_body(list: &[(String, u64)]) -> Column<'static, Message> {
    if list.is_empty() {
        return column![text(t!("images.prune_dialog.empty").into_owned())];
    }
    // Rows of a fixed height, so the dialog's height is known before it scrolls.
    let rows = Column::with_children(list.iter().map(|(id, size)| {
        container(
            row![
                text(short_id(id).to_owned()).size(TEXT_SM).font(MONO),
                space().width(Length::Fill),
                text(format_size(*size))
                    .size(TEXT_SM)
                    .font(MONO)
                    .style(style::text::muted),
            ]
            .spacing(SPACE_4)
            .align_y(Alignment::Center),
        )
        .height(ROW_HEIGHT)
        .center_y(ROW_HEIGHT)
        .into()
    }));
    // A long list scrolls inside the dialog instead of growing past the window.
    let rows: Element<'static, Message> = if list.len() > DIALOG_LIST_ROWS {
        scrollable(rows)
            .direction(style::thin_scrollbar())
            .style(style::scrollable_thin)
            .height(DIALOG_LIST_HEIGHT)
            .into()
    } else {
        rows.into()
    };
    let total: u64 = list.iter().map(|(_, size)| size).sum();
    column![
        text(t!("images.prune_dialog.body").into_owned()),
        container(rows).padding(Padding::from([0.0, SPACE_2])),
        text(t!("images.prune_dialog.total", size = format_size(total)).into_owned())
            .font(SANS_MEDIUM),
    ]
    .spacing(SPACE_2)
}

/// Images the prune dialog lists before it starts to scroll.
const DIALOG_LIST_ROWS: usize = 8;

/// The Images screen. The dialog is drawn by `App::view` over the whole window.
pub fn view(state: &ImagesState) -> Element<'_, Message> {
    let visible = state.visible();
    let prune = button(text(t!("images.prune").into_owned()).size(TEXT_SM))
        .style(style::button::ghost)
        .on_press(msg(Msg::AskPrune));
    let bar = toolbar(
        search_field(
            Id::new(SEARCH_ID),
            t!("images.search").into_owned(),
            &state.query,
            |q| msg(Msg::Query(q)),
        ),
        vec![prune.into()],
        visible.len(),
    );

    let body: Element<'_, Message> = match empty_key(state) {
        Some(key) => center(
            text(t!(key).into_owned())
                .style(style::text::muted)
                .align_x(Alignment::Center),
        )
        .padding(SPACE_6)
        .into(),
        None => {
            let header = list_header(vec![
                (t!("images.col.name").into_owned(), NAME_WIDTH),
                (t!("images.col.id").into_owned(), ID_WIDTH),
                (t!("images.col.size").into_owned(), SIZE_WIDTH),
                (t!("images.col.age").into_owned(), AGE_WIDTH),
                (t!("images.col.used_by").into_owned(), USED_BY_WIDTH),
                (String::new(), Length::Fixed(ACTIONS_WIDTH)),
            ]);
            let now = now_secs();
            let rows = Column::with_children(visible.iter().map(|r| {
                row_view(
                    r,
                    now,
                    state.selected.as_deref() == Some(r.reference.as_str()),
                    state.hovered.as_deref() == Some(r.reference.as_str()),
                    state.pending.contains(&r.reference),
                )
            }));
            // The sensor reports the height of the area the rows scroll in.
            let list = sensor(
                scrollable(rows)
                    .id(Id::new(LIST_ID))
                    .direction(style::thin_scrollbar())
                    .style(style::scrollable_thin)
                    .on_scroll(|v| {
                        msg(Msg::Scrolled {
                            offset: v.absolute_offset().y,
                            height: v.bounds().height,
                        })
                    })
                    .height(Length::Fill),
            )
            .on_show(|size| msg(Msg::Resized(size.height)))
            .on_resize(|size| msg(Msg::Resized(size.height)));
            column![header, list].into()
        }
    };

    column![bar, body]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ImageInfo;

    fn image(id: &str, tags: &[&str], size: u64, used_by: &[&str]) -> ImageInfo {
        ImageInfo {
            id: id.into(),
            repo_tags: tags.iter().map(|t| t.to_string()).collect(),
            size,
            created: 1_700_000_000,
            used_by: used_by.iter().map(|n| n.to_string()).collect(),
        }
    }

    fn state_with(items: Vec<ImageInfo>) -> ImagesState {
        ImagesState {
            items,
            ..ImagesState::default()
        }
    }

    fn references(state: &ImagesState) -> Vec<String> {
        state.visible().into_iter().map(|r| r.reference).collect()
    }

    #[test]
    fn rows_one_per_tag() {
        let items = vec![
            image("sha256:aaa", &["app:1", "app:latest"], 5, &["web"]),
            image("sha256:bbb", &["redis:7"], 9, &[]),
        ];
        let rows = rows(&items);
        let refs: Vec<_> = rows.iter().map(|r| r.reference.as_str()).collect();
        assert_eq!(refs, vec!["app:1", "app:latest", "redis:7"]);
        // The same image twice: same id, size and users; each knows about the other tag.
        assert_eq!(rows[0].id, rows[1].id);
        assert_eq!(rows[0].size, rows[1].size);
        assert_eq!(rows[0].used_by, vec!["web".to_string()]);
        assert_eq!(rows[0].other_tags, 1);
        assert_eq!(rows[1].other_tags, 1);
        assert_eq!(rows[2].other_tags, 0);
        assert_eq!(rows[0].label, "app:1");
    }

    #[test]
    fn dangling_row_uses_id_as_reference() {
        let items = vec![image("sha256:0123456789abcdef", &[], 7, &[])];
        let rows = rows(&items);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].reference, "sha256:0123456789abcdef");
        assert_eq!(rows[0].label, "<none>:<none>");
        assert_eq!(rows[0].other_tags, 0);
        assert_eq!(short_id(&rows[0].id), "0123456789ab");
    }

    #[test]
    fn short_id_drops_the_prefix_and_keeps_twelve_chars() {
        assert_eq!(short_id("sha256:0123456789abcdef"), "0123456789ab");
        assert_eq!(short_id("0123456789abcdef"), "0123456789ab");
        assert_eq!(short_id("sha256:abc"), "abc");
    }

    #[test]
    fn search_by_tag_and_by_id() {
        let mut s = state_with(vec![
            image("sha256:aaaaaaaaaaaa1111", &["Postgres:16"], 5, &[]),
            image("sha256:bbbbbbbbbbbb2222", &["redis:7"], 5, &[]),
            image("sha256:cccccccccccc3333", &[], 5, &[]),
        ]);
        assert_eq!(references(&s).len(), 3);
        s.query = " POSTGRES ".into();
        assert_eq!(references(&s), vec!["Postgres:16"]);
        s.query = "redis:7".into();
        assert_eq!(references(&s), vec!["redis:7"]);
        // By ID, with or without the prefix, in any case.
        s.query = "BBBB".into();
        assert_eq!(references(&s), vec!["redis:7"]);
        s.query = "sha256:cccc".into();
        assert_eq!(references(&s), vec!["sha256:cccccccccccc3333"]);
        s.query = "zzz".into();
        assert!(s.visible().is_empty());
    }

    #[test]
    fn visible_is_sorted_by_tag_with_dangling_last() {
        let s = state_with(vec![
            image("sha256:d", &[], 1, &[]),
            image("sha256:b", &["b:1"], 1, &[]),
            image("sha256:a", &["A:1"], 1, &[]),
        ]);
        assert_eq!(references(&s), vec!["A:1", "b:1", "sha256:d"]);
    }

    #[test]
    fn in_use_image_cannot_be_removed() {
        let s = state_with(vec![
            image("sha256:a", &["web:1"], 1, &["web"]),
            image("sha256:b", &["free:1"], 1, &[]),
        ]);
        assert!(!s.removable("web:1"));
        assert!(s.removable("free:1"));
        assert!(!s.removable("nope:1"));
        let mut s = s;
        s.pending.insert("free:1".into());
        assert!(!s.removable("free:1"), "being removed already");
    }

    #[test]
    fn dangling_lists_only_untagged_unused_images() {
        let mut s = state_with(vec![
            image("sha256:a", &["a:1"], 1, &[]),
            image("sha256:b", &[], 20, &[]),
            image("sha256:c", &[], 30, &["web"]),
            image("sha256:d", &[], 10, &[]),
        ]);
        assert_eq!(
            s.dangling(),
            vec![("sha256:b".to_string(), 20), ("sha256:d".to_string(), 10)]
        );
        s.pending.insert("sha256:b".into());
        assert_eq!(s.dangling(), vec![("sha256:d".to_string(), 10)]);
    }

    #[test]
    fn remove_dialog_closes_when_row_disappears() {
        let mut s = state_with(vec![image("sha256:a", &["a:1", "a:2"], 1, &[])]);
        s.confirm = Some(ImageConfirm::Remove("a:1".into()));
        // The other tag went away: this one is still there.
        s.set_items(vec![image("sha256:a", &["a:1"], 1, &[])]);
        assert!(s.confirm.is_some());
        s.set_items(vec![]);
        assert_eq!(s.confirm, None);
    }

    #[test]
    fn set_items_refreshes_the_prune_list() {
        let mut s = state_with(vec![image("sha256:a", &[], 5, &[])]);
        s.confirm = Some(ImageConfirm::Prune(s.dangling()));
        s.set_items(vec![
            image("sha256:a", &[], 5, &["web"]),
            image("sha256:b", &[], 8, &[]),
        ]);
        assert_eq!(
            s.confirm,
            Some(ImageConfirm::Prune(vec![("sha256:b".into(), 8)]))
        );
        s.set_items(vec![]);
        assert_eq!(s.confirm, Some(ImageConfirm::Prune(vec![])));
        assert!(!s.can_confirm());
    }

    #[test]
    fn prune_with_empty_list_cannot_confirm() {
        let mut s = state_with(vec![]);
        assert!(!s.can_confirm(), "no dialog");
        s.confirm = Some(ImageConfirm::Prune(vec![]));
        assert!(!s.can_confirm());
        s.confirm = Some(ImageConfirm::Prune(vec![("sha256:a".into(), 1)]));
        assert!(s.can_confirm());
        s.confirm = Some(ImageConfirm::Remove("a:1".into()));
        assert!(s.can_confirm());
    }

    #[test]
    fn selection_moves_to_neighbor_when_removed() {
        let abc = || {
            vec![
                image("sha256:a", &["a:1"], 1, &[]),
                image("sha256:b", &["b:1"], 1, &[]),
                image("sha256:c", &["c:1"], 1, &[]),
            ]
        };
        let mut s = state_with(abc());
        s.selected = Some("b:1".into());
        s.set_items(vec![abc().remove(0), abc().remove(2)]);
        assert_eq!(s.selected.as_deref(), Some("c:1"));
        s.set_items(vec![]);
        assert_eq!(s.selected, None);
    }

    #[test]
    fn selection_moves_to_first_visible_when_the_search_hides_it() {
        let mut s = state_with(vec![
            image("sha256:a", &["alpha:1"], 1, &[]),
            image("sha256:b", &["beta:1"], 1, &[]),
        ]);
        s.selected = Some("beta:1".into());
        s.query = "alp".into();
        s.keep_selection_visible();
        assert_eq!(s.selected.as_deref(), Some("alpha:1"));
        s.query = "zzz".into();
        s.keep_selection_visible();
        assert_eq!(s.selected, None);
    }

    #[test]
    fn hover_is_cleared_when_the_row_is_gone() {
        let mut s = state_with(vec![image("sha256:a", &["a:1"], 1, &[])]);
        s.hovered = Some("a:1".into());
        s.set_items(vec![]);
        assert_eq!(s.hovered, None);
    }

    #[test]
    fn format_size_base_1000() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(999), "999 B");
        assert_eq!(format_size(1_000), "1.0 KB");
        assert_eq!(format_size(1_200_000), "1.2 MB");
        assert_eq!(format_size(187_000_000), "187.0 MB");
        assert_eq!(format_size(3_400_000_000), "3.4 GB");
        assert_eq!(format_size(2_000_000_000_000), "2.0 TB");
        // Rounding must not print "1000.0 KB".
        assert_eq!(format_size(999_999), "1.0 MB");
        assert_eq!(format_size(u64::MAX), "18446744.1 TB");
    }

    #[test]
    fn format_age_relative() {
        use rust_i18n::t;
        let now = 1_800_000_000;
        let day = 86_400;
        let age = |secs: i64| format_age(now - secs, now);
        assert_eq!(age(10), t!("images.age.now"));
        assert_eq!(age(-500), t!("images.age.now"), "from the future");
        assert_eq!(age(5 * 60), t!("images.age.minutes", count = 5));
        assert_eq!(age(3 * 3600), t!("images.age.hours", count = 3));
        assert_eq!(age(day), t!("images.age.day", count = 1));
        assert_eq!(age(3 * day), t!("images.age.days", count = 3));
        assert_eq!(age(45 * day), t!("images.age.month", count = 1));
        assert_eq!(age(90 * day), t!("images.age.months", count = 3));
        assert_eq!(age(400 * day), t!("images.age.year", count = 1));
        assert_eq!(age(800 * day), t!("images.age.years", count = 2));
        assert!(age(3 * day).contains('3'));
    }

    #[test]
    fn view_and_dialog_build_in_every_state() {
        let mut s = state_with(vec![
            image(
                "sha256:0123456789abcdef",
                &["nginx:latest", "nginx:1"],
                5,
                &["web"],
            ),
            image(
                "sha256:fedcba9876543210",
                &["x/".repeat(100).as_str()],
                5,
                &[],
            ),
            image("sha256:dddddddddddddddd", &[], 52_000_000, &[]),
        ]);
        let _ = view(&s);
        s.selected = Some("nginx:1".into());
        s.hovered = Some("sha256:dddddddddddddddd".into());
        s.pending.insert("nginx:latest".into());
        let _ = view(&s);
        for confirm in [
            ImageConfirm::Remove("nginx:1".into()),
            ImageConfirm::Remove("sha256:dddddddddddddddd".into()),
            ImageConfirm::Remove("gone:1".into()),
            ImageConfirm::Prune(vec![]),
            ImageConfirm::Prune(
                (0..50)
                    .map(|i| (format!("sha256:{i:064}"), 1_000 * i))
                    .collect(),
            ),
        ] {
            s.confirm = Some(confirm);
            assert!(dialog(&s).is_some());
            let _ = view(&s);
        }
        s.confirm = None;
        assert!(dialog(&s).is_none());
        let _ = view(&ImagesState::default());
        s.query = "zzz".into();
        let _ = view(&s);
    }

    #[test]
    fn images_texts_are_translated() {
        use rust_i18n::t;
        for key in [
            "sidebar.images",
            "images.search",
            "images.empty",
            "images.empty_filtered",
            "images.col.name",
            "images.col.id",
            "images.col.size",
            "images.col.age",
            "images.col.used_by",
            "images.not_used",
            "images.remove",
            "images.in_use",
            "images.remove_dialog.title",
            "images.remove_dialog.body",
            "images.remove_dialog.other_tags",
            "images.remove_dialog.other_tags_one",
            "images.remove_dialog.confirm",
            "images.prune",
            "images.prune_dialog.title",
            "images.prune_dialog.body",
            "images.prune_dialog.total",
            "images.prune_dialog.empty",
            "images.prune_dialog.confirm",
            "images.pruned",
            "images.pruned_one",
            "images.age.now",
            "images.age.minutes",
            "images.age.hours",
            "images.age.day",
            "images.age.days",
            "images.age.month",
            "images.age.months",
            "images.age.year",
            "images.age.years",
            "actions.go_images",
            "actions.prune_images",
        ] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(
                    t!(
                        key,
                        locale = locale,
                        name = "x",
                        names = "x",
                        count = 2,
                        size = "x"
                    ),
                    key,
                    "missing {locale} translation for {key}"
                );
            }
        }
    }
}
