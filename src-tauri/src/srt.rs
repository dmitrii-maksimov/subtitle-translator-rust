//! Minimal SRT (SubRip) parser/composer + sanitizer.
//!
//! Ported from the Python `srt` library usage plus `core/srt_io.py` and
//! `core/sanitize.py`. The format is simple enough to hand-roll, which keeps
//! the dependency surface tiny and the behavior fully under our control.

use std::fs;
use std::io;
use std::path::Path;

/// A single subtitle cue. Times are stored as whole milliseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtitle {
    pub index: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub content: String,
}

/// Format milliseconds as an SRT timecode `HH:MM:SS,mmm`.
pub fn format_timecode(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let millis = ms % 1000;
    format!("{:02}:{:02}:{:02},{:03}", h, m, s, millis)
}

/// Parse a single `HH:MM:SS,mmm` (or `.mmm`) timecode into milliseconds.
fn parse_timecode(tc: &str) -> Option<u64> {
    let tc = tc.trim();
    // Split off the milliseconds part on either ',' or '.'.
    let (hms, millis) = if let Some(pos) = tc.find([',', '.']) {
        (&tc[..pos], &tc[pos + 1..])
    } else {
        (tc, "0")
    };
    let mut parts = hms.split(':');
    let h: u64 = parts.next()?.trim().parse().ok()?;
    let m: u64 = parts.next()?.trim().parse().ok()?;
    let s: u64 = parts.next()?.trim().parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    // Milliseconds may be fewer/more than 3 digits; normalize to 3.
    let millis_digits: String = millis.chars().filter(|c| c.is_ascii_digit()).collect();
    let millis_val: u64 = if millis_digits.is_empty() {
        0
    } else {
        let padded = format!("{:0<3}", &millis_digits[..millis_digits.len().min(3)]);
        padded.parse().unwrap_or(0)
    };
    Some(h * 3_600_000 + m * 60_000 + s * 1000 + millis_val)
}

/// Locate the `-->` arrow in a timing line and parse both timecodes.
fn parse_timing_line(line: &str) -> Option<(u64, u64)> {
    let arrow = line.find("-->")?;
    let start = parse_timecode(line[..arrow].trim())?;
    // The right side may carry position coords (X1:... ) — take the first token.
    let rest = line[arrow + 3..].trim();
    let end_tok = rest.split_whitespace().next()?;
    let end = parse_timecode(end_tok)?;
    Some((start, end))
}

/// Parse SRT text into a list of cues. Lenient: skips malformed blocks the way
/// the Python `srt.parse` would tolerate loose input.
pub fn parse(text: &str) -> Vec<Subtitle> {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = Vec::new();
    let mut lines = normalized.lines().peekable();
    let mut auto_index = 0u32;

    while lines.peek().is_some() {
        // Skip blank separator lines.
        while let Some(l) = lines.peek() {
            if l.trim().is_empty() {
                lines.next();
            } else {
                break;
            }
        }
        if lines.peek().is_none() {
            break;
        }

        // Optional numeric index line.
        let mut parsed_index: Option<u32> = None;
        let first = *lines.peek().unwrap();
        if first.trim().parse::<u32>().is_ok() && !first.contains("-->") {
            parsed_index = first.trim().parse::<u32>().ok();
            lines.next();
        }

        // Timing line.
        let timing = match lines.peek() {
            Some(l) if l.contains("-->") => *l,
            _ => {
                // Not a valid block; drop the line to avoid an infinite loop.
                lines.next();
                continue;
            }
        };
        let (start_ms, end_ms) = match parse_timing_line(timing) {
            Some(v) => v,
            None => {
                lines.next();
                continue;
            }
        };
        lines.next();

        // Content lines until a blank line or EOF.
        let mut content_lines = Vec::new();
        while let Some(l) = lines.peek() {
            if l.trim().is_empty() {
                break;
            }
            content_lines.push(*l);
            lines.next();
        }

        auto_index += 1;
        out.push(Subtitle {
            index: parsed_index.unwrap_or(auto_index),
            start_ms,
            end_ms,
            content: content_lines.join("\n"),
        });
    }
    out
}

/// Compose cues into SRT text with `\n` line endings (LF). CRLF conversion is
/// done by [`write_srt_crlf`], matching `core/srt_io.py`.
pub fn compose(entries: &[Subtitle]) -> String {
    let mut s = String::new();
    for e in entries {
        s.push_str(&e.index.to_string());
        s.push('\n');
        s.push_str(&format_timecode(e.start_ms));
        s.push_str(" --> ");
        s.push_str(&format_timecode(e.end_ms));
        s.push('\n');
        let content = e.content.replace("\r\n", "\n").replace('\r', "\n");
        s.push_str(&content);
        s.push('\n');
        s.push('\n');
    }
    s
}

/// Sort by start time and reassign 1-based indices, mirroring
/// `srt.sort_and_reindex`.
pub fn sort_and_reindex(mut entries: Vec<Subtitle>) -> Vec<Subtitle> {
    entries.sort_by_key(|e| (e.start_ms, e.end_ms));
    for (i, e) in entries.iter_mut().enumerate() {
        e.index = (i + 1) as u32;
    }
    entries
}

/// Write cues to disk as UTF-8 with forced CRLF line endings and a trailing
/// newline — byte-for-byte the behavior of the Python writer.
pub fn write_srt_crlf(path: &Path, entries: &[Subtitle]) -> io::Result<()> {
    let mut text = compose(entries);
    text = text.replace("\r\n", "\n").replace('\r', "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    let text = text.replace('\n', "\r\n");
    fs::write(path, text)
}

// ---- sentinel (core/srt_io.py) ----

/// Placeholder cue appended after the last translated line while a file is still
/// downloading, telling the viewer to pause.
pub const SENTINEL_TEXT: &str =
    "SUBTITLES NOT TRANSLATED YET — MOVIE STILL DOWNLOADING — PLEASE PAUSE";

pub fn is_sentinel(sub: &Subtitle) -> bool {
    sub.content.trim() == SENTINEL_TEXT
}

/// Drop a trailing sentinel cue if present.
pub fn strip_sentinel(mut entries: Vec<Subtitle>) -> Vec<Subtitle> {
    if entries.last().map(is_sentinel).unwrap_or(false) {
        entries.pop();
    }
    entries
}

/// Format milliseconds as `HH:MM:SS`.
pub fn ms_to_hms(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

fn make_sentinel(after_end_ms: u64) -> Subtitle {
    Subtitle {
        index: 0,
        start_ms: after_end_ms + 1,
        end_ms: after_end_ms + 4 * 3_600_000,
        content: SENTINEL_TEXT.to_string(),
    }
}

/// Read `out_srt`, strip any trailing sentinel, append `new_entries`, re-append a
/// fresh sentinel, re-index, and write the whole file (CRLF). Ported from
/// `write_translated_with_sentinel`.
pub fn write_translated_with_sentinel(out_srt: &Path, new_entries: &[Subtitle]) -> io::Result<()> {
    let mut existing = if out_srt.exists() {
        match fs::read_to_string(out_srt) {
            Ok(text) => parse(&text),
            Err(_) => Vec::new(),
        }
    } else {
        Vec::new()
    };
    existing = strip_sentinel(existing);
    let mut all: Vec<Subtitle> = existing;
    all.extend(new_entries.iter().cloned());
    if let Some(last) = all.last() {
        let last_end = last.end_ms;
        all.push(make_sentinel(last_end));
    }
    for (i, e) in all.iter_mut().enumerate() {
        e.index = (i + 1) as u32;
    }
    write_srt_crlf(out_srt, &all)
}

// ---- sanitize (core/sanitize.py) ----

/// Drop pure index or timestamp lines the model may have leaked into a cue,
/// preserving all other line breaks.
pub fn sanitize_content(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let tmp = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut cleaned = Vec::new();
    for ln in tmp.split('\n') {
        let stripped = ln.trim();
        if is_index_line(stripped) || is_timestamp_line(stripped) {
            continue;
        }
        cleaned.push(ln);
    }
    cleaned.join("\n")
}

/// `^\d{1,5}$`
fn is_index_line(s: &str) -> bool {
    !s.is_empty() && s.len() <= 5 && s.chars().all(|c| c.is_ascii_digit())
}

/// `^\d{1,2}:\d{2}:\d{2}[,.]\d{3}\s+-->\s+\d{1,2}:\d{2}:\d{2}[,.]\d{3}$`
fn is_timestamp_line(s: &str) -> bool {
    let arrow = match s.find("-->") {
        Some(a) => a,
        None => return false,
    };
    let left = s[..arrow].trim();
    let right = s[arrow + 3..].trim();
    is_ts_token(left) && is_ts_token(right)
}

fn is_ts_token(t: &str) -> bool {
    // HH:MM:SS[,.]mmm  (1-2 digit hours)
    let (hms, millis) = match t.find([',', '.']) {
        Some(p) => (&t[..p], &t[p + 1..]),
        None => return false,
    };
    if millis.len() != 3 || !millis.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    let parts: Vec<&str> = hms.split(':').collect();
    if parts.len() != 3 {
        return false;
    }
    let ok_len = [(1usize, 2usize), (2, 2), (2, 2)];
    for (i, p) in parts.iter().enumerate() {
        if !p.chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        let (min, max) = ok_len[i];
        if p.len() < min || p.len() > max {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timecode_roundtrip() {
        assert_eq!(format_timecode(3_661_500), "01:01:01,500");
        assert_eq!(parse_timecode("01:01:01,500"), Some(3_661_500));
        assert_eq!(parse_timecode("00:00:01.250"), Some(1250));
    }

    #[test]
    fn parse_and_compose_roundtrip() {
        let text = "1\n00:00:01,000 --> 00:00:02,000\nHello\n\n2\n00:00:03,000 --> 00:00:04,000\nWorld\nsecond line\n";
        let subs = parse(text);
        assert_eq!(subs.len(), 2);
        assert_eq!(subs[0].content, "Hello");
        assert_eq!(subs[1].content, "World\nsecond line");
        let composed = compose(&subs);
        let reparsed = parse(&composed);
        assert_eq!(reparsed.len(), 2);
        assert_eq!(reparsed[1].content, "World\nsecond line");
    }

    #[test]
    fn reindex_sorts_and_numbers() {
        let subs = vec![
            Subtitle { index: 99, start_ms: 5000, end_ms: 6000, content: "b".into() },
            Subtitle { index: 3, start_ms: 1000, end_ms: 2000, content: "a".into() },
        ];
        let out = sort_and_reindex(subs);
        assert_eq!(out[0].index, 1);
        assert_eq!(out[0].content, "a");
        assert_eq!(out[1].index, 2);
    }

    #[test]
    fn sanitize_strips_index_and_timestamp_lines() {
        let raw = "12\n00:00:01,000 --> 00:00:02,000\nReal text\nMore text";
        assert_eq!(sanitize_content(raw), "Real text\nMore text");
        assert_eq!(sanitize_content(""), "");
        // A normal line that happens to be short digits is stripped only if it
        // is the whole line (matches Python behavior).
        assert_eq!(sanitize_content("Hello 123"), "Hello 123");
    }
}
