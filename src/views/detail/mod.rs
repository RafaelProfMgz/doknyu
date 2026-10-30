//! State of the container shown in the dock: the live logs and the stats history.
//!
//! The dock only says which container and which tab; this is what the tabs keep while it is
//! open. `App` creates it when the dock opens (or switches container) and drops it when it
//! closes, so nothing outlives the container on screen.

pub mod log_highlight;
pub mod logs;
pub mod stats;
pub mod terminal;

use std::ops::Range;

use iced::Task;
use iced::widget::Id;
use iced::widget::operation::{AbsoluteOffset, scroll_to};
use iced::widget::text_editor::{self, Action};

use self::log_highlight::{LineHl, editor_line};
use crate::ui::tokens::LOG_LINE_HEIGHT;

use crate::app::Message;
use crate::logbuf::LogBuffer;
use crate::model::{LogLine, RawStats};
use crate::stats::{StatsHistory, sample_from_raw};

#[derive(Debug, Clone)]
pub enum Msg {
    /// Lines of the log stream of `(id, generation)`. A batch of an older subscription is
    /// dropped, so a restart cannot bring back lines of the previous run.
    LogBatch {
        id: String,
        generation: u64,
        lines: Vec<LogLine>,
    },
    /// The stream of `(id, generation)` ended (the container stopped, or it is gone).
    LogsEnded {
        id: String,
        generation: u64,
    },
    /// The log list was scrolled; the offset is the distance to the end of the logs.
    Scrolled {
        from_bottom: f32,
        viewport_h: f32,
    },
    /// The area of the log list changed its height.
    Resized(f32),
    /// "Jump to end": follow the new lines again.
    JumpToEnd,
    ToggleTimestamps,
    Query(String),
    NextMatch,
    PrevMatch,
    /// Copies the logs to the clipboard.
    Copy,
    /// Something happened in the log editor (a click, a drag, a key). The editor is read-only,
    /// so only what does not edit is applied.
    Editor(Action),
    /// A reading of the stats stream of `(id, generation)`; dropped if that is not the one
    /// being followed.
    StatsSample {
        id: String,
        generation: u64,
        raw: RawStats,
    },
    /// The stats stream of `(id, generation)` ended (the container stopped, or it is gone).
    StatsEnded {
        id: String,
        generation: u64,
    },
}

/// The lines of the buffer the log editor holds, and the list layout around them.
#[derive(Debug, Clone)]
pub struct LogWindow {
    /// Which lines of the buffer are in the editor (the visible ones, plus the overscan).
    pub range: Range<usize>,
    /// How many lines the buffer had when the editor was built: the spacers around the editor
    /// stand for the lines left out of that many.
    pub count: usize,
    /// A selection holds the window: new lines enter the buffer but the editor is not rebuilt,
    /// since that would drop the selection.
    pub frozen: bool,
    /// How to color each line of the editor.
    pub lines: Vec<LineHl>,
    /// The line of the editor that holds the search match the user is on.
    pub current: Option<usize>,
    /// Counts the times the editor was built (see `LogHlSettings`).
    pub version: u64,
    /// `DetailState::pushed` when the editor was built: with a full buffer the count stays the
    /// same while the lines move, so this tells whether the editor is out of date.
    pub pushed: u64,
}

#[derive(Debug, Clone)]
pub struct DetailState {
    pub id: String,
    /// Bumped each time the container runs again; part of the identity of the log and stats
    /// subscriptions, so a new run gets new streams.
    pub generation: u64,
    pub logs: LogBuffer,
    /// The stream of the current generation ended.
    pub logs_ended: bool,
    /// Auto-scroll: new lines stay in view. Paused by scrolling up.
    pub follow: bool,
    /// Distance in pixels from the end of the logs to the bottom of the view (0 = at the end).
    pub from_bottom: f32,
    /// Height of the log list, 0 until it is first drawn.
    pub viewport_h: f32,
    pub timestamps: bool,
    pub query: String,
    /// Indices of the lines that contain the query.
    pub matches: Vec<usize>,
    /// The match the user is on, an index into `matches`.
    pub cursor: usize,
    /// The latest stats samples (a window of 60, one per second).
    pub stats: StatsHistory,
    /// The stats stream of the current generation ended.
    pub stats_ended: bool,
    /// CPUs the daemon reported in the last sample; the CPU chart goes up to 100% of each.
    pub cpus: u32,
    /// The visible lines as read-only text, so they can be selected and copied like in a
    /// terminal. Built again by [`Self::rebuild_editor`]; `window` says what is in it.
    pub editor: text_editor::Content,
    pub window: LogWindow,
    /// Lines received since the detail opened; only grows (see `LogWindow::pushed`).
    pub pushed: u64,
}

impl DetailState {
    pub fn new(id: String) -> Self {
        Self {
            id,
            generation: 0,
            logs: LogBuffer::new(),
            logs_ended: false,
            follow: true,
            from_bottom: 0.0,
            viewport_h: 0.0,
            timestamps: false,
            query: String::new(),
            matches: Vec::new(),
            cursor: 0,
            stats: StatsHistory::new(),
            stats_ended: false,
            cpus: 1,
            editor: text_editor::Content::new(),
            window: LogWindow {
                range: 0..0,
                count: 0,
                frozen: false,
                lines: Vec::new(),
                current: None,
                version: 0,
                pushed: 0,
            },
            pushed: 0,
        }
    }

    /// The text in the log editor.
    pub fn editor_text(&self) -> String {
        self.editor.text()
    }

    /// Whether some text of the log editor is selected.
    pub fn has_selection(&self) -> bool {
        self.editor.selection().is_some()
    }

    /// Drops the selection and lets the editor follow the logs again; at the end of the logs
    /// that also means following the new lines.
    pub fn clear_selection(&mut self) {
        self.follow = logs::is_at_bottom(self.from_bottom);
        self.rebuild_editor();
    }

    /// Builds the editor again with the lines in view: the text, how to color it and where the
    /// current search match is. Also ends a freeze, which drops any selection.
    pub fn rebuild_editor(&mut self) {
        let count = self.logs.len();
        let from_bottom = if self.follow { 0.0 } else { self.from_bottom };
        let range = logs::visible_range(count, self.viewport_h, from_bottom);
        let current = self.matches.get(self.cursor).copied();
        let mut text = String::new();
        let mut lines = Vec::with_capacity(range.len());
        let mut current_in_window = None;
        for (i, line) in self
            .logs
            .iter()
            .enumerate()
            .skip(range.start)
            .take(range.len())
        {
            if !lines.is_empty() {
                text.push('\n');
            }
            text.push_str(&editor_line(line, self.timestamps));
            lines.push(LineHl::of(line, self.timestamps, &self.query));
            if current == Some(i) {
                current_in_window = Some(lines.len() - 1);
            }
        }
        self.editor = text_editor::Content::with_text(&text);
        self.window = LogWindow {
            range,
            count,
            frozen: false,
            lines,
            current: current_in_window,
            version: self.window.version + 1,
            pushed: self.pushed,
        };
    }

    /// Keeps the editor up to date with the logs: it is built again unless a selection holds
    /// it, and even then when the view moved to lines it does not have.
    /// Building it is costly (the text is shaped again), so it is skipped when it would show the
    /// same lines it already has.
    fn sync_editor(&mut self) {
        let from_bottom = if self.follow { 0.0 } else { self.from_bottom };
        if self.window.frozen {
            let seen = logs::visible_window(self.window.count, self.viewport_h, from_bottom, 0);
            if seen.start >= self.window.range.start && seen.end <= self.window.range.end {
                return;
            }
        } else {
            let count = self.logs.len();
            let range = logs::visible_range(count, self.viewport_h, from_bottom);
            if self.window.pushed == self.pushed
                && self.window.count == count
                && self.window.range == range
            {
                return;
            }
        }
        self.rebuild_editor();
    }

    /// The container runs again: a new generation (a new log stream, which brings the history
    /// back), so the buffer starts empty to avoid duplicated lines. The search text stays.
    pub fn resubscribe(&mut self) {
        self.generation += 1;
        self.logs = LogBuffer::new();
        self.logs_ended = false;
        self.stats = StatsHistory::new();
        self.stats_ended = false;
        self.matches.clear();
        self.cursor = 0;
        self.follow = true;
        self.from_bottom = 0.0;
        self.rebuild_editor();
    }

    /// Puts the list back at its end, as a freshly drawn one is (see `logs::view`).
    /// Also drops the selection and the freeze that comes with it, as the editor is built again.
    pub fn reset_scroll(&mut self) {
        self.follow = true;
        self.from_bottom = 0.0;
        self.rebuild_editor();
    }

    /// Sets the search text and finds its lines; the cursor goes to the newest one.
    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.matches = self.logs.search(&self.query);
        self.cursor = self.matches.len().saturating_sub(1);
        self.rebuild_editor();
    }

    /// "n of m": the position of the current match and how many there are; `None` without any.
    pub fn match_position(&self) -> Option<(usize, usize)> {
        (!self.matches.is_empty()).then(|| (self.cursor + 1, self.matches.len()))
    }

    pub fn update(&mut self, m: Msg) -> Task<Message> {
        match m {
            Msg::LogBatch {
                id,
                generation,
                lines,
            } => {
                if id != self.id || generation != self.generation {
                    return Task::none();
                }
                let added = lines.len();
                self.push_lines(lines);
                // The list is anchored to its end: new lines would slide the text up under a
                // user who scrolled away. Moving the offset by what was added keeps the same
                // lines on screen (a selection already holds its own layout).
                if !self.follow && !self.window.frozen && added > 0 {
                    let max = logs::max_from_bottom(self.logs.len(), self.viewport_h);
                    self.from_bottom = (self.from_bottom + added as f32 * LOG_LINE_HEIGHT).min(max);
                    self.sync_editor();
                    return scroll_list_to(self.from_bottom);
                }
                self.sync_editor();
                Task::none()
            }
            Msg::LogsEnded { id, generation } => {
                if id == self.id && generation == self.generation {
                    self.logs_ended = true;
                }
                Task::none()
            }
            Msg::Scrolled {
                from_bottom,
                viewport_h,
            } => {
                self.follow = logs::is_at_bottom(from_bottom);
                self.from_bottom = if self.follow { 0.0 } else { from_bottom };
                if viewport_h > 0.0 {
                    self.viewport_h = viewport_h;
                }
                // Back at the end: follow the lines again, even over a selection.
                if self.follow && self.window.frozen {
                    self.rebuild_editor();
                } else {
                    self.sync_editor();
                }
                Task::none()
            }
            Msg::Resized(height) => {
                self.viewport_h = height;
                self.sync_editor();
                Task::none()
            }
            Msg::JumpToEnd => {
                self.reset_scroll();
                scroll_list_to(0.0)
            }
            Msg::ToggleTimestamps => {
                self.timestamps = !self.timestamps;
                self.rebuild_editor();
                Task::none()
            }
            Msg::Query(query) => {
                self.set_query(query);
                self.show_current_match()
            }
            Msg::NextMatch => self.step_match(true),
            Msg::PrevMatch => self.step_match(false),
            Msg::Copy => iced::clipboard::write(self.logs.to_text(self.timestamps)),
            Msg::Editor(action) => {
                self.edit(action);
                Task::none()
            }
            Msg::StatsSample {
                id,
                generation,
                raw,
            } => {
                if id == self.id && generation == self.generation {
                    self.cpus = raw.online_cpus.max(1);
                    self.stats.push(sample_from_raw(&raw));
                }
                Task::none()
            }
            Msg::StatsEnded { id, generation } => {
                if id == self.id && generation == self.generation {
                    self.stats_ended = true;
                }
                Task::none()
            }
        }
    }

    /// Applies an action of the log editor. Edits are dropped (the text is only to be read), and
    /// so is its own scrolling (the list scrolls). Whatever starts a selection pauses the
    /// follow and freezes the window, so the selection stays under the user's mouse.
    fn edit(&mut self, action: Action) {
        if action.is_edit() || matches!(action, Action::Scroll { .. }) {
            return;
        }
        let selects = matches!(
            action,
            Action::Click(_)
                | Action::Drag(_)
                | Action::Select(_)
                | Action::SelectWord
                | Action::SelectLine
                | Action::SelectAll
        );
        self.editor.perform(action);
        if selects {
            self.follow = false;
            self.window.frozen = true;
        }
    }

    /// Appends lines and keeps `matches` in step without searching the whole buffer again:
    /// the buffer drops its oldest lines when full, which moves every index.
    fn push_lines(&mut self, lines: Vec<LogLine>) {
        let before = self.logs.len();
        let added = lines.len();
        self.pushed += added as u64;
        for line in lines {
            self.logs.push(line);
        }
        if self.query.is_empty() {
            return;
        }
        let dropped = (before + added).saturating_sub(self.logs.len());
        if dropped > 0 {
            let gone = self.matches.partition_point(|&i| i < dropped);
            self.matches.drain(..gone);
            for i in &mut self.matches {
                *i -= dropped;
            }
            self.cursor = self.cursor.saturating_sub(gone);
        }
        let was_empty = self.matches.is_empty();
        let needle = self.query.to_lowercase();
        let first_new = self.logs.len() - added.min(self.logs.len());
        for (i, line) in self.logs.iter().enumerate().skip(first_new) {
            if line.text.to_lowercase().contains(&needle) {
                self.matches.push(i);
            }
        }
        if was_empty {
            self.cursor = self.matches.len().saturating_sub(1);
        }
        self.cursor = self.cursor.min(self.matches.len().saturating_sub(1));
    }

    /// Moves to the next (or previous) match, wrapping around at the ends.
    fn step_match(&mut self, forward: bool) -> Task<Message> {
        let count = self.matches.len();
        if count == 0 {
            return Task::none();
        }
        self.cursor = if forward {
            (self.cursor + 1) % count
        } else {
            (self.cursor + count - 1) % count
        };
        self.show_current_match()
    }

    /// Scrolls so that the current match is in view; this pauses the follow unless it is
    /// already at the end.
    fn show_current_match(&mut self) -> Task<Message> {
        let Some(&line) = self.matches.get(self.cursor) else {
            return Task::none();
        };
        let y = logs::offset_for_line(line, self.logs.len(), self.viewport_h);
        self.from_bottom = y;
        self.follow = logs::is_at_bottom(y);
        self.rebuild_editor();
        scroll_list_to(y)
    }
}

/// Scrolls the log list to `from_bottom` pixels above its end.
pub fn scroll_list_to(from_bottom: f32) -> Task<Message> {
    scroll_to(
        Id::new(logs::LIST_ID),
        AbsoluteOffset {
            x: None,
            y: Some(from_bottom),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LogLine, LogStream, RawStats};
    use crate::ui::tokens::LOG_LINE_HEIGHT;

    fn line(text: &str) -> LogLine {
        LogLine {
            stream: LogStream::Stdout,
            timestamp: None,
            text: text.into(),
        }
    }

    fn batch(id: &str, generation: u64, texts: &[&str]) -> Msg {
        Msg::LogBatch {
            id: id.into(),
            generation,
            lines: texts.iter().map(|t| line(t)).collect(),
        }
    }

    #[test]
    fn log_batch_appends() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(batch("web-id", 0, &["a", "b"]));
        let _ = d.update(batch("web-id", 0, &["c"]));
        let texts: Vec<_> = d.logs.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["a", "b", "c"]);
    }

    #[test]
    fn stale_batches_are_dropped() {
        let mut d = DetailState::new("web-id".into());
        d.resubscribe();
        assert_eq!(d.generation, 1);
        let _ = d.update(batch("web-id", 0, &["old"]));
        let _ = d.update(batch("other-id", 1, &["foreign"]));
        assert!(d.logs.is_empty());
        let _ = d.update(batch("web-id", 1, &["new"]));
        assert_eq!(d.logs.len(), 1);
    }

    #[test]
    fn scroll_up_disables_follow() {
        let mut d = DetailState::new("web-id".into());
        assert!(d.follow);
        let _ = d.update(Msg::Scrolled {
            from_bottom: 120.0,
            viewport_h: 300.0,
        });
        assert!(!d.follow);
        let _ = d.update(Msg::Scrolled {
            from_bottom: 0.0,
            viewport_h: 300.0,
        });
        assert!(d.follow, "reaching the end follows again");
    }

    #[test]
    fn jump_to_end_follows_again() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(Msg::Scrolled {
            from_bottom: 50.0,
            viewport_h: 300.0,
        });
        assert!(!d.follow);
        let _ = d.update(Msg::JumpToEnd);
        assert!(d.follow);
        assert_eq!(d.from_bottom, 0.0);
    }

    #[test]
    fn resubscribe_clears_buffer_and_follows() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(batch("web-id", 0, &["a"]));
        let _ = d.update(Msg::Query("a".into()));
        let _ = d.update(Msg::LogsEnded {
            id: "web-id".into(),
            generation: 0,
        });
        assert!(d.logs_ended);
        let _ = d.update(Msg::Scrolled {
            from_bottom: 80.0,
            viewport_h: 300.0,
        });
        d.resubscribe();
        assert_eq!(d.generation, 1);
        assert!(d.logs.is_empty());
        assert!(d.matches.is_empty());
        assert!(d.follow && !d.logs_ended);
        assert_eq!(d.query, "a", "the search text stays");
    }

    #[test]
    fn stale_end_is_ignored() {
        let mut d = DetailState::new("web-id".into());
        d.resubscribe();
        let _ = d.update(Msg::LogsEnded {
            id: "web-id".into(),
            generation: 0,
        });
        assert!(!d.logs_ended);
    }

    #[test]
    fn toggle_timestamps_flips() {
        let mut d = DetailState::new("web-id".into());
        assert!(!d.timestamps);
        let _ = d.update(Msg::ToggleTimestamps);
        assert!(d.timestamps);
        let _ = d.update(Msg::ToggleTimestamps);
        assert!(!d.timestamps);
    }

    #[test]
    fn query_finds_matching_lines_and_picks_the_latest() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(batch("web-id", 0, &["GET /", "error: a", "ok", "Error: b"]));
        let _ = d.update(Msg::Query("error".into()));
        assert_eq!(d.matches, [1, 3]);
        assert_eq!(d.cursor, 1, "starts at the newest match");
        assert_eq!(d.match_position(), Some((2, 2)));
        let _ = d.update(Msg::Query("zzz".into()));
        assert_eq!(d.match_position(), None);
        let _ = d.update(Msg::Query(String::new()));
        assert!(d.matches.is_empty());
    }

    #[test]
    fn next_and_prev_match_wrap_around() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(Msg::Resized(40.0));
        let _ = d.update(batch("web-id", 0, &["x1", "y", "x2", "x3"]));
        let _ = d.update(Msg::Query("x".into()));
        assert_eq!(d.cursor, 2);
        let _ = d.update(Msg::NextMatch);
        assert_eq!(d.cursor, 0);
        let _ = d.update(Msg::PrevMatch);
        assert_eq!(d.cursor, 2);
        let _ = d.update(Msg::PrevMatch);
        assert_eq!(d.cursor, 1);
        assert!(!d.follow, "going to a match pauses the follow");
    }

    #[test]
    fn next_match_without_matches_does_nothing() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(Msg::NextMatch);
        let _ = d.update(Msg::PrevMatch);
        assert!(d.follow);
    }

    #[test]
    fn new_lines_extend_the_matches() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(batch("web-id", 0, &["a", "needle 1"]));
        let _ = d.update(Msg::Query("NEEDLE".into()));
        let _ = d.update(batch("web-id", 0, &["b", "needle 2"]));
        assert_eq!(d.matches, [1, 3]);
        assert_eq!(d.matches, d.logs.search("needle"));
    }

    #[test]
    fn matches_shift_when_the_buffer_drops_old_lines() {
        use crate::logbuf::MAX_LINES;
        let mut d = DetailState::new("web-id".into());
        let mut first: Vec<LogLine> = (0..MAX_LINES).map(|_| line("filler")).collect();
        first[0] = line("hit at the very start");
        first[5] = line("hit later");
        first[MAX_LINES - 1] = line("hit at the end");
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: first,
        });
        let _ = d.update(Msg::Query("hit".into()));
        assert_eq!(d.matches, [0, 5, MAX_LINES - 1]);
        let _ = d.update(Msg::NextMatch); // wraps to the first one
        assert_eq!(d.cursor, 0);

        let more: Vec<LogLine> = vec![line("filler"), line("filler"), line("hit new")];
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: more,
        });
        assert_eq!(d.matches, d.logs.search("hit"));
        assert_eq!(d.matches, [2, MAX_LINES - 4, MAX_LINES - 1]);
        assert!(d.cursor < d.matches.len());
    }

    use iced::Point;
    use iced::widget::text_editor::{Action, Edit};

    #[test]
    fn log_editor_ignores_edits() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(batch("web-id", 0, &["alpha", "beta"]));
        assert_eq!(d.editor_text(), "alpha\nbeta");
        for edit in [
            Edit::Insert('x'),
            Edit::Enter,
            Edit::Backspace,
            Edit::Delete,
            Edit::Paste(std::sync::Arc::new("pasted".into())),
        ] {
            let _ = d.update(Msg::Editor(Action::Edit(edit)));
        }
        assert_eq!(d.editor_text(), "alpha\nbeta", "typing changes nothing");
    }

    #[test]
    fn log_selection_pauses_follow() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(batch("web-id", 0, &["alpha", "beta"]));
        assert!(d.follow);
        let _ = d.update(Msg::Editor(Action::Drag(Point::new(10.0, 5.0))));
        assert!(!d.follow);
        // A click also starts a selection; so does every selecting action.
        for action in [
            Action::Click(Point::new(1.0, 1.0)),
            Action::SelectWord,
            Action::SelectLine,
            Action::SelectAll,
        ] {
            d.follow = true;
            let _ = d.update(Msg::Editor(action));
            assert!(!d.follow);
        }
    }

    #[test]
    fn selection_survives_new_batches() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(batch("web-id", 0, &["alpha", "beta"]));
        let _ = d.update(Msg::Editor(Action::SelectAll));
        assert!(d.has_selection());
        let before = d.editor_text();
        let _ = d.update(batch("web-id", 0, &["gamma", "delta"]));
        assert_eq!(d.logs.len(), 4, "the buffer keeps growing");
        assert_eq!(d.editor_text(), before, "the content does not move");
        assert!(d.has_selection());
        // Moving the window (here: the jump to the end) builds it again.
        let _ = d.update(Msg::JumpToEnd);
        assert_eq!(d.editor_text(), "alpha\nbeta\ngamma\ndelta");
        assert!(!d.has_selection());
        assert!(d.follow);
    }

    #[test]
    fn an_up_to_date_editor_is_not_built_again() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(Msg::Resized(400.0));
        let _ = d.update(batch("web-id", 0, &["a", "b"]));
        let built = d.window.version;
        // The list growing makes the scrollable report its viewport again, at the bottom.
        let _ = d.update(Msg::Scrolled {
            from_bottom: 0.0,
            viewport_h: 400.0,
        });
        let _ = d.update(Msg::Resized(400.0));
        assert_eq!(d.window.version, built);
        let _ = d.update(batch("web-id", 0, &["c"]));
        assert_eq!(d.window.version, built + 1);
    }

    #[test]
    fn a_full_buffer_still_shows_new_lines() {
        use crate::logbuf::MAX_LINES;
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(Msg::Resized(400.0));
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: numbered(0, MAX_LINES),
        });
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: numbered(MAX_LINES, MAX_LINES + 1),
        });
        assert!(d.editor_text().ends_with(&format!("line {MAX_LINES}")));
    }

    #[test]
    fn the_editor_follows_the_lines_without_a_selection() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(batch("web-id", 0, &["a"]));
        let _ = d.update(batch("web-id", 0, &["b"]));
        assert_eq!(d.editor_text(), "a\nb");
        d.timestamps = false;
        let _ = d.update(Msg::ToggleTimestamps);
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: vec![LogLine {
                stream: LogStream::Stdout,
                timestamp: Some("T".into()),
                text: "c".into(),
            }],
        });
        assert_eq!(d.editor_text(), "a\nb\nT c");
    }

    #[test]
    fn clearing_the_selection_unfreezes_the_window() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(batch("web-id", 0, &["a"]));
        let _ = d.update(Msg::Editor(Action::SelectAll));
        let _ = d.update(batch("web-id", 0, &["b"]));
        assert_eq!(d.editor_text(), "a");
        d.clear_selection();
        assert!(!d.has_selection());
        assert_eq!(d.editor_text(), "a\nb");
    }

    /// The text of the first line of the editor, which says which lines are on screen.
    fn first_shown(d: &DetailState) -> String {
        d.editor
            .line(0)
            .map(|l| l.text.into_owned())
            .unwrap_or_default()
    }

    fn numbered(from: usize, to: usize) -> Vec<LogLine> {
        (from..to).map(|i| line(&format!("line {i}"))).collect()
    }

    fn scrolled_up(d: &mut DetailState, lines_up: f32) {
        let _ = d.update(Msg::Scrolled {
            from_bottom: lines_up * LOG_LINE_HEIGHT,
            viewport_h: 10.0 * LOG_LINE_HEIGHT,
        });
    }

    #[test]
    fn a_batch_does_not_move_the_lines_of_a_list_scrolled_up() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(Msg::Resized(10.0 * LOG_LINE_HEIGHT));
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: numbered(0, 100),
        });
        scrolled_up(&mut d, 30.0);
        assert!(!d.follow);
        let (start, shown) = (d.window.range.start, first_shown(&d));
        let before = d.from_bottom;

        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: numbered(100, 105),
        });
        assert_eq!(first_shown(&d), shown, "the same lines stay on screen");
        assert_eq!(
            d.window.range.start, start,
            "the window stays on the same lines"
        );
        assert_eq!(d.from_bottom, before + 5.0 * LOG_LINE_HEIGHT);
        assert!(!d.follow);
    }

    #[test]
    fn a_batch_does_not_move_the_lines_when_the_buffer_drops_old_ones() {
        use crate::logbuf::MAX_LINES;
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(Msg::Resized(10.0 * LOG_LINE_HEIGHT));
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: numbered(0, MAX_LINES),
        });
        scrolled_up(&mut d, 30.0);
        let shown = first_shown(&d);
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: numbered(MAX_LINES, MAX_LINES + 3),
        });
        assert_eq!(d.logs.len(), MAX_LINES);
        assert_eq!(first_shown(&d), shown);
    }

    #[test]
    fn a_batch_keeps_the_match_in_view() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(Msg::Resized(10.0 * LOG_LINE_HEIGHT));
        let mut lines = numbered(0, 100);
        lines[40] = line("needle");
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines,
        });
        let _ = d.update(Msg::Query("needle".into()));
        assert!(!d.follow);
        let at = d.window.current;
        assert!(at.is_some());
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: numbered(100, 120),
        });
        assert_eq!(d.window.current, at, "the match stays where it was");
    }

    #[test]
    fn a_followed_list_is_not_adjusted() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(Msg::LogBatch {
            id: "web-id".into(),
            generation: 0,
            lines: numbered(0, 50),
        });
        assert!(d.follow);
        assert_eq!(d.from_bottom, 0.0);
    }

    #[test]
    fn reset_scroll_drops_the_selection_and_the_freeze() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(batch("web-id", 0, &["a"]));
        let _ = d.update(Msg::Editor(Action::SelectAll));
        assert!(d.window.frozen && !d.follow);
        d.reset_scroll();
        assert!(d.follow && !d.window.frozen && !d.has_selection());
        let _ = d.update(batch("web-id", 0, &["b"]));
        assert_eq!(d.editor_text(), "a\nb");
    }

    #[test]
    fn resize_is_remembered() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(Msg::Resized(240.0));
        assert_eq!(d.viewport_h, 240.0);
    }

    fn raw(mem: u64) -> RawStats {
        RawStats {
            cpu_total: 0,
            precpu_total: 0,
            system: 0,
            presystem: 0,
            online_cpus: 4,
            mem_usage: mem,
            mem_inactive_file: 0,
            mem_limit: 1000,
            net_rx: 0,
            net_tx: 0,
            blk_read: 0,
            blk_write: 0,
        }
    }

    fn sample(id: &str, generation: u64, mem: u64) -> Msg {
        Msg::StatsSample {
            id: id.into(),
            generation,
            raw: raw(mem),
        }
    }

    #[test]
    fn stats_sample_pushed_to_history() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(sample("web-id", 0, 10));
        let _ = d.update(sample("web-id", 0, 20));
        assert_eq!(d.stats.mem_series(), [10.0, 20.0]);
        assert_eq!(d.stats.latest().map(|s| s.mem_limit), Some(1000));
        assert_eq!(d.cpus, 4, "the chart of the CPU scales with the CPU count");
    }

    #[test]
    fn stale_stats_are_dropped() {
        let mut d = DetailState::new("web-id".into());
        d.resubscribe();
        let _ = d.update(sample("web-id", 0, 1));
        let _ = d.update(sample("other-id", 1, 2));
        assert!(d.stats.latest().is_none());
        let _ = d.update(sample("web-id", 1, 3));
        assert!(d.stats.latest().is_some());
    }

    #[test]
    fn stats_end_is_per_generation_and_resubscribing_starts_the_history_over() {
        let mut d = DetailState::new("web-id".into());
        let _ = d.update(sample("web-id", 0, 1));
        let _ = d.update(Msg::StatsEnded {
            id: "web-id".into(),
            generation: 0,
        });
        assert!(d.stats_ended);
        d.resubscribe();
        assert!(d.stats.latest().is_none() && !d.stats_ended);
        let _ = d.update(Msg::StatsEnded {
            id: "web-id".into(),
            generation: 0,
        });
        assert!(!d.stats_ended, "the end of an older stream is ignored");
    }
}
