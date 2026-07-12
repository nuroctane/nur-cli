//! Lightweight markdown → styled ratatui lines. Covers what coding agents
//! actually emit: headings, bullets, numbered lists, fenced code, inline
//! bold/italic/code, blockquotes, rules.

use crate::theme;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

pub fn render_markdown(text: &str, base: Style) -> Vec<Line<'static>> {
    let mut out = Vec::new();
    let mut in_code = false;
    let code_style = Style::default().fg(theme::CODE_FG).bg(theme::CODE_BG);

    for raw in text.lines() {
        let trimmed = raw.trim_start();

        // Fence toggles.
        if trimmed.starts_with("```") {
            let lang = trimmed.trim_start_matches('`').trim();
            if !in_code {
                in_code = true;
                let label = if lang.is_empty() {
                    "── code".to_string()
                } else {
                    format!("── {lang}")
                };
                out.push(Line::from(Span::styled(label, theme::style_faint())));
            } else {
                in_code = false;
                out.push(Line::from(Span::styled(
                    "──".to_string(),
                    theme::style_faint(),
                )));
            }
            continue;
        }

        if in_code {
            out.push(Line::from(Span::styled(format!("  {raw}"), code_style)));
            continue;
        }

        // Horizontal rule.
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            out.push(Line::from(Span::styled(
                "─".repeat(24),
                theme::style_faint(),
            )));
            continue;
        }

        // Headings.
        if let Some(rest) = heading(trimmed) {
            out.push(Line::from(Span::styled(
                rest.to_string(),
                Style::default()
                    .fg(theme::META_BLUE_SKY)
                    .add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        // Blockquote.
        if let Some(rest) = trimmed.strip_prefix("> ") {
            let mut spans = vec![Span::styled("▌ ".to_string(), theme::style_faint())];
            spans.extend(inline(rest, theme::style_thinking()));
            out.push(Line::from(spans));
            continue;
        }

        // Bullets.
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            let indent = raw.len() - trimmed.len();
            let mut spans = vec![
                Span::raw(" ".repeat(indent)),
                Span::styled("• ".to_string(), Style::default().fg(theme::META_BLUE_BRIGHT)),
            ];
            spans.extend(inline(rest, base));
            out.push(Line::from(spans));
            continue;
        }

        // Numbered list — keep the number, tint it.
        if let Some(dot) = trimmed.find(". ") {
            if dot <= 3 && trimmed[..dot].chars().all(|c| c.is_ascii_digit()) && dot > 0 {
                let indent = raw.len() - trimmed.len();
                let mut spans = vec![
                    Span::raw(" ".repeat(indent)),
                    Span::styled(
                        trimmed[..dot + 2].to_string(),
                        Style::default().fg(theme::META_BLUE_BRIGHT),
                    ),
                ];
                spans.extend(inline(&trimmed[dot + 2..], base));
                out.push(Line::from(spans));
                continue;
            }
        }

        out.push(Line::from(inline(raw, base)));
    }
    out
}

fn heading(s: &str) -> Option<&str> {
    for prefix in ["#### ", "### ", "## ", "# "] {
        if let Some(rest) = s.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    None
}

/// Inline markdown: **bold**, *italic*, _italic_, `code`.
fn inline(s: &str, base: Style) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut cur = String::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;

    let flush = |cur: &mut String, spans: &mut Vec<Span<'static>>| {
        if !cur.is_empty() {
            spans.push(Span::styled(std::mem::take(cur), base));
        }
    };

    while i < chars.len() {
        // `code`
        if chars[i] == '`' {
            if let Some(end) = find(&chars, i + 1, '`') {
                flush(&mut cur, &mut spans);
                let code: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(
                    code,
                    Style::default().fg(theme::CODE_FG).bg(theme::CODE_BG),
                ));
                i = end + 1;
                continue;
            }
        }
        // **bold** — body must not start or end with whitespace
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find2(&chars, i + 2, '*') {
                if end > i + 2
                    && !chars[i + 2].is_whitespace()
                    && !chars[end - 1].is_whitespace()
                {
                    flush(&mut cur, &mut spans);
                    let body: String = chars[i + 2..end].iter().collect();
                    spans.push(Span::styled(body, base.add_modifier(Modifier::BOLD)));
                    i = end + 2;
                    continue;
                }
            }
        }
        // *italic* — same whitespace rule
        if chars[i] == '*' {
            if let Some(end) = find(&chars, i + 1, '*') {
                if end > i + 1
                    && !chars[i + 1].is_whitespace()
                    && !chars[end - 1].is_whitespace()
                {
                    flush(&mut cur, &mut spans);
                    let body: String = chars[i + 1..end].iter().collect();
                    spans.push(Span::styled(body, base.add_modifier(Modifier::ITALIC)));
                    i = end + 1;
                    continue;
                }
            }
        }
        cur.push(chars[i]);
        i += 1;
    }
    flush(&mut cur, &mut spans);
    if spans.is_empty() {
        spans.push(Span::styled(String::new(), base));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn flat(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn code_fence_and_inline() {
        let md = "before `x` **b**\n```rust\nlet a = 1;\n```\nafter";
        let out = render_markdown(md, Style::default());
        let f = flat(&out);
        assert!(f[0].contains("before"));
        assert!(f[1].contains("rust"));
        assert!(f[2].contains("let a = 1;"));
        assert!(f[4].contains("after"));
    }

    #[test]
    fn bullets_and_headings() {
        let md = "# Title\n- one\n- two\n1. three";
        let out = render_markdown(md, Style::default());
        let f = flat(&out);
        assert_eq!(f[0], "Title");
        assert!(f[1].contains("• one"));
        assert!(f[3].contains("1. three"));
    }

    #[test]
    fn unclosed_markers_are_literal() {
        let out = render_markdown("a ** b ` c *", Style::default());
        assert_eq!(flat(&out)[0], "a ** b ` c *");
    }
}

fn find(chars: &[char], from: usize, ch: char) -> Option<usize> {
    (from..chars.len()).find(|&j| chars[j] == ch)
}

fn find2(chars: &[char], from: usize, ch: char) -> Option<usize> {
    let mut j = from;
    while j + 1 < chars.len() {
        if chars[j] == ch && chars[j + 1] == ch {
            return Some(j);
        }
        j += 1;
    }
    None
}
