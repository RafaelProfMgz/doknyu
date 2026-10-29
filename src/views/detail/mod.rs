//! State of the container shown in the dock: the live logs today, the stats in phase 3.3.
//!
//! The dock only says which container and which tab; this is what the tabs keep while it is
//! open. `App` creates it when the dock opens (or switches container) and drops it when it
//! closes, so nothing outlives the container on screen.

pub mod logs;

use iced::Task;
use iced::widget::Id;
use iced::widget::operation::{AbsoluteOffset, scroll_to};

use crate::app::Message;
use crate::logbuf::LogBuffer;
use crate::model::LogLine;

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
}

#[derive(Debug, Clone)]
pub struct DetailState {
    pub id: String,
    /// Bumped each time the container runs again; part of the identity of the log
    /// subscription, so a new run gets a new stream.
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
        }
    }

    /// The container runs again: a new generation (a new log stream, which brings the history
    /// back), so the buffer starts empty to avoid duplicated lines. The search text stays.
    pub fn resubscribe(&mut self) {
        self.generation += 1;
        self.logs = LogBuffer::new();
        self.logs_ended = false;
        self.matches.clear();
        self.cursor = 0;
        self.follow = true;
        self.from_bottom = 0.0;
    }

    /// Puts the list back at its end, as a freshly drawn one is (see `logs::view`).
    pub fn reset_scroll(&mut self) {
        self.follow = true;
        self.from_bottom = 0.0;
    }

    /// Sets the search text and finds its lines; the cursor goes to the newest one.
    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.matches = self.logs.search(&self.query);
        self.cursor = self.matches.len().saturating_sub(1);
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
                if id == self.id && generation == self.generation {
                    self.push_lines(lines);
                }
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
                Task::none()
            }
            Msg::Resized(height) => {
                self.viewport_h = height;
                Task::none()
            }
            Msg::JumpToEnd => {
                self.reset_scroll();
                scroll_list_to(0.0)
            }
            Msg::ToggleTimestamps => {
                self.timestamps = !self.timestamps;
                Task::none()
            }
            Msg::Query(query) => {
                self.set_query(query);
                self.show_current_match()
            }
            Msg::NextMatch => self.step_match(true),
            Msg::PrevMatch => self.step_match(false),
            Msg::Copy => iced::clipboard::write(self.logs.to_text(self.timestamps)),
        }
    }

    /// Appends lines and keeps `matches` in step without searching the whole buffer again:
    /// the buffer drops its oldest lines when full, which moves every index.
    fn push_lines(&mut self, lines: Vec<LogLine>) {
        let before = self.logs.len();
        let added = lines.len();
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
