//! Span-aware word wrapping so transcript scrolling is exact.
//!
//! Wrapping repeats each logical line's **continuation gutter**, so a wrapped
//! paragraph stays under its own text instead of collapsing to column 0:
//! leading whitespace is repeated verbatim, and a leading marker glyph
//! (`•`, `◦`, `▪`, `▏`, `│`, `└`, `✓`, `☐`, `▎`) is replaced by spaces of the
//! same width. Without this, every wrapped prose row, list item, quote, and
//! tool-output line snapped back to the left edge and the body read as a wall.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

/// Block markers that open a line. Bars are a continuous rule, so they REPEAT
/// on wrapped rows; item markers are one-shot, so they blank out instead.
const GUTTER_BARS: &[char] = &['▏', '│', '└', '▎', '▍', '▌'];
const GUTTER_ITEM_MARKERS: &[char] = &['•', '◦', '▪', '✓', '☐'];

/// Wrap styled lines to `width` columns, preserving span styles.
/// Prefers breaking at spaces; falls back to hard breaks for long tokens.
pub fn wrap_lines(lines: &[Line<'static>], width: u16) -> Vec<Line<'static>> {
    let width = width.max(4) as usize;
    let mut out = Vec::new();
    for line in lines {
        wrap_one(line, width, &mut out);
    }
    out
}

/// The spans a wrapped row repeats so text stays aligned under its block: the
/// line's leading whitespace, plus a marker glyph turned into spaces.
fn continuation_gutter(line: &Line<'_>) -> Vec<Span<'static>> {
    let mut gutter: Vec<Span<'static>> = Vec::new();
    for span in &line.spans {
        let content = span.content.as_ref();
        let trimmed = content.trim();
        if trimmed.is_empty() {
            // Pure whitespace: repeat it verbatim.
            gutter.push(Span::styled(content.to_string(), span.style));
            continue;
        }
        // A leading marker keeps the block aligned on continuation rows: bars
        // repeat their glyph, item markers (bullets, tasks, `1.`) blank out.
        let marker_chars: Vec<char> = trimmed
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '`')
            .collect();
        let is_item_marker = marker_chars
            .iter()
            .all(|c| GUTTER_ITEM_MARKERS.contains(c))
            || is_ordered_marker(&marker_chars);
        let is_bar = marker_chars.iter().any(|c| GUTTER_BARS.contains(c))
            && marker_chars.iter().all(|c| GUTTER_BARS.contains(c));
        if is_bar || is_item_marker {
            let content = if is_bar {
                content.to_string()
            } else {
                " ".repeat(UnicodeWidthStr::width(content))
            };
            gutter.push(Span::styled(content, span.style));
            continue;
        }
        break;
    }
    gutter
}

/// Ordered-list marker (`1.`, `12.`) - also blanks out on continuation rows.
fn is_ordered_marker(chars: &[char]) -> bool {
    if chars.len() < 2 || *chars.last().unwrap() != '.' {
        return false;
    }
    chars[..chars.len() - 1]
        .iter()
        .all(|c| c.is_ascii_digit())
}

fn wrap_one(line: &Line<'static>, width: usize, out: &mut Vec<Line<'static>>) {
    // Flatten to (char, style) stream.
    let mut chars: Vec<(char, Style)> = Vec::new();
    for span in &line.spans {
        for ch in span.content.chars() {
            chars.push((ch, span.style));
        }
    }
    if chars.is_empty() {
        out.push(Line::default());
        return;
    }

    let gutter = continuation_gutter(line);
    let gutter_w: usize = gutter
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    // Continuation rows pay for the gutter, so they wrap earlier than the first.
    let cont_width = width.saturating_sub(gutter_w).max(4);

    let mut row: Vec<(char, Style)> = Vec::new();
    let mut row_w = 0usize;
    let mut first_row = true;

    let mut i = 0usize;
    while i < chars.len() {
        let (ch, st) = chars[i];
        let w = ch.width().unwrap_or(0);
        let budget = if first_row { width } else { cont_width };
        if row_w + w > budget && !row.is_empty() {
            // Find last space in the row to break at.
            let brk = row.iter().rposition(|(c, _)| *c == ' ');
            match brk {
                Some(p) if p > 0 => {
                    let rest: Vec<(char, Style)> = row.split_off(p + 1);
                    // Drop the trailing space from the emitted row.
                    row.pop();
                    emit_row(std::mem::take(&mut row), first_row, &gutter, out);
                    first_row = false;
                    row = rest;
                    row_w = row.iter().map(|(c, _)| c.width().unwrap_or(0)).sum();
                    continue; // re-attempt current char with the shorter row
                }
                _ => {
                    emit_row(std::mem::take(&mut row), first_row, &gutter, out);
                    first_row = false;
                    row_w = 0;
                    continue;
                }
            }
        }
        row.push((ch, st));
        row_w += w;
        i += 1;
    }
    emit_row(row, first_row, &gutter, out);
}

fn emit_row(
    row: Vec<(char, Style)>,
    first_row: bool,
    gutter: &[Span<'static>],
    out: &mut Vec<Line<'static>>,
) {
    let mut line = row_to_line(row);
    if !first_row && !gutter.is_empty() {
        line.spans.splice(0..0, gutter.iter().cloned());
    }
    out.push(line);
}

fn row_to_line(row: Vec<(char, Style)>) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut cur = String::new();
    let mut cur_style: Option<Style> = None;
    for (ch, st) in row {
        match cur_style {
            Some(s) if s == st => cur.push(ch),
            Some(s) => {
                spans.push(Span::styled(std::mem::take(&mut cur), s));
                cur.push(ch);
                cur_style = Some(st);
            }
            None => {
                cur.push(ch);
                cur_style = Some(st);
            }
        }
    }
    if let Some(s) = cur_style {
        if !cur.is_empty() {
            spans.push(Span::styled(cur, s));
        }
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn wraps_at_spaces() {
        let lines = vec![Line::from("hello brave new world")];
        let w = wrap_lines(&lines, 12);
        assert_eq!(text(&w), vec!["hello brave", "new world"]);
    }

    #[test]
    fn hard_breaks_long_tokens() {
        let lines = vec![Line::from("abcdefghijklmnop")];
        let w = wrap_lines(&lines, 6);
        assert_eq!(text(&w), vec!["abcdef", "ghijkl", "mnop"]);
    }

    #[test]
    fn empty_line_survives() {
        let lines = vec![Line::default(), Line::from("x")];
        let w = wrap_lines(&lines, 10);
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn wide_chars_do_not_panic() {
        let lines = vec![Line::from("日本語のテキストです、こんにちは世界")];
        let w = wrap_lines(&lines, 8);
        assert!(w.len() >= 4);
    }

    #[test]
    fn preserves_styles_across_break() {
        use ratatui::style::Color;
        let styled = Style::default().fg(Color::Red);
        let lines = vec![Line::from(vec![
            Span::raw("aaaa "),
            Span::styled("bbbb cccc", styled),
        ])];
        let w = wrap_lines(&lines, 7);
        assert_eq!(text(&w), vec!["aaaa", "bbbb", "cccc"]);
    }

    #[test]
    fn wrapped_prose_stays_under_its_own_gutter() {
        // The assistant gutter: "  " on every logical line after the first.
        let lines = vec![Line::from(vec![
            Span::raw("  "),
            Span::raw("one two three four five six"),
        ])];
        let w = wrap_lines(&lines, 16);
        let t = text(&w);
        assert!(t.len() > 1, "should wrap: {t:?}");
        for row in t.iter().skip(1) {
            assert!(row.starts_with("  "), "continuation kept its gutter: {t:?}");
        }
    }

    #[test]
    fn list_marker_becomes_spaces_on_continuation_rows() {
        let lines = vec![Line::from(vec![
            Span::raw("  "),
            Span::styled("• ", Style::default()),
            Span::raw("alpha beta gamma delta epsilon"),
        ])];
        let w = wrap_lines(&lines, 18);
        let t = text(&w);
        assert!(t[0].starts_with("  • "), "marker stays on row 0: {t:?}");
        for row in t.iter().skip(1) {
            assert!(
                row.starts_with("    ") && !row.contains('•'),
                "continuation aligns under the text without repeating the marker: {t:?}"
            );
        }
    }

    #[test]
    fn ordered_marker_indents_its_continuation_rows() {
        let lines = vec![Line::from(vec![
            Span::raw("  "),
            Span::styled("12. ", Style::default()),
            Span::raw("alpha beta gamma delta epsilon zeta"),
        ])];
        let w = wrap_lines(&lines, 20);
        let t = text(&w);
        assert!(t[0].starts_with("  12. "), "{t:?}");
        for row in t.iter().skip(1) {
            assert!(
                row.starts_with("      ") && !row.contains("12."),
                "ordered continuation aligns under the text: {t:?}"
            );
        }
    }

    #[test]
    fn quote_bar_repeats_on_wrapped_rows() {
        let lines = vec![Line::from(vec![
            Span::styled("▏ ", Style::default()),
            Span::raw("quoted words that will certainly wrap here"),
        ])];
        let w = wrap_lines(&lines, 18);
        let t = text(&w);
        assert!(t.len() > 1, "should wrap: {t:?}");
        for row in &t {
            assert!(
                row.starts_with('▏'),
                "every quoted row keeps its bar: {t:?}"
            );
        }
    }
}
