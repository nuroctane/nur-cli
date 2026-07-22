//! Byte-level SSE framing.
//!
//! Network chunks split anywhere — including the middle of a multi-byte UTF-8
//! character. Decoding each chunk on arrival turns a split `é`/`✓`/emoji into
//! replacement characters, so we buffer **bytes**, split on the event boundary,
//! and only decode once an event is complete.

/// Accumulates raw bytes and yields complete SSE event payloads (`data:` joined).
#[derive(Default)]
pub struct SseParser {
    buf: Vec<u8>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a network chunk; returns the `data:` payload of every event that
    /// became complete. Incomplete trailing bytes stay buffered.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();
        while let Some((end, sep_len)) = find_boundary(&self.buf) {
            let event: Vec<u8> = self.buf.drain(..end + sep_len).collect();
            let event = &event[..end];
            // The event is whole, so this decode never splits a character.
            let text = String::from_utf8_lossy(event);
            if let Some(data) = extract_data(&text) {
                out.push(data);
            }
        }
        out
    }

    /// Flush whatever is still buffered once the body is over.
    ///
    /// A well-behaved server terminates the last event with a blank line, but
    /// plenty do not: the final `data:` frame arrives and the connection simply
    /// closes. [`push`](Self::push) can never yield that frame — it only emits
    /// on a boundary — so without this the last event of a stream is silently
    /// dropped. That loses the final content delta, and worse, an error frame
    /// or `finish_reason` that arrives last, which is why a failing stream could
    /// present as a turn that just stopped with no explanation.
    ///
    /// Returns `None` when the buffer holds nothing, or only a partial frame
    /// with no `data:` line (a truncated comment or half-written field), so a
    /// genuinely cut-off stream does not fabricate an event.
    pub fn finish(&mut self) -> Option<String> {
        if self.buf.is_empty() {
            return None;
        }
        let rest: Vec<u8> = std::mem::take(&mut self.buf);
        // Whatever is left is all we will ever get, so decoding it now cannot
        // split a character that a later chunk would have completed.
        extract_data(&String::from_utf8_lossy(&rest))
    }
}

/// Locate the end of the first event: a blank line (`\n\n` or `\r\n\r\n`).
/// Returns (offset of the boundary, length of the separator).
fn find_boundary(buf: &[u8]) -> Option<(usize, usize)> {
    let mut i = 0;
    while i < buf.len() {
        if buf[i] == b'\n' {
            // \n\n
            if buf.get(i + 1) == Some(&b'\n') {
                return Some((i, 2));
            }
            // \n\r\n  (i.e. …\r\n\r\n with the leading \r part of the payload)
            if buf.get(i + 1) == Some(&b'\r') && buf.get(i + 2) == Some(&b'\n') {
                return Some((i, 3));
            }
        }
        i += 1;
    }
    None
}

/// Join every `data:` line of one event.
fn extract_data(raw: &str) -> Option<String> {
    let mut out = String::new();
    for line in raw.lines() {
        // Boundary detection keeps a trailing \r on the last line of a CRLF event.
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(rest.trim_start());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multibyte_split_across_chunks_is_not_corrupted() {
        let payload = "data: {\"delta\":\"héllo 日本 🚀\"}\n\n";
        let bytes = payload.as_bytes();
        let mut p = SseParser::new();
        // Split in the middle of the multi-byte characters, byte by byte.
        let mut events = Vec::new();
        for b in bytes {
            events.extend(p.push(&[*b]));
        }
        assert_eq!(events.len(), 1);
        assert!(
            events[0].contains("héllo 日本 🚀"),
            "corrupted: {}",
            events[0]
        );
        assert!(!events[0].contains('\u{FFFD}'), "replacement char leaked");
    }

    #[test]
    fn handles_crlf_and_lf_boundaries() {
        let mut p = SseParser::new();
        let evs = p.push(b"data: one\r\n\r\ndata: two\n\n");
        assert_eq!(evs, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn joins_multiple_data_lines_and_skips_comments() {
        let mut p = SseParser::new();
        let evs = p.push(b": keep-alive\ndata: a\ndata: b\n\n");
        assert_eq!(evs, vec!["a\nb".to_string()]);
    }

    #[test]
    fn partial_event_stays_buffered() {
        let mut p = SseParser::new();
        assert!(p.push(b"data: incomp").is_empty());
        assert!(p.push(b"lete").is_empty());
        assert_eq!(p.push(b"\n\n"), vec!["incomplete".to_string()]);
    }

    #[test]
    fn finish_flushes_a_last_event_with_no_trailing_blank_line() {
        let mut p = SseParser::new();
        let evs = p.push(b"data: one\n\ndata: two\n");
        assert_eq!(evs, vec!["one".to_string()]);
        // `two` never got its blank line — the connection just closed.
        assert_eq!(p.finish(), Some("two".to_string()));
        assert_eq!(p.finish(), None, "flushing twice must not repeat the event");
    }

    #[test]
    fn finish_flushes_an_event_with_no_newline_at_all() {
        let mut p = SseParser::new();
        assert!(p.push(b"data: {\"error\":\"boom\"}").is_empty());
        assert_eq!(p.finish(), Some("{\"error\":\"boom\"}".to_string()));
    }

    #[test]
    fn finish_on_a_clean_stream_yields_nothing() {
        let mut p = SseParser::new();
        assert_eq!(p.push(b"data: one\n\n"), vec!["one".to_string()]);
        assert_eq!(p.finish(), None);
    }

    #[test]
    fn finish_does_not_invent_an_event_from_a_partial_frame() {
        // A truncated comment / half-written field has no `data:` line.
        let mut p = SseParser::new();
        assert!(p.push(b": keep-ali").is_empty());
        assert_eq!(p.finish(), None);
        let mut p = SseParser::new();
        assert!(p.push(b"eve").is_empty());
        assert_eq!(p.finish(), None);
    }

    #[test]
    fn finish_preserves_multibyte_in_the_trailing_event() {
        let mut p = SseParser::new();
        assert!(p.push("data: héllo 日本 🚀".as_bytes()).is_empty());
        let last = p.finish().expect("trailing event");
        assert!(last.contains("héllo 日本 🚀"), "corrupted: {last}");
        assert!(!last.contains('\u{FFFD}'), "replacement char leaked");
    }

    #[test]
    fn chunk_boundary_between_the_two_newlines() {
        let mut p = SseParser::new();
        assert!(p.push(b"data: x\n").is_empty());
        assert_eq!(
            p.push(b"\ndata: y\n\n"),
            vec!["x".to_string(), "y".to_string()]
        );
    }
}
