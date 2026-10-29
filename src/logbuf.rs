//! Bounded log buffer with ANSI sanitizing and search. Core only: no UI deps.

use std::collections::VecDeque;

use crate::model::LogLine;

pub const MAX_LINES: usize = 10_000;
pub const MAX_LINE_CHARS: usize = 4_000;

/// Lossy UTF-8 decode, strip ANSI escapes (CSI/OSC), drop trailing `\r`/`\n`,
/// truncate to `MAX_LINE_CHARS` characters followed by `…`.
pub fn sanitize(raw: &[u8]) -> String {
    let decoded = String::from_utf8_lossy(raw);
    let mut out = String::with_capacity(decoded.len().min(MAX_LINE_CHARS + 4));
    let mut chars = decoded.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: parameters/intermediates until a final byte in 0x40..=0x7E.
            Some('[') => {
                for n in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            // OSC: until BEL or ST (ESC \).
            Some(']') => {
                while let Some(n) = chars.next() {
                    if n == '\x07' {
                        break;
                    }
                    if n == '\x1b' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            // Other two-byte escape: dropped.
            _ => {}
        }
    }
    let trimmed = out.trim_end_matches(['\r', '\n']);
    match trimmed.char_indices().nth(MAX_LINE_CHARS) {
        Some((idx, _)) => format!("{}…", &trimmed[..idx]),
        None => trimmed.to_string(),
    }
}

#[derive(Debug, Clone, Default)]
pub struct LogBuffer {
    lines: VecDeque<LogLine>,
}

impl LogBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a line, discarding the oldest once `MAX_LINES` is exceeded.
    pub fn push(&mut self, line: LogLine) {
        if self.lines.len() >= MAX_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(line);
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &LogLine> {
        self.lines.iter()
    }

    /// Indices of lines containing `q`, case-insensitive. Empty query matches nothing.
    pub fn search(&self, q: &str) -> Vec<usize> {
        if q.is_empty() {
            return Vec::new();
        }
        let q = q.to_lowercase();
        self.lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.text.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect()
    }

    /// Plain text for clipboard copy, one line per entry.
    pub fn to_text(&self, timestamps: bool) -> String {
        let mut out = String::new();
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            if let (true, Some(ts)) = (timestamps, &l.timestamp) {
                out.push_str(ts);
                out.push(' ');
            }
            out.push_str(&l.text);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LogStream;

    fn line(text: &str, ts: Option<&str>) -> LogLine {
        LogLine {
            stream: LogStream::Stdout,
            timestamp: ts.map(str::to_string),
            text: text.to_string(),
        }
    }

    #[test]
    fn sanitize_strips_ansi() {
        assert_eq!(sanitize(b"\x1b[31mERRO\x1b[0m"), "ERRO");
        assert_eq!(sanitize(b"a\x1b]0;title\x07b\x1b]8;;x\x1b\\c"), "abc");
    }

    #[test]
    fn sanitize_invalid_utf8() {
        let s = sanitize(b"ok \xff fim");
        assert!(s.contains("ok ") && s.contains(" fim"));
    }

    #[test]
    fn sanitize_truncates_long_line() {
        let s = sanitize("a".repeat(10_000).as_bytes());
        assert_eq!(s.chars().count(), 4_001);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn sanitize_strips_carriage_return() {
        assert_eq!(sanitize(b"linha\r"), "linha");
        assert_eq!(sanitize(b"linha\r\n"), "linha");
    }

    #[test]
    fn buffer_caps_at_max() {
        let mut b = LogBuffer::new();
        for i in 0..=MAX_LINES {
            b.push(line(&i.to_string(), None));
        }
        assert_eq!(b.len(), MAX_LINES);
        assert_eq!(b.iter().next().unwrap().text, "1");
    }

    #[test]
    fn search_is_case_insensitive() {
        let mut b = LogBuffer::new();
        b.push(line("Hello World", None));
        b.push(line("nothing", None));
        b.push(line("say HELLO", None));
        assert_eq!(b.search("hello"), vec![0, 2]);
        assert!(b.search("").is_empty());
    }

    #[test]
    fn to_text_with_and_without_timestamps() {
        let mut b = LogBuffer::new();
        b.push(line("one", Some("2026-10-29T10:00:00Z")));
        b.push(line("two", None));
        assert_eq!(b.to_text(false), "one\ntwo");
        assert_eq!(b.to_text(true), "2026-10-29T10:00:00Z one\ntwo");
    }
}
