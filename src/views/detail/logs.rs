//! The Logs tab: the live stream, the bar with search and options, and the virtualized list.
//!
//! The list is a `scrollable` anchored to the bottom: its offset is the distance to the end of
//! the logs, so a fresh list starts at the end and a growing one stays there (follow). Every
//! line has the same height, so only the lines in view (plus a few) are built, as the text of a
//! read-only `text_editor` (so they can be selected and copied), and two spacers stand in for
//! the rest. The colors come from `log_highlight`.

use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use futures::{Stream, StreamExt};
use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{
    Id, center, column, container, row, rule, scrollable, sensor, space, stack, text, text_editor,
};
use iced::{Alignment, Element, Length, Padding, Pixels, Subscription};
use rust_i18n::t;

use super::log_highlight::{LogHighlighter, LogHlSettings, log_format};
use super::{DetailState, Msg};
use crate::app::Message;
use crate::docker::DockerClient;
use crate::model::LogLine;
use crate::ui::components::{chip, icon_button, search_field};
use crate::ui::fonts::MONO;
use crate::ui::icons::Icon;
use crate::ui::style;
use crate::ui::tokens::{BORDER_WIDTH, LOG_LINE_HEIGHT, SPACE_2, SPACE_3, SPACE_4, TEXT_SM};

/// Widget id of the search field of the logs, so `FocusLogSearch` can focus it.
pub const SEARCH_ID: &str = "logs-search";
/// Widget id of the scrollable with the lines.
pub const LIST_ID: &str = "logs-list";

/// How many lines of history the daemon sends when the stream starts.
const LOG_TAIL: usize = 500;
/// Lines that arrive within this window, or up to `LOG_BATCH_MAX`, reach the UI as one message.
const LOG_BATCH_WINDOW: Duration = Duration::from_millis(100);
const LOG_BATCH_MAX: usize = 200;

/// Lines built above and below the visible ones, so a quick scroll does not show gaps.
const OVERSCAN: usize = 5;
/// Height assumed for the list until it reports its real one.
const FALLBACK_VIEWPORT_H: f32 = 480.0;
/// Pixels from the end that still count as "at the end".
const AT_BOTTOM_TOLERANCE: f32 = 1.0;

pub fn is_at_bottom(from_bottom: f32) -> bool {
    from_bottom <= AT_BOTTOM_TOLERANCE
}

fn effective_height(viewport_h: f32) -> f32 {
    if viewport_h > 0.0 {
        viewport_h
    } else {
        FALLBACK_VIEWPORT_H
    }
}

/// The lines to build: those in view, `from_bottom` pixels above the end of `count` lines in a
/// list `viewport_h` tall, plus the overscan.
pub fn visible_range(count: usize, viewport_h: f32, from_bottom: f32) -> Range<usize> {
    visible_window(count, viewport_h, from_bottom, OVERSCAN)
}

/// Like [`visible_range`] with `overscan` lines above and below instead of the usual few.
pub fn visible_window(
    count: usize,
    viewport_h: f32,
    from_bottom: f32,
    overscan: usize,
) -> Range<usize> {
    let viewport_h = effective_height(viewport_h);
    let content = count as f32 * LOG_LINE_HEIGHT;
    let max = (content - viewport_h).max(0.0);
    let top = max - from_bottom.clamp(0.0, max);
    let first = (top / LOG_LINE_HEIGHT).floor() as usize;
    let last = ((top + viewport_h) / LOG_LINE_HEIGHT).ceil() as usize;
    first.saturating_sub(overscan)..(last + overscan).min(count)
}

/// The largest `from_bottom` for `count` lines: the top of the logs.
pub fn max_from_bottom(count: usize, viewport_h: f32) -> f32 {
    (count as f32 * LOG_LINE_HEIGHT - effective_height(viewport_h)).max(0.0)
}

/// The `from_bottom` offset that puts line `line` in the middle of the list (or as close as
/// the ends allow).
pub fn offset_for_line(line: usize, count: usize, viewport_h: f32) -> f32 {
    let viewport_h = effective_height(viewport_h);
    let max = (count as f32 * LOG_LINE_HEIGHT - viewport_h).max(0.0);
    let top =
        (line as f32 * LOG_LINE_HEIGHT - (viewport_h - LOG_LINE_HEIGHT) / 2.0).clamp(0.0, max);
    max - top
}

/// Lowercase for comparing: characters whose lowercase is more than one character are kept.
fn fold(c: char) -> char {
    let mut lower = c.to_lowercase();
    match (lower.next(), lower.next()) {
        (Some(only), None) => only,
        _ => c,
    }
}

/// Where `query` occurs in `text`, ignoring case, as ranges of character positions.
pub fn highlight_ranges(text: &str, query: &str) -> Vec<Range<usize>> {
    let needle: Vec<char> = query.chars().map(fold).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let hay: Vec<char> = text.chars().map(fold).collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if hay[i..i + needle.len()] == needle[..] {
            found.push(i..i + needle.len());
            i += needle.len();
        } else {
            i += 1;
        }
    }
    found
}

/// Groups a stream into batches: a batch is sent when `max` lines are in it, or `window` after
/// its first line, or when the stream ends. A busy container then costs one UI update per
/// window instead of one per line.
pub fn batched<S>(source: S, window: Duration, max: usize) -> impl Stream<Item = Vec<LogLine>>
where
    S: Stream<Item = LogLine> + Send + 'static,
{
    futures::stream::unfold(Box::pin(source.fuse()), move |mut source| async move {
        let mut batch = vec![source.next().await?];
        let deadline = tokio::time::Instant::now() + window;
        while batch.len() < max {
            match tokio::time::timeout_at(deadline, source.next()).await {
                Ok(Some(line)) => batch.push(line),
                Ok(None) | Err(_) => break,
            }
        }
        Some((batch, source))
    })
}

/// The log messages of one container: batches of lines, then `LogsEnded`. Any error ends the
/// stream the same way: the list reload tells whether the container is gone.
pub fn log_messages(
    client: Arc<dyn DockerClient>,
    id: String,
    generation: u64,
) -> impl Stream<Item = Message> {
    let lines = client
        .logs(&id, LOG_TAIL)
        .take_while(|line| std::future::ready(line.is_ok()))
        .filter_map(|line| std::future::ready(line.ok()));
    let batch_id = id.clone();
    batched(lines, LOG_BATCH_WINDOW, LOG_BATCH_MAX)
        .map(move |lines| {
            Message::Detail(Msg::LogBatch {
                id: batch_id.clone(),
                generation,
                lines,
            })
        })
        .chain(futures::stream::once(std::future::ready(Message::Detail(
            Msg::LogsEnded { id, generation },
        ))))
}

/// Identity of the log subscription: another container or generation starts a new stream.
pub struct LogsSource {
    pub client: Arc<dyn DockerClient>,
    pub id: String,
    pub generation: u64,
}

impl Hash for LogsSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        self.generation.hash(state);
    }
}

pub fn subscription(source: LogsSource) -> Subscription<Message> {
    Subscription::run_with(source, |s| {
        log_messages(s.client.clone(), s.id.clone(), s.generation)
    })
}

fn msg(m: Msg) -> Message {
    Message::Detail(m)
}

/// The bar above the lines: search with its "n of m" and arrows, timestamps, copy, jump to end.
fn bar<'a>(d: &'a DetailState) -> Element<'a, Message> {
    let has_matches = !d.matches.is_empty();
    let count = match d.match_position() {
        Some((n, total)) => Some(t!("logs.match_of", n = n, total = total).into_owned()),
        None if !d.query.is_empty() => Some(t!("logs.no_matches").into_owned()),
        None => None,
    };
    let mut line = row![
        container(search_field(
            Id::new(SEARCH_ID),
            t!("logs.search").into_owned(),
            &d.query,
            |q| msg(Msg::Query(q)),
        ))
        .width(Length::Fill),
    ]
    .spacing(SPACE_2)
    .align_y(Alignment::Center);
    if let Some(count) = count {
        line = line.push(text(count).size(TEXT_SM).style(style::text::faint));
    }
    line = line
        .push(icon_button(
            Icon::ChevronUp,
            has_matches.then(|| msg(Msg::PrevMatch)),
            t!("logs.prev_match").into_owned(),
        ))
        .push(icon_button(
            Icon::ChevronDown,
            has_matches.then(|| msg(Msg::NextMatch)),
            t!("logs.next_match").into_owned(),
        ))
        .push(chip(
            t!("logs.timestamps").into_owned(),
            d.timestamps,
            msg(Msg::ToggleTimestamps),
        ))
        .push(icon_button(
            Icon::Copy,
            (!d.logs.is_empty()).then(|| msg(Msg::Copy)),
            t!("logs.copy").into_owned(),
        ))
        .push(icon_button(
            Icon::ArrowDownToLine,
            (!d.follow).then(|| msg(Msg::JumpToEnd)),
            t!("logs.jump_end").into_owned(),
        ));
    column![
        container(line).padding(Padding::from([SPACE_2, SPACE_3])),
        rule::horizontal(BORDER_WIDTH).style(style::rule_border),
    ]
    .into()
}

/// The visible lines as a read-only editor, so text can be selected and copied like in a
/// terminal, with a spacer above and below as tall as the lines left out. The editor takes
/// the height of its text and leaves the wheel to the scrollable around it. The match the user
/// is on gets a selected-row band behind the (transparent) editor.
fn lines<'a>(d: &'a DetailState) -> Element<'a, Message> {
    let w = &d.window;
    let settings = LogHlSettings::new(w.version, w.lines.clone());
    let editor = text_editor(&d.editor)
        .font(MONO)
        .size(TEXT_SM)
        .line_height(LineHeight::Absolute(Pixels(LOG_LINE_HEIGHT)))
        .padding(Padding::from([0.0, SPACE_3]))
        .wrapping(Wrapping::None)
        .height(Length::Shrink)
        .style(style::text_editor_log)
        .highlight_with::<LogHighlighter>(settings, log_format)
        .on_action(|action| msg(Msg::Editor(action)));
    let body: Element<'a, Message> = match w.current {
        Some(i) => stack![
            column![
                space().height(i as f32 * LOG_LINE_HEIGHT),
                container(space())
                    .width(Length::Fill)
                    .height(LOG_LINE_HEIGHT)
                    .style(style::container::row(true, false)),
            ],
            editor,
        ]
        .width(Length::Fill)
        .into(),
        None => editor.into(),
    };
    let top = w.range.start as f32 * LOG_LINE_HEIGHT;
    let bottom = w.count.saturating_sub(w.range.end) as f32 * LOG_LINE_HEIGHT;
    let rows = column![space().height(top), body, space().height(bottom)];
    // The sensor reports the height of the area the lines scroll in.
    sensor(
        scrollable(rows)
            .id(Id::new(LIST_ID))
            .direction(style::thin_scrollbar())
            .style(style::scrollable_thin)
            .anchor_bottom()
            .on_scroll(|v| {
                msg(Msg::Scrolled {
                    from_bottom: v.absolute_offset().y,
                    viewport_h: v.bounds().height,
                })
            })
            .height(Length::Fill),
    )
    .on_show(|size| msg(Msg::Resized(size.height)))
    .on_resize(|size| msg(Msg::Resized(size.height)))
    .into()
}

/// The Logs tab of the dock.
pub fn view<'a>(d: &'a DetailState) -> Element<'a, Message> {
    let body: Element<'a, Message> = if d.logs.is_empty() {
        center(text(t!("logs.empty").into_owned()).style(style::text::muted))
            .padding(SPACE_4)
            .into()
    } else {
        lines(d)
    };
    column![bar(d), body]
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use futures::StreamExt;

    use super::*;
    use crate::app::Message;
    use crate::docker::{DockerClient, FakeClient};
    use crate::model::{LogLine, LogStream};
    use crate::ui::tokens::LOG_LINE_HEIGHT;

    fn line(text: &str) -> LogLine {
        LogLine {
            stream: LogStream::Stdout,
            timestamp: None,
            text: text.into(),
        }
    }

    #[test]
    fn visible_range_covers_the_viewport_plus_overscan() {
        let h = LOG_LINE_HEIGHT;
        // 1000 lines, viewport of 10 lines, at the bottom.
        let r = visible_range(1000, 10.0 * h, 0.0);
        assert_eq!(r.end, 1000);
        assert_eq!(r.start, 1000 - 10 - OVERSCAN);
        // At the very top (the offset from the bottom is the whole scroll range).
        let r = visible_range(1000, 10.0 * h, 990.0 * h);
        assert_eq!(r.start, 0);
        assert_eq!(r.end, 10 + OVERSCAN);
        // In the middle.
        let r = visible_range(1000, 10.0 * h, 500.0 * h);
        assert_eq!(r.start, 490 - OVERSCAN);
        assert_eq!(r.end, 500 + OVERSCAN);
    }

    #[test]
    fn visible_range_handles_few_lines_and_no_lines() {
        assert_eq!(visible_range(0, 300.0, 0.0), 0..0);
        assert_eq!(visible_range(3, 300.0, 0.0), 0..3);
        // An offset past the end (stale) still gives a valid range.
        let r = visible_range(10, 100.0, 99_999.0);
        assert!(r.start <= r.end && r.end <= 10);
        // An unknown viewport falls back to a sensible height instead of drawing nothing.
        let r = visible_range(10_000, 0.0, 0.0);
        assert!(r.len() > 10 && r.len() < 10_000);
    }

    #[test]
    fn offset_for_line_centers_it_within_the_range() {
        let h = LOG_LINE_HEIGHT;
        let n = 1000;
        let vh = 10.0 * h;
        let y = offset_for_line(500, n, vh);
        // The line is inside the part that is visible with this offset.
        let top = n as f32 * h - vh - y;
        assert!(top <= 500.0 * h && 501.0 * h <= top + vh);
        // The ends clamp to the scroll range.
        assert_eq!(offset_for_line(n - 1, n, vh), 0.0);
        assert_eq!(offset_for_line(0, n, vh), n as f32 * h - vh);
        // Everything fits: nothing to scroll.
        assert_eq!(offset_for_line(2, 5, 1000.0), 0.0);
    }

    #[test]
    fn highlight_finds_every_occurrence_ignoring_case() {
        assert_eq!(
            highlight_ranges("Error: error ERROR", "error"),
            [0..5, 7..12, 13..18]
        );
        assert_eq!(
            highlight_ranges("abc", "x"),
            Vec::<std::ops::Range<usize>>::new()
        );
        assert_eq!(
            highlight_ranges("abc", ""),
            Vec::<std::ops::Range<usize>>::new()
        );
        // Ranges are in characters, so multi-byte text is safe.
        assert_eq!(highlight_ranges("ação ÇÃO", "ção"), [1..4, 5..8]);
        // Overlaps do not repeat.
        assert_eq!(highlight_ranges("aaaa", "aa"), [0..2, 2..4]);
    }

    #[test]
    fn at_bottom_has_a_small_tolerance() {
        assert!(is_at_bottom(0.0));
        assert!(is_at_bottom(0.5));
        assert!(!is_at_bottom(LOG_LINE_HEIGHT));
    }

    #[tokio::test(start_paused = true)]
    async fn batches_group_lines_by_window() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let source = tokio_stream_from(rx);
        let mut s = Box::pin(batched(source, Duration::from_millis(100), 200));
        tx.send(line("a")).unwrap();
        tx.send(line("b")).unwrap();
        let first = s.next().await.unwrap();
        assert_eq!(first.len(), 2, "both arrived inside the window");
        tx.send(line("c")).unwrap();
        drop(tx);
        let second = s.next().await.unwrap();
        assert_eq!(second.len(), 1);
        assert!(s.next().await.is_none(), "ends with the source");
    }

    #[tokio::test(start_paused = true)]
    async fn batches_are_capped_by_size() {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        for i in 0..450 {
            tx.send(line(&i.to_string())).unwrap();
        }
        drop(tx);
        let sizes: Vec<usize> = batched(tokio_stream_from(rx), Duration::from_millis(100), 200)
            .map(|b| b.len())
            .collect()
            .await;
        assert_eq!(sizes, [200, 200, 50]);
    }

    fn tokio_stream_from(
        mut rx: tokio::sync::mpsc::UnboundedReceiver<LogLine>,
    ) -> impl futures::Stream<Item = LogLine> {
        futures::stream::poll_fn(move |cx| rx.poll_recv(cx))
    }

    #[tokio::test(start_paused = true)]
    async fn log_messages_replay_history_then_end_with_the_stream() {
        let fake = Arc::new(FakeClient::with_sample_data());
        fake.push_log("web-id", line("old"));
        let client: Arc<dyn DockerClient> = fake.clone();
        let mut s = Box::pin(log_messages(client, "web-id".into(), 3));
        fake.push_log("web-id", line("live"));
        let Some(Message::Detail(crate::views::detail::Msg::LogBatch {
            id,
            generation,
            lines,
        })) = s.next().await
        else {
            panic!("expected a batch");
        };
        assert_eq!((id.as_str(), generation), ("web-id", 3));
        let texts: Vec<_> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["old", "live"]);
        fake.close_streams("web-id");
        assert!(matches!(
            s.next().await,
            Some(Message::Detail(crate::views::detail::Msg::LogsEnded {
                generation: 3,
                ..
            }))
        ));
        assert!(s.next().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn log_messages_for_a_missing_container_just_end() {
        let fake = Arc::new(FakeClient::new());
        let client: Arc<dyn DockerClient> = fake;
        let msgs: Vec<_> = log_messages(client, "nope".into(), 0).collect().await;
        assert_eq!(msgs.len(), 1);
        assert!(matches!(
            msgs[0],
            Message::Detail(crate::views::detail::Msg::LogsEnded { .. })
        ));
    }

    #[test]
    fn view_builds_for_every_state() {
        use crate::views::detail::DetailState;
        let mut d = DetailState::new("web-id".into());
        let _: iced::Element<'_, Message> = view(&d);
        for i in 0..50 {
            d.logs.push(LogLine {
                stream: if i % 3 == 0 {
                    LogStream::Stderr
                } else {
                    LogStream::Stdout
                },
                timestamp: Some("2026-10-29T12:00:00Z".into()),
                text: format!("line {i} error"),
            });
        }
        d.rebuild_editor();
        let _: iced::Element<'_, Message> = view(&d);
        d.timestamps = true;
        d.set_query("error".into());
        assert!(d.window.current.is_some(), "the band behind the match");
        let _: iced::Element<'_, Message> = view(&d);
        d.follow = false;
        d.from_bottom = 100.0;
        d.viewport_h = 200.0;
        d.set_query("nothing".into());
        assert!(d.window.current.is_none());
        let _: iced::Element<'_, Message> = view(&d);
        // A selection holds the window while the buffer grows past it.
        d.window.frozen = true;
        d.logs.push(line("one more"));
        let _: iced::Element<'_, Message> = view(&d);
    }

    #[test]
    fn the_editor_has_one_line_per_log_line() {
        use crate::views::detail::DetailState;
        let mut d = DetailState::new("web-id".into());
        for text in ["one", "", "tw\ro\u{2028}x", "three"] {
            d.logs.push(line(text));
        }
        d.rebuild_editor();
        assert_eq!(d.editor.line_count(), 4);
        assert_eq!(d.window.range, 0..4);
        assert_eq!(d.window.lines.len(), 4);
    }

    #[test]
    fn the_window_is_only_the_visible_lines() {
        use crate::views::detail::DetailState;
        let mut d = DetailState::new("web-id".into());
        for i in 0..1000 {
            d.logs.push(line(&format!("line {i}")));
        }
        d.viewport_h = 10.0 * LOG_LINE_HEIGHT;
        d.rebuild_editor();
        assert_eq!(d.window.range, visible_range(1000, d.viewport_h, 0.0));
        assert_eq!(d.editor.line_count(), d.window.range.len());
        assert!(d.editor.line_count() < 30);
        assert_eq!(d.window.count, 1000);
        let last = d.editor.line(d.editor.line_count() - 1).unwrap();
        assert_eq!(last.text, "line 999");
    }

    #[test]
    fn texts_are_translated() {
        for key in [
            "logs.search",
            "logs.no_matches",
            "logs.timestamps",
            "logs.copy",
            "logs.prev_match",
            "logs.next_match",
            "logs.jump_end",
            "logs.empty",
        ] {
            for locale in ["en", "pt-BR"] {
                assert_ne!(
                    rust_i18n::t!(key, locale = locale),
                    key,
                    "missing {locale} translation for {key}"
                );
            }
        }
        for locale in ["en", "pt-BR"] {
            let text = rust_i18n::t!("logs.match_of", locale = locale, n = 2, total = 7);
            assert!(text.contains('2') && text.contains('7'), "{locale}: {text}");
        }
    }
}
