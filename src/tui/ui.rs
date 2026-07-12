//! Rendering for the Meta CLI TUI — Meta-blue surfaces, motion, cursors.

use super::app::{fmt_num, App, Cell};
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
    draw_input(f, app, chunks[2]);
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
}

// ── session picker ─────────────────────────────────────────────────────────
fn draw_session_picker(f: &mut Frame, app: &App, area: Rect) {
    let Some(p) = &app.picker else { return };
    let rows = p.visible();

    let w = 84.min(area.width.saturating_sub(4));
    let h = 22.min(area.height.saturating_sub(2));
    let rect = Rect {
        x: (area.width.saturating_sub(w)) / 2,
        y: (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);

    let scope = if p.this_cwd_only {
        "this workspace"
    } else {
        "all workspaces"
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::META_BLUE))
        .style(Style::default().bg(theme::SURFACE_2))
        .title(Span::styled(
            format!("  resume session · {} ({})  ", rows.len(), scope),
            Style::default()
                .fg(theme::META_BLUE)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    if rows.is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "  no sessions in this workspace — Tab to show all".to_string(),
                theme::style_faint(),
            )))
            .style(Style::default().bg(theme::SURFACE_2)),
            inner,
        );
        return;
    }

    // Two lines per row + a footer line.
    let body_h = inner.height.saturating_sub(1) as usize;
    let per_row = 2usize;
    let vis_rows = (body_h / per_row).max(1);
    let sel = p.idx.min(rows.len() - 1);
    let start = sel.saturating_sub(vis_rows.saturating_sub(1));

    let mut lines: Vec<Line> = Vec::new();
    for (i, r) in rows.iter().enumerate().skip(start).take(vis_rows) {
        let selected = i == sel;
        let (fg, bg) = if selected {
            (theme::BG, theme::META_BLUE)
        } else {
            (theme::FG, theme::SURFACE_2)
        };
        let marker = if selected { "❯ " } else { "  " };
        let short = &r.id[..8.min(r.id.len())];
        lines.push(Line::from(vec![
            Span::styled(
                format!("{marker}{short}  "),
                Style::default()
                    .fg(fg)
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{}  ·  {} msgs  ·  {} tok", r.when, r.messages, fmt_num(r.tokens)),
                Style::default().fg(if selected { theme::BG } else { theme::MUTED }).bg(bg),
            ),
            Span::styled(
                if r.here { "  · here".to_string() } else { String::new() },
                Style::default()
                    .fg(if selected { theme::BG } else { theme::SUCCESS })
                    .bg(bg),
            ),
        ]));
        // Second line: first user prompt — plus the workspace when browsing all.
        let avail = (inner.width as usize).saturating_sub(6);
        let detail = if p.this_cwd_only {
            r.preview.clone()
        } else {
            format!("{}  —  {}", short_path(&r.cwd), r.preview)
        };
        lines.push(Line::from(vec![
            Span::styled("    ".to_string(), Style::default().bg(bg)),
            Span::styled(
                truncate(&detail, avail),
                Style::default()
                    .fg(if selected { theme::BG } else { theme::FAINT })
                    .bg(bg),
            ),
        ]));
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        Rect {
            height: inner.height.saturating_sub(1),
            ..inner
        },
    );

    // Footer hints.
    let footer = Rect {
        x: inner.x,
        y: inner.bottom().saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ↑↓ ".to_string(), theme::style_tool()),
            Span::styled("move   ".to_string(), theme::style_faint()),
            Span::styled("enter ".to_string(), theme::style_tool()),
            Span::styled("resume   ".to_string(), theme::style_faint()),
            Span::styled("tab ".to_string(), theme::style_tool()),
            Span::styled(
                if p.this_cwd_only {
                    "show all workspaces   "
                } else {
                    "only this workspace   "
                }
                .to_string(),
                theme::style_faint(),
            ),
            Span::styled("esc ".to_string(), theme::style_tool()),
            Span::styled("cancel".to_string(), theme::style_faint()),
        ]))
        .style(Style::default().bg(theme::SURFACE_2)),
        footer,
    );
}

// ── transcript ─────────────────────────────────────────────────────────────
fn draw_transcript(f: &mut Frame, app: &mut App, area: Rect) {
    let inner_w = area.width.saturating_sub(2).max(10);
    let mut lines: Vec<Line<'static>> = Vec::new();
    for cell in &app.cells {
        cell_lines(app, cell, &mut lines);
    }
    let wrapped = wrap::wrap_lines(&lines, inner_w);
    let total = wrapped.len() as u16;
    let viewport = area.height;

    let max_scroll = total.saturating_sub(viewport);
    if app.scroll_from_bottom > max_scroll {
        app.scroll_from_bottom = max_scroll;
    }
    let top = max_scroll.saturating_sub(app.scroll_from_bottom);

    let visible: Vec<Line> = wrapped
        .into_iter()
        .skip(top as usize)
        .take(viewport as usize)
        .collect();

    let para = Paragraph::new(visible).style(theme::style_canvas());
    let inner = Rect {
        x: area.x + 1,
        y: area.y,
        width: inner_w,
        height: area.height,
    };
    f.render_widget(para, inner);

    // Scroll indicator when not following the bottom.
    if app.scroll_from_bottom > 0 {
        let tag = format!(" ↓ {} ", app.scroll_from_bottom);
        let w = tag.width() as u16;
        let r = Rect {
            x: area.right().saturating_sub(w + 2),
            y: area.bottom().saturating_sub(1),
            width: w.min(area.width),
            height: 1,
        };
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
    }
}

fn cell_lines(app: &App, cell: &Cell, out: &mut Vec<Line<'static>>) {
    let tick = app.spinner_epoch.elapsed();
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
        Cell::Thinking { text, active } => {
            out.push(Line::default());
            let display: String = if *active {
                text.clone()
            } else {
                text.lines().last().unwrap_or("").to_string()
            };
            let head = if *active {
                theme::spinner_frame(tick)
            } else {
                "·"
            };
            let head_style = if *active {
                Style::default().fg(theme::META_BLUE_SKY)
            } else {
                theme::style_faint()
            };
            if display.is_empty() && *active {
                out.push(Line::from(vec![
                    Span::styled(format!("{head} "), head_style),
                    Span::styled("thinking".to_string(), theme::style_thinking()),
                ]));
            } else {
                for (i, l) in display.lines().enumerate() {
                    let prefix = if i == 0 {
                        format!("{head} ")
                    } else {
                        "  ".into()
                    };
                    out.push(Line::from(vec![
                        Span::styled(prefix, head_style),
                        Span::styled(l.to_string(), theme::style_thinking()),
                    ]));
                }
            }
        }
        Cell::Tool {
            name,
            args,
            result,
            ok,
        } => {
            out.push(Line::default());
            let (bullet, bullet_style) = match ok {
                Some(true) => ("✓ ", theme::style_success()),
                Some(false) => ("✗ ", theme::style_error()),
                None => (
                    // Live tool call — braille spinner in Meta blue.
                    "",
                    Style::default().fg(theme::META_BLUE),
                ),
            };
            let mut head_spans = Vec::new();
            if ok.is_none() {
                head_spans.push(Span::styled(
                    format!("{} ", theme::spinner_frame(tick)),
                    Style::default().fg(theme::META_BLUE),
                ));
            } else {
                head_spans.push(Span::styled(bullet.to_string(), bullet_style));
            }
            head_spans.push(Span::styled(
                name.clone(),
                Style::default()
                    .fg(theme::FG)
                    .add_modifier(Modifier::BOLD),
            ));
            head_spans.push(Span::styled(
                format!("  {}", summarize_args(name, args)),
                theme::style_faint(),
            ));
            out.push(Line::from(head_spans));
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
                    let shown = 3usize;
                    for (i, l) in all.iter().take(shown).enumerate() {
                        let prefix = if i == 0 { "└ " } else { "  " };
                        out.push(Line::from(vec![
                            Span::raw("  ".to_string()),
                            Span::styled(prefix.to_string(), theme::style_faint()),
                            Span::styled(truncate(l, 200), theme::style_faint()),
                        ]));
                    }
                    if all.len() > shown {
                        out.push(Line::from(vec![
                            Span::raw("    ".to_string()),
                            Span::styled(
                                format!("… +{} lines", all.len() - shown),
                                theme::style_faint(),
                            ),
                        ]));
                    }
                }
            }
        }
        Cell::Info(text) => {
            out.push(Line::default());
            for (i, l) in text.lines().enumerate() {
                let prefix = if i == 0 { "● " } else { "  " };
                out.push(Line::from(vec![
                    Span::styled(prefix.to_string(), Style::default().fg(theme::META_BLUE)),
                    Span::styled(l.to_string(), theme::style_status()),
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
    out.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled("Spark".to_string(), theme::style_title()),
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
            "unofficial  ·  not affiliated with Meta".to_string(),
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
            "/help  ·  Shift+Tab modes  ·  Enter send  ·  Esc cancel".to_string(),
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
    let secs = app.turn_started.elapsed().as_secs();
    let mut spans = vec![Span::raw(" ".to_string())];

    if app.cancelling {
        // Distinct "stopping" chrome — not a happy thinking spinner.
        spans.push(Span::styled(
            "◼ ".to_string(),
            Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!("cancelling…  {secs}s  "),
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
        spans.push(Span::styled(format!("{secs}s"), theme::style_faint()));
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
fn draw_input(f: &mut Frame, app: &App, area: Rect) {
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
    let mut lines: Vec<Line> = Vec::new();
    if text.is_empty() {
        let hint = if app.busy {
            "type a follow-up — Enter queues it"
        } else {
            "plan, build, debug  ·  / for commands"
        };
        lines.push(Line::from(vec![
            Span::styled(
                "❯ ".to_string(),
                Style::default()
                    .fg(theme::META_BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(hint.to_string(), theme::style_faint()),
        ]));
    } else {
        for (i, l) in text.split('\n').enumerate() {
            let prefix = if i == 0 { "❯ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(
                    prefix.to_string(),
                    Style::default()
                        .fg(theme::META_BLUE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(l.to_string(), Style::default().fg(theme::FG)),
            ]));
        }
    }

    let (cur_line, cur_col) = app.input.cursor_line_col();
    let h = inner.height as usize;
    let top = cur_line.saturating_sub(h.saturating_sub(1));

    // Display width (not char count) of everything left of the cursor, so the
    // caret is right even with CJK/emoji, and long lines scroll horizontally.
    let cur_disp_w: usize = text
        .split('\n')
        .nth(cur_line)
        .map(|l| {
            l.chars()
                .take(cur_col)
                .collect::<String>()
                .width()
        })
        .unwrap_or(cur_col);
    let usable = (inner.width as usize).saturating_sub(3); // "❯ " + 1 margin
    let x_off = cur_disp_w.saturating_sub(usable) as u16;

    // Paint Meta block cursor into the line (blink) — not while a modal owns keys.
    if app.approval.is_none() && app.picker.is_none() && theme::blink_on(app.spinner_epoch.elapsed())
    {
        let vis_idx = cur_line.saturating_sub(top);
        if let Some(line) = lines.get_mut(vis_idx) {
            // Under placeholder (empty text) cursor sits after ❯
            if text.is_empty() {
                // Replace hint with cursor + faint remainder.
                *line = Line::from(vec![
                    Span::styled(
                        "❯ ".to_string(),
                        Style::default()
                            .fg(theme::META_BLUE)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" ".to_string(), theme::style_cursor_on()),
                    Span::styled(
                        if app.busy {
                            " type a follow-up…"
                        } else {
                            " plan, build, debug  ·  / for commands"
                        }
                        .to_string(),
                        theme::style_faint(),
                    ),
                ]);
            } else {
                // Insert reverse block at cursor column within the text span.
                inject_block_cursor(line, cur_col);
            }
        }
    }

    let visible: Vec<Line> = lines.into_iter().skip(top).take(h).collect();
    f.render_widget(
        Paragraph::new(visible)
            .scroll((0, x_off))
            .style(theme::style_surface()),
        inner,
    );

    // Keep hardware cursor hidden; we draw our own Meta caret.
    // (Hidden at app start.) Position still set for terminals that show it.
    if app.approval.is_none() && app.picker.is_none() {
        let cx = inner.x + 2 + (cur_disp_w as u16).saturating_sub(x_off);
        let cy = inner.y + (cur_line - top) as u16;
        if cx < inner.right() && cy < inner.bottom() {
            f.set_cursor_position((cx, cy));
        }
    }
}

/// Inject a reverse-video Meta block at `col` within the text portion of a line
/// that is `[prefix "❯ ", text...]`.
fn inject_block_cursor(line: &mut Line<'static>, col: usize) {
    // Flatten text after first span (prefix).
    if line.spans.is_empty() {
        return;
    }
    let prefix = line.spans[0].clone();
    let mut text = String::new();
    for s in line.spans.iter().skip(1) {
        text.push_str(&s.content);
    }
    let mut chars: Vec<char> = text.chars().collect();
    let col = col.min(chars.len());
    let before: String = chars.drain(..col).collect();
    let under = chars.first().copied().unwrap_or(' ');
    let after: String = if chars.is_empty() {
        String::new()
    } else {
        chars[1..].iter().collect()
    };

    let mut spans = vec![prefix];
    if !before.is_empty() {
        spans.push(Span::styled(before, Style::default().fg(theme::FG)));
    }
    spans.push(Span::styled(under.to_string(), theme::style_cursor_on()));
    if !after.is_empty() {
        spans.push(Span::styled(after, Style::default().fg(theme::FG)));
    } else if under == ' ' {
        // trailing cursor already painted as space block
    }
    line.spans = spans;
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

    let left = vec![
        Span::raw(" ".to_string()),
        state_dot,
        Span::styled(
            format!("{} tok", fmt_num(u.total_tokens)),
            theme::style_status(),
        ),
        Span::styled("  ·  ".to_string(), theme::style_faint()),
        Span::styled(
            format!("${:.4}", u.estimated_cost_usd()),
            theme::style_status(),
        ),
        Span::styled("  ·  ".to_string(), theme::style_faint()),
        Span::styled(format!("ctx {ctx_pct:.0}%"), ctx_style),
    ];

    let right_text = if let Some(t) = app.quit_armed {
        if t.elapsed().as_secs() < 2 {
            "ctrl+c again to quit ".to_string()
        } else {
            default_right(app)
        }
    } else {
        default_right(app)
    };

    let left_w: usize = left.iter().map(|s| s.content.width()).sum();
    let right_w = right_text.width();
    let pad = (area.width as usize).saturating_sub(left_w + right_w);

    let mut spans = left;
    spans.push(Span::raw(" ".repeat(pad)));
    spans.push(Span::styled(right_text, theme::style_faint()));
    f.render_widget(
        Paragraph::new(Line::from(spans)).style(
            Style::default().bg(theme::SURFACE).fg(theme::MUTED),
        ),
        area,
    );
}

fn default_right(app: &App) -> String {
    let mode = app.permission_mode.get().label();
    format!(
        "{}  ·  {}  ·  {}  ·  {} ",
        app.cfg.model,
        mode,
        &app.session_id[..8.min(app.session_id.len())],
        if app.cancelling {
            "cancelling"
        } else if app.busy {
            app.status.as_str()
        } else {
            "ready"
        }
    )
}

// ── palette ────────────────────────────────────────────────────────────────
fn draw_palette(f: &mut Frame, app: &App, input_area: Rect) {
    let matches = app.palette_matches();
    if matches.is_empty() {
        return;
    }
    let h = (matches.len() as u16).min(10) + 2;
    let w = 58.min(f.area().width.saturating_sub(4));
    let y = input_area.y.saturating_sub(h);
    let rect = Rect {
        x: input_area.x + 1,
        y,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::META_BLUE))
        .style(Style::default().bg(theme::SURFACE_2))
        .title(Span::styled(
            " commands ",
            Style::default().fg(theme::META_BLUE_SKY),
        ));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

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
    let pretty = pretty_args(&a.args);
    let arg_lines: Vec<&str> = pretty.lines().take(10).collect();
    let h = (arg_lines.len() as u16 + 6).min(area.height.saturating_sub(2));
    let w = 72.min(area.width.saturating_sub(4));
    let rect = Rect {
        x: (area.width.saturating_sub(w)) / 2,
        y: (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::META_BLUE))
        .style(Style::default().bg(theme::SURFACE_2))
        .title(Span::styled(
            format!("  approve · {}  ", a.name),
            Style::default()
                .fg(theme::META_BLUE)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::default());
    for l in &arg_lines {
        lines.push(Line::from(Span::styled(
            format!("  {}", truncate(l, (inner.width as usize).saturating_sub(4))),
            Style::default().fg(theme::FG),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        Span::styled(
            "  y ".to_string(),
            Style::default()
                .fg(theme::BG)
                .bg(theme::SUCCESS)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" once   ".to_string(), theme::style_status()),
        Span::styled(
            " a ".to_string(),
            Style::default()
                .fg(theme::BG)
                .bg(theme::META_BLUE)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" always {}   ", a.name),
            theme::style_status(),
        ),
        Span::styled(
            " n ".to_string(),
            Style::default()
                .fg(theme::BG)
                .bg(theme::ERROR)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" deny".to_string(), theme::style_status()),
    ]));
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );
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

