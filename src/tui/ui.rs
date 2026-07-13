//! Rendering for the Meta CLI TUI — Meta-blue surfaces, motion, cursors.

use super::app::{fmt_num, line_to_plain, App, Cell, TextRange};
use super::{markdown, wrap};
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    // Solid Meta-dark canvas so empty regions never flash terminal default.
    f.render_widget(
        Block::default().style(theme::style_canvas()),
        area,
    );

    let input_lines = app.input.line_count().min(6) as u16;
    let busy_h = if app.busy { 1 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(busy_h),
            Constraint::Length(input_lines + 2),
            Constraint::Length(1),
        ])
        .split(area);

    draw_transcript(f, app, chunks[0]);
    if app.busy {
        draw_busy_line(f, app, chunks[1]);
    }
    draw_input(f, app, chunks[2]); // publishes input_inner for click-to-caret
    draw_statusline(f, app, chunks[3]);

    if !app.palette_matches().is_empty() && app.approval.is_none() && app.picker.is_none() {
        draw_palette(f, app, chunks[2]);
    }
    if app.approval.is_some() {
        draw_approval(f, app, area);
    }
    if app.picker.is_some() {
        draw_session_picker(f, app, area);
    }
    // Grok-style hover dialogue over thoughts / tools / turns (above everything
    // except approval/picker, which already short-circuit interaction).
    if app.approval.is_none() && app.picker.is_none() {
        draw_hover_peek(f, app, area);
    }
}

// ── sessions picker (`/sessions` · `/resume` · Ctrl+R) ────────────────────
// Thick custom frame, rotating border accents + entry separators, entry-stable scroll.
fn draw_session_picker(f: &mut Frame, app: &mut App, area: Rect) {
    if app.picker.is_none() {
        return;
    }
    let phase = (app.spinner_epoch.elapsed().as_millis() / theme::SPINNER_MS) as usize;
    let spin = theme::SPINNER[phase % theme::SPINNER.len()];

    // Snapshot list data so we can mutate picker hit/scroll freely.
    let (total, this_cwd_only, mut sel, mut start) = {
        let p = app.picker.as_ref().unwrap();
        let total = p.visible().len();
        (total, p.this_cwd_only, p.idx, p.scroll)
    };

    let w = 82.min(area.width.saturating_sub(4)).max(54);
    let h = 22.min(area.height.saturating_sub(2)).max(12);
    let rect = Rect {
        x: (area.width.saturating_sub(w)) / 2,
        y: (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    f.render_widget(
        Block::default().style(Style::default().bg(theme::SURFACE_2)),
        rect,
    );

    let scope_label = if this_cwd_only { "here" } else { "all" };
    let title = format!(" {spin}  sessions  ·  {total} ");
    let footer = " ↑↓/wheel  ·  ↵ open  ·  tab scope  ·  esc/✕ ";
    draw_modal_frame(f, rect, phase, theme::META_BLUE, &title, Some(scope_label), footer);

    let pad = 2u16;
    let inner = Rect {
        x: rect.x.saturating_add(pad),
        y: rect.y.saturating_add(pad),
        width: rect.width.saturating_sub(pad * 2),
        height: rect.height.saturating_sub(pad * 2),
    };

    let close = Rect {
        x: rect.x + rect.width.saturating_sub(5),
        y: rect.y,
        width: 3,
        height: 1,
    };
    let scope = Rect {
        x: close.x.saturating_sub(8),
        y: rect.y,
        width: 7,
        height: 1,
    };

    let mut hit = super::app::PickerHit {
        frame: rect,
        close,
        body: inner,
        scope,
        rows: Vec::new(),
    };

    if total == 0 {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  nothing here  ·  ".to_string(), theme::style_faint()),
                Span::styled("tab / space".to_string(), theme::style_tool()),
                Span::styled("  show all workspaces".to_string(), theme::style_faint()),
            ]))
            .style(Style::default().bg(theme::SURFACE_2)),
            inner,
        );
        if let Some(p) = &mut app.picker {
            p.hit = hit;
            p.vis_page = 1;
        }
        return;
    }

    const CONTENT: usize = 2;
    const SEP: usize = 1;
    let stride = CONTENT + SEP;
    let body_h = inner.height as usize;
    let vis_rows = (body_h / stride).max(1);

    if let Some(p) = &mut app.picker {
        p.vis_page = vis_rows;
        p.clamp_scroll();
        sel = p.idx;
        start = p.scroll;
    }

    let col_w = (inner.width as usize).saturating_sub(4).max(20);
    let rows_snapshot: Vec<super::app::SessionRow> = app
        .picker
        .as_ref()
        .unwrap()
        .visible()
        .into_iter()
        .cloned()
        .collect();

    let mut lines: Vec<Line> = Vec::new();
    let mut drawn = 0usize;
    for (i, r) in rows_snapshot
        .iter()
        .enumerate()
        .skip(start)
        .take(vis_rows)
    {
        let selected = i == sel;
        let bg = if selected {
            theme::META_BLUE
        } else {
            theme::SURFACE_2
        };
        let prompt_fg = if selected { theme::BG } else { theme::FG };
        let meta_fg = if selected {
            theme::BLUE_100
        } else {
            theme::FAINT
        };
        let marker = if selected { "❯ " } else { "  " };

        let row_y = inner.y + (drawn as u16 * stride as u16);
        if row_y + 1 < inner.y + inner.height {
            hit.rows.push((
                i,
                Rect {
                    x: inner.x,
                    y: row_y,
                    width: inner.width,
                    height: CONTENT as u16,
                },
            ));
        }

        lines.push(Line::from(vec![
            Span::styled(
                marker.to_string(),
                Style::default()
                    .fg(if selected { theme::BG } else { theme::META_BLUE })
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                truncate(&r.preview, col_w),
                Style::default()
                    .fg(prompt_fg)
                    .bg(bg)
                    .add_modifier(if selected {
                        Modifier::BOLD
                    } else {
                        Modifier::empty()
                    }),
            ),
        ]));

        let short = &r.id[..8.min(r.id.len())];
        let place = if this_cwd_only {
            String::new()
        } else {
            format!("  ·  {}", short_path(&r.cwd))
        };
        let here = if r.here && !this_cwd_only {
            "  ·  here"
        } else {
            ""
        };
        let cost = if r.cost > 0.0 {
            format!("  ·  ${:.3}", r.cost)
        } else {
            String::new()
        };
        let meta = format!(
            "    {}  ·  {} msgs  ·  {} tok{cost}{place}{here}  ·  {short}",
            r.when,
            r.messages,
            fmt_num(r.tokens),
        );
        lines.push(Line::from(Span::styled(
            truncate(&meta, col_w + 2),
            Style::default().fg(meta_fg).bg(bg),
        )));
        lines.push(picker_separator_line(inner.width as usize, phase, i));
        drawn += 1;
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );

    if let Some(p) = &mut app.picker {
        p.hit = hit;
    }
}

/// Thick double-line frame with a traveling border accent (phase).
/// Shared ornate modal chrome (double border, traveling accent, inner hairline,
/// title + footer). Every dialog in the TUI is drawn through this so the picker,
/// command palette, and approval modal share one look. `hue` tints the border
/// (per-tool colour for approvals, Meta blue elsewhere); `right_label` draws the
/// `[label] ✕` cluster used by the sessions picker (None omits it).
fn draw_modal_frame(
    f: &mut Frame,
    rect: Rect,
    phase: usize,
    hue: Color,
    title: &str,
    right_label: Option<&str>,
    footer: &str,
) {
    let buf = f.buffer_mut();
    let border = Style::default().fg(hue).bg(theme::SURFACE_2);
    let accent = Style::default()
        .fg(theme::META_BLUE_SKY)
        .bg(theme::SURFACE_2)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(theme::BORDER).bg(theme::SURFACE_2);
    let title_s = Style::default()
        .fg(hue)
        .bg(theme::SURFACE_2)
        .add_modifier(Modifier::BOLD);
    let mute = Style::default().fg(theme::MUTED).bg(theme::SURFACE_2);

    let x0 = rect.x;
    let y0 = rect.y;
    let x1 = rect.x + rect.width.saturating_sub(1);
    let y1 = rect.y + rect.height.saturating_sub(1);
    if rect.width < 6 || rect.height < 4 {
        return;
    }

    // Corners (thick double)
    buf[(x0, y0)].set_char('╔').set_style(border);
    buf[(x1, y0)].set_char('╗').set_style(border);
    buf[(x0, y1)].set_char('╚').set_style(border);
    buf[(x1, y1)].set_char('╝').set_style(border);

    // Perimeter length for traveling accent
    let top_len = rect.width.saturating_sub(2) as usize;
    let side_len = rect.height.saturating_sub(2) as usize;
    let peri = top_len
        .saturating_mul(2)
        .saturating_add(side_len.saturating_mul(2))
        .max(1);
    let head = phase % peri;

    // Top / bottom edges
    for i in 0..top_len {
        let x = x0 + 1 + i as u16;
        let on = i == head || i == (head + peri / 3) % peri;
        let st = if on { accent } else { border };
        buf[(x, y0)].set_char('═').set_style(st);
        let bi = top_len + side_len + (top_len - 1 - i);
        let on_b = bi % peri == head || bi % peri == (head + peri / 3) % peri;
        buf[(x, y1)]
            .set_char('═')
            .set_style(if on_b { accent } else { border });
    }
    // Sides
    for i in 0..side_len {
        let y = y0 + 1 + i as u16;
        let ri = top_len + i;
        let li = top_len + side_len + top_len + (side_len - 1 - i);
        let on_r = ri % peri == head;
        let on_l = li % peri == head;
        buf[(x1, y)]
            .set_char('║')
            .set_style(if on_r { accent } else { border });
        buf[(x0, y)]
            .set_char('║')
            .set_style(if on_l { accent } else { border });
    }

    // Inner hairline (thicker feel)
    if rect.width > 4 && rect.height > 4 {
        let ix0 = x0 + 1;
        let iy0 = y0 + 1;
        let ix1 = x1 - 1;
        let iy1 = y1 - 1;
        buf[(ix0, iy0)].set_char('┌').set_style(dim);
        buf[(ix1, iy0)].set_char('┐').set_style(dim);
        buf[(ix0, iy1)].set_char('└').set_style(dim);
        buf[(ix1, iy1)].set_char('┘').set_style(dim);
        for x in (ix0 + 1)..ix1 {
            buf[(x, iy0)].set_char('─').set_style(dim);
            buf[(x, iy1)].set_char('─').set_style(dim);
        }
        for y in (iy0 + 1)..iy1 {
            buf[(ix0, y)].set_char('│').set_style(dim);
            buf[(ix1, y)].set_char('│').set_style(dim);
        }
    }

    // Title into top edge — reserve room on the right only when a label is shown.
    let reserve = if right_label.is_some() { 14 } else { 2 };
    let title_chars: Vec<char> = title.chars().collect();
    let max_t = top_len.saturating_sub(reserve).max(8);
    for (i, ch) in title_chars.iter().take(max_t).enumerate() {
        let x = x0 + 2 + i as u16;
        if x < x1 {
            buf[(x, y0)].set_char(*ch).set_style(title_s);
        }
    }
    // Scope + close on the right of top edge (picker only)
    if let Some(scope_label) = right_label {
        let right = format!(" [{scope_label}]  ✕ ");
        let rc: Vec<char> = right.chars().collect();
        let start_x = x1.saturating_sub(rc.len() as u16 + 1);
        for (i, ch) in rc.iter().enumerate() {
            let x = start_x + i as u16;
            if x > x0 && x < x1 {
                let st = if *ch == '✕' {
                    Style::default()
                        .fg(theme::ERROR)
                        .bg(theme::SURFACE_2)
                        .add_modifier(Modifier::BOLD)
                } else {
                    mute
                };
                buf[(x, y0)].set_char(*ch).set_style(st);
            }
        }
    }

    // Footer into bottom edge
    let fc: Vec<char> = footer.chars().collect();
    let max_f = top_len.saturating_sub(2);
    for (i, ch) in fc.iter().take(max_f).enumerate() {
        let x = x0 + 2 + i as u16;
        if x < x1 {
            buf[(x, y1)]
                .set_char(*ch)
                .set_style(Style::default().fg(theme::FAINT).bg(theme::SURFACE_2));
        }
    }
}

/// The content rect inside a `draw_modal_frame` (2-cell padding, matching the picker).
fn modal_inner(rect: Rect) -> Rect {
    let pad = 2u16;
    Rect {
        x: rect.x.saturating_add(pad),
        y: rect.y.saturating_add(pad),
        width: rect.width.saturating_sub(pad * 2),
        height: rect.height.saturating_sub(pad * 2),
    }
}

/// Modal animation phase (drives the traveling border accent), shared by all dialogs.
fn modal_phase(app: &App) -> usize {
    (app.spinner_epoch.elapsed().as_millis() / theme::SPINNER_MS) as usize
}

/// Soft rotating separator between session rows.
fn picker_separator_line(width: usize, phase: usize, row_i: usize) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }
    let glyphs = ['·', '─', '·', '·', '─', '·'];
    let mut s = String::with_capacity(width);
    s.push_str("  ");
    let w = width.saturating_sub(2);
    for i in 0..w {
        let g = glyphs[(i + phase + row_i * 2) % glyphs.len()];
        s.push(g);
    }
    Line::from(Span::styled(
        s,
        Style::default().fg(theme::BORDER).bg(theme::SURFACE_2),
    ))
}

// ── transcript ─────────────────────────────────────────────────────────────
fn draw_transcript(f: &mut Frame, app: &mut App, area: Rect) {
    let inner_w = area.width.saturating_sub(2).max(10);
    // Spinner frame bucket so animated cells re-wrap only when the glyph changes.
    let spin_i = (app.spinner_epoch.elapsed().as_millis() / theme::SPINNER_MS) as u64;

    // Per-cell wrap cache: finished rows are stable; live thinking/tools/stream
    // only recompute when content or spinner frame changes.
    if app.wrap_cache_width != inner_w || app.wrap_cache_keys.len() != app.cells.len() {
        app.wrap_cache_width = inner_w;
        app.wrap_cache_keys.clear();
        app.wrap_cache_parts.clear();
        app.wrap_cache_keys.resize(app.cells.len(), 0);
        app.wrap_cache_parts
            .resize_with(app.cells.len(), Vec::new);
    }
    // Grow if cells were appended without len mismatch (shouldn't happen).
    while app.wrap_cache_keys.len() < app.cells.len() {
        app.wrap_cache_keys.push(0);
        app.wrap_cache_parts.push(Vec::new());
    }

    let mut wrapped: Vec<Line<'static>> = Vec::new();
    let mut owner: Vec<Option<usize>> = Vec::new(); // index into `prompts`
    let mut is_prompt_head: Vec<bool> = Vec::new();
    let mut prompts: Vec<String> = Vec::new();
    let mut current: Option<usize> = None;

    // Rebuild hit-test maps: headers (click) + any peekable line (hover).
    let mut hit_headers: Vec<Option<usize>> = Vec::new();
    let mut line_cells: Vec<Option<usize>> = Vec::new();
    let mut plain_lines: Vec<String> = Vec::new();

    for (cell_idx, cell) in app.cells.iter().enumerate() {
        if let Cell::User(text) = cell {
            prompts.push(text.clone());
            current = Some(prompts.len() - 1);
        }
        let key = cell_wrap_key(cell, spin_i);
        let need = app.wrap_cache_keys.get(cell_idx).copied() != Some(key)
            || app
                .wrap_cache_parts
                .get(cell_idx)
                .map(|p| p.is_empty() && key != 0)
                .unwrap_or(true);
        if need {
            let mut cell_out: Vec<Line<'static>> = Vec::new();
            cell_lines(app, cell, cell_idx, &mut cell_out);
            let w = wrap::wrap_lines(&cell_out, inner_w);
            if let Some(slot) = app.wrap_cache_parts.get_mut(cell_idx) {
                *slot = w;
            }
            if let Some(k) = app.wrap_cache_keys.get_mut(cell_idx) {
                *k = key;
            }
        }
        let w = app
            .wrap_cache_parts
            .get(cell_idx)
            .cloned()
            .unwrap_or_default();
        let collapsible = cell.is_collapsible();
        let peekable = cell.is_peekable();
        let mut header_marked = false;
        for (i, line) in w.into_iter().enumerate() {
            // First non-empty line of a collapsible card is the click target.
            let empty = line
                .spans
                .iter()
                .all(|s| s.content.chars().all(|c| c.is_whitespace()));
            let is_header = collapsible && !header_marked && !empty;
            if is_header {
                header_marked = true;
            }
            plain_lines.push(line_to_plain(&line));
            wrapped.push(line);
            owner.push(current);
            // A User cell renders a blank spacer line then the prompt; either
            // being on screen means the prompt itself is visible.
            is_prompt_head.push(matches!(cell, Cell::User(_)) && i <= 1);
            hit_headers.push(if is_header { Some(cell_idx) } else { None });
            // Hover any non-blank line of a peekable card (incl. turn strip).
            line_cells.push(if peekable && !empty {
                Some(cell_idx)
            } else {
                None
            });
        }
    }
    app.hit_headers = hit_headers;
    app.line_cells = line_cells;
    app.plain_lines = plain_lines;

    let total = wrapped.len() as u16;
    let viewport = area.height;

    // Sticky banner takes rows off the body — max_scroll must use body height
    // or the thumb/drag math fights the sticky and feels janky.
    const STICKY_H: u16 = 3;
    // Pre-pass sticky using full viewport estimate, then refine.
    let max_scroll_full = total.saturating_sub(viewport);
    let top_guess = max_scroll_full.saturating_sub(app.scroll_from_bottom.min(max_scroll_full));
    let sticky_guess: bool = sticky_owner(
        &owner,
        &is_prompt_head,
        top_guess as usize,
        (top_guess as usize + viewport as usize).min(wrapped.len()),
    )
    .is_some();
    let sticky_h = if sticky_guess { STICKY_H } else { 0 };
    let body_h = viewport.saturating_sub(sticky_h);
    // Wide scrollbar rail (2 cols) so drag is easy to grab.
    let sb_w: u16 = 2;

    // Publish metrics for PageUp/Home + scrollbar drag (body, not full viewport).
    app.view_h = body_h;
    app.view_total = total;

    let max_scroll = total.saturating_sub(body_h);
    if app.scroll_from_bottom > max_scroll {
        app.scroll_from_bottom = max_scroll;
    }
    let top = max_scroll.saturating_sub(app.scroll_from_bottom);
    app.transcript_top = top;

    let vis_lo = top as usize;
    let vis_hi = (vis_lo + body_h as usize).min(wrapped.len());
    let sticky: Option<String> = sticky_owner(&owner, &is_prompt_head, vis_lo, vis_hi)
        .map(|oi| prompts[oi].clone());
    let sticky_h = if sticky.is_some() { STICKY_H } else { 0 };
    let body_y = area.y + sticky_h;
    let body_h = viewport.saturating_sub(sticky_h);
    // Re-sync if sticky appearance changed body height.
    app.view_h = body_h;
    let max_scroll = total.saturating_sub(body_h);
    if app.scroll_from_bottom > max_scroll {
        app.scroll_from_bottom = max_scroll;
    }
    let top = max_scroll.saturating_sub(app.scroll_from_bottom);
    app.transcript_top = top;

    let text_w = area.width.saturating_sub(2 + sb_w).max(10);

    let sel = app.selection;
    let visible: Vec<Line> = wrapped
        .into_iter()
        .enumerate()
        .skip(top as usize)
        .take(body_h as usize)
        .map(|(abs_i, line)| {
            if let Some(range) = sel {
                apply_selection_style(line, abs_i, range)
            } else {
                line
            }
        })
        .collect();

    let body_rect = Rect {
        x: area.x + 1,
        y: body_y,
        width: text_w,
        height: body_h,
    };
    app.transcript_body = body_rect;
    f.render_widget(Paragraph::new(visible).style(theme::style_canvas()), body_rect);

    if let Some(prompt) = sticky {
        draw_sticky_banner(
            f,
            &prompt,
            Rect {
                x: area.x,
                y: area.y,
                width: area.width.saturating_sub(sb_w),
                height: sticky_h,
            },
        );
    }

    // Draggable scrollbar on the right edge of the transcript.
    let track = Rect {
        x: area.right().saturating_sub(sb_w),
        y: body_y,
        width: sb_w,
        height: body_h,
    };
    app.scrollbar_track = track;
    draw_scrollbar(f, app, track, max_scroll, top, total, body_h);

    // Floating "↓ N · End" chip — click jumps to latest (hit-tested in App).
    if app.scroll_from_bottom > 0 && body_h > 0 {
        let tag = format!(" ↓ {} · End ", app.scroll_from_bottom);
        let w = tag.width() as u16;
        let r = Rect {
            x: area.right().saturating_sub(w + sb_w + 1),
            y: area.bottom().saturating_sub(1),
            width: w.min(area.width.saturating_sub(sb_w)),
            height: 1,
        };
        app.jump_chip = r;
        f.render_widget(
            Paragraph::new(Span::styled(
                tag,
                Style::default()
                    .fg(theme::BG)
                    .bg(theme::META_BLUE)
                    .add_modifier(Modifier::BOLD),
            )),
            r,
        );
    } else {
        app.jump_chip = Rect::default();
    }
}

/// Which prompt (if any) should be pinned above the viewport.
///
/// The prompt owning the topmost visible line — but only when that prompt's own
/// lines have scrolled off. If you can already see the prompt, pinning a copy of
/// it would just be noise.
fn sticky_owner(
    owner: &[Option<usize>],
    is_prompt_head: &[bool],
    vis_lo: usize,
    vis_hi: usize,
) -> Option<usize> {
    let oi = (*owner.get(vis_lo)?)?;
    let visible_on_screen = (vis_lo..vis_hi).any(|i| is_prompt_head[i] && owner[i] == Some(oi));
    if visible_on_screen {
        None
    } else {
        Some(oi)
    }
}

/// Full-width sticky prompt banner — 3 rows, high contrast, hard to miss.
fn draw_sticky_banner(f: &mut Frame, prompt: &str, area: Rect) {
    f.render_widget(Clear, area);
    let bar = Style::default().bg(theme::META_BLUE);
    let surface = Style::default().bg(theme::SURFACE);

    // Row 0: solid Meta-blue title bar.
    let title = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "  PROMPT  ".to_string(),
                Style::default()
                    .fg(theme::BG)
                    .bg(theme::META_BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " · scroll follows this turn ".to_string(),
                Style::default().fg(theme::BLUE_100).bg(theme::META_BLUE),
            ),
        ]))
        .style(bar),
        title,
    );

    // Rows 1..: prompt text, wrapped, on surface with left accent.
    if area.height >= 2 {
        let body = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1),
        };
        let text = prompt.replace('\n', " ");
        let avail = (area.width as usize).saturating_sub(4);
        // Split across body rows.
        let mut lines: Vec<Line> = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        let rows = body.height as usize;
        for r in 0..rows {
            if i >= chars.len() && r > 0 {
                break;
            }
            let end = (i + avail).min(chars.len());
            let chunk: String = chars[i..end].iter().collect();
            i = end;
            let prefix = if r == 0 { " ❯ " } else { "   " };
            lines.push(Line::from(vec![
                Span::styled(
                    prefix.to_string(),
                    Style::default()
                        .fg(theme::META_BLUE)
                        .bg(theme::SURFACE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    chunk,
                    Style::default()
                        .fg(theme::FG)
                        .bg(theme::SURFACE)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
            if i >= chars.len() {
                break;
            }
        }
        // Left accent bar via a full-row style.
        f.render_widget(Paragraph::new(lines).style(surface), body);
        // Bottom edge underline.
        if area.height >= 3 {
            let edge = Rect {
                x: area.x,
                y: area.bottom().saturating_sub(1),
                width: area.width,
                height: 1,
            };
            // Overpaint last body row border-style already content; draw a
            // thin rule using faint dashes under the last content if space.
            let _ = edge;
        }
    }
}

/// Vertical scrollbar: track + thumb. Drag handled in `App::on_mouse`.
fn draw_scrollbar(
    f: &mut Frame,
    app: &App,
    track: Rect,
    max_scroll: u16,
    top: u16,
    total: u16,
    viewport: u16,
) {
    if track.height == 0 || track.width == 0 {
        return;
    }
    // Always paint the track so users know it's there.
    let track_style = Style::default().fg(theme::BLUE_500).bg(theme::SURFACE);
    let empty: String = "│".to_string();
    let mut lines: Vec<Line> = (0..track.height)
        .map(|_| Line::from(Span::styled(empty.clone(), track_style)))
        .collect();

    if total > viewport && max_scroll > 0 {
        // Thumb size proportional to visible fraction; min 1 cell.
        let ratio = (viewport as f64 / total as f64).clamp(0.05, 1.0);
        let thumb_h = ((track.height as f64) * ratio).round().max(1.0) as u16;
        let thumb_h = thumb_h.min(track.height);
        // Position: top of content → thumb at top.
        let travel = track.height.saturating_sub(thumb_h);
        let pos = if max_scroll == 0 {
            0
        } else {
            ((top as f64 / max_scroll as f64) * travel as f64).round() as u16
        };
        let thumb_style = if app.scrollbar_drag {
            Style::default()
                .fg(theme::BG)
                .bg(theme::META_BLUE_SKY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(theme::BG)
                .bg(theme::META_BLUE)
                .add_modifier(Modifier::BOLD)
        };
        for row in pos..pos.saturating_add(thumb_h).min(track.height) {
            lines[row as usize] = Line::from(Span::styled("█".to_string(), thumb_style));
        }
    } else {
        // Nothing to scroll — faint full track.
        for line in &mut lines {
            *line = Line::from(Span::styled(
                "│".to_string(),
                Style::default().fg(theme::FAINT).bg(theme::BG),
            ));
        }
    }

    f.render_widget(Paragraph::new(lines), track);
}

fn cell_lines(app: &App, cell: &Cell, cell_idx: usize, out: &mut Vec<Line<'static>>) {
    let tick = app.spinner_epoch.elapsed();
    let flash = app
        .expand_flash
        .as_ref()
        .filter(|(i, t)| *i == cell_idx && t.elapsed().as_millis() < theme::SETTLE_MS)
        .map(|(_, t)| theme::settle_progress(t.elapsed(), theme::SETTLE_MS));
    match cell {
        Cell::Banner => banner_lines(app, out),
        Cell::User(text) => {
            out.push(Line::default());
            for (i, l) in text.lines().enumerate() {
                let prefix = if i == 0 { "❯ " } else { "  " };
                out.push(Line::from(vec![
                    Span::styled(
                        prefix.to_string(),
                        Style::default()
                            .fg(theme::META_BLUE)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(l.to_string(), theme::style_user()),
                ]));
            }
        }
        Cell::Assistant { text, streaming } => {
            out.push(Line::default());
            let md = markdown::render_markdown(text, theme::style_assistant());
            if md.is_empty() && *streaming {
                // Waiting for first token — quiet Meta pulse.
                out.push(Line::from(vec![
                    Span::styled(
                        "● ".to_string(),
                        Style::default().fg(theme::META_BLUE),
                    ),
                    Span::styled(
                        theme::pulse_frame(tick).to_string(),
                        Style::default().fg(theme::META_BLUE_SKY),
                    ),
                ]));
            }
            for (i, mut l) in md.into_iter().enumerate() {
                let prefix = if i == 0 {
                    Span::styled(
                        "● ".to_string(),
                        Style::default().fg(theme::META_BLUE),
                    )
                } else {
                    Span::raw("  ".to_string())
                };
                l.spans.insert(0, prefix);
                out.push(l);
            }
            if *streaming {
                // Blinking Meta block caret at end of stream.
                if let Some(last) = out.last_mut() {
                    if theme::blink_on(tick) {
                        last.spans.push(Span::styled("█".to_string(), theme::style_cursor_on()));
                    } else {
                        last.spans.push(Span::styled(
                            "▏".to_string(),
                            theme::style_cursor_off(),
                        ));
                    }
                }
            }
        }
        Cell::Thinking {
            text,
            active,
            started,
            duration,
            expanded,
        } => {
            out.push(Line::default());
            let live = *active;
            let dur_label = if live {
                theme::fmt_elapsed_live(started.elapsed())
            } else {
                duration
                    .map(theme::fmt_duration)
                    .unwrap_or_else(|| "—".into())
            };
            // Always show a real chevron when finished; spinner only while active.
            let chevron = if live {
                theme::spinner_frame(tick)
            } else if *expanded {
                theme::CHEVRON_EXPANDED
            } else {
                theme::CHEVRON_COLLAPSED
            };
            let head_hue = if flash.is_some() {
                theme::BLUE_100
            } else if live {
                theme::VIOLET
            } else {
                theme::MUTED
            };
            let lines_n = text.lines().filter(|l| !l.trim().is_empty()).count();
            let mut head = vec![
                Span::styled(
                    format!("{chevron} "),
                    Style::default().fg(head_hue).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "thought".to_string(),
                    Style::default()
                        .fg(if live { theme::VIOLET } else { theme::MUTED })
                        .add_modifier(if live {
                            Modifier::ITALIC
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::raw(" ".to_string()),
                // High-contrast duration chip — impossible to miss.
                Span::styled(
                    if live {
                        format!(" {dur_label} ")
                    } else {
                        format!(" took {dur_label} ")
                    },
                    theme::style_duration_chip(live),
                ),
            ];
            if !*expanded {
                head.push(Span::styled(
                    if lines_n > 0 {
                        format!("  ·  {lines_n} lines · click to peek · ▸ expands")
                    } else {
                        "  ·  click to peek · ▸ expands".to_string()
                    },
                    Style::default().fg(theme::FAINT),
                ));
            } else {
                head.push(Span::styled(
                    "  ·  click ▾ to collapse".to_string(),
                    Style::default().fg(theme::FAINT),
                ));
            }
            out.push(Line::from(head));

            // Completely collapsed by default: header only. Body only when expanded.
            if *expanded {
                if text.is_empty() {
                    out.push(Line::from(vec![
                        Span::raw("  ".to_string()),
                        Span::styled(
                            if live {
                                format!("{} thinking…", theme::pulse_frame(tick))
                            } else {
                                "(empty)".into()
                            },
                            theme::style_thinking_violet(),
                        ),
                    ]));
                } else {
                    for l in text.lines() {
                        out.push(Line::from(vec![
                            Span::raw("  ".to_string()),
                            Span::styled(l.to_string(), theme::style_thinking_violet()),
                        ]));
                    }
                }
            }
        }
        Cell::Tool {
            name,
            args,
            result,
            ok,
            started,
            duration,
            expanded,
        } => {
            out.push(Line::default());
            let hue = theme::tool_color(name);
            let running = ok.is_none();
            let dur_label = if running {
                theme::fmt_elapsed_live(started.elapsed())
            } else {
                duration
                    .map(theme::fmt_duration)
                    .unwrap_or_else(|| "—".into())
            };
            let chevron = if running {
                theme::spinner_frame(tick)
            } else if *expanded {
                theme::CHEVRON_EXPANDED
            } else {
                theme::CHEVRON_COLLAPSED
            };
            let status_glyph = match ok {
                None => None,
                Some(true) => Some(("✓", theme::SUCCESS)),
                Some(false) => Some(("✗", theme::ERROR)),
            };
            let chev_hue = if flash.is_some() {
                theme::BLUE_100
            } else if running {
                hue
            } else {
                theme::MUTED
            };
            let mut head_spans = vec![Span::styled(
                format!("{chevron} "),
                Style::default().fg(chev_hue).add_modifier(Modifier::BOLD),
            )];
            if let Some((g, c)) = status_glyph {
                head_spans.push(Span::styled(format!("{g} "), Style::default().fg(c)));
            }
            head_spans.push(Span::styled(
                name.clone(),
                Style::default().fg(hue).add_modifier(Modifier::BOLD),
            ));
            head_spans.push(Span::styled(
                format!("  {}", summarize_args(name, args)),
                theme::style_faint(),
            ));
            head_spans.push(Span::styled(
                format!("  ·  {}", theme::tool_family(name)),
                Style::default().fg(theme::FAINT),
            ));
            head_spans.push(Span::raw(" ".to_string()));
            head_spans.push(Span::styled(
                if running {
                    format!(" {dur_label} ")
                } else {
                    format!(" took {dur_label} ")
                },
                theme::style_duration_chip(running),
            ));
            if !*expanded {
                let extra = match result {
                    Some(r) => {
                        let n = r.lines().filter(|l| !l.trim().is_empty()).count();
                        if n > 0 {
                            format!("  ·  {n} lines · hover peek · click ▸")
                        } else {
                            "  ·  hover peek · click ▸".into()
                        }
                    }
                    None => "  ·  hover peek · click ▸".into(),
                };
                head_spans.push(Span::styled(extra, Style::default().fg(theme::FAINT)));
            } else {
                head_spans.push(Span::styled(
                    "  ·  click ▾ to collapse".to_string(),
                    Style::default().fg(theme::FAINT),
                ));
            }
            out.push(Line::from(head_spans));

            // Completely collapsed by default — no preview rows.
            if *expanded {
                match result {
                    None => out.push(Line::from(vec![
                        Span::raw("  ".to_string()),
                        Span::styled(
                            format!("{} running", theme::pulse_frame(tick)),
                            Style::default().fg(theme::META_BLUE_SKY),
                        ),
                    ])),
                    Some(r) => {
                        let all: Vec<&str> = r.lines().filter(|l| !l.trim().is_empty()).collect();
                        if all.is_empty() {
                            out.push(Line::from(vec![
                                Span::raw("  ".to_string()),
                                Span::styled("(no output)".to_string(), theme::style_faint()),
                            ]));
                        } else {
                            for (i, l) in all.iter().enumerate() {
                                let prefix = if i == 0 { "└ " } else { "  " };
                                out.push(Line::from(vec![
                                    Span::raw("  ".to_string()),
                                    Span::styled(prefix.to_string(), theme::style_faint()),
                                    Span::styled(truncate(l, 200), theme::style_faint()),
                                ]));
                            }
                        }
                    }
                }
            }
        }
        Cell::TurnDone {
            duration,
            thought,
            interrupted,
        } => {
            out.push(Line::default());
            let (glyph, label) = if *interrupted {
                ("◼", "turn cancelled")
            } else {
                ("✓", "turn")
            };
            let d = theme::fmt_duration(*duration);
            let th = theme::fmt_duration(*thought);
            let mut spans = vec![
                Span::styled(
                    format!("{glyph} "),
                    Style::default()
                        .fg(if *interrupted {
                            theme::WARN
                        } else {
                            theme::SUCCESS
                        })
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(label.to_string(), Style::default().fg(theme::MUTED)),
                Span::raw(" ".to_string()),
                Span::styled(
                    format!(" took {d} "),
                    theme::style_turn_chip(*interrupted),
                ),
            ];
            // Always post thought timer at end of finished output.
            spans.push(Span::raw(" ".to_string()));
            spans.push(Span::styled(
                format!(" thought {th} "),
                theme::style_duration_chip(false),
            ));
            spans.push(Span::styled(
                "  ·  click to peek".to_string(),
                Style::default().fg(theme::FAINT),
            ));
            out.push(Line::from(spans));
        }
        Cell::Info { text, tone } => {
            out.push(Line::default());
            let hue = tone.color();
            for (i, l) in text.lines().enumerate() {
                let (prefix, style) = if i == 0 {
                    // First line carries the tone: glyph + colour + emphasis.
                    (
                        format!("{} ", tone.glyph()),
                        Style::default().fg(hue).add_modifier(Modifier::BOLD),
                    )
                } else {
                    ("  ".to_string(), theme::style_status())
                };
                out.push(Line::from(vec![
                    Span::styled(prefix, Style::default().fg(hue)),
                    Span::styled(l.to_string(), style),
                ]));
            }
        }
        Cell::Error(text) => {
            out.push(Line::default());
            for (i, l) in text.lines().enumerate() {
                let prefix = if i == 0 { "✗ " } else { "  " };
                out.push(Line::from(vec![
                    Span::styled(prefix.to_string(), theme::style_error()),
                    Span::styled(l.to_string(), theme::style_error()),
                ]));
            }
        }
    }
}

/// Highlight drag-selected characters with a Meta-blue selection wash.
fn apply_selection_style(line: Line<'static>, line_idx: usize, range: TextRange) -> Line<'static> {
    let (a, b) = range.normalized();
    if line_idx < a.line || line_idx > b.line {
        return line;
    }
    let plain = line_to_plain(&line);
    let chars: Vec<char> = plain.chars().collect();
    if chars.is_empty() {
        return line;
    }
    let (from, to) = if a.line == b.line {
        (a.col.min(chars.len()), b.col.min(chars.len()))
    } else if line_idx == a.line {
        (a.col.min(chars.len()), chars.len())
    } else if line_idx == b.line {
        (0, b.col.min(chars.len()))
    } else {
        (0, chars.len())
    };
    if from >= to {
        return line;
    }
    // Rebuild the line as three runs: before · selected · after.
    // Selected uses inverted Meta blue so it reads like OS selection.
    let before: String = chars[..from].iter().collect();
    let mid: String = chars[from..to].iter().collect();
    let after: String = chars[to..].iter().collect();
    let mut spans = Vec::new();
    if !before.is_empty() {
        // Keep first original style if present for the unselected prefix.
        if let Some(s0) = line.spans.first() {
            spans.push(Span::styled(before, s0.style));
        } else {
            spans.push(Span::raw(before));
        }
    }
    spans.push(Span::styled(
        mid,
        Style::default()
            .fg(theme::BG)
            .bg(theme::META_BLUE)
            .add_modifier(Modifier::BOLD),
    ));
    if !after.is_empty() {
        if let Some(s0) = line.spans.last() {
            spans.push(Span::styled(after, s0.style));
        } else {
            spans.push(Span::raw(after));
        }
    }
    Line::from(spans)
}

/// Floating dialogue: full thought / tool / turn content.
///
/// Uses click-pinned peek first (always works). Free hover only when the
/// terminal emits all-motion mouse events (we enable CSI ?1003h for that).
fn draw_hover_peek(f: &mut Frame, app: &App, area: Rect) {
    let Some(idx) = app.active_peek_cell() else { return };
    let Some(cell) = app.cells.get(idx) else { return };
    if !cell.is_peekable() {
        return;
    }
    // If already expanded in-place, skip the floating box (content is visible).
    if cell.is_collapsible() && cell.expanded() {
        return;
    }
    let Some(title) = cell.peek_title() else { return };
    let body = cell.peek_body().unwrap_or_default();
    let pinned = app.peek_pinned == Some(idx);

    let max_w = (area.width.saturating_mul(7) / 10).clamp(40, 96);
    let max_h = (area.height.saturating_mul(6) / 10).clamp(8, 28);
    let w = max_w.min(area.width.saturating_sub(4));
    let h = max_h.min(area.height.saturating_sub(2));
    if w < 20 || h < 5 {
        return;
    }

    // Pinned: center-ish; hover: anchor near mouse.
    let (mut x, mut y) = if pinned {
        (
            area.width.saturating_sub(w) / 2,
            area.height.saturating_sub(h) / 3,
        )
    } else {
        (
            app.mouse_col.saturating_add(2),
            app.mouse_row.saturating_add(1),
        )
    };
    if x + w > area.width {
        x = area.width.saturating_sub(w);
    }
    if y + h > area.height {
        y = if pinned {
            area.height.saturating_sub(h)
        } else {
            app.mouse_row.saturating_sub(h)
        };
    }
    if y + h > area.height {
        y = area.height.saturating_sub(h);
    }
    let rect = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    f.render_widget(
        Block::default().style(Style::default().bg(theme::SURFACE_2)),
        rect,
    );

    let border_hue = match cell {
        Cell::Thinking { .. } => theme::VIOLET,
        Cell::Tool { name, .. } => theme::tool_color(name),
        Cell::TurnDone { interrupted, .. } => {
            if *interrupted {
                theme::WARN
            } else {
                theme::SUCCESS
            }
        }
        _ => theme::META_BLUE,
    };

    let footer = if pinned {
        "  esc close  ·  e expand in place  ·  click ▸ chevron  "
    } else {
        "  click card to pin  ·  esc close  ·  e expand  "
    };
    let phase = modal_phase(app);
    draw_modal_frame(
        f,
        rect,
        phase,
        border_hue,
        &format!(" {title} "),
        None,
        footer,
    );
    let inner = modal_inner(rect);

    let mut lines: Vec<Line> = Vec::new();
    let max_lines = inner.height as usize;
    let max_cols = (inner.width as usize).saturating_sub(1).max(8);
    for (i, raw) in body.lines().enumerate() {
        if i >= max_lines.saturating_sub(1) {
            lines.push(Line::from(Span::styled(
                format!("… +more · click ▸ to expand"),
                Style::default().fg(theme::FAINT),
            )));
            break;
        }
        // Soft wrap long lines into the dialogue.
        let mut rest = raw;
        let mut first = true;
        while !rest.is_empty() {
            if lines.len() >= max_lines.saturating_sub(1) {
                lines.push(Line::from(Span::styled(
                    "…".to_string(),
                    Style::default().fg(theme::FAINT),
                )));
                break;
            }
            let take = rest
                .chars()
                .take(max_cols)
                .collect::<String>()
                .chars()
                .count();
            let chunk: String = rest.chars().take(take).collect();
            let advanced = chunk.len();
            rest = if advanced >= rest.len() {
                ""
            } else {
                &rest[advanced..]
            };
            let style = if matches!(cell, Cell::Thinking { .. }) {
                theme::style_thinking_violet()
            } else {
                Style::default().fg(theme::FG)
            };
            lines.push(Line::from(Span::styled(
                if first {
                    chunk
                } else {
                    format!("  {chunk}")
                },
                style,
            )));
            first = false;
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(empty)".to_string(),
            theme::style_faint(),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );
}

fn banner_lines(app: &App, out: &mut Vec<Line<'static>>) {
    let logo = [
        r#"███╗   ███╗██╗   ██╗███████╗███████╗"#,
        r#"████╗ ████║██║   ██║██╔════╝██╔════╝"#,
        r#"██╔████╔██║██║   ██║███████╗█████╗  "#,
        r#"██║╚██╔╝██║██║   ██║╚════██║██╔══╝  "#,
        r#"██║ ╚═╝ ██║╚██████╔╝███████║███████╗"#,
        r#"╚═╝     ╚═╝ ╚═════╝ ╚══════╝╚══════╝"#,
    ];
    out.push(Line::default());
    for (i, row) in logo.iter().enumerate() {
        let (r, g, b) = theme::GRADIENT[i.min(theme::GRADIENT.len() - 1)];
        out.push(Line::from(Span::styled(
            format!("  {row}"),
            Style::default().fg(Color::Rgb(r, g, b)),
        )));
    }
    // Model-agnostic title row + feature-loaded subtitle (not model-tied).
    let model_label = crate::config::model_display_name(&app.cfg.model);
    out.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled(model_label, theme::style_title()),
        Span::styled("  ·  ".to_string(), theme::style_faint()),
        Span::styled("Meta Model API".to_string(), theme::style_status()),
        Span::styled("  ·  ".to_string(), theme::style_faint()),
        Span::styled(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            theme::style_faint(),
        ),
    ]));
    out.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled(
            "fully loaded  ·  streaming TUI · tools · sandbox · subagents".to_string(),
            theme::style_faint(),
        ),
    ]));
    out.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled(
            "Graphify · PLUR · Ruflo · Executor · 800+ skills  ·  unofficial"
                .to_string(),
            theme::style_faint(),
        ),
    ]));
    out.push(Line::default());
    out.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled("model  ".to_string(), theme::style_faint()),
        Span::styled(app.cfg.model.clone(), Style::default().fg(theme::META_BLUE_SKY)),
        Span::styled("    cwd  ".to_string(), theme::style_faint()),
        Span::styled(app.cwd.display().to_string(), theme::style_status()),
    ]));
    out.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled(
            "/help  ·  drag-select  ·  scrollbar  ·  peek cards  ·  timers  ·  Shift+Tab  ·  Esc"
                .to_string(),
            theme::style_faint(),
        ),
    ]));
    out.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled(
            format!(
                "mode  {}  —  {}",
                app.permission_mode.get().badge(),
                app.permission_mode.get().description()
            ),
            Style::default().fg(theme::META_BLUE_SKY),
        ),
    ]));
    out.push(Line::default());
}

// ── busy line ──────────────────────────────────────────────────────────────
fn draw_busy_line(f: &mut Frame, app: &App, area: Rect) {
    let tick = app.spinner_epoch.elapsed();
    let elapsed = app.turn_started.elapsed();
    let live = theme::fmt_elapsed_live(elapsed);
    let mut spans = vec![Span::raw(" ".to_string())];

    if app.cancelling {
        // Distinct "stopping" chrome — not a happy thinking spinner.
        spans.push(Span::styled(
            "◼ ".to_string(),
            Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!("cancelling…  {live}  "),
            Style::default().fg(theme::WARN),
        ));
        spans.push(Span::styled(
            "waiting for in-flight work".to_string(),
            theme::style_faint(),
        ));
    } else {
        let spin = theme::spinner_frame(tick);
        spans.push(Span::styled(
            spin.to_string(),
            Style::default()
                .fg(theme::META_BLUE)
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!("  {}  ", capitalize(&app.status)),
            Style::default().fg(theme::META_BLUE_SKY),
        ));
        spans.push(Span::styled(live, theme::style_faint()));
        // Decorative ease-out activity strip (perceived motion, not real %).
        let bar_w = 12usize.min(area.width.saturating_sub(48) as usize);
        if bar_w >= 6 {
            spans.push(Span::styled("  ".to_string(), theme::style_faint()));
            spans.push(Span::styled(
                theme::activity_bar(elapsed, bar_w),
                Style::default().fg(theme::BLUE_500),
            ));
        }
        spans.push(Span::styled(
            "  ·  esc cancel".to_string(),
            theme::style_faint(),
        ));
        if !app.queue.is_empty() {
            spans.push(Span::styled(
                format!("  ·  {} queued", app.queue.len()),
                Style::default().fg(theme::WARN),
            ));
        }
    }
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::style_canvas()),
        area,
    );
}

// ── input ──────────────────────────────────────────────────────────────────
fn draw_input(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = !app.busy && app.approval.is_none();
    let border_color = if app.approval.is_some() {
        theme::BORDER
    } else if app.busy {
        theme::BORDER
    } else if focused {
        theme::META_BLUE
    } else {
        theme::BORDER
    };

    let title = if app.busy {
        let t = if app.queue.is_empty() {
            " meta · working ".to_string()
        } else {
            format!(" meta · working · {} queued ", app.queue.len())
        };
        Span::styled(t, theme::style_faint())
    } else {
        Span::styled(
            " meta ",
            Style::default()
                .fg(theme::META_BLUE)
                .add_modifier(Modifier::BOLD),
        )
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color))
        .style(theme::style_surface())
        .title(title);
    let inner = block.inner(area);
    f.render_widget(block, area);

    let text = app.input.text();
    let focused = app.approval.is_none() && app.picker.is_none();
    let sel = app.input.selection_range();
    let mut lines: Vec<Line> = Vec::new();
    let sel_style = Style::default()
        .fg(theme::BG)
        .bg(theme::META_BLUE_SKY)
        .add_modifier(Modifier::BOLD);
    let normal = Style::default().fg(theme::FG);

    // Absolute char index at the start of each line.
    let mut line_starts: Vec<usize> = vec![0];
    {
        let mut acc = 0usize;
        for (i, ch) in text.chars().enumerate() {
            if ch == '\n' {
                line_starts.push(i + 1);
            }
            acc = i + 1;
        }
        let _ = acc;
    }

    if text.is_empty() {
        let hint = if app.busy {
            "type a follow-up — Enter queues it"
        } else {
            "plan, build, debug  ·  click to place caret  ·  Ctrl+A/C/V"
        };
        let mut spans = vec![Span::styled(
            "❯ ".to_string(),
            Style::default()
                .fg(theme::META_BLUE)
                .add_modifier(Modifier::BOLD),
        )];
        spans.push(Span::styled(
            " ".to_string(),
            if focused {
                theme::style_cursor_on()
            } else {
                Style::default()
            },
        ));
        spans.push(Span::styled(format!(" {hint}"), theme::style_faint()));
        lines.push(Line::from(spans));
    } else {
        let (cur_line, cur_col) = app.input.cursor_line_col();
        for (i, l) in text.split('\n').enumerate() {
            let prefix = if i == 0 { "❯ " } else { "  " };
            let mut spans = vec![Span::styled(
                prefix.to_string(),
                Style::default()
                    .fg(theme::META_BLUE)
                    .add_modifier(Modifier::BOLD),
            )];
            let base = line_starts.get(i).copied().unwrap_or(0);
            let chars: Vec<char> = l.chars().collect();
            // Paint selection + caret per character.
            let mut run = String::new();
            let mut run_sel = false;
            let flush = |run: &mut String, run_sel: bool, spans: &mut Vec<Span>| {
                if run.is_empty() {
                    return;
                }
                let style = if run_sel { sel_style } else { normal };
                spans.push(Span::styled(std::mem::take(run), style));
            };
            for (ci, ch) in chars.iter().enumerate() {
                let abs = base + ci;
                let is_sel = sel.map(|(lo, hi)| abs >= lo && abs < hi).unwrap_or(false);
                let is_caret = focused && i == cur_line && ci == cur_col;
                if is_caret {
                    flush(&mut run, run_sel, &mut spans);
                    run_sel = false;
                    spans.push(Span::styled(
                        ch.to_string(),
                        if is_sel {
                            Style::default()
                                .fg(theme::BG)
                                .bg(theme::META_BLUE)
                                .add_modifier(Modifier::BOLD)
                        } else {
                            theme::style_cursor_on()
                        },
                    ));
                } else if is_sel != run_sel && !run.is_empty() {
                    flush(&mut run, run_sel, &mut spans);
                    run_sel = is_sel;
                    run.push(*ch);
                } else {
                    if run.is_empty() {
                        run_sel = is_sel;
                    }
                    run.push(*ch);
                }
            }
            // Caret past end of line.
            if focused && i == cur_line && cur_col >= chars.len() {
                flush(&mut run, run_sel, &mut spans);
                spans.push(Span::styled(" ".to_string(), theme::style_cursor_on()));
            } else {
                flush(&mut run, run_sel, &mut spans);
            }
            lines.push(Line::from(spans));
        }
    }

    let (cur_line, cur_col) = app.input.cursor_line_col();
    let h = inner.height as usize;
    let top = cur_line.saturating_sub(h.saturating_sub(1));

    let cur_disp_w: usize = text
        .split('\n')
        .nth(cur_line)
        .map(|l| l.chars().take(cur_col).collect::<String>().width())
        .unwrap_or(cur_col);
    let usable = (inner.width as usize).saturating_sub(3);
    let x_off = cur_disp_w.saturating_sub(usable) as u16;

    let visible: Vec<Line> = lines.into_iter().skip(top).take(h).collect();
    f.render_widget(
        Paragraph::new(visible)
            .scroll((0, x_off))
            .style(theme::style_surface()),
        inner,
    );

    if app.approval.is_none() && app.picker.is_none() {
        let cx = inner.x + 2 + (cur_disp_w as u16).saturating_sub(x_off);
        let cy = inner.y + (cur_line - top) as u16;
        if cx < inner.right() && cy < inner.bottom() {
            f.set_cursor_position((cx, cy));
        }
    }

    // Geometry for click-to-caret / next mouse frame.
    app.input_inner = inner;
    app.input_scroll_top = top;
    app.input_x_off = x_off;
}

// ── statusline ─────────────────────────────────────────────────────────────
fn draw_statusline(f: &mut Frame, app: &App, area: Rect) {
    let u = &app.u_session;
    let ctx_used = app.u_last.input_tokens + app.u_last.output_tokens;
    let ctx_pct = if app.cfg.context_window > 0 {
        (ctx_used as f64 / app.cfg.context_window as f64 * 100.0).min(100.0)
    } else {
        0.0
    };
    let ctx_style = if ctx_pct >= 80.0 {
        theme::style_error()
    } else if ctx_pct >= 60.0 {
        theme::style_warn()
    } else {
        theme::style_status()
    };

    let tick = app.spinner_epoch.elapsed();
    let state_dot = if app.cancelling {
        Span::styled("◼ ".to_string(), Style::default().fg(theme::WARN))
    } else if app.busy {
        Span::styled(
            format!("{} ", theme::spinner_frame(tick)),
            Style::default().fg(theme::META_BLUE),
        )
    } else {
        Span::styled("● ".to_string(), theme::style_success())
    };

    // Each metric gets its own hue from the standard ramp so the statusline is
    // scannable at a glance instead of one grey run-on.
    let sep = || Span::styled("  ·  ".to_string(), Style::default().fg(theme::BLUE_500));
    let left = vec![
        Span::raw(" ".to_string()),
        state_dot,
        Span::styled(
            format!("{} tok", fmt_num(u.total_tokens)),
            Style::default().fg(theme::BLUE_200),
        ),
        sep(),
        Span::styled(
            format!("${:.4}", u.estimated_cost_usd()),
            Style::default().fg(theme::TEAL),
        ),
        sep(),
        Span::styled(format!("ctx {ctx_pct:.0}%"), ctx_style),
    ];

    let quitting = app
        .quit_armed
        .map(|t| t.elapsed().as_secs() < 2)
        .unwrap_or(false);

    let right: Vec<Span> = if quitting {
        vec![Span::styled(
            "ctrl+c again to quit ".to_string(),
            Style::default()
                .fg(theme::WARN)
                .add_modifier(Modifier::BOLD),
        )]
    } else {
        let mode = app.permission_mode.get();
        let state = if app.cancelling {
            ("cancelling", theme::WARN)
        } else if app.busy {
            (app.status.as_str(), theme::BLUE_300)
        } else {
            ("ready", theme::SUCCESS)
        };
        vec![
            Span::styled(app.cfg.model.clone(), Style::default().fg(theme::BLUE_300)),
            // Reasoning effort rides with the model, violet like the thought cards.
            Span::styled(" · ".to_string(), Style::default().fg(theme::FAINT)),
            Span::styled(
                effort_label(&app.cfg.reasoning_effort),
                Style::default().fg(theme::VIOLET),
            ),
            sep(),
            // Mode is the thing you most need to be sure of before a tool runs.
            Span::styled(
                mode.label().to_string(),
                Style::default()
                    .fg(theme::INDIGO)
                    .add_modifier(Modifier::BOLD),
            ),
            sep(),
            Span::styled(
                app.session_id[..8.min(app.session_id.len())].to_string(),
                Style::default().fg(theme::FAINT),
            ),
            sep(),
            Span::styled(state.0.to_string(), Style::default().fg(state.1)),
            Span::raw(" ".to_string()),
        ]
    };

    let left_w: usize = left.iter().map(|s| s.content.width()).sum();
    let right_w: usize = right.iter().map(|s| s.content.width()).sum();
    let pad = (area.width as usize).saturating_sub(left_w + right_w);

    let mut spans = left;
    spans.push(Span::raw(" ".repeat(pad)));
    spans.extend(right);
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(
            Style::default().bg(theme::SURFACE).fg(theme::MUTED),
        ),
        area,
    );
}

/// Compact reasoning-effort tag for the statusline (e.g. `high` → `※high`).
fn effort_label(effort: &str) -> String {
    let e = effort.trim();
    if e.is_empty() {
        "※?".to_string()
    } else {
        format!("※{e}")
    }
}

// ── palette ────────────────────────────────────────────────────────────────
fn draw_palette(f: &mut Frame, app: &App, input_area: Rect) {
    let matches = app.palette_matches();
    if matches.is_empty() {
        return;
    }
    // content rows + 2 border + 2 inner padding, so the ornate frame has room.
    let content = (matches.len() as u16).min(10);
    let h = content + 4;
    let w = 60.min(f.area().width.saturating_sub(4)).max(34);
    let y = input_area.y.saturating_sub(h);
    let rect = Rect {
        x: input_area.x + 1,
        y,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    f.render_widget(
        Block::default().style(Style::default().bg(theme::SURFACE_2)),
        rect,
    );
    let phase = modal_phase(app);
    draw_modal_frame(
        f,
        rect,
        phase,
        theme::META_BLUE,
        " ⌘  commands ",
        None,
        " ↑↓ move  ·  ↵ run  ·  esc close ",
    );
    let inner = modal_inner(rect);

    let sel = app.palette_idx.min(matches.len() - 1);
    // Scroll the window so the selection is always visible (>10 commands).
    let vis = inner.height as usize;
    let start = sel.saturating_sub(vis.saturating_sub(1));
    let lines: Vec<Line> = matches
        .iter()
        .enumerate()
        .skip(start)
        .take(vis)
        .map(|(i, (name, desc))| {
            if i == sel {
                Line::from(vec![
                    Span::styled(
                        format!(" {name:<12}"),
                        Style::default()
                            .fg(theme::BG)
                            .bg(theme::META_BLUE)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {desc} "),
                        Style::default().fg(theme::BG).bg(theme::META_BLUE),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        format!(" {name:<12}"),
                        Style::default().fg(theme::META_BLUE_SKY),
                    ),
                    Span::styled(format!(" {desc}"), theme::style_faint()),
                ])
            }
        })
        .collect();
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );
}

// ── approval modal ─────────────────────────────────────────────────────────
fn draw_approval(f: &mut Frame, app: &App, area: Rect) {
    let Some(a) = &app.approval else { return };
    let preview = approval_preview(&a.name, &a.args);
    // body rows + 2 border + 2 inner padding.
    let max_body = (area.height.saturating_sub(6)).min(18).max(6) as usize;
    let body_lines: Vec<&str> = preview.iter().map(|s| s.as_str()).take(max_body).collect();
    let overflow = preview.len() > max_body;
    let content = body_lines.len() as u16 + if overflow { 1 } else { 0 };
    let h = (content + 4).min(area.height.saturating_sub(2)).max(7);
    let w = 78.min(area.width.saturating_sub(4)).max(48);
    let rect = Rect {
        x: (area.width.saturating_sub(w)) / 2,
        y: (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    f.render_widget(
        Block::default().style(Style::default().bg(theme::SURFACE_2)),
        rect,
    );
    let family = theme::tool_family(&a.name);
    let hue = theme::tool_color(&a.name);
    let phase = modal_phase(app);
    draw_modal_frame(
        f,
        rect,
        phase,
        hue,
        &format!(" ⚠ approve · {} · {family} ", a.name),
        None,
        "  y once   ·   a always   ·   n deny  ",
    );
    let inner = modal_inner(rect);

    let col_w = (inner.width as usize).saturating_sub(4).max(20);
    let mut lines: Vec<Line> = Vec::new();
    for l in &body_lines {
        let style = if l.starts_with('+') && !l.starts_with("+++") {
            Style::default().fg(theme::SUCCESS)
        } else if l.starts_with('-') && !l.starts_with("---") {
            Style::default().fg(theme::ERROR)
        } else if l.starts_with("@@") || l.starts_with("path ") || l.starts_with("cmd ") {
            Style::default().fg(theme::META_BLUE_SKY)
        } else {
            Style::default().fg(theme::MUTED)
        };
        lines.push(Line::from(Span::styled(
            format!("  {}", truncate(l, col_w)),
            style,
        )));
    }
    if preview.len() > max_body {
        lines.push(Line::from(Span::styled(
            format!("  … +{} more lines", preview.len() - max_body),
            theme::style_faint(),
        )));
    }
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );
}

/// Human-readable approval body: unified mini-diff for edits, command for bash.
fn approval_preview(tool: &str, args: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(args) else {
        return pretty_args(args)
            .lines()
            .map(|s| s.to_string())
            .take(16)
            .collect();
    };
    match tool {
        "edit_file" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("?");
            let old = v.get("old_string").and_then(|x| x.as_str()).unwrap_or("");
            let new = v.get("new_string").and_then(|x| x.as_str()).unwrap_or("");
            let mut out = vec![format!("path {path}")];
            out.extend(mini_unified_diff(old, new, 12));
            out
        }
        "write_file" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("?");
            let content = v.get("content").and_then(|x| x.as_str()).unwrap_or("");
            let mut out = vec![format!("path {path}  (write)")];
            for l in content.lines().take(12) {
                out.push(format!("+{l}"));
            }
            if content.lines().count() > 12 {
                out.push(format!("… +{} lines", content.lines().count() - 12));
            }
            out
        }
        "multi_edit" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("?");
            let mut out = vec![format!("path {path}  (multi_edit)")];
            if let Some(edits) = v.get("edits").and_then(|e| e.as_array()) {
                out.push(format!("@@ {} edit(s)", edits.len()));
                for (i, e) in edits.iter().take(4).enumerate() {
                    let old = e.get("old_string").and_then(|x| x.as_str()).unwrap_or("");
                    let new = e.get("new_string").and_then(|x| x.as_str()).unwrap_or("");
                    out.push(format!("── edit {} ──", i + 1));
                    out.extend(mini_unified_diff(old, new, 4));
                }
                if edits.len() > 4 {
                    out.push(format!("… +{} more edits", edits.len() - 4));
                }
            }
            out
        }
        "apply_patch" => {
            let patch = v
                .get("patch")
                .or_else(|| v.get("input"))
                .and_then(|x| x.as_str())
                .unwrap_or(args);
            patch
                .lines()
                .take(16)
                .map(|s| s.to_string())
                .collect()
        }
        "bash" => {
            let cmd = v.get("command").and_then(|x| x.as_str()).unwrap_or(args);
            vec![format!("cmd {cmd}")]
        }
        _ => pretty_args(args)
            .lines()
            .map(|s| s.to_string())
            .take(14)
            .collect(),
    }
}

/// Tiny unified-diff for approval trust (not a full Myers diff).
fn mini_unified_diff(old: &str, new: &str, max_lines: usize) -> Vec<String> {
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    let mut out = Vec::new();
    out.push(format!(
        "@@ -{} +{} @@",
        old_lines.len(),
        new_lines.len()
    ));
    // Prefer showing the change region: first differing line onward.
    let mut i = 0usize;
    while i < old_lines.len() && i < new_lines.len() && old_lines[i] == new_lines[i] {
        i += 1;
    }
    let context = i.saturating_sub(1);
    for l in old_lines.iter().skip(context).take(max_lines) {
        out.push(format!("-{l}"));
    }
    for l in new_lines.iter().skip(context).take(max_lines) {
        out.push(format!("+{l}"));
    }
    if old_lines.len().saturating_sub(context) > max_lines
        || new_lines.len().saturating_sub(context) > max_lines
    {
        out.push("…".into());
    }
    out
}

/// Stable-ish fingerprint so wrap cache can skip finished cells.
fn cell_wrap_key(cell: &Cell, spin_i: u64) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    match cell {
        Cell::Banner => 1u8.hash(&mut h),
        Cell::User(t) => {
            2u8.hash(&mut h);
            t.hash(&mut h);
        }
        Cell::Assistant { text, streaming } => {
            3u8.hash(&mut h);
            text.hash(&mut h);
            streaming.hash(&mut h);
            if *streaming {
                spin_i.hash(&mut h);
            }
        }
        Cell::Thinking {
            text,
            active,
            duration,
            expanded,
            ..
        } => {
            4u8.hash(&mut h);
            text.hash(&mut h);
            active.hash(&mut h);
            expanded.hash(&mut h);
            duration.map(|d| d.as_millis()).hash(&mut h);
            if *active {
                spin_i.hash(&mut h);
            }
        }
        Cell::Tool {
            name,
            args,
            result,
            ok,
            duration,
            expanded,
            ..
        } => {
            5u8.hash(&mut h);
            name.hash(&mut h);
            args.hash(&mut h);
            result.hash(&mut h);
            ok.hash(&mut h);
            expanded.hash(&mut h);
            duration.map(|d| d.as_millis()).hash(&mut h);
            if ok.is_none() {
                spin_i.hash(&mut h);
            }
        }
        Cell::TurnDone {
            duration,
            thought,
            interrupted,
        } => {
            6u8.hash(&mut h);
            duration.as_millis().hash(&mut h);
            thought.as_millis().hash(&mut h);
            interrupted.hash(&mut h);
        }
        Cell::Info { text, tone } => {
            7u8.hash(&mut h);
            text.hash(&mut h);
            // tone as discriminant
            format!("{tone:?}").hash(&mut h);
        }
        Cell::Error(t) => {
            8u8.hash(&mut h);
            t.hash(&mut h);
        }
    }
    h.finish()
}

// ── helpers ────────────────────────────────────────────────────────────────
fn summarize_args(tool: &str, args: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
        let key = match tool {
            "bash" => "command",
            "read_file" | "write_file" | "edit_file" | "multi_edit" | "apply_patch"
            | "list_dir" => "path",
            "grep" | "glob" => "pattern",
            "web_fetch" => "url",
            "web_search" => "query",
            "git_diff" => "mode",
            "agent" => "description",
            "memory" => "action",
            "skill" => "name",
            _ => "",
        };
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            return truncate(s, 80);
        }
        if let Some(obj) = v.as_object() {
            for (_, val) in obj {
                if let Some(s) = val.as_str() {
                    return truncate(s, 80);
                }
            }
        }
    }
    truncate(args, 80)
}

/// Last two path components — enough to recognize a repo without eating the row.
fn short_path(p: &str) -> String {
    let parts: Vec<&str> = p
        .split(['\\', '/'])
        .filter(|s| !s.is_empty())
        .collect();
    match parts.len() {
        0 => p.to_string(),
        1 => parts[0].to_string(),
        n => format!("{}\\{}", parts[n - 2], parts[n - 1]),
    }
}

fn pretty_args(args: &str) -> String {
    serde_json::from_str::<serde_json::Value>(args)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .unwrap_or_else(|_| args.to_string())
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ⏎ ");
    if s.chars().count() <= max {
        s
    } else {
        let t: String = s.chars().take(max).collect();
        format!("{t}…")
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Transcript shape: [banner, PROMPT_A, work…, PROMPT_B, work…]
    //   idx: 0 banner | 1,2 prompt A head | 3,4,5 A's work | 6,7 prompt B head | 8,9 B's work
    fn fixture() -> (Vec<Option<usize>>, Vec<bool>) {
        let owner = vec![
            None,
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(0),
            Some(1),
            Some(1),
            Some(1),
            Some(1),
        ];
        let head = vec![
            false, true, true, false, false, false, true, true, false, false,
        ];
        (owner, head)
    }

    #[test]
    fn pins_the_prompt_once_it_scrolls_off() {
        let (owner, head) = fixture();
        // Viewport shows only A's work (3..6) — A's prompt is above, so pin it.
        assert_eq!(sticky_owner(&owner, &head, 3, 6), Some(0));
        // Viewport shows only B's work — pin B.
        assert_eq!(sticky_owner(&owner, &head, 8, 10), Some(1));
    }

    #[test]
    fn no_header_when_the_prompt_is_already_on_screen() {
        let (owner, head) = fixture();
        // Viewport starts at A's prompt: you can see it, so don't duplicate it.
        assert_eq!(sticky_owner(&owner, &head, 1, 6), None);
        assert_eq!(sticky_owner(&owner, &head, 6, 10), None);
    }

    #[test]
    fn no_header_above_the_first_prompt() {
        let (owner, head) = fixture();
        // Banner region belongs to no prompt.
        assert_eq!(sticky_owner(&owner, &head, 0, 4), None);
    }

    #[test]
    fn header_follows_the_top_line_not_the_newest_prompt() {
        let (owner, head) = fixture();
        // Scrolled back into A's work while B exists below: header must say A.
        assert_eq!(sticky_owner(&owner, &head, 4, 9), Some(0));
    }
}

