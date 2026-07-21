//! Markdown → styled ratatui lines, powered by [`tui-markdown`] (joshka).
//!
//! We hand the assistant's markdown to `tui_markdown::from_str` for real parsing
//! and structure — headings, **bold**, *italic*, ~~strikethrough~~, ordered /
//! unordered / task lists, nested blockquotes, rules, and code — then layer the
//! Meta theme on top: plain text takes the assistant foreground, and everything
//! is owned into `'static` for the transcript's wrap cache.
//!
//! [`tui-markdown`]: https://github.com/joshka/tui-markdown

use crate::theme;
use ratatui::style::Style;
use ratatui::text::{Line, Span};

/// Render markdown to styled lines. `base` supplies the default foreground for
/// plain (uncoloured) text — e.g. the assistant message colour.
pub fn render_markdown(text: &str, base: Style) -> Vec<Line<'static>> {
    // `from_str` borrows from `text`; we copy each span into an owned String
    // so the result is `'static` and cacheable.
    let parsed = tui_markdown::from_str(text);
    let default_fg = base.fg.unwrap_or(theme::FG);

    let mut out: Vec<Line<'static>> = Vec::with_capacity(parsed.lines.len());
    for line in parsed.lines {
        // Remap tui-markdown's stock palette to Nur transcript hues at line level
        // (code blocks / quotes set the style on the line).
        let line_style = meta_palette(line.style);
        let line_has_fg = line_style.fg.is_some();
        let mut spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len());
        for span in line.spans {
            let mut style = meta_palette(span.style);
            if style.fg.is_none() && !line_has_fg {
                style.fg = Some(default_fg);
            }
            spans.push(Span::styled(span.content.into_owned(), style));
        }
        let mut l = Line::from(spans);
        l.style = line_style;
        l.alignment = line.alignment;
        out.push(l);
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(String::new(), base)));
    }
    out
}

/// Translate tui-markdown's stock colours into the Nur **transcript** palette.
/// Goal: clear structure (headings / links / code / quotes / lists) without
/// collapsing everything into white-or-gold. Modifiers stay as-is.
fn meta_palette(mut style: Style) -> Style {
    use ratatui::style::Color;
    // Inline code / code blocks: mint-on-dark (not gold-on-dark).
    if style.bg == Some(Color::Black) {
        style.bg = Some(theme::CODE_BG);
        if matches!(style.fg, Some(Color::White) | None) {
            style.fg = Some(theme::MD_CODE);
        }
    }
    // H1 banner uses a cyan background bar — drop bg, use cool heading hue.
    if style.bg == Some(Color::Cyan) {
        style.bg = None;
        style.fg = Some(theme::MD_H1);
    }
    style.fg = match style.fg {
        Some(Color::Cyan) => Some(theme::MD_H2),        // H2 / H3
        Some(Color::LightCyan) => Some(theme::MD_H3),   // H4–H6
        Some(Color::Green) => Some(theme::MD_QUOTE),    // blockquotes
        Some(Color::Blue) => Some(theme::MD_LINK),      // links
        Some(Color::LightBlue) => Some(theme::MD_LIST), // list markers
        Some(Color::Yellow) | Some(Color::LightYellow) => Some(theme::AMBER),
        Some(Color::Magenta) | Some(Color::LightMagenta) => Some(theme::LAVENDER),
        Some(Color::Red) | Some(Color::LightRed) => Some(theme::ERROR),
        other => other,
    };
    style
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::{Color, Modifier};

    fn flat(lines: &[Line<'static>]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_headings_and_text() {
        let out = render_markdown("# Title\n\nsome text", Style::default().fg(Color::White));
        let text = flat(&out);
        assert!(text.contains("Title"), "heading text present: {text:?}");
        assert!(text.contains("some text"), "body present: {text:?}");
    }

    #[test]
    fn plain_text_gets_the_base_foreground() {
        let base = Style::default().fg(Color::Rgb(1, 2, 3));
        let out = render_markdown("just words", base);
        // At least one span carries the base fg (plain text is not left uncoloured).
        let got = out
            .iter()
            .flat_map(|l| l.spans.iter())
            .any(|s| s.style.fg == Some(Color::Rgb(1, 2, 3)));
        assert!(got, "plain text should adopt the base foreground");
    }

    #[test]
    fn bold_carries_the_bold_modifier() {
        let out = render_markdown("a **strong** word", Style::default());
        let has_bold = out
            .iter()
            .flat_map(|l| l.spans.iter())
            .any(|s| s.style.add_modifier.contains(Modifier::BOLD));
        assert!(has_bold, "bold span should keep the BOLD modifier");
    }

    #[test]
    fn lists_and_code_do_not_panic_and_produce_lines() {
        let md = "- one\n- two\n  - nested\n\n```rust\nlet a = 1;\n```\n\n> quote\n\n- [x] done\n- [ ] todo";
        let out = render_markdown(md, Style::default());
        assert!(out.len() >= 6, "list/code/quote should yield several lines");
        assert!(flat(&out).contains("let a = 1;"), "code content preserved");
    }

    #[test]
    fn empty_input_is_safe() {
        assert_eq!(render_markdown("", Style::default()).len(), 1);
    }
}
