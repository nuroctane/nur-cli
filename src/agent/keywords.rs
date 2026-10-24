//! Prompt keywords: standalone words in the user's prose that change how one
//! turn runs, the way `ultrathink` does in Claude Code and OMP.
//!
//! A keyword counts only as its own word in prose. Inside a fenced block, an
//! inline code span, an identifier (`ultrathink_mode`), a path
//! (`docs/ultrathink.md`) or a flag (`--ultrathink`) it is just text, so
//! pasting code or file names never changes the effort by accident.

/// `ultrathink`: the strongest reasoning effort the route accepts, for this
/// turn only. The configured effort is untouched.
pub const ULTRATHINK: &str = "ultrathink";

/// Does `text` use `word` as a standalone prose word (ASCII case-insensitive)?
pub fn has_keyword(text: &str, word: &str) -> bool {
    let prose = strip_code(text);
    let hay = prose.to_ascii_lowercase();
    let needle = word.to_ascii_lowercase();
    let bytes = hay.as_bytes();
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let start = from + at;
        let end = start + needle.len();
        // A period glues only inside a dotted name (`docs.ultrathink`,
        // `ultrathink.md`); as sentence punctuation it is a boundary.
        let before_ok = match start.checked_sub(1).map(|i| bytes[i]) {
            None => true,
            Some(b'.') => start < 2 || !bytes[start - 2].is_ascii_alphanumeric(),
            Some(b) => !is_word_byte(b),
        };
        let after_ok = match bytes.get(end).copied() {
            None => true,
            Some(b'.') => bytes
                .get(end + 1)
                .is_none_or(|b| !b.is_ascii_alphanumeric()),
            Some(b) => !is_word_byte(b),
        };
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

/// Bytes that glue a match into a larger token: identifiers, paths, flags
/// and addresses. (Periods are handled at the call site.)
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'/' | b'\\' | b'@')
}

/// `text` with fenced blocks and inline code spans blanked out.
fn strip_code(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            in_fence = !in_fence;
            out.push('\n');
            continue;
        }
        if in_fence {
            out.push('\n');
            continue;
        }
        let mut in_span = false;
        for c in line.chars() {
            if c == '`' {
                in_span = !in_span;
                out.push(' ');
            } else {
                out.push(if in_span { ' ' } else { c });
            }
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ways a keyword can be meant, and the ways it shows up by accident.
    #[test]
    fn only_a_standalone_prose_word_counts() {
        for meant in [
            "ultrathink about the cache invalidation",
            "Fix the race. Ultrathink.",
            "please (ultrathink) this one",
            "ULTRATHINK: why does the stream drop?",
        ] {
            assert!(has_keyword(meant, ULTRATHINK), "{meant}");
        }
        for accidental in [
            "rename `ultrathink` to deepthink",
            "```\nultrathink\n```",
            "read docs/ultrathink.md",
            "the ultrathink_mode flag",
            "pass --ultrathink to the bench",
            "ultrathinking is not the keyword",
            "see https://example.com/ultrathink",
        ] {
            assert!(!has_keyword(accidental, ULTRATHINK), "{accidental}");
        }
    }
}
