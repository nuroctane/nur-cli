//! ANSI/SGR-aware rendering of tool output.
//!
//! Shell tools (`bash`, `cargo`, `git`, `ls --color`) emit ANSI escape codes.
//! Before this module the transcript printed those bytes raw — mojibake like
//! `[32m` all over command output. Inspired by what [`tui-term`] does with a
//! full vt100 screen (too heavy for captured, non-interactive output), we parse
//! just the SGR subset into ratatui [`Style`]s and strip every other escape
//! sequence, so `cargo test` output shows green ✓s instead of noise.
//!
//! [`tui-term`]: https://crates.io/crates/tui-term

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

/// Strip every ANSI escape sequence (CSI, OSC, and lone ESC codes).
pub fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            // CSI: ESC [ params... final-byte (@ through ~)
            Some('[') => {
                chars.next();
                for c2 in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c2) {
                        break;
                    }
                }
            }
            // OSC: ESC ] ... (BEL or ESC \)
            Some(']') => {
                chars.next();
                while let Some(c2) = chars.next() {
                    if c2 == '\u{7}' {
                        break;
                    }
                    if c2 == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            // Two-char escapes (ESC c, ESC =, …)
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    out
}

/// Parse one line of SGR-coloured text into spans over `base`.
///
/// Non-SGR escapes are dropped. Unstyled runs keep `base`; SGR styling layers
/// on top of it (reset returns to `base`, not to terminal default).
pub fn line_to_spans(line: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut run = String::new();
    let mut cur = base;
    let mut chars = line.chars().peekable();

    let flush = |run: &mut String, style: Style, spans: &mut Vec<Span<'static>>| {
        if !run.is_empty() {
            spans.push(Span::styled(std::mem::take(run), style));
        }
    };

    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            run.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                let mut params = String::new();
                let mut fin = ' ';
                for c2 in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&c2) {
                        fin = c2;
                        break;
                    }
                    params.push(c2);
                }
                if fin == 'm' {
                    flush(&mut run, cur, &mut spans);
                    cur = apply_sgr(cur, base, &params);
                }
                // Every other CSI (cursor moves, erase) is dropped.
            }
            Some(']') => {
                chars.next();
                while let Some(c2) = chars.next() {
                    if c2 == '\u{7}' {
                        break;
                    }
                    if c2 == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next();
            }
            None => {}
        }
    }
    flush(&mut run, cur, &mut spans);
    spans
}

/// Apply one SGR parameter string (e.g. `"1;32"`, `"38;5;208"`) to a style.
fn apply_sgr(mut style: Style, base: Style, params: &str) -> Style {
    let ps: Vec<u16> = params
        .split(';')
        .map(|p| p.parse::<u16>().unwrap_or(0))
        .collect();
    let ps = if ps.is_empty() { vec![0] } else { ps };
    let mut i = 0usize;
    while i < ps.len() {
        match ps[i] {
            0 => style = base,
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            7 => style = style.add_modifier(Modifier::REVERSED),
            9 => style = style.add_modifier(Modifier::CROSSED_OUT),
            22 => {
                style = style.remove_modifier(Modifier::BOLD | Modifier::DIM);
            }
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            30..=37 => style.fg = Some(basic_color((ps[i] - 30) as u8, false)),
            39 => style.fg = base.fg,
            40..=47 => style.bg = Some(basic_color((ps[i] - 40) as u8, false)),
            49 => style.bg = base.bg,
            90..=97 => style.fg = Some(basic_color((ps[i] - 90) as u8, true)),
            100..=107 => style.bg = Some(basic_color((ps[i] - 100) as u8, true)),
            38 | 48 => {
                // Extended colour: 38;5;n (256) or 38;2;r;g;b (truecolor).
                let is_fg = ps[i] == 38;
                let col = match ps.get(i + 1) {
                    Some(5) => {
                        let c = ps.get(i + 2).copied().map(|n| Color::Indexed(n as u8));
                        i += 2;
                        c
                    }
                    Some(2) => {
                        let (r, g, b) = (
                            ps.get(i + 2).copied().unwrap_or(0) as u8,
                            ps.get(i + 3).copied().unwrap_or(0) as u8,
                            ps.get(i + 4).copied().unwrap_or(0) as u8,
                        );
                        i += 4;
                        Some(Color::Rgb(r, g, b))
                    }
                    _ => None,
                };
                if let Some(c) = col {
                    if is_fg {
                        style.fg = Some(c);
                    } else {
                        style.bg = Some(c);
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    style
}

/// Map the 8 basic ANSI colours onto readable choices for the Meta-dark canvas.
fn basic_color(n: u8, bright: bool) -> Color {
    use crate::theme;
    match (n, bright) {
        (0, _) => theme::FAINT, // "black" would vanish on the dark canvas
        (1, false) => theme::ERROR,
        (1, true) => Color::Rgb(255, 128, 128),
        (2, false) => theme::SUCCESS,
        (2, true) => Color::Rgb(110, 235, 160),
        (3, false) => theme::WARN,
        (3, true) => Color::Rgb(255, 210, 110),
        (4, false) => theme::META_BLUE,
        (4, true) => theme::META_BLUE_SKY,
        (5, false) => theme::VIOLET,
        (5, true) => theme::LAVENDER,
        (6, false) => theme::TEAL,
        (6, true) => theme::SEAFOAM,
        (7, false) => theme::MUTED,
        (7, true) => theme::FG,
        _ => theme::FG,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_csi_and_osc() {
        let s = "\u{1b}[1;32mok\u{1b}[0m plain \u{1b}]0;title\u{7}end";
        assert_eq!(strip(s), "ok plain end");
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(strip("no escapes"), "no escapes");
        let spans = line_to_spans("no escapes", Style::default());
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "no escapes");
    }

    #[test]
    fn sgr_colors_a_run() {
        let base = Style::default();
        let spans = line_to_spans("a \u{1b}[32mgreen\u{1b}[0m b", base);
        let texts: Vec<&str> = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(texts, vec!["a ", "green", " b"]);
        assert_eq!(spans[1].style.fg, Some(crate::theme::SUCCESS));
        assert_eq!(spans[2].style, base, "reset returns to base");
    }

    #[test]
    fn truecolor_and_256() {
        let spans = line_to_spans("\u{1b}[38;2;10;20;30mx\u{1b}[38;5;100my", Style::default());
        assert_eq!(spans[0].style.fg, Some(Color::Rgb(10, 20, 30)));
        assert_eq!(spans[1].style.fg, Some(Color::Indexed(100)));
    }

    #[test]
    fn bold_then_not_bold() {
        let spans = line_to_spans("\u{1b}[1mB\u{1b}[22mn", Style::default());
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
        assert!(!spans[1].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn cursor_moves_are_dropped() {
        assert_eq!(strip("a\u{1b}[2Kb\u{1b}[1;1Hc"), "abc");
        let spans = line_to_spans("a\u{1b}[2Kb", Style::default());
        let text: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "ab");
    }
}
