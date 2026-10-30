//! Colors of the log editor.
//!
//! The editor (a read-only `text_editor`) shows the visible lines as plain text; what makes a
//! line `stderr`, a timestamp or a search hit stand out is a [`Highlighter`], which iced runs
//! over the lines in order and which hands back byte ranges with a color name. The names are
//! turned into colors of the active palette by [`log_format`], so nothing here is a literal.
//!
//! The editor tells the highlighter which line it is on, but not what the line is: the
//! per-line facts (`stderr`, the length of the timestamp, where the search hits are) come in
//! the settings, built together with the text, so both always describe the same lines.

use std::ops::Range;
use std::sync::Arc;

use iced::advanced::text::highlighter::{Format, Highlighter};
use iced::{Font, Theme};

use super::logs::highlight_ranges;
use crate::model::{LogLine, LogStream};
use crate::ui::theme::palette;

/// What a stretch of text is, as far as its color goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hl {
    /// A `stderr` line.
    Warning,
    /// An occurrence of the search.
    Accent,
    /// The timestamp in front of a line.
    Faint,
}

/// The color of each kind of highlight, from the palette of the theme.
pub fn log_format(hl: &Hl, theme: &Theme) -> Format<Font> {
    let p = palette(theme);
    Format {
        color: Some(match hl {
            Hl::Warning => p.warning,
            Hl::Accent => p.accent,
            Hl::Faint => p.faint,
        }),
        font: None,
    }
}

/// The characters that would split a line in two inside the editor.
const LINE_BREAKS: [char; 7] = [
    '\r', '\n', '\u{b}', '\u{c}', '\u{85}', '\u{2028}', '\u{2029}',
];

fn single_line(text: &str) -> String {
    text.replace(LINE_BREAKS, " ")
}

/// The text of a log line as the editor shows it: one line, with its timestamp in front when
/// they are on. Every log line is exactly one line of the editor, so line `i` of the editor is
/// line `start + i` of the buffer.
pub fn editor_line(line: &LogLine, show_timestamp: bool) -> String {
    match (show_timestamp, &line.timestamp) {
        (true, Some(ts)) => format!("{} {}", single_line(ts), single_line(&line.text)),
        _ => single_line(&line.text),
    }
}

/// What the highlighter needs to know about one line of the editor.
#[derive(Debug, Clone, PartialEq)]
pub struct LineHl {
    pub stderr: bool,
    /// Bytes of the timestamp and the space after it; 0 when the line has none.
    pub prefix: usize,
    /// Bytes of the whole line.
    pub len: usize,
    /// Where the search occurs, in bytes of the whole line.
    pub hits: Vec<Range<usize>>,
}

impl LineHl {
    /// The facts of `line` as [`editor_line`] shows it.
    pub fn of(line: &LogLine, show_timestamp: bool, query: &str) -> Self {
        let text = single_line(&line.text);
        let prefix = match (show_timestamp, &line.timestamp) {
            (true, Some(ts)) => single_line(ts).len() + 1,
            _ => 0,
        };
        // The search finds characters; the editor wants bytes.
        let offsets: Vec<usize> = text
            .char_indices()
            .map(|(i, _)| i)
            .chain(std::iter::once(text.len()))
            .collect();
        let hits = highlight_ranges(&text, query)
            .into_iter()
            .map(|r| prefix + offsets[r.start]..prefix + offsets[r.end])
            .collect();
        Self {
            stderr: line.stream == LogStream::Stderr,
            prefix,
            len: prefix + text.len(),
            hits,
        }
    }

    /// The highlights of the line; later ones paint over earlier ones.
    fn spans(&self) -> Vec<(Range<usize>, Hl)> {
        let mut spans = Vec::new();
        if self.stderr && self.len > 0 {
            spans.push((0..self.len, Hl::Warning));
        }
        if self.prefix > 0 {
            spans.push((0..self.prefix, Hl::Faint));
        }
        spans.extend(self.hits.iter().cloned().map(|r| (r, Hl::Accent)));
        spans
    }
}

/// The lines in the editor. `version` changes whenever the editor is built again: a fresh
/// editor has no colors yet, even if its lines would be colored as the old ones were.
#[derive(Debug, Clone, PartialEq)]
pub struct LogHlSettings {
    version: u64,
    lines: Arc<[LineHl]>,
}

impl LogHlSettings {
    pub fn new(version: u64, lines: Vec<LineHl>) -> Self {
        Self {
            version,
            lines: lines.into(),
        }
    }
}

pub struct LogHighlighter {
    settings: LogHlSettings,
    current: usize,
}

impl Highlighter for LogHighlighter {
    type Settings = LogHlSettings;
    type Highlight = Hl;
    type Iterator<'a> = std::vec::IntoIter<(Range<usize>, Hl)>;

    fn new(settings: &Self::Settings) -> Self {
        Self {
            settings: settings.clone(),
            current: 0,
        }
    }

    fn update(&mut self, new_settings: &Self::Settings) {
        self.settings = new_settings.clone();
        self.current = 0;
    }

    fn change_line(&mut self, line: usize) {
        self.current = self.current.min(line);
    }

    fn highlight_line(&mut self, _line: &str) -> Self::Iterator<'_> {
        let spans = self
            .settings
            .lines
            .get(self.current)
            .map(LineHl::spans)
            .unwrap_or_default();
        self.current += 1;
        spans.into_iter()
    }

    fn current_line(&self) -> usize {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{LogLine, LogStream};

    fn line(stream: LogStream, ts: Option<&str>, text: &str) -> LogLine {
        LogLine {
            stream,
            timestamp: ts.map(Into::into),
            text: text.into(),
        }
    }

    #[test]
    fn highlight_marks_stderr_and_matches() {
        let lines = [
            line(LogStream::Stdout, None, "all fine"),
            line(LogStream::Stderr, None, "bad thing"),
            line(LogStream::Stdout, Some("T1"), "an Error here, error"),
            line(LogStream::Stderr, None, "ação ERROR"),
        ];
        let rows: Vec<LineHl> = lines.iter().map(|l| LineHl::of(l, true, "error")).collect();
        let settings = LogHlSettings::new(1, rows);
        let mut h = LogHighlighter::new(&settings);
        assert_eq!(h.current_line(), 0);

        assert!(h.highlight_line("all fine").next().is_none(), "plain text");
        let stderr: Vec<_> = h.highlight_line("bad thing").collect();
        assert_eq!(stderr, [(0..9, Hl::Warning)]);
        // The timestamp is faint and every occurrence of the search is in the accent color.
        let found: Vec<_> = h.highlight_line("T1 an Error here, error").collect();
        assert_eq!(
            found,
            [(0..3, Hl::Faint), (6..11, Hl::Accent), (18..23, Hl::Accent)]
        );
        assert_eq!(h.current_line(), 3);
        // Stderr and a match together: the match comes last so it wins; ranges are in bytes.
        let both: Vec<_> = h.highlight_line("ação ERROR").collect();
        assert_eq!(both, [(0..12, Hl::Warning), (7..12, Hl::Accent)]);
        assert_eq!(h.current_line(), 4);
        // Past the lines it knows: nothing.
        assert!(h.highlight_line("extra").next().is_none());
    }

    #[test]
    fn new_settings_start_over() {
        let a = LogHlSettings::new(1, Vec::new());
        let mut h = LogHighlighter::new(&a);
        let _ = h.highlight_line("x").count();
        let _ = h.highlight_line("y").count();
        assert_eq!(h.current_line(), 2);
        h.update(&LogHlSettings::new(2, Vec::new()));
        assert_eq!(h.current_line(), 0);
        h.change_line(5);
        assert_eq!(
            h.current_line(),
            0,
            "a later change does not move it forward"
        );
    }

    #[test]
    fn formats_use_the_palette_of_the_theme() {
        use crate::ui::theme::theme;
        use crate::ui::tokens::{DARK, LIGHT};
        for p in [&DARK, &LIGHT] {
            let t = theme(p);
            assert_eq!(log_format(&Hl::Warning, &t).color, Some(p.warning));
            assert_eq!(log_format(&Hl::Accent, &t).color, Some(p.accent));
            assert_eq!(log_format(&Hl::Faint, &t).color, Some(p.faint));
        }
    }
}
