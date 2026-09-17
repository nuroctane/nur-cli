//! Markdown → styled ratatui lines, composed for transcript legibility.
//!
//! Two engines, deliberately split:
//!
//! - [`pulldown_cmark`] parses **block structure** - headings, paragraphs,
//!   lists (flat / nested / ordered / task), blockquotes, rules, fenced code -
//!   and this module decides how each one *looks*: spacing, gutters, markers,
//!   and which [`crate::theme`] role colours it.
//! - [`tui_markdown`] renders the **inline and code** styling (`**bold**`,
//!   `*italic*`, `` `code` ``, `~~struck~~`, links, and syntect-highlighted
//!   code). We hand it the source slice of one block, so its own literal block
//!   markers (`## `, `>`, ```` ``` ````, `- `) never reach the screen.
//!
//! Everything here is drawn from theme roles, so all 23 themes stay correct
//! with no per-theme branching.
//!
//! [`tui-markdown`]: https://github.com/joshka/tui-markdown

use crate::theme;
use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::ops::Range;

/// Render markdown to styled lines. `base` supplies the default foreground for
/// plain (uncoloured) text - e.g. the assistant message colour.
pub fn render_markdown(text: &str, base: Style) -> Vec<Line<'static>> {
    #[cfg(feature = "image-peek")]
    let annotated = crate::tui::latex::annotate_display_math(text);
    #[cfg(feature = "image-peek")]
    let text = annotated.as_str();

    let default_fg = base.fg.unwrap_or(theme::FG());
    let mut composer = Composer::new(text, default_fg);
    composer.run();
    if composer.out.is_empty() {
        composer
            .out
            .push(Line::from(Span::styled(String::new(), base)));
    }
    composer.out
}

/// One list level: ordered or not, plus the next number to print.
struct ListLevel {
    ordered: bool,
    next: u64,
}

/// The list item whose content block is being emitted.
struct ItemState {
    /// Nesting depth (0 = outermost), which sets the indent.
    depth: usize,
    kind: Marker,
}

#[derive(Clone, Copy)]
enum Marker {
    Bullet,
    Ordered(u64),
    Task(bool),
}

struct Composer<'a> {
    src: &'a str,
    base_fg: Color,
    out: Vec<Line<'static>>,
    lists: Vec<ListLevel>,
    /// Quote nesting depth (0 = not inside a quote).
    quote_depth: usize,
    /// Set while a heading is open.
    heading: Option<u8>,
    /// Set while a list item's content is being emitted.
    item: Option<ItemState>,
    /// Source range of the inline block being collected.
    block: Option<Range<usize>>,
    /// Pending inline run collected straight from events - pulldown emits no
    /// `Paragraph` wrapper for tight list items, so their text arrives as bare
    /// inline events and has to be tracked here.
    inline: Option<Range<usize>>,
    /// Source range of the fenced code block being collected.
    code: Option<Range<usize>>,
    /// Whether the next block must be preceded by a blank separator line.
    needs_blank: bool,
    /// True until the first block is emitted (never a leading blank).
    at_start: bool,
}

impl<'a> Composer<'a> {
    fn new(src: &'a str, base_fg: Color) -> Self {
        Self {
            src,
            base_fg,
            out: Vec::new(),
            lists: Vec::new(),
            quote_depth: 0,
            heading: None,
            item: None,
            block: None,
            inline: None,
            code: None,
            needs_blank: false,
            at_start: true,
        }
    }

    fn run(&mut self) {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_TASKLISTS);
        options.insert(Options::ENABLE_STRIKETHROUGH);
        for (event, range) in Parser::new_ext(self.src, options).into_offset_iter() {
            self.event(event, range);
        }
        // Trailing inline text (a document ending in a tight item) still emits.
        self.flush_inline();
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        match &event {
            // Inline runs (text, code, emphasis, links, breaks): accumulate the
            // covered source range; the block events below flush it.
            Event::Text(_)
            | Event::Code(_)
            | Event::InlineHtml(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::SoftBreak
            | Event::HardBreak
            | Event::Start(Tag::Emphasis)
            | Event::End(TagEnd::Emphasis)
            | Event::Start(Tag::Strong)
            | Event::End(TagEnd::Strong)
            | Event::Start(Tag::Strikethrough)
            | Event::End(TagEnd::Strikethrough)
            | Event::Start(Tag::Link { .. })
            | Event::End(TagEnd::Link) => {
                self.note_inline(&range);
                return;
            }
            _ => {}
        }
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                self.heading = Some(heading_level(level));
                self.block = Some(range);
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(level) = self.heading.take() {
                    // The heading is emitted from its block range; its inline
                    // events must not also flush as a loose run.
                    self.inline = None;
                    self.emit_heading(level);
                }
            }
            Event::Start(Tag::Paragraph) => self.block = Some(range),
            Event::End(TagEnd::Paragraph) => {
                if let Some(range) = self.block.take() {
                    self.inline = None;
                    self.emit_inline(&range);
                }
            }
            Event::Start(Tag::CodeBlock(_)) => {
                self.flush_inline();
                self.code = Some(range);
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(range) = self.code.take() {
                    self.emit_code(&range);
                }
            }
            Event::Start(Tag::List(start)) => {
                // A tight item's own text ends where its nested list begins.
                self.flush_inline();
                if self.lists.is_empty() {
                    self.blank_before_block();
                }
                self.lists.push(ListLevel {
                    ordered: start.is_some(),
                    next: start.unwrap_or(1),
                });
            }
            Event::End(TagEnd::List(_)) => {
                self.lists.pop();
                self.needs_blank = true;
            }
            Event::Start(Tag::Item) => {
                let depth = self.lists.len().saturating_sub(1);
                let kind = match self.lists.last_mut() {
                    Some(level) if level.ordered => {
                        let n = level.next;
                        level.next += 1;
                        Marker::Ordered(n)
                    }
                    _ => Marker::Bullet,
                };
                self.item = Some(ItemState { depth, kind });
            }
            Event::End(TagEnd::Item) => {
                self.flush_inline();
                self.item = None;
                // Items stack tight; the list as a whole carries the spacing.
                self.needs_blank = false;
            }
            Event::TaskListMarker(checked) => {
                if let Some(item) = self.item.as_mut() {
                    item.kind = Marker::Task(checked);
                }
                // The checkbox is ours now - never let it reach the slice.
                self.inline = None;
            }
            Event::Start(Tag::BlockQuote(_)) => {
                self.flush_inline();
                self.blank_before_block();
                self.quote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                self.flush_inline();
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.needs_blank = true;
            }
            Event::Rule => {
                self.flush_inline();
                self.blank_before_block();
                self.out.push(rule_line());
                self.mark_emitted();
            }
            _ => {}
        }
    }

    /// Widen the pending inline run to cover `range`.
    fn note_inline(&mut self, range: &Range<usize>) {
        match self.inline.as_mut() {
            Some(run) => run.end = run.end.max(range.end),
            None => self.inline = Some(range.clone()),
        }
    }

    fn flush_inline(&mut self) {
        if let Some(range) = self.inline.take() {
            self.emit_inline(&range);
        }
    }

    /// Emit the blank separator a block needs (never leading, never doubled).
    fn blank_before_block(&mut self) {
        if !self.at_start && self.needs_blank {
            self.out.push(Line::default());
        }
        self.needs_blank = false;
    }

    /// Record that a block just ended and the next one wants a separator.
    fn mark_emitted(&mut self) {
        self.needs_blank = true;
        self.at_start = false;
    }

    fn emit_heading(&mut self, level: u8) {
        let Some(range) = self.block.clone() else {
            return;
        };
        self.blank_before_block();
        let (lines, _) = self.inline_lines(&range);
        let (marker, fg) = heading_style(level);
        for (i, mut line) in lines.into_iter().enumerate() {
            // The crate re-adds a markdown marker when it renders a heading
            // (including setext forms like `Title\n====`, where the source has
            // no hashes to strip). The block layer owns that decoration.
            strip_rendered_heading_marker(&mut line);
            line.spans.insert(
                0,
                if i == 0 {
                    marker.clone().unwrap_or_else(|| Span::raw(String::new()))
                } else {
                    Span::raw("  ".to_string())
                },
            );
            // Headings are structural: weight them so they never read as body.
            for span in line.spans.iter_mut() {
                span.style = span.style.add_modifier(Modifier::BOLD);
                if span.style.fg.is_none() || span.style.fg == Some(self.base_fg) {
                    span.style.fg = Some(fg);
                }
            }
            self.out.push(line);
        }
        // One blank line under a heading, then content - a heading needs to
        // breathe, but the gap must not read as a paragraph break of its own.
        self.out.push(Line::default());
        self.needs_blank = false;
        self.at_start = false;
    }

    fn emit_inline(&mut self, range: &Range<usize>) {
        let item = self.item.take();
        if item.is_none() {
            self.blank_before_block();
        }
        let (lines, _) = self.inline_lines(range);
        let prefix = self.prefix_for(item.as_ref());
        for (i, mut line) in lines.into_iter().enumerate() {
            let head = if i == 0 {
                prefix.clone()
            } else {
                prefix.continuation()
            };
            line.spans.splice(0..0, head.spans());
            self.out.push(line);
        }
        self.item = item;
        // List items stack tight; prose blocks want a separator after them.
        if self.item.is_none() {
            self.mark_emitted();
        } else {
            self.at_start = false;
        }
    }

    /// The gutter a block's lines carry: quote bar, indentation, list marker.
    fn prefix_for(&self, item: Option<&ItemState>) -> Prefix {
        Prefix {
            quote: self.quote_depth,
            indent: item.map(|i| i.depth).unwrap_or(0),
            marker: item.map(|i| i.kind),
        }
    }

    /// Inline content of one source slice, with the crate's stock palette
    /// mapped onto Nur's transcript hues. Inside a quote the text steps down so
    /// the bar reads as quoted material rather than body text, and bare URLs
    /// recede instead of competing with the prose they annotate.
    fn inline_lines(&self, range: &Range<usize>) -> (Vec<Line<'static>>, bool) {
        let mut slice = self.src.get(range.clone()).unwrap_or("");
        // The crate renders its own block markers; the block layer owns those,
        // so strip the heading marker before handing the slice over.
        slice = strip_heading_marker(slice);
        let quoted = self.quote_depth > 0;
        let parsed = tui_markdown::from_str(slice);
        let mut lines: Vec<Line<'static>> = Vec::with_capacity(parsed.lines.len());
        for line in parsed.lines {
            let line_style = meta_palette(line.style);
            let line_has_fg = line_style.fg.is_some();
            let mut spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len());
            for span in line.spans {
                let mut style = meta_palette(span.style);
                if style.fg.is_none() && !line_has_fg {
                    style.fg = Some(self.base_fg);
                }
                let content = span.content.into_owned();
                if looks_like_url(&content) {
                    // Recede, but stay readable: an aggressive dim drops below
                    // the repo's own 3.0 contrast floor on palettes like
                    // solarized, where the link hue is already low-contrast.
                    style = Style::default()
                        .fg(theme::dim(theme::MD_LINK(), 0.15))
                        .add_modifier(Modifier::UNDERLINED);
                } else if quoted {
                    // A step down from body text, not a fade: quoted material
                    // must still clear the contrast floor in every theme.
                    if let Some(fg) = style.fg {
                        style.fg = Some(theme::dim(fg, 0.22));
                    }
                }
                spans.push(Span::styled(content, style));
            }
            let mut l = Line::from(spans);
            l.style = line_style;
            lines.push(l);
        }
        let empty = lines.is_empty();
        if empty {
            lines.push(Line::from(Span::styled(
                String::new(),
                Style::default().fg(self.base_fg),
            )));
        }
        (lines, empty)
    }

    /// Fenced code: the crate supplies syntax highlighting (and `CODE_BG` when
    /// highlighting is unavailable); this layer owns the frame, so the fence
    /// lines never render and every row carries the code band.
    fn emit_code(&mut self, range: &Range<usize>) {
        self.blank_before_block();
        let slice = self.src.get(range.clone()).unwrap_or("");
        let parsed = tui_markdown::from_str(slice);
        // Own every span (the crate borrows the slice) and drop the fences.
        let mut owned: Vec<Line<'static>> = Vec::with_capacity(parsed.lines.len());
        for line in parsed.lines {
            let spans: Vec<Span<'static>> = line
                .spans
                .into_iter()
                .map(|span| Span::styled(span.content.into_owned(), meta_palette(span.style)))
                .collect();
            owned.push(Line::from(spans));
        }
        let body: Vec<Line<'static>> = owned.into_iter().filter(|l| !is_fence(l)).collect();
        let body = if body.is_empty() {
            vec![Line::default()]
        } else {
            body
        };
        let code_fg = theme::MD_CODE();
        let code_bg = theme::CODE_BG();
        // tui-markdown hardcodes syntect's dark `base16-ocean.dark` theme, whose
        // pastel palette is unreadable on a light code band. On light palettes
        // the highlighted fg is therefore dropped for the theme's own code role.
        let light_band = relative_luminance(code_bg) > 0.5;
        for line in body {
            let mut spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 2);
            spans.push(Span::styled("  ".to_string(), Style::default().bg(code_bg)));
            for mut span in line.spans {
                if span.style.fg.is_none() || light_band {
                    span.style.fg = Some(code_fg);
                }
                span.style = span.style.bg(code_bg);
                spans.push(span);
            }
            // A trailing pad cell keeps the band continuous past the last glyph.
            spans.push(Span::styled(" ".to_string(), Style::default().bg(code_bg)));
            self.out.push(Line::from(spans));
        }
        self.mark_emitted();
    }
}

/// A block's gutter: quote bars, indentation, and a list marker.
#[derive(Default, Clone)]
struct Prefix {
    quote: usize,
    indent: usize,
    marker: Option<Marker>,
}

impl Prefix {
    fn spans(&self) -> Vec<Span<'static>> {
        let mut spans: Vec<Span<'static>> = Vec::new();
        for _ in 0..self.quote {
            spans.push(Span::styled(
                "▏ ".to_string(),
                Style::default().fg(theme::MD_QUOTE()),
            ));
        }
        if self.indent > 0 {
            spans.push(Span::raw("  ".repeat(self.indent)));
        }
        match self.marker {
            Some(Marker::Bullet) => spans.push(Span::styled(
                "• ".to_string(),
                Style::default().fg(theme::MD_LIST()),
            )),
            Some(Marker::Ordered(n)) => spans.push(Span::styled(
                format!("{n}. "),
                Style::default().fg(theme::MD_LIST()),
            )),
            Some(Marker::Task(true)) => spans.push(Span::styled(
                "✓ ".to_string(),
                Style::default().fg(theme::SUCCESS()),
            )),
            Some(Marker::Task(false)) => spans.push(Span::styled(
                "☐ ".to_string(),
                // The list-marker role, not FAINT: FAINT falls under the 3.0
                // contrast floor on the light palettes (heavenly, pearl).
                Style::default().fg(theme::MD_LIST()),
            )),
            None => {}
        }
        spans
    }

    /// Same gutter minus the item marker - for continuation lines of a block.
    fn continuation(&self) -> Prefix {
        Prefix {
            quote: self.quote,
            indent: self.indent,
            marker: None,
        }
    }
}

/// Heading decoration by level: (marker, colour).
fn heading_style(level: u8) -> (Option<Span<'static>>, Color) {
    match level {
        1 => (
            Some(Span::styled(
                "▌ ".to_string(),
                Style::default().fg(theme::NUR_GOLD()),
            )),
            theme::MD_H1(),
        ),
        2 => (
            Some(Span::styled(
                "▍ ".to_string(),
                Style::default().fg(theme::MD_H2()),
            )),
            theme::MD_H2(),
        ),
        3 => (
            Some(Span::styled(
                "▪ ".to_string(),
                Style::default().fg(theme::MD_H3()),
            )),
            theme::MD_H3(),
        ),
        _ => (None, theme::MD_H3()),
    }
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

/// `## Title` / `Title ##` → `Title`. The crate re-adds a marker from the raw
/// source, so a heading slice is cleaned before it is handed over.
fn strip_heading_marker(slice: &str) -> &str {
    let trimmed = slice.trim_start();
    let after_hashes = trimmed.trim_start_matches('#');
    if after_hashes.len() == trimmed.len() {
        return slice;
    }
    after_hashes.trim_start().trim_end_matches('#').trim_end()
}

/// Remove the marker span the crate emits for a rendered heading line (`## `),
/// leaving its text. Handles the marker sharing a span with the text too.
fn strip_rendered_heading_marker(line: &mut Line<'static>) {
    let Some(first) = line.spans.first() else {
        return;
    };
    let content = first.content.as_ref();
    let hashes = content.len() - content.trim_start_matches('#').len();
    if hashes == 0 {
        return;
    }
    // Only a pure marker span ("## ") or a marker prefix ("## Title") counts;
    // a heading whose text legitimately starts with '#' ("#C#" style tags) is
    // left alone because its marker was already stripped from the source.
    let rest = content[hashes..].trim_start();
    if !rest.is_empty() && content[hashes..].starts_with(|c: char| c.is_whitespace()) {
        let trimmed = rest.to_string();
        line.spans[0] = Span::styled(trimmed, first.style);
    } else if rest.is_empty() {
        line.spans.remove(0);
    }
}

fn is_fence(line: &Line<'_>) -> bool {
    let text: String = line
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect::<String>();
    text.trim_start().starts_with("```")
}

/// A span carrying a bare URL (the crate renders links as `label (url)`). It
/// stays in the line so the click target survives, but it stops shouting.
fn looks_like_url(content: &str) -> bool {
    let t = content.trim_start_matches(['(', ' ', '<']);
    t.starts_with("http://") || t.starts_with("https://")
}

/// WCAG relative luminance of a colour, used to tell a light code band from a
/// dark one so syntax colours can be dropped where they would be unreadable.
fn relative_luminance(color: Color) -> f64 {
    // Palette backgrounds are always `Rgb`; anything else (transparent mode's
    // `Reset`, or a named colour) is treated as dark, which keeps highlighting.
    let (r, g, b) = match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    };
    let ch = |c: u8| {
        let c = f64::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
}

/// A section break: short and fixed, so it never wraps or reflows.
fn rule_line() -> Line<'static> {
    Line::from(Span::styled(
        "─".repeat(46),
        Style::default().fg(theme::BORDER()),
    ))
}

/// Translate `tui-markdown`'s stock colours into the Nur **transcript** palette.
/// Goal: clear structure (headings / links / code / quotes / lists) without
/// collapsing everything into white-or-gold. Modifiers stay as-is.
fn meta_palette(mut style: Style) -> Style {
    // Inline code / code blocks: mint-on-dark (not gold-on-dark).
    if style.bg == Some(Color::Black) {
        style.bg = Some(theme::CODE_BG());
        if matches!(style.fg, Some(Color::White) | None) {
            style.fg = Some(theme::MD_CODE());
        }
    }
    style.fg = match style.fg {
        Some(Color::Cyan) => Some(theme::MD_H2()),      // H2 / H3
        Some(Color::LightCyan) => Some(theme::MD_H3()), // H4-H6
        Some(Color::Green) => Some(theme::MD_QUOTE()),  // blockquotes
        Some(Color::Blue) => Some(theme::MD_LINK()),    // links
        Some(Color::LightBlue) => Some(theme::MD_LIST()), // list markers
        Some(Color::Yellow) | Some(Color::LightYellow) => Some(theme::AMBER()),
        Some(Color::Magenta) | Some(Color::LightMagenta) => Some(theme::LAVENDER()),
        Some(Color::Red) | Some(Color::LightRed) => Some(theme::ERROR()),
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
    fn headings_lose_their_markers_and_keep_their_text() {
        let text = flat(&render_markdown(
            "# Title\n\n## Sub\n\n### Deep",
            Style::default(),
        ));
        assert!(text.contains("Title") && text.contains("Sub") && text.contains("Deep"));
        assert!(
            !text.contains('#'),
            "heading markers must not render: {text:?}"
        );
    }

    #[test]
    fn quotes_render_a_bar_not_a_marker() {
        let text = flat(&render_markdown("> quoted line", Style::default()));
        assert!(text.contains("▏"), "quote bar expected: {text:?}");
        assert!(
            !text.contains('>'),
            "raw quote marker must not render: {text:?}"
        );
        assert!(text.contains("quoted line"));
    }

    #[test]
    fn fenced_code_drops_the_fences_and_keeps_the_body() {
        let out = render_markdown("```rust\nlet a = 1;\n```", Style::default());
        let text = flat(&out);
        assert!(text.contains("let a = 1;"), "code body kept: {text:?}");
        assert!(!text.contains("```"), "fences must not render: {text:?}");
    }

    #[test]
    fn lists_get_markers_and_task_boxes() {
        let out = render_markdown(
            "- one\n- two\n\n1. first\n\n- [x] done\n- [ ] todo",
            Style::default(),
        );
        let text = flat(&out);
        assert!(text.contains("• one") && text.contains("• two"), "{text:?}");
        assert!(text.contains("1. first"), "{text:?}");
        assert!(text.contains("✓ done"), "{text:?}");
        assert!(text.contains("☐ todo"), "{text:?}");
        assert!(
            !text.contains("[x]"),
            "checkbox syntax must not render: {text:?}"
        );
        assert!(
            !text.contains("\n- "),
            "dash bullets must be replaced: {text:?}"
        );
    }

    #[test]
    fn nested_lists_indent() {
        let out = render_markdown("- outer\n  - inner", Style::default());
        let lines: Vec<String> = out
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        let outer = lines.iter().find(|l| l.contains("outer")).expect("outer");
        let inner = lines.iter().find(|l| l.contains("inner")).expect("inner");
        assert!(
            inner.find("inner").unwrap() > outer.find("outer").unwrap(),
            "nested item must indent: {lines:?}"
        );
    }

    #[test]
    fn rules_and_spacing_between_blocks() {
        let flat_lines: Vec<String> = render_markdown("one\n\ntwo", Style::default())
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(
            flat_lines.iter().filter(|l| l.trim().is_empty()).count() >= 1,
            "blocks need a blank separator: {flat_lines:?}"
        );

        let rule = flat(&render_markdown("a\n\n---\n\nb", Style::default()));
        assert!(rule.contains('─'), "rule must render as a rule: {rule:?}");
        assert!(
            !rule.contains("---"),
            "raw rule marker must not render: {rule:?}"
        );
    }

    #[test]
    fn code_rows_carry_the_code_band() {
        let out = render_markdown("```\nx\n```", Style::default());
        let row = out
            .iter()
            .find(|l| l.spans.iter().any(|s| s.content.contains('x')))
            .expect("code row");
        assert!(
            row.spans
                .iter()
                .any(|s| s.style.bg == Some(theme::CODE_BG())),
            "code rows should sit on the code background"
        );
    }

    #[test]
    fn plain_text_gets_the_base_foreground() {
        let base = Style::default().fg(Color::Rgb(1, 2, 3));
        let out = render_markdown("just words", base);
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
    fn links_keep_their_url_for_the_click_target() {
        let text = flat(&render_markdown(
            "see [notes](https://example.test/a)",
            Style::default(),
        ));
        assert!(text.contains("notes"), "{text:?}");
        assert!(
            text.contains("https://example.test/a"),
            "the URL must stay visible so it remains clickable: {text:?}"
        );
    }

    #[test]
    fn empty_input_is_safe() {
        assert_eq!(render_markdown("", Style::default()).len(), 1);
    }

    #[test]
    fn blockquote_with_multiple_paragraphs_keeps_the_bar() {
        let out = render_markdown("> one\n>\n> two", Style::default());
        let bars = out
            .iter()
            .filter(|l| l.spans.iter().any(|s| s.content.contains('▏')))
            .count();
        assert!(bars >= 2, "each quote paragraph keeps its bar: {bars}");
    }

    #[test]
    fn heading_marker_stripper_handles_closed_and_bare_forms() {
        assert_eq!(strip_heading_marker("## Title"), "Title");
        assert_eq!(strip_heading_marker("Title ##"), "Title ##");
        assert_eq!(strip_heading_marker("#One"), "One");
        assert_eq!(strip_heading_marker("plain"), "plain");
    }

    /// Control syntax must never survive into the transcript, whatever shape
    /// the source takes. This is the battery that keeps the composer honest:
    /// every entry previously rendered as literal markdown punctuation.
    #[test]
    fn no_markdown_control_syntax_ever_reaches_the_screen() {
        let cases: &[(&str, &[&str])] = &[
            ("## Head", &["#"]),
            ("Head\n====", &["=", "#"]),
            ("Sub\n----", &["-"]),
            ("> quoted", &[">"]),
            ("```rust\nlet x = 1;\n```", &["```"]),
            ("- item", &[]),
            ("- [ ] todo", &["[ ]", "[x]"]),
            ("- [x] done", &["[x]"]),
            ("1. ordered", &[]),
            ("---", &["---"]),
            ("| a | b |\n| - | - |\n| 1 | 2 |", &[]),
            ("<div>html</div>", &[]),
        ];
        for (src, forbidden) in cases {
            let text = flat(&render_markdown(src, Style::default()));
            for needle in *forbidden {
                assert!(
                    !text.contains(needle),
                    "control syntax {needle:?} leaked from {src:?} -> {text:?}"
                );
            }
        }
    }

    /// Emphasis, code, links and breaks must still work after the block
    /// rewrite - the composer feeds the crate slices, it does not replace it.
    #[test]
    fn inline_styling_still_applies() {
        let out = render_markdown(
            "**bold** and *italic* and `code` and ~~gone~~",
            Style::default(),
        );
        let mods: Vec<Modifier> = out
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.style.add_modifier)
            .collect();
        assert!(mods.iter().any(|m| m.contains(Modifier::BOLD)), "{mods:?}");
        assert!(mods.iter().any(|m| m.contains(Modifier::ITALIC)), "{mods:?}");
        assert!(
            mods.iter().any(|m| m.contains(Modifier::CROSSED_OUT)),
            "strikethrough should survive: {mods:?}"
        );
        let text = flat(&out);
        assert!(!text.contains('*'), "emphasis markers must be consumed: {text:?}");
        assert!(!text.contains('~'), "strikethrough markers must be consumed: {text:?}");
    }

    /// Structure that nests must not lose its gutters.
    #[test]
    fn nested_quotes_and_lists_keep_their_structure() {
        let quoted_list = flat(&render_markdown("> - one\n> - two", Style::default()));
        assert!(quoted_list.contains('▏'), "quote bar kept: {quoted_list:?}");
        assert!(quoted_list.contains("• one"), "list inside quote: {quoted_list:?}");

        let list_with_quote = flat(&render_markdown("- item\n\n  > aside", Style::default()));
        assert!(list_with_quote.contains("• item"), "{list_with_quote:?}");
        assert!(list_with_quote.contains('▏'), "{list_with_quote:?}");
    }

    #[test]
    fn escaped_punctuation_is_not_treated_as_structure() {
        let text = flat(&render_markdown(r"\*not a bullet\*", Style::default()));
        assert!(text.contains("*not a bullet*"), "{text:?}");
    }
}
