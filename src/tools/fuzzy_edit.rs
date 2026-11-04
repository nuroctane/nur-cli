//! Fallback matching for the edit tools.
//!
//! Models reproduce file text imperfectly: smart quotes arrive as ASCII,
//! trailing whitespace is lost in transit, indentation gets re-flowed. An
//! exact-match miss is not evidence the text is absent. The matchers here
//! order from safest to loosest; callers use the loosest tier that matches
//! uniquely and report which tier fired so the model can correct its quoting.
//!
//! Safety is unchanged: a needle that matches more than once at ANY tier
//! refuses the whole edit (`Ambiguity`), and replacements splice into the
//! ORIGINAL bytes at the matched span, so line endings and untouched regions
//! of the file are preserved.

/// Fold the characters models and terminals mangle to ASCII equivalents:
/// curly single/double quotes, en/em dashes, NBSP and other Unicode spaces.
/// Applied to BOTH sides before comparing, never to the replacement text.
pub fn fold_chars(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{201B}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{201F}' => '"',
        '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
        '\u{00A0}' | '\u{2007}' | '\u{202F}' => ' ',
        _ => c,
    }
}

/// Unicode-folded copy of `s`.
pub fn folded(s: &str) -> String {
    s.chars().map(fold_chars).collect()
}

/// A matched span in the original text; `start..end` are byte offsets.
#[derive(Debug, Clone, Copy)]
pub struct FuzzyHit {
    pub start: usize,
    pub end: usize,
}

/// How loose the match that produced a hit was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FuzzyTier {
    /// Byte-identical.
    Exact,
    /// Matched after Unicode folding.
    Unicode,
    /// Matched after Unicode folding + per-line trailing-whitespace trim
    /// (also covers CRLF-vs-LF: the stray `\r` is trailing whitespace).
    TrimEnd,
    /// Matched after also ignoring per-line leading whitespace (indent).
    Indent,
}

impl FuzzyTier {
    /// Human-readable name for tool-result messages.
    pub fn label(self) -> &'static str {
        match self {
            FuzzyTier::Exact => "exact",
            FuzzyTier::Unicode => "unicode-normalized",
            FuzzyTier::TrimEnd => "trailing-whitespace-tolerant",
            FuzzyTier::Indent => "indent-flexible",
        }
    }
}

/// A tier matched the needle at more than one site.
#[derive(Debug)]
pub struct Ambiguity;

impl std::fmt::Display for Ambiguity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "old_string matched more than once")
    }
}

/// Find `needle` in `haystack` under the loosest tier that matches uniquely.
/// `Ok(None)` means no tier matched; `Err(Ambiguity)` means a tier matched
/// more than once and the edit must be refused.
pub fn find_unique(
    haystack: &str,
    needle: &str,
) -> Result<Option<(FuzzyHit, FuzzyTier)>, Ambiguity> {
    if needle.is_empty() {
        return Ok(None);
    }
    match find_all_unique(haystack, needle.len(), |h| h.find(needle)) {
        Err(amb) => return Err(amb),
        Ok(Some(span)) => return Ok(Some((span, FuzzyTier::Exact))),
        Ok(None) => {}
    }
    // Unicode fold: compare on folded copies, splice at original offsets.
    // Folding is 1:1 per char, so char indexes align and spans map back by
    // counting chars (see `span_to_original`).
    let hay_f = folded(haystack);
    let needle_f = folded(needle);
    let folded_changed = hay_f.as_str() != haystack;
    if folded_changed {
        match find_all_unique(&hay_f, needle_f.len(), |h| h.find(&needle_f)) {
            Err(amb) => return Err(amb),
            Ok(Some(span)) => {
                let hit = span_to_original(haystack, &hay_f, span.start, span.end - span.start);
                return Ok(Some((hit, FuzzyTier::Unicode)));
            }
            Ok(None) => {}
        }
    }
    // Per-line tiers on folded lines.
    match line_match_unique(haystack, &needle_f, false) {
        Err(amb) => return Err(amb),
        Ok(Some(hit)) => return Ok(Some((hit, FuzzyTier::TrimEnd))),
        Ok(None) => {}
    }
    match line_match_unique(haystack, &needle_f, true) {
        Err(amb) => Err(amb),
        Ok(Some(hit)) => Ok(Some((hit, FuzzyTier::Indent))),
        Ok(None) => Ok(None),
    }
}

fn find_all_unique(
    hay: &str,
    needle_len: usize,
    find: impl Fn(&str) -> Option<usize>,
) -> Result<Option<FuzzyHit>, Ambiguity> {
    let mut hits = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = find(&hay[from..]) {
        let at = from + rel;
        hits.push(at);
        // Advance one character, not one needle: overlapping occurrences are
        // ambiguities, and a byte step could split a UTF-8 character.
        from = at + hay[at..].chars().next().map(char::len_utf8).unwrap_or(1);
        if from > hay.len() || hits.len() > 1 {
            break;
        }
    }
    match hits.len() {
        1 => Ok(Some(FuzzyHit {
            start: hits[0],
            end: hits[0] + needle_len,
        })),
        0 => Ok(None),
        _ => Err(Ambiguity),
    }
}

/// Map a span in the folded text back to original byte offsets by char
/// count: folding is 1:1 per char, so the char index of a folded byte
/// offset identifies the same character in the original.
fn span_to_original(
    original: &str,
    folded_text: &str,
    folded_start: usize,
    folded_len: usize,
) -> FuzzyHit {
    let chars_before = folded_text[..folded_start].chars().count();
    let chars_in = folded_text[folded_start..folded_start + folded_len]
        .chars()
        .count();
    let mut start = original.len();
    let mut end = original.len();
    let mut seen = 0usize;
    let mut byte = 0usize;
    for ch in original.chars() {
        if seen == chars_before {
            start = byte;
        }
        if seen == chars_before + chars_in {
            break;
        }
        byte += ch.len_utf8();
        seen += 1;
        if seen == chars_before + chars_in {
            end = byte;
        }
    }
    if chars_in == 0 {
        end = start;
    }
    FuzzyHit { start, end }
}

/// Line-based matcher: compare the needle's lines against windows of the
/// haystack's lines. `indent_flex` also ignores leading whitespace. Returns
/// the span covering the matched lines in the ORIGINAL text (start of the
/// first line through the end of the last, excluding its terminator).
/// `Err(Ambiguity)` when the window matches at 2+ sites.
fn line_match_unique(
    haystack: &str,
    needle_folded: &str,
    indent_flex: bool,
) -> Result<Option<FuzzyHit>, Ambiguity> {
    let needle_lines: Vec<String> = needle_folded
        .split('\n')
        .map(|l| {
            let l = l.trim_end();
            if indent_flex {
                l.trim_start().to_string()
            } else {
                l.to_string()
            }
        })
        .collect();
    if needle_lines.is_empty() || needle_lines.iter().all(|l| l.is_empty()) {
        return Ok(None);
    }
    // A one-line needle is already covered by TrimEnd (leading whitespace is
    // part of the line content there); on its own it is dangerously loose.
    if indent_flex && needle_lines.len() == 1 {
        return Ok(None);
    }
    let hay_lines: Vec<&str> = haystack.split('\n').collect();
    let n = needle_lines.len();
    if hay_lines.len() < n {
        return Ok(None);
    }
    let mut hits: Vec<(usize, usize)> = Vec::new();
    for w in 0..=(hay_lines.len() - n) {
        let ok = needle_lines
            .iter()
            .zip(&hay_lines[w..w + n])
            .all(|(nl, hl)| {
                let key_full = folded(hl);
                let key = key_full.trim_end();
                let key = if indent_flex { key.trim_start() } else { key };
                key == nl.as_str()
            });
        if ok {
            hits.push((w, w + n - 1));
            if hits.len() > 1 {
                return Err(Ambiguity);
            }
        }
    }
    let Some((first, last)) = hits.pop() else {
        return Ok(None);
    };
    let start = byte_offset_of_line(haystack, first);
    let last_start = byte_offset_of_line(haystack, last);
    let end = last_start + hay_lines[last].len();
    Ok(Some(FuzzyHit { start, end }))
}

fn byte_offset_of_line(text: &str, line_index: usize) -> usize {
    if line_index == 0 {
        return 0;
    }
    let mut seen = 0usize;
    for (i, b) in text.bytes().enumerate() {
        if b == b'\n' {
            seen += 1;
            if seen == line_index {
                return i + 1;
            }
        }
    }
    text.len()
}

/// Shift every line of `text` (LF) by `delta` columns of leading whitespace:
/// positive inserts spaces, negative strips up to that many. Used by the
/// indent-flexible tier so the replacement takes the file's indentation.
pub fn apply_indent_delta(text: &str, delta: isize) -> String {
    if delta == 0 {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len() + delta.unsigned_abs() * 4);
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if line.is_empty() {
            continue;
        }
        if delta > 0 {
            for _ in 0..delta {
                out.push(' ');
            }
            out.push_str(line);
        } else {
            let strip = (-delta) as usize;
            let cut = line
                .char_indices()
                .take(strip)
                .take_while(|(_, c)| *c == ' ')
                .count();
            out.push_str(&line[cut..]);
        }
    }
    out
}

/// Leading whitespace of a line, in chars.
fn leading_ws(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// Indent delta to move the needle's first line onto the matched file span's
/// first line, for the indent-flexible tier.
pub fn indent_delta_for(needle: &str, matched_span: &str) -> isize {
    let needle_ws = needle.lines().next().map(leading_ws).unwrap_or(0);
    let file_ws = matched_span.lines().next().map(leading_ws).unwrap_or(0);
    file_ws as isize - needle_ws as isize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(hay: &str, needle: &str) -> (String, FuzzyTier) {
        let (h, tier) = find_unique(hay, needle).unwrap().expect("no match");
        (hay[h.start..h.end].to_string(), tier)
    }

    #[test]
    fn exact_tier_wins_and_spans_original_bytes() {
        let (span, tier) = hit("alpha beta gamma", "beta");
        assert_eq!(span, "beta");
        assert_eq!(tier, FuzzyTier::Exact);
    }

    #[test]
    fn smart_quotes_in_file_match_ascii_in_needle() {
        let hay = "say \u{201C}hello\u{201D} now";
        let (span, tier) = hit(hay, "say \"hello\" now");
        assert_eq!(span, hay);
        assert_eq!(tier, FuzzyTier::Unicode);
    }

    #[test]
    fn em_dash_and_nbsp_fold() {
        let hay = "a\u{2014}b\u{00A0}c";
        let (span, tier) = hit(hay, "a-b c");
        assert_eq!(span, hay);
        assert_eq!(tier, FuzzyTier::Unicode);
    }

    #[test]
    fn trailing_whitespace_in_file_is_tolerated() {
        let hay = "fn a() {   \n    body\n}\n";
        let (span, tier) = hit(hay, "fn a() {\n    body\n}");
        assert!(span.starts_with("fn a() {"), "{span:?}");
        assert!(span.ends_with("}"), "{span:?}");
        assert_eq!(tier, FuzzyTier::TrimEnd);
    }

    #[test]
    fn indent_drift_matches_indent_flex() {
        let hay = "if x {\n    do_a();\n    do_b();\n}\n";
        let (span, tier) = hit(hay, "  do_a();\n  do_b();");
        assert!(span.contains("do_a();"));
        assert_eq!(tier, FuzzyTier::Indent);
    }

    #[test]
    fn ambiguous_matches_are_refused_at_every_tier() {
        assert!(matches!(find_unique("x x x", "x"), Err(Ambiguity)));
        // The needle matches at windows 0 and 2 once trailing whitespace is
        // tolerated: that ambiguity must refuse, not fall through to Indent.
        let hay = "a\nb\na\nb\na\n";
        assert!(matches!(find_unique(hay, "a\nb\na"), Err(Ambiguity)));
    }

    #[test]
    fn crlf_lines_match_lf_needle_via_trim_end() {
        let hay = "one\r\ntwo\r\n";
        let (span, tier) = hit(hay, "one\ntwo");
        assert_eq!(span, "one\r\ntwo\r");
        assert_eq!(tier, FuzzyTier::TrimEnd);
    }

    #[test]
    fn span_offsets_survive_multibyte_text() {
        // é (2 bytes) stays, the NBSP (2 bytes) folds to a 1-byte space: the
        // matched span in the ORIGINAL must still cover both bytes exactly.
        let hay = "caf\u{e9}\u{00A0}42";
        let (span, tier) = hit(hay, "caf\u{e9} 42");
        assert_eq!(span, hay);
        assert_eq!(tier, FuzzyTier::Unicode);
    }

    #[test]
    fn indent_delta_moves_replacement_onto_the_file() {
        assert_eq!(indent_delta_for("  x;\n", "    x;\n"), 2);
        assert_eq!(indent_delta_for("    x;\n", "  x;\n"), -2);
        assert_eq!(apply_indent_delta("a\n  b\n", 2), "  a\n    b\n");
        assert_eq!(apply_indent_delta("  a\n    b\n", -2), "a\n  b\n");
        assert_eq!(apply_indent_delta("a\n  b\n", -4), "a\nb\n");
    }

    #[test]
    fn no_match_returns_none_cleanly() {
        assert!(find_unique("alpha\nbeta\n", "gamma").unwrap().is_none());
    }
}
