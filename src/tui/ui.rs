//! Rendering for the Meta CLI TUI.

use super::app::{fmt_num, App, Cell};
use super::{markdown, wrap};
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
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

    if app.palette_matches().len() > 0 && app.approval.is_none() {
        draw_palette(f, app, chunks[2]);
    }
    if app.approval.is_some() {
        draw_approval(f, app, area);
    }
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

    let para = Paragraph::new(visible);
    let inner = Rect {
        x: area.x + 1,
        y: area.y,
        width: inner_w,
        height: area.height,
    };
    f.render_widget(para, inner);

    // Scroll indicator when not following the bottom.
    if app.scroll_from_bottom > 0 {
        let tag = format!(" ↓ {} more ", app.scroll_from_bottom);
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
                Style::default().fg(Color::Black).bg(theme::META_BLUE_BRIGHT),
            )),
            r,
        );
    }
}

fn cell_lines(app: &App, cell: &Cell, out: &mut Vec<Line<'static>>) {
    match cell {
        Cell::Banner => banner_lines(app, out),
        Cell::User(text) => {
            out.push(Line::default());
            for (i, l) in text.lines().enumerate() {
                let prefix = if i == 0 { "❯ " } else { "  " };
                out.push(Line::from(vec![
                    Span::styled(
                        prefix.to_string(),
                        Style::default().fg(theme::META_BLUE_BRIGHT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(l.to_string(), theme::style_user()),
                ]));
            }
        }
        Cell::Assistant { text, streaming } => {
            out.push(Line::default());
            let md = markdown::render_markdown(text, theme::style_assistant());
            for (i, mut l) in md.into_iter().enumerate() {
                let prefix = if i == 0 {
                    Span::styled("⏺ ".to_string(), Style::default().fg(theme::META_BLUE_BRIGHT))
                } else {
                    Span::raw("  ".to_string())
                };
                l.spans.insert(0, prefix);
                out.push(l);
            }
            if *streaming {
                if let Some(last) = out.last_mut() {
                    last.spans.push(Span::styled(
                        "▌".to_string(),
                        Style::default().fg(theme::META_BLUE_BRIGHT),
                    ));
                }
            }
        }
        Cell::Thinking { text, active } => {
            out.push(Line::default());
            let display: String = if *active {
                text.clone()
            } else {
                // Collapse finished thinking to its last line.
                text.lines().last().unwrap_or("").to_string()
            };
            for (i, l) in display.lines().enumerate() {
                let prefix = if i == 0 { "✻ " } else { "  " };
                out.push(Line::from(vec![
                    Span::styled(prefix.to_string(), theme::style_faint()),
                    Span::styled(l.to_string(), theme::style_thinking()),
                ]));
            }
        }
        Cell::Tool {
            name,
            args,
            result,
            ok,
        } => {
            out.push(Line::default());
            let bullet_style = match ok {
                Some(true) => theme::style_success(),
                Some(false) => theme::style_error(),
                None => Style::default().fg(theme::META_BLUE_BRIGHT),
            };
            out.push(Line::from(vec![
                Span::styled("⏺ ".to_string(), bullet_style),
                Span::styled(
                    name.clone(),
                    Style::default().fg(theme::FG).add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("({})", summarize_args(name, args)), theme::style_status()),
            ]));
            match result {
                None => out.push(Line::from(vec![
                    Span::raw("  ".to_string()),
                    Span::styled("⎿ running…".to_string(), theme::style_faint()),
                ])),
                Some(r) => {
                    let all: Vec<&str> = r.lines().filter(|l| !l.trim().is_empty()).collect();
                    let shown = 3usize;
                    for (i, l) in all.iter().take(shown).enumerate() {
                        let prefix = if i == 0 { "⎿ " } else { "  " };
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
            row.to_string(),
            Style::default().fg(Color::Rgb(r, g, b)),
        )));
    }
    out.push(Line::from(vec![
        Span::styled("Spark".to_string(), theme::style_title()),
        Span::styled(" · Meta Model API · ".to_string(), theme::style_status()),
        Span::styled(
            format!("v{}", env!("CARGO_PKG_VERSION")),
            theme::style_faint(),
        ),
    ]));
    out.push(Line::from(Span::styled(
        "unofficial coding agent — not affiliated with Meta".to_string(),
        theme::style_faint(),
    )));
    out.push(Line::default());
    out.push(Line::from(vec![
        Span::styled("  model ".to_string(), theme::style_faint()),
        Span::styled(app.cfg.model.clone(), theme::style_status()),
        Span::styled("   cwd ".to_string(), theme::style_faint()),
        Span::styled(app.cwd.display().to_string(), theme::style_status()),
    ]));
    out.push(Line::from(Span::styled(
        "  /help for commands · Enter to send · \\+Enter for newline · Esc interrupts".to_string(),
        theme::style_faint(),
    )));
}

// ── busy line ──────────────────────────────────────────────────────────────
fn draw_busy_line(f: &mut Frame, app: &App, area: Rect) {
    let frame_i = (app.spinner_epoch.elapsed().as_millis() / 80) as usize % SPINNER.len();
    let secs = app.turn_started.elapsed().as_secs();
    let mut spans = vec![
        Span::raw(" ".to_string()),
        Span::styled(
            SPINNER[frame_i].to_string(),
            Style::default().fg(theme::META_BLUE_BRIGHT),
        ),
        Span::styled(
            format!(" {}… ", capitalize(&app.status)),
            Style::default().fg(theme::META_BLUE_SKY),
        ),
        Span::styled(
            format!("({secs}s · esc to interrupt)"),
            theme::style_faint(),
        ),
    ];
    if !app.queue.is_empty() {
        spans.push(Span::styled(
            format!("  ·  {} queued", app.queue.len()),
            theme::style_warn(),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

// ── input ──────────────────────────────────────────────────────────────────
fn draw_input(f: &mut Frame, app: &App, area: Rect) {
    let border_color = if app.busy {
        theme::FAINT
    } else {
        theme::META_BLUE
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border_color));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let text = app.input.text();
    let mut lines: Vec<Line> = Vec::new();
    if text.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("❯ ".to_string(), Style::default().fg(theme::META_BLUE_BRIGHT)),
            Span::styled(
                if app.busy {
                    "queue a follow-up… (Enter to send)".to_string()
                } else {
                    "plan, build, debug — /help for commands".to_string()
                },
                theme::style_faint(),
            ),
        ]));
    } else {
        for (i, l) in text.split('\n').enumerate() {
            let prefix = if i == 0 { "❯ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(
                    prefix.to_string(),
                    Style::default().fg(theme::META_BLUE_BRIGHT),
                ),
                Span::styled(l.to_string(), Style::default().fg(theme::FG)),
            ]));
        }
    }

    // Vertical scroll so the cursor line stays visible.
    let (cur_line, cur_col) = app.input.cursor_line_col();
    let h = inner.height as usize;
    let top = cur_line.saturating_sub(h.saturating_sub(1));
    let visible: Vec<Line> = lines.into_iter().skip(top).take(h).collect();
    f.render_widget(Paragraph::new(visible), inner);

    if app.approval.is_none() {
        let cx = inner.x + 2 + cur_col as u16;
        let cy = inner.y + (cur_line - top) as u16;
        if cx < inner.right() && cy < inner.bottom() {
            f.set_cursor_position((cx, cy));
        }
    }
}

// ── statusline (usage bottom-left) ─────────────────────────────────────────
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

    let state_dot = if app.busy {
        Span::styled("⏺ ".to_string(), Style::default().fg(theme::META_BLUE_BRIGHT))
    } else {
        Span::styled("⏺ ".to_string(), theme::style_success())
    };

    let left = vec![
        Span::raw(" ".to_string()),
        state_dot,
        Span::styled(
            format!("{} tok", fmt_num(u.total_tokens)),
            theme::style_status(),
        ),
        Span::styled(" · ".to_string(), theme::style_faint()),
        Span::styled(
            format!("${:.4}", u.estimated_cost_usd()),
            theme::style_status(),
        ),
        Span::styled(" · ".to_string(), theme::style_faint()),
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
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn default_right(app: &App) -> String {
    format!(
        "{} · {} · {} ",
        app.cfg.model,
        &app.session_id[..8.min(app.session_id.len())],
        if app.busy { &app.status } else { "idle" }
    )
}

// ── palette ────────────────────────────────────────────────────────────────
fn draw_palette(f: &mut Frame, app: &App, input_area: Rect) {
    let matches = app.palette_matches();
    if matches.is_empty() {
        return;
    }
    let h = (matches.len() as u16).min(8) + 2;
    let w = 56.min(f.area().width.saturating_sub(4));
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
        .title(Span::styled(" commands ", theme::style_faint()));
    let inner = block.inner(rect);
    f.render_widget(block, rect);

    let sel = app.palette_idx.min(matches.len() - 1);
    let lines: Vec<Line> = matches
        .iter()
        .take(inner.height as usize)
        .enumerate()
        .map(|(i, (name, desc))| {
            if i == sel {
                Line::from(vec![
                    Span::styled(
                        format!(" {name:<10}"),
                        Style::default()
                            .fg(Color::White)
                            .bg(theme::META_BLUE)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!(" {desc}"),
                        Style::default().fg(Color::White).bg(theme::META_BLUE),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(format!(" {name:<10}"), theme::style_tool()),
                    Span::styled(format!(" {desc}"), theme::style_faint()),
                ])
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

// ── approval modal ─────────────────────────────────────────────────────────
fn draw_approval(f: &mut Frame, app: &App, area: Rect) {
    let Some(a) = &app.approval else { return };
    let pretty = pretty_args(&a.args);
    let arg_lines: Vec<&str> = pretty.lines().take(10).collect();
    let h = (arg_lines.len() as u16 + 5).min(area.height.saturating_sub(2));
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
        .border_style(Style::default().fg(theme::WARN))
        .title(Span::styled(
            format!(" approve tool · {} ", a.name),
            Style::default().fg(theme::WARN).add_modifier(Modifier::BOLD),
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
        Span::styled("  [y]".to_string(), theme::style_success()),
        Span::styled(" allow once   ".to_string(), theme::style_status()),
        Span::styled("[a]".to_string(), theme::style_tool()),
        Span::styled(" always allow ".to_string(), theme::style_status()),
        Span::styled(format!("{}   ", a.name), theme::style_faint()),
        Span::styled("[n]".to_string(), theme::style_error()),
        Span::styled(" deny".to_string(), theme::style_status()),
    ]));
    f.render_widget(Paragraph::new(lines), inner);
}

// ── helpers ────────────────────────────────────────────────────────────────
fn summarize_args(tool: &str, args: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(args) {
        let key = match tool {
            "bash" => "command",
            "read_file" | "write_file" | "edit_file" => "path",
            "grep" => "pattern",
            "glob" => "pattern",
            _ => "",
        };
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            return truncate(s, 80);
        }
        // Fall back to first string field.
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
