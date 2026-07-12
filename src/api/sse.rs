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
    fn chunk_boundary_between_the_two_newlines() {
        let mut p = SseParser::new();
        assert!(p.push(b"data: x\n").is_empty());
        assert_eq!(p.push(b"\ndata: y\n\n"), vec!["x".to_string(), "y".to_string()]);
    }
}
