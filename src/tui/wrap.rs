//! Span-aware word wrapping so transcript scrolling is exact.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

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

    let mut row: Vec<(char, Style)> = Vec::new();
    let mut row_w = 0usize;

    let mut i = 0usize;
    while i < chars.len() {
        let (ch, st) = chars[i];
        let w = ch.width().unwrap_or(0);
        if row_w + w > width && !row.is_empty() {
            // Find last space in the row to break at.
            let brk = row.iter().rposition(|(c, _)| *c == ' ');
            match brk {
                Some(p) if p > 0 => {
                    let rest: Vec<(char, Style)> = row.split_off(p + 1);
                    // Drop the trailing space from the emitted row.
                    row.pop();
                    out.push(row_to_line(std::mem::take(&mut row)));
                    row = rest;
                    row_w = row.iter().map(|(c, _)| c.width().unwrap_or(0)).sum();
                    continue; // re-attempt current char with the shorter row
                }
                _ => {
                    out.push(row_to_line(std::mem::take(&mut row)));
                    row_w = 0;
                    continue;
                }
            }
        }
        row.push((ch, st));
        row_w += w;
        i += 1;
    }
    out.push(row_to_line(row));
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
