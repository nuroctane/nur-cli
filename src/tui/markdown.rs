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
//! Tables are the one block laid out against the available width (see
//! [`Composer::emit_table`]): a grid at natural width when it fits, a grid
//! whose cells wrap inside their own column when it does not, and stacked
//! `Header: value` records when even narrow columns cannot fit. The wrapper
//! downstream never has to cut a table row, so a table reads cleanly at any
//! window size instead of breaking mid-row wherever the window happens to end.
//!
//! [`tui-markdown`]: https://github.com/joshka/tui-markdown

use crate::theme;
use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::ops::Range;
use unicode_width::UnicodeWidthStr;

/// Render markdown to styled lines with no width limit (tests and previews).
#[cfg(test)]
pub fn render_markdown(text: &str, base: Style) -> Vec<Line<'static>> {
    render_markdown_at(text, base, usize::MAX / 4)
}

/// Render markdown to styled lines for a body `width` columns wide. `base`
/// supplies the default foreground for plain (uncoloured) text - e.g. the
/// assistant message colour. Only tables use the width; everything else is
/// wrapped downstream.
pub fn render_markdown_at(text: &str, base: Style, width: usize) -> Vec<Line<'static>> {
    #[cfg(feature = "image-peek")]
    let annotated = crate::tui::latex::annotate_display_math(text);
    #[cfg(feature = "image-peek")]
    let text = annotated.as_str();

    let default_fg = base.fg.unwrap_or(theme::FG());
    let mut composer = Composer::new(text, default_fg, width.max(1));
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

/// A table being collected: cells arrive as inline runs between
/// `TableCell` events and are laid out once the whole table is known.
#[derive(Default)]
struct TableState {
    aligns: Vec<Alignment>,
    head: Vec<Vec<Span<'static>>>,
    rows: Vec<Vec<Vec<Span<'static>>>>,
    current: Vec<Vec<Span<'static>>>,
}

struct Composer<'a> {
    src: &'a str,
    base_fg: Color,
    /// Body width in columns; tables are laid out to fit inside it.
    width: usize,
    table: Option<TableState>,
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
    fn new(src: &'a str, base_fg: Color, width: usize) -> Self {
        Self {
            src,
            base_fg,
            width,
            table: None,
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
        options.insert(Options::ENABLE_TABLES);
        for (event, range) in Parser::new_ext(self.src, options).into_offset_iter() {
            self.event(event, range);
        }
        // Trailing inline text (a document ending in a tight item) still emits.
        self.flush_inline();
    }

    fn event(&mut self, event: Event<'_>, range: Range<usize>) {
        // A fenced block is emitted whole from its source range when it ends;
        // its text events must not also become an inline run, or a block that
        // ends the message renders a second, flattened copy.
        if self.code.is_some() && !matches!(event, Event::End(TagEnd::CodeBlock)) {
            return;
        }
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
            Event::Start(Tag::Table(aligns)) => {
                self.flush_inline();
                self.table = Some(TableState {
                    aligns,
                    ..Default::default()
                });
            }
            Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => {
                if let Some(t) = self.table.as_mut() {
                    t.current.clear();
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(t) = self.table.as_mut() {
                    t.head = std::mem::take(&mut t.current);
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.current);
                    t.rows.push(row);
                }
            }
            Event::Start(Tag::TableCell) => self.inline = None,
            Event::End(TagEnd::TableCell) => {
                let spans = match self.inline.take() {
                    Some(range) => self.cell_spans(&range),
                    None => Vec::new(),
                };
                if let Some(t) = self.table.as_mut() {
                    t.current.push(spans);
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(table) = self.table.take() {
                    self.emit_table(table);
                }
            }
            _ => {}
        }
    }

    /// One table cell's inline content as a single run of styled spans.
    fn cell_spans(&self, range: &Range<usize>) -> Vec<Span<'static>> {
        let (lines, _) = self.inline_lines(range);
        let mut spans: Vec<Span<'static>> = Vec::new();
        for (i, line) in lines.into_iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(" ".to_string()));
            }
            spans.extend(line.spans);
        }
        trim_spans(spans)
    }

    /// Lay a table out against the body width, in the first form that fits:
    /// a grid at natural width, a grid whose cells wrap inside their column,
    /// or one stacked `Header: value` record per row. Every emitted line fits
    /// the width, so the downstream wrapper never cuts through a row.
    fn emit_table(&mut self, table: TableState) {
        let TableState {
            aligns, head, rows, ..
        } = table;
        let cols = rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(head.len()))
            .max()
            .unwrap_or(0);
        if cols == 0 {
            return;
        }
        self.blank_before_block();
        let prefix = Prefix {
            quote: self.quote_depth,
            indent: self.item.as_ref().map(|i| i.depth).unwrap_or(0),
            marker: None,
        };
        let gutter = spans_width(&prefix.spans());
        let avail = self.width.saturating_sub(gutter).max(1);
        let header: Vec<Vec<Span<'static>>> = (0..cols)
            .map(|c| header_spans(head.get(c).cloned().unwrap_or_default()))
            .collect();
        let body: Vec<Vec<Vec<Span<'static>>>> = rows
            .iter()
            .map(|r| {
                (0..cols)
                    .map(|c| r.get(c).cloned().unwrap_or_default())
                    .collect()
            })
            .collect();
        match grid_widths(&header, &body, avail) {
            Some(widths) => {
                let wrapped = self.emit_grid_row(&prefix, &header, &widths, &aligns);
                self.out.push(table_rule(&prefix, &widths, false));
                let mut wrapped_any = wrapped;
                for (i, row) in body.iter().enumerate() {
                    if i > 0 && wrapped_any {
                        // Wrapped rows need a quiet divider or they run together.
                        self.out.push(table_rule(&prefix, &widths, true));
                    }
                    wrapped_any |= self.emit_grid_row(&prefix, row, &widths, &aligns);
                }
            }
            None => self.emit_stacked(&prefix, &header, &body, avail),
        }
        self.mark_emitted();
    }

    /// One grid row; returns whether any cell had to wrap onto extra lines.
    fn emit_grid_row(
        &mut self,
        prefix: &Prefix,
        cells: &[Vec<Span<'static>>],
        widths: &[usize],
        aligns: &[Alignment],
    ) -> bool {
        let wrapped: Vec<Vec<Vec<Span<'static>>>> = cells
            .iter()
            .zip(widths)
            .map(|(cell, w)| wrap_cell(cell, *w))
            .collect();
        let height = wrapped.iter().map(Vec::len).max().unwrap_or(1).max(1);
        for row in 0..height {
            let mut spans = prefix.spans();
            for (c, cell) in wrapped.iter().enumerate() {
                if c > 0 {
                    spans.push(Span::styled(
                        " │ ".to_string(),
                        Style::default().fg(table_rule_fg()),
                    ));
                }
                let content = cell.get(row).cloned().unwrap_or_default();
                let align = aligns.get(c).copied().unwrap_or(Alignment::None);
                spans.extend(pad_cell(content, widths[c], align));
            }
            self.out.push(Line::from(trim_trailing_pad(spans)));
        }
        height > 1
    }

    /// Narrow fallback: each row becomes a small record, the first field on a
    /// marker line and the rest under it, `Header: value` throughout.
    fn emit_stacked(
        &mut self,
        prefix: &Prefix,
        header: &[Vec<Span<'static>>],
        body: &[Vec<Vec<Span<'static>>>],
        avail: usize,
    ) {
        for (r, row) in body.iter().enumerate() {
            if r > 0 {
                self.out.push(Line::default());
            }
            for (c, cell) in row.iter().enumerate() {
                let mut field = header.get(c).cloned().unwrap_or_default();
                if spans_width(&field) > 0 {
                    field.push(Span::styled(
                        ": ".to_string(),
                        Style::default().fg(table_rule_fg()),
                    ));
                }
                field.extend(cell.iter().cloned());
                // Two columns of marker / hanging indent, the field wrapped
                // under it, so a record fits the width by construction too.
                for (i, part) in wrap_cell(&field, avail.saturating_sub(2).max(1))
                    .into_iter()
                    .enumerate()
                {
                    let mut spans = prefix.spans();
                    spans.push(if c == 0 && i == 0 {
                        Span::styled("▪ ".to_string(), Style::default().fg(theme::MD_LIST()))
                    } else {
                        Span::raw("  ".to_string())
                    });
                    spans.extend(part);
                    self.out.push(Line::from(spans));
                }
            }
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

/// Narrowest a column is squeezed to before the table stacks instead: below
/// this, most words break mid-word and a grid stops being readable.
const MIN_COLUMN: usize = 6;
/// The ` │ ` between columns.
const COLUMN_GAP: usize = 3;

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum()
}

/// Drop leading and trailing whitespace from a run of spans.
fn trim_spans(mut spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    while let Some(first) = spans.first_mut() {
        let trimmed = first.content.trim_start().to_string();
        if trimmed.is_empty() {
            spans.remove(0);
        } else {
            first.content = trimmed.into();
            break;
        }
    }
    while let Some(last) = spans.last_mut() {
        let trimmed = last.content.trim_end().to_string();
        if trimmed.is_empty() {
            spans.pop();
        } else {
            last.content = trimmed.into();
            break;
        }
    }
    spans
}

/// Header cells read as structure: bold, in the heading hue.
fn header_spans(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    spans
        .into_iter()
        .map(|mut s| {
            s.style = s.style.add_modifier(Modifier::BOLD);
            if s.style.bg.is_none() {
                s.style.fg = Some(theme::MD_H2());
            }
            s
        })
        .collect()
}

/// Tallest a wrapped cell may get before the table stacks instead: past this
/// a column has become a ribbon and a record reads better than a grid.
const MAX_CELL_LINES: usize = 4;

/// Column widths for a grid that fits `avail` columns, or `None` when the
/// table reads better stacked (see [`MIN_COLUMN`], [`MAX_CELL_LINES`]).
///
/// Natural widths are used when they fit. Otherwise the room is water-filled:
/// any column that fits within a fair share of what is left keeps its natural
/// width (short ids and numbers never break mid-word), and the long columns
/// share the remainder in proportion to their natural widths.
fn grid_widths(
    header: &[Vec<Span<'static>>],
    body: &[Vec<Vec<Span<'static>>>],
    avail: usize,
) -> Option<Vec<usize>> {
    let cols = header.len();
    let natural: Vec<usize> = (0..cols)
        .map(|c| {
            body.iter()
                .map(|r| spans_width(&r[c]))
                .chain(std::iter::once(spans_width(&header[c])))
                .max()
                .unwrap_or(0)
                .max(1)
        })
        .collect();
    let gaps = COLUMN_GAP * cols.saturating_sub(1);
    let budget = avail.checked_sub(gaps)?;
    if natural.iter().sum::<usize>() <= budget {
        return Some(natural);
    }
    let mut widths = vec![0usize; cols];
    let mut open: Vec<usize> = (0..cols).collect();
    let mut left = budget;
    while !open.is_empty() {
        let fair = left / open.len();
        let (fit, long): (Vec<usize>, Vec<usize>) = open.iter().partition(|c| natural[**c] <= fair);
        if fit.is_empty() {
            let total: usize = long.iter().map(|c| natural[*c]).sum::<usize>().max(1);
            let mut given = 0;
            for c in &long {
                widths[*c] = left * natural[*c] / total;
                given += widths[*c];
            }
            // Rounding leftovers go to the widest column.
            if let Some(widest) = long.iter().max_by_key(|c| natural[**c]) {
                widths[*widest] += left - given;
            }
            break;
        }
        for c in &fit {
            widths[*c] = natural[*c];
            left -= natural[*c];
        }
        open = long;
    }
    if (0..cols).any(|c| widths[c] < natural[c].min(MIN_COLUMN)) {
        return None;
    }
    let tallest = body
        .iter()
        .chain(std::iter::once(&header.to_vec()))
        .flat_map(|row| {
            row.iter()
                .zip(&widths)
                .map(|(cell, w)| wrap_cell(cell, *w).len())
        })
        .max()
        .unwrap_or(1);
    (tallest <= MAX_CELL_LINES).then_some(widths)
}

/// A cell's content broken into lines no wider than `width`.
fn wrap_cell(cell: &[Span<'static>], width: usize) -> Vec<Vec<Span<'static>>> {
    if spans_width(cell) <= width {
        return vec![cell.to_vec()];
    }
    let (lines, _) = crate::tui::wrap::wrap_lines_with_links(
        &[Line::from(cell.to_vec())],
        width.min(u16::MAX as usize) as u16,
    );
    lines.into_iter().map(|l| trim_spans(l.spans)).collect()
}

/// Pad one cell line to exactly `width` columns, honouring the column's
/// markdown alignment (`:--`, `:-:`, `--:`).
fn pad_cell(content: Vec<Span<'static>>, width: usize, align: Alignment) -> Vec<Span<'static>> {
    let used = spans_width(&content);
    let room = width.saturating_sub(used);
    let (left, right) = match align {
        Alignment::Right => (room, 0),
        Alignment::Center => (room / 2, room - room / 2),
        Alignment::Left | Alignment::None => (0, room),
    };
    let mut out = Vec::with_capacity(content.len() + 2);
    if left > 0 {
        out.push(Span::raw(" ".repeat(left)));
    }
    out.extend(content);
    if right > 0 {
        out.push(Span::raw(" ".repeat(right)));
    }
    out
}

/// A row's last cell needs no padding after it: trailing spaces would only
/// push the line toward the wrap edge.
fn trim_trailing_pad(mut spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    while let Some(last) = spans.last() {
        if last.content.trim().is_empty() && last.style.bg.is_none() {
            spans.pop();
        } else {
            break;
        }
    }
    spans
}

/// Table chrome (column bars, the header rule) uses the quote-bar role: it
/// is structure the eye has to follow across a row, so it must clear the
/// contrast floor in every theme, which the plain border role does not.
fn table_rule_fg() -> Color {
    theme::MD_QUOTE()
}

/// The rule under a table header (`─┼─`); `quiet` is the lighter divider
/// between wrapped body rows (thin glyphs, same role, so it still clears the
/// contrast floor).
fn table_rule(prefix: &Prefix, widths: &[usize], quiet: bool) -> Line<'static> {
    let (line, cross) = if quiet {
        ('╌', '┼')
    } else {
        ('─', '┼')
    };
    let mut text = String::new();
    for (c, w) in widths.iter().enumerate() {
        if c > 0 {
            text.push(line);
            text.push(cross);
            text.push(line);
        }
        text.extend(std::iter::repeat_n(line, *w));
    }
    let mut spans = prefix.spans();
    spans.push(Span::styled(text, Style::default().fg(table_rule_fg())));
    Line::from(spans)
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

    // Failure mode: a fenced block's text also lingers as a pending inline run,
    // so a block that ends the message (or precedes a list) renders twice,
    // the second time flattened onto one line.
    #[test]
    fn fenced_code_renders_once_wherever_it_sits() {
        for source in [
            "Result:\n\n```text\n2 + 3 * 4   ->  14\n10 - 6 / 2  ->  7\n```\n",
            "Result:\n\n```text\n2 + 3 * 4   ->  14\n10 - 6 / 2  ->  7\n```\n\n- next item\n",
            "```text\n2 + 3 * 4   ->  14\n10 - 6 / 2  ->  7\n```",
        ] {
            let text = flat(&render_markdown(source, Style::default()));
            assert_eq!(
                text.matches("->  14").count(),
                1,
                "{source:?} rendered {text:?}"
            );
            assert!(
                !text.contains("14 10"),
                "{source:?} flattened the block into a line: {text:?}"
            );
        }
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
        assert!(
            mods.iter().any(|m| m.contains(Modifier::ITALIC)),
            "{mods:?}"
        );
        assert!(
            mods.iter().any(|m| m.contains(Modifier::CROSSED_OUT)),
            "strikethrough should survive: {mods:?}"
        );
        let text = flat(&out);
        assert!(
            !text.contains('*'),
            "emphasis markers must be consumed: {text:?}"
        );
        assert!(
            !text.contains('~'),
            "strikethrough markers must be consumed: {text:?}"
        );
    }

    /// Structure that nests must not lose its gutters.
    #[test]
    fn nested_quotes_and_lists_keep_their_structure() {
        let quoted_list = flat(&render_markdown("> - one\n> - two", Style::default()));
        assert!(quoted_list.contains('▏'), "quote bar kept: {quoted_list:?}");
        assert!(
            quoted_list.contains("• one"),
            "list inside quote: {quoted_list:?}"
        );

        let list_with_quote = flat(&render_markdown("- item\n\n  > aside", Style::default()));
        assert!(list_with_quote.contains("• item"), "{list_with_quote:?}");
        assert!(list_with_quote.contains('▏'), "{list_with_quote:?}");
    }

    fn rows_at(src: &str, width: usize) -> Vec<String> {
        render_markdown_at(src, Style::default(), width)
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    const WIDE_TABLE: &str = "| Engine | Endpoint | Notes |\n|:--|:-:|--:|\n| d1-ultra | api.liquid.ai | Liquid's strongest decision model, multimodal and fluent in forty-four languages |\n| `amos-1` | **none** | keyless |";

    // Failure mode (the reported bug): a table row wider than the window, so
    // the wrapper cuts it mid-row and the columns fall apart. Every rendered
    // line must fit the width it was laid out for, at every width.
    #[test]
    fn table_rows_never_exceed_the_width() {
        for width in [12, 20, 30, 44, 60, 80, 120, 200] {
            for row in rows_at(WIDE_TABLE, width) {
                assert!(
                    UnicodeWidthStr::width(row.as_str()) <= width,
                    "width {width}: row {row:?} is {} wide",
                    UnicodeWidthStr::width(row.as_str())
                );
            }
        }
    }

    // Failure modes: pipe syntax and the dash delimiter row reaching the
    // screen, columns that do not line up, inline styling lost in a cell.
    #[test]
    fn a_table_that_fits_renders_as_an_aligned_grid() {
        let rows = rows_at("| a | bb |\n|---|---|\n| ccc | d |", 80);
        let rows: Vec<&String> = rows.iter().filter(|r| !r.trim().is_empty()).collect();
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert!(
            rows.iter().all(|r| !r.contains('|') && !r.contains("---")),
            "{rows:?}"
        );
        assert!(rows[1].contains('┼'), "header rule expected: {rows:?}");
        let bar = |r: &str| r.find('│').map(|i| UnicodeWidthStr::width(&r[..i]));
        assert_eq!(bar(rows[0]), bar(rows[2]), "columns must line up: {rows:?}");

        let styled = render_markdown_at("| x |\n|---|\n| **bold** `code` |", Style::default(), 80);
        let bold = styled
            .iter()
            .flat_map(|l| l.spans.iter())
            .any(|s| s.content.contains("bold") && s.style.add_modifier.contains(Modifier::BOLD));
        assert!(bold, "inline styling must survive inside a cell");
    }

    #[test]
    fn column_alignment_is_honoured() {
        let rows = rows_at(
            "| left | right | mid |\n|:--|--:|:-:|\n| a | b | c |\n| aaaa | bbbbb | ccc |",
            80,
        );
        let body = rows
            .iter()
            .find(|r| r.starts_with("a "))
            .expect("first body row");
        // `right` is 5 wide: a right-aligned "b" sits at the column's end.
        let cols: Vec<&str> = body.split(" │ ").collect();
        assert_eq!(cols[1], "    b", "{rows:?}");
        assert_eq!(cols[2].trim_end(), " c", "{rows:?}");
    }

    // Failure mode: a narrow window squeezes columns until every word
    // breaks mid-word. Below that point the table becomes stacked records.
    #[test]
    fn a_table_too_wide_for_any_grid_stacks_into_records() {
        let rows = rows_at(WIDE_TABLE, 20);
        let text = rows.join("\n");
        assert!(!text.contains('│'), "too narrow for a grid: {text}");
        assert!(text.contains("▪ Engine: d1-ultra"), "{text}");
        assert!(
            text.contains("Endpoint:") && text.contains("api.liquid.ai"),
            "{text}"
        );
    }

    #[test]
    fn a_shrunk_grid_wraps_long_cells_inside_their_column() {
        let rows = rows_at(WIDE_TABLE, 60);
        let grid: Vec<&String> = rows.iter().filter(|r| r.contains('│')).collect();
        assert!(
            grid.len() > 3,
            "the long notes cell must wrap onto more lines: {rows:?}"
        );
        let bars: Vec<Option<usize>> = grid
            .iter()
            .map(|r| r.find('│').map(|i| UnicodeWidthStr::width(&r[..i])))
            .collect();
        assert!(
            bars.windows(2).all(|w| w[0] == w[1]),
            "wrapped rows keep the column edge: {rows:?}"
        );
        // Short columns keep their natural width: only the long notes wrap,
        // nothing breaks mid-word.
        assert!(grid.iter().any(|r| r.starts_with("d1-ultra ")), "{rows:?}");
        assert!(grid.iter().any(|r| r.contains("api.liquid.ai")), "{rows:?}");
    }

    #[test]
    fn a_table_inside_a_quote_keeps_the_bar() {
        let rows = rows_at("> | a | b |\n> |---|---|\n> | 1 | 2 |", 80);
        let grid: Vec<&String> = rows
            .iter()
            .filter(|r| r.contains('│') || r.contains('┼'))
            .collect();
        assert!(
            !grid.is_empty() && grid.iter().all(|r| r.starts_with('▏')),
            "{rows:?}"
        );
    }

    #[test]
    fn escaped_punctuation_is_not_treated_as_structure() {
        let text = flat(&render_markdown(r"\*not a bullet\*", Style::default()));
        assert!(text.contains("*not a bullet*"), "{text:?}");
    }
}
