//! Rendering for the NurCLI TUI — Nur-gold surfaces, motion, cursors.

use super::app::{fmt_num, line_to_plain, App, Cell, TextRange};
use super::{ansi, grid, markdown, scrollbar::ScrollMetrics, wrap};
use crate::theme;
use ratatui::layout::{Constraint, Direction, Layout, Position, Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};
use ratatui::Frame;
use std::time::Duration;
use unicode_width::UnicodeWidthStr;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    // Solid Meta-dark canvas so empty regions never flash terminal default.
    f.render_widget(
        Block::default().style(theme::style_canvas()),
        area,
    );

    // Too-small terminal: show a terse message instead of crashing.
    if area.width < 20 || area.height < 5 {
        let msg = "terminal too small — resize to ≥ 20×5";
        let p = Paragraph::new(Line::from(Span::styled(
            msg,
            theme::style_faint(),
        )));
        f.render_widget(p, area);
        // Overlays must remain visible even when the base prompt is too small
        // to lay out. Their geometry is fitted independently below.
        if app.approval.is_some() {
            draw_approval(f, app, area);
        }
        if app.picker.is_some() {
            draw_session_picker(f, app, area);
        }
        if app.login.is_some() {
            draw_login(f, app, area);
        }
        if app.model_picker.is_some() {
            draw_model_picker(f, app, area);
        }
        if app.plugin_picker.is_some() {
            draw_plugin_picker(f, app, area);
        }
        if app.ctx_menu.is_some() {
            draw_ctx_menu(f, app);
        }
        return;
    }

    // Soft-wrapped visual height: grow with content up to INPUT_VIEW_MAX, then scroll.
    // Estimate content width from terminal (borders + ❯ prefix).
    let est_w = (area.width as usize).saturating_sub(5).max(8);
    let vcount = app.input.visual_line_count(est_w).max(1);
    const INPUT_VIEW_MAX: usize = 8;
    let input_body = vcount.min(INPUT_VIEW_MAX).max(1) as u16;
    let busy_h = if app.busy { 1 } else { 0 };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(busy_h),
            Constraint::Length(input_body + 2),
            Constraint::Length(1),
        ])
        .split(area);

    draw_transcript(f, app, chunks[0]);
    if app.busy {
        draw_busy_line(f, app, chunks[1]);
    }
    draw_input(f, app, chunks[2]); // publishes input_inner for click-to-caret
    draw_statusline(f, app, chunks[3]);

    if !app.palette_matches().is_empty()
        && app.approval.is_none()
        && app.picker.is_none()
        && app.login.is_none()
    {
        draw_palette(f, app, chunks[2]);
    }
    if app.approval.is_some() {
        draw_approval(f, app, area);
    }
    if app.picker.is_some() {
        draw_session_picker(f, app, area);
    }
    if app.login.is_some() {
        draw_login(f, app, area);
    }
    if app.model_picker.is_some() {
        draw_model_picker(f, app, area);
    }
    if app.plugin_picker.is_some() {
        draw_plugin_picker(f, app, area);
    }
    // Grok-style hover dialogue over thoughts / tools / turns (above everything
    // except approval/picker, which already short-circuit interaction).
    if app.approval.is_none()
        && app.picker.is_none()
        && app.login.is_none()
        && app.model_picker.is_none()
        && app.plugin_picker.is_none()
        && app.ctx_menu.is_none()
    {
        match draw_hover_peek(f, app, area) {
            Some((b, c)) => {
                app.peek_box = b;
                app.peek_close = c;
            }
            None => {
                app.peek_box = Rect::default();
                app.peek_close = Rect::default();
            }
        }
    } else {
        app.peek_box = Rect::default();
        app.peek_close = Rect::default();
    }
    // Context menu overlay — drawn last so it sits on top.
    if app.ctx_menu.is_some() {
        draw_ctx_menu(f, app);
    }
}

// ── secure login modal (`/login`) ───────────────────────────────────────────
fn draw_login(f: &mut Frame, app: &mut App, area: Rect) {
    let stage = app.login.as_ref().map(|m| m.stage);
    match stage {
        Some(super::app::LoginStage::Provider) => draw_login_picker(f, app, area),
        Some(super::app::LoginStage::Method) => draw_login_method(f, app, area),
        Some(super::app::LoginStage::Key) => draw_login_key(f, app, area),
        Some(super::app::LoginStage::Browser) => draw_login_browser(f, app, area),
        None => {}
    }
}

/// Stage: browser vs API key (and optional import of existing CLI session).
fn draw_login_method(f: &mut Frame, app: &App, area: Rect) {
    let Some(m) = &app.login else { return };
    let provider = crate::providers::by_id(&m.provider_id)
        .copied()
        .unwrap_or(*crate::providers::default_provider());
    let want: u16 = if m.error.is_some() { 14 } else { 13 };
    let rect = fit_modal_rect(area, 64, want, 44, 8);
    f.render_widget(Clear, rect);
    f.render_widget(Block::default().style(Style::default().bg(theme::SURFACE_2)), rect);
    let phase = modal_phase(app);
    let failover = m.fallback_key;
    let title = if failover {
        format!(" ↻ {} · failover credentials ", provider.name)
    } else {
        format!(" 🔑 {} · how to sign in ", provider.name)
    };
    draw_modal_frame(
        f,
        rect,
        phase,
        theme::INDIGO,
        &title,
        None,
        "  ↑↓  ·  ↵ choose  ·  esc back  ",
    );
    let inner = modal_inner(rect);
    let mut options: Vec<(&str, String)> = vec![
        (
            "Sign in with browser",
            "URL + code / SSO — no API key to manage".into(),
        ),
        ("Enter API key", format!("env {}", provider.env_key)),
    ];
    if m.can_import {
        options.push((
            "Use existing CLI session",
            "import from Codex / Grok / Kimi / Claude login".into(),
        ));
    }
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            format!("  {}", provider.note),
            theme::style_faint(),
        )),
        Line::default(),
    ];
    for (i, (title, sub)) in options.iter().enumerate() {
        let selected = m.method_sel == i;
        let marker = if selected { "❯ " } else { "  " };
        let title_style = if selected {
            Style::default()
                .fg(theme::BG)
                .bg(theme::META_BLUE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::FG).add_modifier(Modifier::BOLD)
        };
        let sub_style = if selected {
            Style::default().fg(theme::BLUE_100).bg(theme::META_BLUE)
        } else {
            theme::style_faint()
        };
        lines.push(Line::from(Span::styled(
            format!("{marker}{title}"),
            title_style,
        )));
        lines.push(Line::from(Span::styled(
            format!("    {sub}"),
            sub_style,
        )));
        lines.push(Line::default());
    }
    if let Some(e) = &m.error {
        lines.push(Line::from(Span::styled(
            format!("  {e}"),
            theme::style_error(),
        )));
    }
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );
}

/// Browser / device-code wait (Hugging Face–style URL + short code).
fn draw_login_browser(f: &mut Frame, app: &App, area: Rect) {
    let Some(m) = &app.login else { return };
    let provider = crate::providers::by_id(&m.provider_id)
        .copied()
        .unwrap_or(*crate::providers::default_provider());
    let want: u16 = if m.error.is_some() { 14 } else { 13 };
    let rect = fit_modal_rect(area, 72, want, 48, 8);
    f.render_widget(Clear, rect);
    f.render_widget(Block::default().style(Style::default().bg(theme::SURFACE_2)), rect);
    let phase = modal_phase(app);
    let spin = if theme::blink_on(app.spinner_epoch.elapsed()) {
        "◐"
    } else {
        "◑"
    };
    let title = if m.fallback_key {
        format!(" {spin} ↻ {} · failover browser sign-in ", provider.name)
    } else {
        format!(" {spin} {} · browser sign-in ", provider.name)
    };
    draw_modal_frame(
        f,
        rect,
        phase,
        theme::INDIGO,
        &title,
        None,
        "  esc cancel  ·  paste code + ↵ if prompted  ",
    );
    let inner = modal_inner(rect);
    let col = (inner.width as usize).saturating_sub(4);
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            format!("  {}", m.browser_status),
            Style::default().fg(theme::BLUE_100),
        )),
        Line::default(),
    ];
    if !m.browser_user_code.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  code  ".to_string(), theme::style_faint()),
            Span::styled(
                m.browser_user_code.clone(),
                Style::default()
                    .fg(theme::BG)
                    .bg(theme::META_BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::default());
    }
    // Manual OAuth paste buffer (Claude headless / busy-port path).
    if !m.buf.is_empty() || m.browser_user_code.contains("paste") {
        let shown = if m.buf.is_empty() {
            "…".to_string()
        } else {
            truncate(&m.buf, col.saturating_sub(10))
        };
        lines.push(Line::from(vec![
            Span::styled("  paste  ".to_string(), theme::style_faint()),
            Span::styled(
                shown,
                Style::default().fg(theme::FG).add_modifier(Modifier::BOLD),
            ),
        ]));
        lines.push(Line::default());
    }
    if !m.browser_url.is_empty() {
        lines.push(Line::from(Span::styled(
            "  open in browser:".to_string(),
            theme::style_faint(),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", truncate(&m.browser_url, col)),
            Style::default()
                .fg(theme::FG)
                .add_modifier(Modifier::UNDERLINED),
        )));
        lines.push(Line::default());
    }
    lines.push(Line::from(Span::styled(
        "  Complete sign-in in the browser. This window updates when you're done."
            .to_string(),
        theme::style_faint(),
    )));
    if let Some(e) = &m.error {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("  {e}"),
            theme::style_error(),
        )));
    }
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );
}

/// Stage 1 — scrollable, filterable provider list.
/// Scroll/select contract matches `draw_session_picker`: one entry per ↑↓/wheel
/// notch (`step` / `wheel_step`), click row to select, second click confirms.
fn draw_login_picker(f: &mut Frame, app: &mut App, area: Rect) {
    let rect = fit_modal_rect(area, 74, 28, 48, 12);
    f.render_widget(Clear, rect);
    f.render_widget(Block::default().style(Style::default().bg(theme::SURFACE_2)), rect);
    let phase = modal_phase(app);

    let total = app.login.as_ref().map(|m| m.count()).unwrap_or(0);
    let manage = app.login.as_ref().map(|m| m.manage_failover).unwrap_or(false);
    let title = if manage {
        format!(" ↻ manage failover  ·  {} in chain ", app.cfg.fallback_providers.len())
    } else {
        format!(" 🔑 choose a provider  ·  {total} ")
    };
    let hint = if manage {
        " ↑↓ move  ·  space add failover  ·  alt+p privacy tier  ·  type to filter  ·  esc done  "
    } else {
        " ↑↓ move  ·  enter pick  ·  space add failover  ·  alt+p privacy tier  ·  esc  "
    };
    draw_modal_frame(f, rect, phase, theme::INDIGO, &title, None, hint);
    let inner = modal_inner(rect);

    let close = Rect {
        x: rect.x + rect.width.saturating_sub(5),
        y: rect.y,
        width: 3,
        height: 1,
    };
    let mut hit = super::app::PickerHit {
        frame: rect,
        close,
        body: inner,
        scope: Rect::default(),
        foreign: Rect::default(),
        rows: Vec::new(),
    };

    // Filter line (2 rows) + provider list below — same one-step scroll as sessions.
    const FILTER_ROWS: usize = 2;
    let list_h = (inner.height as usize).saturating_sub(FILTER_ROWS).max(1);
    // One screen row per provider (no separator stride — still one-step select).
    let vis_rows = list_h.max(1);

    let (filter, mut sel, mut start) = {
        let m = app.login.as_ref().unwrap();
        (m.filter.clone(), m.sel, m.scroll)
    };

    if let Some(m) = &mut app.login {
        m.vis_page = vis_rows;
        m.clamp_scroll();
        sel = m.sel;
        start = m.scroll;
    }

    let mut caret = filter;
    if theme::blink_on(app.spinner_epoch.elapsed()) {
        caret.push('▉');
    }
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("  search  ".to_string(), theme::style_faint()),
            Span::styled(
                caret,
                Style::default()
                    .fg(theme::BLUE_100)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::default(),
    ];

    let picks = app
        .login
        .as_ref()
        .map(|m| m.filtered())
        .unwrap_or_default();
    let col = (inner.width as usize).saturating_sub(4);

    if total == 0 {
        lines.push(Line::from(Span::styled(
            "  no providers match".to_string(),
            theme::style_faint(),
        )));
        f.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
            inner,
        );
        if let Some(m) = &mut app.login {
            m.hit = hit;
            m.vis_page = 1;
        }
        return;
    }

    for (i, p) in picks.iter().enumerate().skip(start).take(vis_rows) {
        let selected = i == sel;
        let marker = if selected { "❯ " } else { "  " };
        let name_style = if selected {
            Style::default()
                .fg(theme::BG)
                .bg(theme::META_BLUE)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::FG)
        };
        let note_style = if selected {
            Style::default().fg(theme::BLUE_100).bg(theme::META_BLUE)
        } else {
            theme::style_faint()
        };
        // Browser/OAuth badge sits immediately after the name (not after the
        // note) so truncate() never clips the 🌐 off narrow terminals.
        let oauth_badge = if p.browser_auth { " 🌐" } else { "" };
        let name_col = format!("{}{oauth_badge}", p.name);
        let priv_tag = crate::providers::effective_privacy(&app.cfg.provider_privacy, p.id).tag();
        let priv_badge = if priv_tag.is_empty() {
            String::new()
        } else {
            format!("  [{priv_tag}]")
        };
        let fb = app
            .cfg
            .fallback_providers
            .iter()
            .position(|x| x == p.id)
            .map(|i| format!("  ↻#{}", i + 1))
            .unwrap_or_default();
        // Pad name+badge as a unit so notes still align roughly.
        let text = format!("{marker}{name_col:<25} {}{priv_badge}{fb}", p.note);
        let style = if selected { name_style } else { note_style };
        lines.push(Line::from(Span::styled(truncate(&text, col), style)));

        // Hit row: absolute index i, screen y under filter header.
        let drawn = i - start;
        let row_y = inner.y + FILTER_ROWS as u16 + drawn as u16;
        if row_y < inner.y + inner.height {
            hit.rows.push((
                i,
                Rect {
                    x: inner.x,
                    y: row_y,
                    width: inner.width,
                    height: 1,
                },
            ));
        }
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );
    if let Some(m) = &mut app.login {
        m.hit = hit;
    }
}

// ── model chooser (`/model`) ─────────────────────────────────────────────────
/// Live model picker — same scroll/select/filter contract as the provider
/// picker. Shows the active provider's `/models` list; type to filter (or to
/// enter a custom id), ↵ to switch.
fn draw_model_picker(f: &mut Frame, app: &mut App, area: Rect) {
    let rect = fit_modal_rect(area, 74, 28, 48, 12);
    f.render_widget(Clear, rect);
    f.render_widget(Block::default().style(Style::default().bg(theme::SURFACE_2)), rect);
    let phase = modal_phase(app);

    let (provider_name, total, loading) = app
        .model_picker
        .as_ref()
        .map(|m| (m.provider_name.clone(), m.count(), m.loading))
        .unwrap_or_default();
    let title = if loading {
        format!(" ⧗ {provider_name} models  ·  loading… ")
    } else {
        format!(" ◆ {provider_name} models  ·  {total} ")
    };
    draw_modal_frame(
        f,
        rect,
        phase,
        theme::INDIGO,
        &title,
        None,
        " ↑↓/wheel  ·  ↵ switch  ·  type to filter / custom id  ·  esc/✕  ",
    );
    let inner = modal_inner(rect);

    let close = Rect {
        x: rect.x + rect.width.saturating_sub(5),
        y: rect.y,
        width: 3,
        height: 1,
    };
    let mut hit = super::app::PickerHit {
        frame: rect,
        close,
        body: inner,
        scope: Rect::default(),
        foreign: Rect::default(),
        rows: Vec::new(),
    };

    // Filter line (2 rows) + list below — one screen row per model.
    const FILTER_ROWS: usize = 2;
    let list_h = (inner.height as usize).saturating_sub(FILTER_ROWS).max(1);
    let vis_rows = list_h.max(1);

    let (filter, current, error, mut sel, mut start) = {
        let m = app.model_picker.as_ref().unwrap();
        (
            m.filter.clone(),
            m.current.clone(),
            m.error.clone(),
            m.sel,
            m.scroll,
        )
    };

    if let Some(m) = &mut app.model_picker {
        m.vis_page = vis_rows;
        m.clamp_scroll();
        sel = m.sel;
        start = m.scroll;
    }

    let mut caret = filter.clone();
    if theme::blink_on(app.spinner_epoch.elapsed()) {
        caret.push('▉');
    }
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("  filter  ".to_string(), theme::style_faint()),
            Span::styled(
                caret,
                Style::default()
                    .fg(theme::BLUE_100)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::default(),
    ];

    let picks = app
        .model_picker
        .as_ref()
        .map(|m| m.filtered().into_iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let col = (inner.width as usize).saturating_sub(4);

    // Empty states: loading, fetch error, or no filter match.
    if picks.is_empty() {
        let msg = if loading {
            "  fetching models from provider…".to_string()
        } else if let Some(e) = &error {
            format!("  couldn't list models · {e}")
        } else {
            "  no models match — type an id + ↵ to switch".to_string()
        };
        lines.push(Line::from(Span::styled(truncate(&msg, col), theme::style_faint())));
        if error.is_some() && !filter.trim().is_empty() {
            lines.push(Line::from(Span::styled(
                truncate(&format!("  ↵ switch to \"{}\"", filter.trim()), col),
                Style::default().fg(theme::BLUE_100),
            )));
        }
        f.render_widget(
            Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
            inner,
        );
        if let Some(m) = &mut app.model_picker {
            m.hit = hit;
            m.vis_page = 1;
        }
        return;
    }

    for (i, id) in picks.iter().enumerate().skip(start).take(vis_rows) {
        let selected = i == sel;
        let is_current = *id == current;
        let marker = if selected { "❯ " } else { "  " };
        let badge = if is_current { "  ● active" } else { "" };
        let text = format!("{marker}{id}{badge}");
        let style = if selected {
            Style::default()
                .fg(theme::BG)
                .bg(theme::META_BLUE)
                .add_modifier(Modifier::BOLD)
        } else if is_current {
            Style::default()
                .fg(theme::BLUE_100)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::FG)
        };
        lines.push(Line::from(Span::styled(truncate(&text, col), style)));

        let drawn = i - start;
        let row_y = inner.y + FILTER_ROWS as u16 + drawn as u16;
        if row_y < inner.y + inner.height {
            hit.rows.push((
                i,
                Rect {
                    x: inner.x,
                    y: row_y,
                    width: inner.width,
                    height: 1,
                },
            ));
        }
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );
    if let Some(m) = &mut app.model_picker {
        m.hit = hit;
    }
}

// ── plugin marketplace (`/plugins`) ──────────────────────────────────────────
/// Same scroll/select/filter contract as the provider picker.
fn draw_plugin_picker(f: &mut Frame, app: &mut App, area: Rect) {
    let rect = fit_modal_rect(area, 78, 28, 52, 12);
    f.render_widget(Clear, rect);
    f.render_widget(Block::default().style(Style::default().bg(theme::SURFACE_2)), rect);
    let phase = modal_phase(app);

    let (total, busy, installed_n) = app
        .plugin_picker
        .as_ref()
        .map(|m| {
            (
                m.count(),
                m.busy,
                m.rows.iter().filter(|r| r.installed).count(),
            )
        })
        .unwrap_or_default();
    let title = if busy {
        " ⬡ plugins  ·  working… ".to_string()
    } else {
        format!(" ⬡ plugins  ·  {total} shown  ·  {installed_n} installed ")
    };
    draw_modal_frame(
        f,
        rect,
        phase,
        theme::INDIGO,
        &title,
        None,
        " ↑↓/wheel  ·  ↵ install/toggle  ·  filter: design|finance|workflow|…  ·  esc/✕  ",
    );
    let inner = modal_inner(rect);

    let close = Rect {
        x: rect.x + rect.width.saturating_sub(5),
        y: rect.y,
        width: 3,
        height: 1,
    };
    let mut hit = super::app::PickerHit {
        frame: rect,
        close,
        body: inner,
        scope: Rect::default(),
        foreign: Rect::default(),
        rows: Vec::new(),
    };

    const FILTER_ROWS: usize = 2;
    let list_h = (inner.height as usize).saturating_sub(FILTER_ROWS + 1).max(1);
    let vis_rows = list_h.max(1);

    let (filter, status, mut sel, mut start) = {
        let m = app.plugin_picker.as_ref().unwrap();
        (
            m.filter.clone(),
            m.status.clone(),
            m.sel,
            m.scroll,
        )
    };

    if let Some(m) = &mut app.plugin_picker {
        m.vis_page = vis_rows;
        m.clamp_scroll();
        sel = m.sel;
        start = m.scroll;
    }

    let mut caret = filter;
    if theme::blink_on(app.spinner_epoch.elapsed()) {
        caret.push('▉');
    }
    let mut lines: Vec<Line> = vec![
        Line::from(vec![
            Span::styled("  search  ".to_string(), theme::style_faint()),
            Span::styled(
                caret,
                Style::default()
                    .fg(theme::BLUE_100)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::default(),
    ];

    let picks = app
        .plugin_picker
        .as_ref()
        .map(|m| m.filtered().into_iter().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let col = (inner.width as usize).saturating_sub(4);

    if picks.is_empty() {
        lines.push(Line::from(Span::styled(
            "  no plugins match".to_string(),
            theme::style_faint(),
        )));
    } else {
        for (i, p) in picks.iter().enumerate().skip(start).take(vis_rows) {
            let selected = i == sel;
            let marker = if selected { "❯ " } else { "  " };
            let badge = p.status_badge();
            // name (pad) · category · status
            let text = format!(
                "{marker}{:<16} {:<12} {badge}",
                truncate(&p.name, 16),
                truncate(&p.category, 14),
            );
            let style = if selected {
                Style::default()
                    .fg(theme::BG)
                    .bg(theme::META_BLUE)
                    .add_modifier(Modifier::BOLD)
            } else if p.enabled {
                Style::default()
                    .fg(theme::BLUE_100)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::FG)
            };
            lines.push(Line::from(Span::styled(truncate(&text, col), style)));

            let drawn = i - start;
            let row_y = inner.y + FILTER_ROWS as u16 + drawn as u16;
            if row_y < inner.y + inner.height {
                hit.rows.push((
                    i,
                    Rect {
                        x: inner.x,
                        y: row_y,
                        width: inner.width,
                        height: 1,
                    },
                ));
            }
        }
    }

    // Footer: description of selection + status
    if let Some(p) = picks.get(sel) {
        let hint = format!("  {}  ·  {}", p.action_hint(), truncate(&p.description, col.saturating_sub(16)));
        // Pad to bottom-ish by letting Paragraph clip — push after a blank.
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            truncate(&hint, col),
            theme::style_faint(),
        )));
    }
    if let Some(s) = status {
        lines.push(Line::from(Span::styled(
            truncate(&format!("  {s}"), col),
            if busy {
                Style::default().fg(theme::BLUE_100)
            } else {
                theme::style_faint()
            },
        )));
    }

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );
    if let Some(m) = &mut app.plugin_picker {
        m.hit = hit;
    }
}

/// Stage 2 — masked key entry for the chosen provider.
fn draw_login_key(f: &mut Frame, app: &App, area: Rect) {
    let Some(m) = &app.login else { return };
    let provider = crate::providers::by_id(&m.provider_id)
        .copied()
        .unwrap_or(*crate::providers::default_provider());
    let want: u16 = if m.error.is_some() { 12 } else { 11 };
    let rect = fit_modal_rect(area, 64, want, 44, 8);
    f.render_widget(Clear, rect);
    f.render_widget(Block::default().style(Style::default().bg(theme::SURFACE_2)), rect);
    let phase = modal_phase(app);
    let title = if m.fallback_key {
        format!(" ↻ {} · failover key ", provider.name)
    } else {
        format!(" 🔑 {} ", provider.name)
    };
    draw_modal_frame(
        f,
        rect,
        phase,
        theme::INDIGO,
        &title,
        None,
        "  ↵ save  ·  ctrl+v paste  ·  ctrl+u clear  ·  esc back  ",
    );
    let inner = modal_inner(rect);

    let field_w = (inner.width as usize).saturating_sub(4).max(8);
    let dots = m.buf.chars().count().min(field_w.saturating_sub(1));
    let mut field = "•".repeat(dots);
    if theme::blink_on(app.spinner_epoch.elapsed()) {
        field.push('▉');
    }
    let key_hint = if provider.key_optional {
        format!("{} API key  (optional for local)", provider.name)
    } else {
        format!("{} API key  ·  env {}", provider.name, provider.env_key)
    };
    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(format!("  {key_hint}"), theme::style_faint())),
        Line::default(),
        Line::from(vec![
            Span::raw("  ".to_string()),
            Span::styled(
                format!("{field:<field_w$}"),
                Style::default().fg(theme::BLUE_100).bg(theme::SURFACE).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::default(),
        Line::from(vec![
            Span::raw("  ".to_string()),
            Span::styled(format!("{} chars", m.buf.chars().count()), theme::style_faint()),
            Span::styled(
                if m.fallback_key {
                    "   ·   stored in ~/.nur/provider_keys.json".to_string()
                } else {
                    "   ·   stored only in ~/.nur/auth.json".to_string()
                },
                theme::style_faint(),
            ),
        ]),
        Line::from(vec![
            Span::raw("  ".to_string()),
            Span::styled(
                format!("model {}  ·  {}", provider.default_model, provider.base_url),
                theme::style_faint(),
            ),
        ]),
    ];
    if let Some(e) = &m.error {
        lines.push(Line::default());
        lines.push(Line::from(vec![
            Span::raw("  ".to_string()),
            Span::styled(truncate(e, field_w), theme::style_error()),
        ]));
    }
    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::SURFACE_2)),
        inner,
    );
}

// ── sessions picker (`/sessions` · `/resume`) ────────────────────
// Thick custom frame, rotating border accents + entry separators, entry-stable scroll.
fn draw_session_picker(f: &mut Frame, app: &mut App, area: Rect) {
    if app.picker.is_none() {
        return;
    }
    let phase = (app.spinner_epoch.elapsed().as_millis() / theme::SPINNER_MS) as usize;
    let spin = theme::SPINNER[phase % theme::SPINNER.len()];

    // Snapshot list data so we can mutate picker hit/scroll freely.
    let (total, this_cwd_only, foreign_only, mut sel, mut start) = {
        let p = app.picker.as_ref().unwrap();
        let total = p.visible().len();
        (total, p.this_cwd_only, p.foreign_only, p.idx, p.scroll)
    };

    let rect = fit_modal_rect(area, 82, 40, 54, 8);
    f.render_widget(Clear, rect);
    f.render_widget(
        Block::default().style(Style::default().bg(theme::SURFACE_2)),
        rect,
    );

    let scope_label = if this_cwd_only { "here" } else { "all" };
    // Two windows, one modal: `c` switches between them, so each footer names
    // the *other* one.
    let (title, footer) = if foreign_only {
        (
            format!(" {spin}  takeover · import  ·  {total} "),
            " ↑↓/wheel  ·  ↵ import & resume  ·  tab scope  ·  c sessions  ·  esc/✕ ",
        )
    } else {
        (
            format!(" {spin}  sessions  ·  {total} "),
            " ↑↓/wheel  ·  ↵ open  ·  tab scope  ·  c takeover  ·  esc/✕ ",
        )
    };
    // Both windows default to every workspace and narrow with Tab.
    let right = Some(scope_label);
    draw_modal_frame(f, rect, phase, theme::META_BLUE, &title, right, footer);

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
        foreign: Rect::default(),
        rows: Vec::new(),
    };

    if total == 0 {
        let (empty, hint) = if foreign_only {
            (
                "  no Claude/Codex/Cursor/Grok sessions  ·  ",
                "tab scope · c sessions",
            )
        } else {
            ("  nothing here  ·  ", "tab scope · c takeover")
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(empty.to_string(), theme::style_faint()),
                Span::styled(hint.to_string(), theme::style_tool()),
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

        let mut prow: Vec<Span> = vec![Span::styled(
            marker.to_string(),
            Style::default()
                .fg(if selected { theme::BG } else { theme::META_BLUE })
                .bg(bg)
                .add_modifier(Modifier::BOLD),
        )];
        // Category tag for chagent imports (Claude/Codex/Cursor/Grok).
        let tag_w = if r.is_foreign() {
            let tag = format!("{} ", crate::agent::chagent::tool_label(&r.source));
            let w = tag.chars().count();
            prow.push(Span::styled(
                tag,
                Style::default()
                    .fg(if selected { theme::BG } else { theme::WARN })
                    .bg(bg)
                    .add_modifier(Modifier::BOLD),
            ));
            w
        } else {
            0
        };
        prow.push(Span::styled(
            truncate(&r.preview, col_w.saturating_sub(tag_w)),
            Style::default()
                .fg(prompt_fg)
                .bg(bg)
                .add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
        lines.push(Line::from(prow));

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
        let meta = if r.is_foreign() {
            // Not yet imported — no native msg/token/cost stats exist.
            format!(
                "    {}  ·  ↵ imports into a nur session{place}  ·  {short}",
                r.when,
            )
        } else {
            let cost = if r.cost > 0.0 {
                format!("  ·  ${:.3}", r.cost)
            } else {
                String::new()
            };
            format!(
                "    {}  ·  {} msgs  ·  {} tok{cost}{place}{here}  ·  {short}",
                r.when,
                r.messages,
                fmt_num(r.tokens),
            )
        };
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
    // Traveling accent head cycles the aurora ring so every modal edge shimmers.
    let accent = Style::default()
        .fg(theme::aurora_at(phase as f64 / 40.0))
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

/// Fit a modal inside the current terminal, relaxing its preferred minimums
/// when the window is smaller than the design target. Every overlay must stay
/// inside the frame so a resize immediately produces a visible, usable modal.
fn fit_modal_rect(area: Rect, desired_w: u16, desired_h: u16, min_w: u16, min_h: u16) -> Rect {
    let max_w = area.width.saturating_sub(2).max(1);
    let max_h = area.height.saturating_sub(2).max(1);
    let width = desired_w.min(max_w).max(min_w.min(max_w));
    let height = desired_h.min(max_h).max(min_h.min(max_h));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn constrain_rect(area: Rect, rect: Rect) -> Rect {
    let max_w = area.width.saturating_sub(2).max(1);
    let max_h = area.height.saturating_sub(2).max(1);
    let width = rect.width.min(max_w);
    let height = rect.height.min(max_h);
    Rect {
        x: rect
            .x
            .max(area.x)
            .min(area.right().saturating_sub(width)),
        y: rect
            .y
            .max(area.y)
            .min(area.bottom().saturating_sub(height)),
        width,
        height,
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
    // Wide scrollbar rail (2 cols) so drag is easy to grab.
    let sb_w: u16 = 2;
    // Wrap to the exact width lines render at (1 col left margin, 1 col gap,
    // then the rail) — otherwise the last columns clip under the scrollbar.
    let inner_w = area.width.saturating_sub(2 + sb_w).max(10);
    // Spinner frame bucket so animated cells re-wrap only when the glyph changes.
    let spin_i = (app.spinner_epoch.elapsed().as_millis() / theme::SPINNER_MS) as u64;
    let elapsed = app.spinner_epoch.elapsed();

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
    let mut prompt_cells: Vec<usize> = Vec::new(); // prompt ordinal → cell index
    let mut current: Option<usize> = None;

    // Rebuild hit-test maps: headers, peekable lines, exact "click to peek" span.
    let mut hit_headers: Vec<Option<usize>> = Vec::new();
    let mut line_cells: Vec<Option<usize>> = Vec::new();
    let mut line_cell_all: Vec<Option<usize>> = Vec::new();
    let mut hit_click_to_peek: Vec<Option<(usize, usize, usize)>> = Vec::new();
    let mut hit_expand_phrase: Vec<Option<(usize, usize, usize)>> = Vec::new();
    let mut hit_queue_actions: Vec<Vec<(usize, usize, usize, u8)>> = Vec::new();
    let mut hit_urls: Vec<Vec<(usize, usize, String)>> = Vec::new();
    let mut plain_lines: Vec<String> = Vec::new();

    for (cell_idx, cell) in app.cells.iter().enumerate() {
        if let Cell::User(text) = cell {
            // Animated separator before every turn except the very first — a
            // quiet shimmering rule so you can see where each exchange begins.
            if !prompts.is_empty() {
                wrapped.push(turn_separator(inner_w as usize, elapsed));
                owner.push(Some(prompts.len()));
                is_prompt_head.push(false);
                hit_headers.push(None);
                line_cells.push(None);
                line_cell_all.push(None);
                hit_click_to_peek.push(None);
                hit_expand_phrase.push(None);
                hit_queue_actions.push(Vec::new());
                hit_urls.push(Vec::new());
                plain_lines.push(String::new());
            }
            prompts.push(text.clone());
            prompt_cells.push(cell_idx);
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
            cell_lines(app, cell, cell_idx, inner_w as usize, &mut cell_out);
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
            let plain = line_to_plain(&line);
            // Exact hitbox for the words "click to peek" (display columns).
            let ctp = if is_header {
                if let Some(byte_i) = plain.find("click to peek") {
                    let start = plain[..byte_i].chars().count();
                    let end = start + "click to peek".chars().count();
                    Some((cell_idx, start, end))
                } else {
                    None
                }
            } else {
                None
            };
            // Expand / collapse phrase on header (and "… · ▸ expands" body hint).
            let exp = {
                let phrases = [
                    "▸ expands",
                    "▾ collapse",
                    "click ▾ to collapse",
                    "▸ expand",
                ];
                let mut found = None;
                for p in phrases {
                    if let Some(byte_i) = plain.find(p) {
                        let start = plain[..byte_i].chars().count();
                        let end = start + p.chars().count();
                        found = Some((cell_idx, start, end));
                        break;
                    }
                }
                found
            };
            // Queued follow-up actions (send now + dismiss may share one line).
            let mut qa: Vec<(usize, usize, usize, u8)> = Vec::new();
            if matches!(cell, Cell::Queued { .. }) {
                if let Some(byte_i) = plain.find("steer") {
                    let start = plain[..byte_i].chars().count();
                    let end = start + "steer".chars().count();
                    qa.push((cell_idx, start, end, 2u8));
                }
                if let Some(byte_i) = plain.find("send now") {
                    let start = plain[..byte_i].chars().count();
                    let end = start + "send now".chars().count();
                    qa.push((cell_idx, start, end, 0u8));
                }
                if let Some(byte_i) = plain.find("dismiss") {
                    let start = plain[..byte_i].chars().count();
                    let end = start + "dismiss".chars().count();
                    qa.push((cell_idx, start, end, 1u8));
                }
            }
            // Clickable http(s) URLs on this visual line (after wrap).
            let urls = crate::open_uri::find_url_spans(&plain);
            plain_lines.push(plain);
            wrapped.push(line);
            owner.push(current);
            // A User cell renders spacer → top border → padding → first text
            // row; any of those on screen means the prompt itself is visible.
            is_prompt_head.push(matches!(cell, Cell::User(_)) && i <= 3);
            hit_headers.push(if is_header { Some(cell_idx) } else { None });
            line_cells.push(if peekable && !empty {
                Some(cell_idx)
            } else {
                None
            });
            line_cell_all.push(if !empty { Some(cell_idx) } else { None });
            hit_click_to_peek.push(ctp);
            hit_expand_phrase.push(exp);
            hit_queue_actions.push(qa);
            hit_urls.push(urls);
        }
    }
    app.hit_headers = hit_headers;
    app.line_cells = line_cells;
    app.line_cell_all = line_cell_all;
    app.hit_click_to_peek = hit_click_to_peek;
    app.hit_expand_phrase = hit_expand_phrase;
    app.hit_queue_actions = hit_queue_actions;
    app.hit_urls = hit_urls;
    app.plain_lines = plain_lines;

    let total = wrapped.len() as u16;
    let viewport = area.height;

    // Sticky banner takes rows off the body — max_scroll must use body height
    // or the thumb/drag math fights the sticky and feels janky.
    const STICKY_H: u16 = 4;
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
    let sticky_oi = sticky_owner(&owner, &is_prompt_head, vis_lo, vis_hi);
    let sticky: Option<String> = sticky_oi.map(|oi| prompts[oi].clone());
    let sticky_cell = sticky_oi.and_then(|oi| prompt_cells.get(oi).copied());
    // Freeze sticky while drag-selecting: sticky appearing/disappearing mid-drag
    // changes body height and makes the highlight jump under the pointer.
    let sticky_h = if sticky.is_some() && app.select_anchor.is_none() && !app.selecting {
        STICKY_H
    } else {
        0
    };
    let sticky = if sticky_h > 0 { sticky } else { None };
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

    let text_w = inner_w;

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
        let banner = Rect {
            x: area.x,
            y: area.y,
            width: area.width.saturating_sub(sb_w),
            height: sticky_h,
        };
        draw_sticky_banner(f, &prompt, banner);
        // Publish so right/double-click on the header opens that prompt's menu.
        app.sticky_banner = banner;
        app.sticky_cell = sticky_cell;
    } else {
        app.sticky_banner = Rect::default();
        app.sticky_cell = None;
    }

    // Draggable scrollbar on the right edge of the transcript.
    let track = Rect {
        x: area.right().saturating_sub(sb_w),
        y: body_y,
        width: sb_w,
        height: body_h,
    };
    app.scrollbar_track = track;
    draw_scrollbar(f, app, track, top, total, body_h);

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

/// Full-width sticky prompt banner — title bar, padded prompt text with wide
/// margins, and a bottom hairline so it reads as a card pinned over the
/// transcript. The whole rect (including padding) is the right/double-click
/// hitbox for the prompt's context menu.
fn draw_sticky_banner(f: &mut Frame, prompt: &str, area: Rect) {
    f.render_widget(Clear, area);
    let bar = Style::default().bg(theme::META_BLUE);
    let surface = Style::default().bg(theme::SURFACE);

    // Row 0: solid Nur-gold title bar.
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
                " · scroll follows this turn · double/right-click for menu ".to_string(),
                Style::default().fg(theme::BLUE_100).bg(theme::META_BLUE),
            ),
        ]))
        .style(bar),
        title,
    );

    // Middle rows: prompt text with wide side margins on the surface.
    let hairline_h: u16 = if area.height >= 3 { 1 } else { 0 };
    if area.height >= 2 {
        let body = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: area.height.saturating_sub(1 + hairline_h),
        };
        let text = prompt.replace('\n', " ");
        // Wider margins all around: 5-col gutter each side.
        let avail = (area.width as usize).saturating_sub(10).max(8);
        let mut lines: Vec<Line> = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0;
        let rows = body.height as usize;
        for r in 0..rows {
            if i >= chars.len() && r > 0 {
                break;
            }
            let end = (i + avail).min(chars.len());
            let mut chunk: String = chars[i..end].iter().collect();
            i = end;
            // More prompt than rows: elide the tail.
            if r + 1 == rows && i < chars.len() {
                chunk.push('…');
            }
            let prefix = if r == 0 { "  ❯  " } else { "     " };
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
        f.render_widget(Paragraph::new(lines).style(surface), body);
    }

    // Bottom hairline: separates the pinned card from the scrolling body.
    if hairline_h == 1 {
        let edge = Rect {
            x: area.x,
            y: area.bottom().saturating_sub(1),
            width: area.width,
            height: 1,
        };
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "▁".repeat(area.width as usize),
                Style::default().fg(theme::BORDER).bg(theme::SURFACE),
            ))),
            edge,
        );
    }
}

/// Vertical scrollbar with a fractional (1/8-cell) thumb, hover and drag
/// states. Geometry comes from [`ScrollMetrics`] (tui-scrollbar-style subcell
/// math) so the thumb glides instead of jumping whole rows; drag is handled in
/// `App::on_mouse` with a grab offset so the thumb never leaps to the pointer.
fn draw_scrollbar(f: &mut Frame, app: &App, track: Rect, top: u16, total: u16, viewport: u16) {
    if track.height == 0 || track.width == 0 {
        return;
    }
    let m = ScrollMetrics::new(total as usize, viewport as usize, top as usize, track.height);
    let scrollable = m.max_offset() > 0;

    // Thumb hue steps up as you interact: idle → hover → drag.
    let thumb_fg = if app.scrollbar_drag {
        theme::META_BLUE_SKY
    } else if app.scrollbar_hover {
        theme::BLUE_250
    } else {
        theme::META_BLUE
    };
    let track_fg = if !scrollable {
        theme::dim(theme::BORDER, 0.4)
    } else if app.scrollbar_hover || app.scrollbar_drag {
        theme::BLUE_500
    } else {
        theme::BORDER
    };

    let buf = f.buffer_mut();
    let lane_x = track.x + track.width.saturating_sub(1); // thumb lane (right col)
    let edge_x = track.x; // hover-expansion lane (left col)
    for row in 0..track.height {
        let y = track.y + row;
        match m.glyph(row as usize) {
            Some(g) => {
                buf[(lane_x, y)]
                    .set_char(g)
                    .set_style(Style::default().fg(thumb_fg).bg(theme::BG));
                // Hover/drag: thumb grows to both columns — chunky, grabbable.
                if (app.scrollbar_hover || app.scrollbar_drag) && track.width >= 2 {
                    buf[(edge_x, y)]
                        .set_char(g)
                        .set_style(Style::default().fg(thumb_fg).bg(theme::BG));
                }
            }
            None => {
                buf[(lane_x, y)]
                    .set_char('│')
                    .set_style(Style::default().fg(track_fg).bg(theme::BG));
                if track.width >= 2 {
                    buf[(edge_x, y)]
                        .set_char(' ')
                        .set_style(Style::default().bg(theme::BG));
                }
            }
        }
    }
}

fn cell_lines(app: &App, cell: &Cell, cell_idx: usize, width: usize, out: &mut Vec<Line<'static>>) {
    let tick = app.spinner_epoch.elapsed();
    let flash = app
        .expand_flash
        .as_ref()
        .filter(|(i, t)| *i == cell_idx && t.elapsed().as_millis() < theme::SETTLE_MS)
        .map(|(_, t)| theme::settle_progress(t.elapsed(), theme::SETTLE_MS));
    match cell {
        Cell::Banner => banner_lines(app, out),
        Cell::User(text) => user_prompt_card(text, width, out),
        Cell::Assistant { text, streaming } => {
            out.push(Line::default());
            let md = markdown::render_markdown(text, theme::style_assistant());
            // Cool teal/mint bullet — chrome stays gold; answers should not.
            let bullet = theme::SEAFOAM;
            // render_markdown always yields ≥1 line, so gate on the source text.
            if text.trim().is_empty() && *streaming {
                out.push(Line::from(vec![
                    Span::styled("● ".to_string(), Style::default().fg(bullet)),
                    Span::styled(
                        theme::pulse_frame(tick).to_string(),
                        Style::default().fg(theme::CYAN),
                    ),
                ]));
            }
            for (i, mut l) in md.into_iter().enumerate() {
                let prefix = if i == 0 {
                    Span::styled("● ".to_string(), Style::default().fg(bullet))
                } else {
                    Span::raw("  ".to_string())
                };
                l.spans.insert(0, prefix);
                out.push(l);
            }
            if *streaming {
                if let Some(last) = out.last_mut() {
                    if theme::blink_on(tick) {
                        last.spans.push(Span::styled(
                            "█".to_string(),
                            Style::default()
                                .fg(theme::BG)
                                .bg(theme::SEAFOAM)
                                .add_modifier(Modifier::BOLD),
                        ));
                    } else {
                        last.spans.push(Span::styled(
                            "▏".to_string(),
                            Style::default().fg(theme::SEAFOAM),
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
            // Edit tools render a green/red diff (from the tool args) right in
            // the transcript card — visible collapsed, full when expanded.
            let diff: Option<Vec<String>> = if is_edit_tool(name) {
                Some(approval_preview(name, args))
            } else {
                None
            };
            let counts = diff.as_ref().map(|d| diff_counts(d));
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
            // +adds / -dels chips for edit tools (green / red, like a PR).
            if let Some((add, del)) = counts {
                head_spans.push(Span::raw("  ".to_string()));
                head_spans.push(Span::styled(
                    format!(" +{add} "),
                    Style::default()
                        .fg(theme::BG)
                        .bg(theme::SUCCESS)
                        .add_modifier(Modifier::BOLD),
                ));
                head_spans.push(Span::styled(
                    format!(" -{del} "),
                    Style::default()
                        .fg(theme::BG)
                        .bg(theme::ERROR)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            if !*expanded {
                // Always expose "click to peek" so write/edit cards open a
                // full-content dialogue (hit-test looks for that exact phrase).
                let extra = if diff.is_some() {
                    "  ·  click to peek · ▸ expands".to_string()
                } else {
                    match result {
                        Some(r) => {
                            let n = r.lines().filter(|l| !l.trim().is_empty()).count();
                            if n > 0 {
                                format!("  ·  {n} lines · click to peek · ▸ expands")
                            } else {
                                "  ·  click to peek · ▸ expands".into()
                            }
                        }
                        None => "  ·  click to peek · ▸ expands".into(),
                    }
                };
                head_spans.push(Span::styled(extra, Style::default().fg(theme::FAINT)));
            } else {
                head_spans.push(Span::styled(
                    "  ·  click to peek · ▾ collapse".to_string(),
                    Style::default().fg(theme::FAINT),
                ));
            }
            out.push(Line::from(head_spans));

            // Body. Edit tools → green/red diff bands:
            //   collapsed: compact (path + a few lines) so ▸ expand is obvious
            //   expanded: full change (capped for huge writes; peek for rest)
            // Other tools → result text only when expanded.
            if let Some(diff) = &diff {
                // Prefer full content when expanded (not the short approval cap).
                let body_lines: Vec<String> = if *expanded {
                    transcript_edit_diff(name, args)
                } else {
                    diff.clone()
                };
                // Collapsed: path + a few hunk rows (so ▸ expand is obvious).
                // Expanded: full change up to 120 lines (peek for the rest).
                let show = if *expanded {
                    body_lines.len().min(120)
                } else {
                    body_lines.len().min(4)
                };
                for l in body_lines.iter().take(show) {
                    out.push(diff_line(l, 2, width));
                }
                if body_lines.len() > show {
                    let more = body_lines.len() - show;
                    let hint = if *expanded {
                        format!("… +{more} more · click to peek for full file")
                    } else {
                        format!("… +{more} more · ▸ expands · peek for full")
                    };
                    out.push(Line::from(vec![
                        Span::raw("  ".to_string()),
                        Span::styled(hint, theme::style_faint()),
                    ]));
                }
                // Outcome under the card.
                // Failed (e.g. old_string miss): clean failure line — not a TUI bug;
                // the model tried an edit that didn't match the file.
                // Success when expanded: show the short "edited path (N replacements)" note
                // so expand/collapse is always visibly different even on tiny hunks.
                match ok {
                    Some(false) => {
                        if let Some(r) = result {
                            let first =
                                r.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                            let clean = strip_tool_error_prefix(first);
                            out.push(Line::from(vec![
                                Span::raw("  ".to_string()),
                                Span::styled("✗ ".to_string(), theme::style_error()),
                                Span::styled(
                                    truncate(
                                        &format!("failed · {clean}"),
                                        width.saturating_sub(6),
                                    ),
                                    theme::style_error(),
                                ),
                            ]));
                        }
                    }
                    Some(true) if *expanded => {
                        if let Some(r) = result {
                            let first =
                                r.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
                            if !first.is_empty() {
                                out.push(Line::from(vec![
                                    Span::raw("  ".to_string()),
                                    Span::styled("✓ ".to_string(), theme::style_success()),
                                    Span::styled(
                                        truncate(first, width.saturating_sub(6)),
                                        theme::style_faint(),
                                    ),
                                ]));
                            }
                        }
                    }
                    _ => {}
                }
            } else if *expanded {
                match result {
                    None => out.push(Line::from(vec![
                        Span::raw("  ".to_string()),
                        Span::styled(
                            format!("{} running", theme::pulse_frame(tick)),
                            Style::default().fg(hue),
                        ),
                    ])),
                    Some(r) => {
                        let all: Vec<&str> = r.lines().filter(|l| !l.trim().is_empty()).collect();
                        let body = theme::style_tool_result(name);
                        let gutter = Style::default().fg(theme::dim(hue, 0.45));
                        if all.is_empty() {
                            out.push(Line::from(vec![
                                Span::raw("  ".to_string()),
                                Span::styled("(no output)".to_string(), theme::style_faint()),
                            ]));
                        } else {
                            // Full output when expanded so drag-select / copy can
                            // grab shell commands and long tool results (wrap
                            // handles width — do not truncate away copyable text).
                            for (i, l) in all.iter().enumerate() {
                                let prefix = if i == 0 { "└ " } else { "  " };
                                let mut spans = vec![
                                    Span::raw("  ".to_string()),
                                    Span::styled(prefix.to_string(), gutter),
                                ];
                                // Shell output keeps ANSI colours (cargo, git,
                                // ls); default base is family-tinted for non-ANSI.
                                spans.extend(ansi::line_to_spans(l, body));
                                out.push(Line::from(spans));
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
        Cell::Queued { text } => {
            out.push(Line::default());
            let hue = theme::WARN;
            // Preview (one line) so the card stays compact.
            let preview: String = text.chars().take(72).collect();
            let ellip = if text.chars().count() > 72 { "…" } else { "" };
            out.push(Line::from(vec![
                Span::styled(
                    "⏳ ".to_string(),
                    Style::default().fg(hue).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "queued follow-up  ".to_string(),
                    Style::default().fg(hue).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{preview}{ellip}"),
                    theme::style_status(),
                ),
            ]));
            out.push(Line::from(vec![
                Span::raw("  ".to_string()),
                Span::styled(
                    "steer".to_string(),
                    Style::default()
                        .fg(theme::BG)
                        .bg(theme::WARN)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  ·  ".to_string(), theme::style_faint()),
                Span::styled(
                    "send now".to_string(),
                    Style::default()
                        .fg(theme::BG)
                        .bg(theme::META_BLUE)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled("  ·  ".to_string(), theme::style_faint()),
                Span::styled(
                    "dismiss".to_string(),
                    Style::default().fg(theme::MUTED).add_modifier(Modifier::UNDERLINED),
                ),
                Span::styled(
                    "  ·  inject mid-turn, or cancel + restart".to_string(),
                    theme::style_faint(),
                ),
            ]));
        }
        Cell::Graph { lines, live } => {
            out.push(Line::default());
            let hue = theme::META_BLUE;
            let head = if *live {
                "◈ execution graph · live"
            } else {
                "◈ execution graph"
            };
            out.push(Line::from(Span::styled(
                head.to_string(),
                Style::default().fg(hue).add_modifier(Modifier::BOLD),
            )));
            for l in lines {
                // Status glyphs already baked into the line text; render as-is so
                // the tree keeps its shape. Dim connector/label rows a touch.
                out.push(Line::from(Span::styled(l.clone(), theme::style_status())));
            }
        }
        Cell::Swarm { live, detail } => swarm_card(width, *live, *detail, tick, out),
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

// ── subagent swarm card ──────────────────────────────────────────────────

/// Sparkline ramp for the per-agent activity trace (index = intensity 0..8).
const PULSE_RAMP: [char; 9] = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Smallest pane that still reads: glyph + kind + a few words of task.
const MIN_PANE_W: u16 = 26;
/// Rows per pane: top border, task, tool, trace + stats, bottom border.
const PANE_H: u16 = 5;
/// Detail mode adds one row for the status line under the tool.
const PANE_H_DETAIL: u16 = 6;
/// Panes drawn at once; the rest are summarised in the footer.
const MAX_PANES: usize = 8;
/// Canvas rows the grid may grow to. This is a *height budget*, not a pane
/// count: a narrow terminal fits fewer columns, so it shows fewer panes rather
/// than turning into a tall stack that swallows a short window.
const MAX_CARD_ROWS: usize = 12;
/// Below this the framed grid is dropped for a one-line-per-agent list — two
/// columns of border chrome are not worth it on a very narrow terminal.
const COMPACT_BELOW: usize = MIN_PANE_W as usize + 2;
/// Agents listed in compact mode.
const MAX_COMPACT: usize = 6;

/// Colour + glyph for a run state. Running agents share the gold chrome hue;
/// finished ones settle into the transcript's success/error palette so a wall
/// of panes reads at a glance.
fn run_look(state: crate::agent::swarm::RunState, tick: Duration) -> (Color, String) {
    use crate::agent::swarm::RunState as S;
    match state {
        S::Running => (theme::NUR_GOLD, theme::spinner_frame(tick).to_string()),
        S::Done => (theme::SUCCESS, "✓".into()),
        S::Failed => (theme::ERROR, "✗".into()),
        S::Cancelled => (theme::MUTED, "⊘".into()),
    }
}

/// Compact token count: 4200 → `4.2k`.
fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

/// Render the activity trace into `width` characters, newest at the right.
/// A running agent's trace breathes: the newest sample pulses with the tick so
/// a stalled pane is visibly different from a busy one.
fn pulse_line(pulse: &[u8], width: usize, running: bool, tick: Duration) -> String {
    if width == 0 {
        return String::new();
    }
    let tail: Vec<u8> = pulse.iter().rev().take(width).rev().copied().collect();
    let mut s: String = std::iter::repeat_n(' ', width.saturating_sub(tail.len()))
        .chain(tail.iter().map(|v| PULSE_RAMP[(*v as usize).min(8)]))
        .collect();
    if running && !s.is_empty() {
        // Replace the last cell with a breathing head.
        let head = theme::pulse_frame(tick);
        s.pop();
        s.push_str(head);
    }
    s
}

/// `/swarm` — the subagent table as a tiled grid of live panes.
///
/// Layout comes from [`grid`]: zones in percent space, snapped to character
/// rects that tile the card exactly, then each pane is painted into one shared
/// [`grid::Canvas`] so borders stay aligned no matter how the terminal is
/// sized.
fn swarm_card(
    width: usize,
    live: bool,
    detail: bool,
    tick: Duration,
    out: &mut Vec<Line<'static>>,
) {
    use crate::agent::swarm::{self, RunState};

    let runs = swarm::snapshot();
    // Never draw wider than the viewport: the card must shrink with the window,
    // not spill and wrap. Below one legible pane it drops the frames entirely.
    let w = width.max(4);
    let compact = w < COMPACT_BELOW;

    let running = runs.iter().filter(|r| r.state == RunState::Running).count();
    let done = runs.iter().filter(|r| r.state == RunState::Done).count();
    let failed = runs
        .iter()
        .filter(|r| matches!(r.state, RunState::Failed | RunState::Cancelled))
        .count();
    let tokens: u64 = runs.iter().map(|r| r.tokens).sum();

    out.push(Line::default());

    // Header strip: title + as many rollup chips as fit, widest-value first.
    let mut chips: Vec<(String, Color)> = Vec::new();
    if running > 0 {
        chips.push((format!("{running} running"), theme::NUR_GOLD_SKY));
    }
    if done > 0 {
        chips.push((format!("{done} done"), theme::SUCCESS));
    }
    if failed > 0 {
        chips.push((format!("{failed} ended early"), theme::ERROR));
    }
    if tokens > 0 {
        chips.push((format!("Σ {} tok", fmt_tokens(tokens)), theme::MUTED));
    }
    let title = clip_to(if live { "◈ swarm · live" } else { "◈ swarm" }, w);
    let mut header = vec![Span::styled(
        title.clone(),
        Style::default()
            .fg(theme::NUR_GOLD)
            .add_modifier(Modifier::BOLD),
    )];
    let sep = if compact { " · " } else { "  ·  " };
    let mut used = UnicodeWidthStr::width(title.as_str());
    for (text, color) in chips {
        let need = sep.len() + UnicodeWidthStr::width(text.as_str());
        if used + need > w {
            break;
        }
        used += need;
        header.push(Span::styled(sep.to_string(), theme::style_faint()));
        header.push(Span::styled(text, Style::default().fg(color)));
    }
    out.push(clip_line(Line::from(header), w));

    if runs.is_empty() {
        let hint = if compact {
            "  no subagents yet"
        } else {
            "  no subagents yet — the agent tool populates this as it fans out"
        };
        out.push(Line::from(Span::styled(
            clip_to(hint, w),
            theme::style_faint(),
        )));
        return;
    }

    // Newest first: an in-flight agent should never be pushed off the grid by
    // finished history.
    let mut ordered = runs;
    ordered.sort_by_key(|r| (r.state != RunState::Running, std::cmp::Reverse(r.id)));

    if compact {
        let overflow = ordered.len().saturating_sub(MAX_COMPACT);
        ordered.truncate(MAX_COMPACT);
        for run in &ordered {
            out.push(compact_row(run, w, tick));
        }
        out.push(Line::from(Span::styled(
            clip_to(&swarm_footer(overflow, live, true), w),
            theme::style_faint(),
        )));
        return;
    }

    // A narrow window fits fewer columns, so it also shows fewer panes — the
    // card stays a card instead of becoming a tall stack of one-wide boxes.
    let pane_h = if detail { PANE_H_DETAIL } else { PANE_H };
    let fit_columns = (w / MIN_PANE_W as usize).max(1);
    let fit_rows = (MAX_CARD_ROWS / pane_h as usize).max(1);
    let max_panes = MAX_PANES.min(fit_columns * fit_rows);
    let overflow = ordered.len().saturating_sub(max_panes);
    ordered.truncate(max_panes);

    let layout = grid::layout_for(ordered.len(), w as u16, MIN_PANE_W);
    let Some(zones) = grid::model_to_zones(&layout) else {
        return;
    };
    let height = layout.rows as u16 * pane_h;
    let area = Rect {
        x: 0,
        y: 0,
        width: w as u16,
        height,
    };
    let rects = grid::zones_to_rects(&zones, area);

    let mut canvas = grid::Canvas::new(w, height as usize);
    for (run, rect) in ordered.iter().zip(rects.iter()) {
        draw_pane(&mut canvas, *rect, run, detail, tick);
    }
    out.extend(canvas.into_lines());

    out.push(Line::from(Span::styled(
        clip_to(&swarm_footer(overflow, live, false), w),
        theme::style_faint(),
    )));
}

/// Truncate to `width` display columns, marking the cut with an ellipsis.
fn clip_to(text: &str, width: usize) -> String {
    if UnicodeWidthStr::width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let cw = UnicodeWidthStr::width(ch.to_string().as_str());
        if used + cw + 1 > width {
            break;
        }
        out.push(ch);
        used += cw;
    }
    out.push('…');
    out
}

/// Hard-clip a styled line to `width` columns, keeping styles intact. The last
/// line of defence for the card's "never wider than the viewport" invariant —
/// composed rows can only get shorter, never wrap.
fn clip_line(line: Line<'static>, width: usize) -> Line<'static> {
    let total: usize = line
        .spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();
    if total <= width {
        return line;
    }
    let mut out: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for span in line.spans {
        let w = UnicodeWidthStr::width(span.content.as_ref());
        if used + w <= width.saturating_sub(1) {
            used += w;
            out.push(span);
            continue;
        }
        let room = width.saturating_sub(1).saturating_sub(used);
        if room > 0 {
            let text = clip_to(span.content.as_ref(), room + 1);
            used += UnicodeWidthStr::width(text.as_str());
            out.push(Span::styled(text, span.style));
        }
        break;
    }
    if used < width {
        out.push(Span::styled("…".to_string(), theme::style_faint()));
    }
    Line::from(out)
}

fn swarm_footer(overflow: usize, live: bool, compact: bool) -> String {
    let mut s = String::from("  ");
    if overflow > 0 {
        s.push_str(&format!("+{overflow} more · "));
    }
    s.push_str(match (live, compact) {
        (true, true) => "live · /swarm off",
        (true, false) => "updates live · /swarm off to freeze · /swarm clear to drop finished",
        (false, true) => "frozen · /swarm",
        (false, false) => "frozen · /swarm to resume",
    });
    s
}

/// One agent as a single dense row, for terminals too narrow to frame a pane.
fn compact_row(
    run: &crate::agent::swarm::AgentRun,
    width: usize,
    tick: Duration,
) -> Line<'static> {
    use crate::agent::swarm::RunState;
    let running = run.state == RunState::Running;
    let (hue, glyph) = run_look(run.state, tick);
    let elapsed = if running {
        theme::fmt_elapsed_live(run.elapsed())
    } else {
        theme::fmt_duration(run.elapsed())
    };
    let head = format!("{glyph} {}·{} ", run.id, run.kind);
    // Shed the trailing stats before the body: knowing *what* an agent is doing
    // beats knowing how long it has been at it once space runs out.
    let budget = width
        .saturating_sub(2)
        .saturating_sub(UnicodeWidthStr::width(head.as_str()));
    let tail = [
        format!(" {elapsed} {}⚒", run.tools_done),
        format!(" {elapsed}"),
        String::new(),
    ]
    .into_iter()
    .find(|t| budget.saturating_sub(UnicodeWidthStr::width(t.as_str())) >= 6)
    .unwrap_or_default();
    let room = budget.saturating_sub(UnicodeWidthStr::width(tail.as_str()));
    let body = run.tool.as_deref().unwrap_or(&run.activity);
    let body = clip_to(body, room);
    let pad = room.saturating_sub(UnicodeWidthStr::width(body.as_str()));
    clip_line(
        Line::from(vec![
            Span::styled("  ".to_string(), theme::style_faint()),
            Span::styled(head, Style::default().fg(hue)),
            Span::styled(body, theme::style_status()),
            Span::styled(" ".repeat(pad), theme::style_faint()),
            Span::styled(tail, theme::style_faint()),
        ]),
        width,
    )
}

/// Paint one agent pane into the shared canvas.
fn draw_pane(
    canvas: &mut grid::Canvas,
    rect: Rect,
    run: &crate::agent::swarm::AgentRun,
    detail: bool,
    tick: Duration,
) {
    use crate::agent::swarm::RunState;
    if rect.width < 8 || rect.height < 3 {
        return;
    }
    let running = run.state == RunState::Running;
    let (hue, glyph) = run_look(run.state, tick);
    let border = Style::default().fg(if running {
        theme::dim(hue, 0.55)
    } else {
        theme::BORDER
    });

    canvas.frame(rect, border, None);

    let x = rect.x as usize + 2;
    let inner_w = rect.width.saturating_sub(4) as usize;
    let right = rect.x as usize + rect.width as usize - 2;
    let top = rect.y as usize;
    let last = rect.y as usize + rect.height as usize - 1;

    // Title woven into the top border: "╭ ⠋ 3·explore ──── 12.4s ─╮". The id
    // makes a pane referable ("what is #3 doing") when several are in flight.
    let elapsed = if running {
        theme::fmt_elapsed_live(run.elapsed())
    } else {
        theme::fmt_duration(run.elapsed())
    };
    let title = format!("{glyph} {}·{}", run.id, run.kind);
    let title_room = inner_w.saturating_sub(elapsed.chars().count() + 3);
    canvas.text(x - 1, top, " ", border, 1);
    let used = canvas.text_clipped(
        x,
        top,
        &title,
        Style::default().fg(hue).add_modifier(Modifier::BOLD),
        title_room,
    );
    canvas.text(x + used, top, " ", border, 1);
    canvas.text_right(
        right,
        top,
        &format!(" {elapsed} "),
        theme::style_duration_chip(running),
    );

    // Body rows: task, current tool, (detail: status), then the trace pinned to
    // the last inner row so panes of different heights still line up.
    let mut y = rect.y as usize + 1;
    if y < last {
        canvas.text_clipped(x, y, &run.task, theme::style_assistant(), inner_w);
        y += 1;
    }
    if y < last {
        let (mark, style) = match (&run.tool, running) {
            (Some(_), _) => ("▸ ", Style::default().fg(theme::CYAN)),
            (None, true) => ("· ", theme::style_status()),
            (None, false) => ("· ", theme::style_faint()),
        };
        let label = run.tool.as_deref().unwrap_or(&run.activity);
        let used = canvas.text(x, y, mark, style, inner_w);
        canvas.text_clipped(x + used, y, label, style, inner_w.saturating_sub(used));
        y += 1;
    }
    if detail && y < last {
        canvas.text_clipped(x, y, &run.activity, theme::style_faint(), inner_w);
    }

    let trace_y = last.saturating_sub(1);
    if trace_y > rect.y as usize {
        // Stats right-aligned; the activity trace flows into whatever is left.
        let mut stats = format!("{}⚒", run.tools_done);
        if run.tools_failed > 0 {
            stats.push_str(&format!(" {}✗", run.tools_failed));
        }
        if run.tokens > 0 {
            stats.push(' ');
            stats.push_str(&fmt_tokens(run.tokens));
        }
        let stats_w = UnicodeWidthStr::width(stats.as_str());
        canvas.text_right(right, trace_y, &stats, theme::style_faint());
        let trace_w = inner_w.saturating_sub(stats_w + 2);
        let trace = pulse_line(&run.pulse, trace_w, running, tick);
        canvas.text(
            x,
            trace_y,
            &trace,
            Style::default().fg(if running { hue } else { theme::dim(hue, 0.5) }),
            trace_w,
        );
    }
}

/// User prompt as a bordered card — a rounded Nur-gold frame with a padding
/// row above/below and inner margins, so prompts read as distinct blocks in
/// the transcript. Every border/padding row belongs to the prompt cell, which
/// also makes the right-click / double-click context-menu hitbox much larger
/// than the text alone.
fn user_prompt_card(text: &str, width: usize, out: &mut Vec<Line<'static>>) {
    let w = width.max(12);
    let border = Style::default().fg(theme::META_BLUE);
    let label = Style::default()
        .fg(theme::META_BLUE_SKY)
        .add_modifier(Modifier::BOLD);
    // Inner text width: "│  " + text + "  │"
    let text_w = w.saturating_sub(6).max(4);

    out.push(Line::default());

    // Top border with a " ❯ you " label woven in.
    let title = " ❯ you ";
    let dashes = w.saturating_sub(2 + 1 + title.chars().count());
    out.push(Line::from(vec![
        Span::styled("╭─".to_string(), border),
        Span::styled(title.to_string(), label),
        Span::styled(format!("{}╮", "─".repeat(dashes)), border),
    ]));

    // Padding row + wrapped text rows + padding row, all inside │ … │.
    let blank_inner = |out: &mut Vec<Line<'static>>| {
        out.push(Line::from(vec![
            Span::styled("│".to_string(), border),
            Span::raw(" ".repeat(w.saturating_sub(2))),
            Span::styled("│".to_string(), border),
        ]));
    };
    blank_inner(out);
    for src in text.lines() {
        let chars: Vec<char> = src.chars().collect();
        let mut i = 0usize;
        loop {
            let end = (i + text_w).min(chars.len());
            let chunk: String = chars[i..end].iter().collect();
            let pad = text_w.saturating_sub(chunk.chars().count());
            out.push(Line::from(vec![
                Span::styled("│  ".to_string(), border),
                Span::styled(chunk, theme::style_user()),
                Span::raw(" ".repeat(pad)),
                Span::styled("  │".to_string(), border),
            ]));
            i = end;
            if i >= chars.len() {
                break;
            }
        }
        if chars.is_empty() {
            blank_inner(out);
        }
    }
    blank_inner(out);
    out.push(Line::from(Span::styled(
        format!("╰{}╯", "─".repeat(w.saturating_sub(2))),
        border,
    )));
}

/// Highlight drag-selected characters with a Nur-gold selection wash.
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

/// Stable click-to-peek dialogue only (hover peeks removed).
/// Geometry is frozen on first open and never moves until closed.
fn draw_hover_peek(f: &mut Frame, app: &mut App, area: Rect) -> Option<(Rect, Rect)> {
    let idx = app.active_peek_cell()?;

    struct PeekData {
        title: String,
        body: String,
        hue: Color,
        diff: Option<Vec<String>>,
        thinking: bool,
        #[cfg_attr(not(feature = "image-peek"), allow(dead_code))]
        image: Option<String>,
    }
    let p = {
        let cell = app.cells.get(idx)?;
        if !cell.is_peekable() {
            return None;
        }
        let hue = match cell {
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
        let (diff, image) = if let Cell::Tool { name, args, .. } = cell {
            let diff = if is_edit_tool(name) {
                Some(approval_preview(name, args))
            } else {
                None
            };
            let image = if name == "look" {
                image_arg_path(args)
            } else {
                None
            };
            (diff, image)
        } else {
            (None, None)
        };
        PeekData {
            title: cell.peek_title()?,
            body: cell.peek_body().unwrap_or_default(),
            hue,
            diff,
            thinking: matches!(cell, Cell::Thinking { .. }),
            image,
        }
    };

    // Freeze geometry once — never re-anchor to mouse or re-center.
    let rect = if let Some(r) = app.peek_frozen {
        let fitted = constrain_rect(area, r);
        app.peek_frozen = Some(fitted);
        fitted
    } else {
        let desired_w = (area.width.saturating_mul(7) / 10).clamp(40, 96);
        let desired_h = (area.height.saturating_mul(6) / 10).clamp(8, 28);
        let r = fit_modal_rect(area, desired_w, desired_h, 20, 5);
        app.peek_frozen = Some(r);
        r
    };

    f.render_widget(Clear, rect);
    // Static frame — no phase/pulse animation (that felt jumpy).
    f.render_widget(
        Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_style(Style::default().fg(p.hue))
            .style(Style::default().bg(theme::SURFACE_2))
            .title(format!(" {} ", p.title))
            .title_bottom(" Esc · outside · ✕ · Ctrl+C "),
        rect,
    );
    let inner = Rect {
        x: rect.x.saturating_add(1),
        y: rect.y.saturating_add(1),
        width: rect.width.saturating_sub(2),
        height: rect.height.saturating_sub(2),
    };

    // Clickable ✕ on the top-right of the box (matches the sessions picker).
    let close = Rect::new(rect.x + rect.width.saturating_sub(4), rect.y, 3, 1);
    {
        let cx = rect.x + rect.width.saturating_sub(3);
        let buf = f.buffer_mut();
        buf[(cx, rect.y)].set_char('✕').set_style(
            Style::default()
                .fg(theme::ERROR)
                .bg(theme::SURFACE_2)
                .add_modifier(Modifier::BOLD),
        );
    }

    // Vision peek: render the actual image via the terminal's graphics
    // protocol (sixel / kitty / iTerm2, halfblocks fallback) — ratatui-image.
    // Gated behind `image-peek`; without it, peeks fall through to text/diff.
    #[cfg(feature = "image-peek")]
    if let Some(path) = &p.image {
        if let Some(proto) = app.image_protocol(path) {
            f.render_stateful_widget(
                ratatui_image::StatefulImage::default(),
                inner,
                proto,
            );
            app.peek_rows = inner.height;
            return Some((rect, close));
        }
    }

    // Content: green/red diff bands for edit tools, soft-wrapped (ANSI-clean)
    // text for everything else. Capped generously; the body scrolls.
    const PEEK_MAX_ROWS: usize = 400;
    let mut lines: Vec<Line> = Vec::new();
    let max_cols = (inner.width as usize).saturating_sub(2).max(8);

    if let Some(diff) = &p.diff {
        let w = (inner.width as usize).saturating_sub(1);
        for l in diff.iter().take(PEEK_MAX_ROWS) {
            lines.push(diff_line(l, 0, w));
        }
        if diff.len() > PEEK_MAX_ROWS {
            lines.push(Line::from(Span::styled(
                format!("… +{} more · e expands in place", diff.len() - PEEK_MAX_ROWS),
                theme::style_faint(),
            )));
        }
    } else {
        let clean = ansi::strip(&p.body);
        let style = if p.thinking {
            theme::style_thinking_violet()
        } else {
            Style::default().fg(theme::FG)
        };
        'outer: for raw in clean.lines() {
            // Soft wrap long lines into the dialogue.
            let mut rest = raw;
            let mut first = true;
            loop {
                if lines.len() >= PEEK_MAX_ROWS {
                    lines.push(Line::from(Span::styled(
                        "… truncated".to_string(),
                        Style::default().fg(theme::FAINT),
                    )));
                    break 'outer;
                }
                let chunk: String = rest.chars().take(max_cols).collect();
                let advanced = chunk.len();
                rest = if advanced >= rest.len() {
                    ""
                } else {
                    &rest[advanced..]
                };
                lines.push(Line::from(Span::styled(
                    if first { chunk } else { format!("  {chunk}") },
                    style,
                )));
                first = false;
                if rest.is_empty() {
                    break;
                }
            }
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "(empty)".to_string(),
            theme::style_faint(),
        )));
    }

    let total_rows = lines.len() as u16;
    app.peek_rows = total_rows;
    let bg = Style::default().bg(theme::SURFACE_2);

    if total_rows <= inner.height {
        // Fits — plain paragraph, no scroll state to keep.
        app.peek_scroll = 0;
        f.render_widget(Paragraph::new(lines).style(bg), inner);
    } else {
        // Overflows — tui-scrollview: content renders into an offscreen buffer
        // and the wheel (over the box) drives the offset; it draws its own
        // scrollbar on the right edge.
        use ratatui::widgets::StatefulWidget;
        let scroll = app
            .peek_scroll
            .min(total_rows.saturating_sub(inner.height));
        app.peek_scroll = scroll;
        let size = Size::new(inner.width.saturating_sub(1).max(1), total_rows);
        let mut sv = tui_scrollview::ScrollView::new(size);
        sv.render_widget(
            Paragraph::new(lines).style(bg),
            Rect::new(0, 0, size.width, size.height),
        );
        let mut st =
            tui_scrollview::ScrollViewState::with_offset(Position::new(0, scroll));
        StatefulWidget::render(sv, inner, f.buffer_mut(), &mut st);
    }
    Some((rect, close))
}

/// Workspace image path from a `look` tool's args, if it's a decodable format.
fn image_arg_path(args: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(args).ok()?;
    let p = v.get("path")?.as_str()?;
    let ext = std::path::Path::new(p)
        .extension()?
        .to_str()?
        .to_lowercase();
    if matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp") {
        Some(p.to_string())
    } else {
        None
    }
}

/// Per-character aurora shimmer for a run of text — a colour wave travels
/// through it over time. `row_offset` phases successive rows into a diagonal.
fn shimmer_spans(text: &str, elapsed: Duration, row_offset: usize, period_ms: u128) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let span = chars.len().max(1);
    chars
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            let col = i + row_offset;
            Span::styled(
                c.to_string(),
                Style::default().fg(theme::aurora_cell(elapsed, col, span, period_ms)),
            )
        })
        .collect()
}

/// A full-width horizontal rule whose colour shimmers along the aurora ring.
/// `glyph` is repeated; the whole strip drifts over time.
fn aurora_rule(width: usize, elapsed: Duration, glyph: char, period_ms: u128) -> Line<'static> {
    if width == 0 {
        return Line::default();
    }
    let spans: Vec<Span<'static>> = (0..width)
        .map(|i| {
            Span::styled(
                glyph.to_string(),
                Style::default().fg(theme::aurora_cell(elapsed, i, width, period_ms)),
            )
        })
        .collect();
    Line::from(spans)
}

/// A soft, mostly-dim separator between transcript turns with a travelling
/// bright node — quiet but alive.
fn turn_separator(width: usize, elapsed: Duration) -> Line<'static> {
    if width < 6 {
        return Line::default();
    }
    let inner = width.saturating_sub(4);
    // Position of the bright node sweeping left→right, ease-out restart.
    let cycle = 2600u128;
    let t = theme::ease_out((elapsed.as_millis() % cycle) as f64 / cycle as f64);
    let head = (t * inner as f64) as usize;
    let mut spans = vec![Span::raw("  ".to_string())];
    for i in 0..inner {
        let d = i.abs_diff(head);
        let (ch, col) = match d {
            0 => ('◆', theme::aurora_cell(elapsed, i, inner, 1600)),
            1 => ('◇', theme::dim(theme::aurora_cell(elapsed, i, inner, 1600), 0.35)),
            _ => ('·', theme::BORDER),
        };
        spans.push(Span::styled(ch.to_string(), Style::default().fg(col)));
    }
    Line::from(spans)
}

fn banner_lines(app: &App, out: &mut Vec<Line<'static>>) {
    // NUR logotype — same motion (diagonal shimmer + aurora underline) as before.
    let logo = [
        r#"███╗   ██╗██╗   ██╗██████╗ "#,
        r#"████╗  ██║██║   ██║██╔══██╗"#,
        r#"██╔██╗ ██║██║   ██║██████╔╝"#,
        r#"██║╚██╗██║██║   ██║██╔══██╗"#,
        r#"██║ ╚████║╚██████╔╝██║  ██║"#,
        r#"╚═╝  ╚═══╝ ╚═════╝ ╚═╝  ╚═╝"#,
    ];
    let elapsed = app.spinner_epoch.elapsed();
    out.push(Line::default());
    for (i, row) in logo.iter().enumerate() {
        // Diagonal aurora wave sweeping the logotype.
        let mut spans = vec![Span::raw("  ".to_string())];
        spans.extend(shimmer_spans(row, elapsed, i * 3, 2400));
        out.push(Line::from(spans));
    }
    // Shimmering underline beneath the logotype.
    out.push(aurora_rule(40, elapsed, '─', 2200));

    // Row 1: "<active provider> loaded  ·  v<cli>".
    let provider = crate::config::active_provider_label(&app.cfg);
    let sparkle = theme::frame_at(theme::SPARKLE, elapsed, 200);
    let mut title_row = vec![
        Span::raw("  ".to_string()),
        Span::styled(
            format!("{sparkle} "),
            Style::default().fg(theme::aurora_cell(elapsed, 0, 1, 1500)),
        ),
    ];
    title_row.extend(shimmer_spans(&format!("{provider} loaded"), elapsed, 0, 2000));
    title_row.push(Span::styled("   ·   ".to_string(), theme::style_faint()));
    title_row.push(Span::styled(
        format!("v{}", env!("CARGO_PKG_VERSION")),
        theme::style_faint(),
    ));
    out.push(Line::from(title_row));

    // Session facts only — model · provider · cwd · session. Feature maps,
    // ecosystem packs, and mode tips live behind /help · /tips · /ecosystem.
    let session8 = &app.session_id[..8.min(app.session_id.len())];
    out.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled("model  ".to_string(), theme::style_faint()),
        Span::styled(
            app.cfg.model.clone(),
            Style::default().fg(theme::META_BLUE_SKY),
        ),
        Span::styled("    provider  ".to_string(), theme::style_faint()),
        Span::styled(provider.clone(), Style::default().fg(theme::SEAFOAM)),
    ]));
    out.push(Line::from(vec![
        Span::raw("  ".to_string()),
        Span::styled("cwd  ".to_string(), theme::style_faint()),
        Span::styled(app.cwd.display().to_string(), theme::style_status()),
        Span::styled("    session  ".to_string(), theme::style_faint()),
        Span::styled(session8.to_string(), theme::style_faint()),
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
        // Spinner cycles through the aurora ring as it turns.
        let spin = theme::spinner_frame(tick);
        spans.push(Span::styled(
            spin.to_string(),
            Style::default()
                .fg(theme::aurora_cell(tick, 0, 1, 900))
                .add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!("  {}  ", capitalize(&app.status)),
            Style::default().fg(theme::META_BLUE_SKY),
        ));
        spans.push(Span::styled(live, theme::style_faint()));
        // Decorative ease-out activity strip, per-cell aurora colour.
        let bar_w = 16usize.min(area.width.saturating_sub(48) as usize);
        if bar_w >= 6 {
            spans.push(Span::raw("  ".to_string()));
            let glyphs: Vec<char> = theme::activity_bar(elapsed, bar_w).chars().collect();
            for (i, ch) in glyphs.iter().enumerate() {
                let c = if *ch == '·' {
                    theme::dim(theme::aurora_cell(elapsed, i, bar_w, 1400), 0.4)
                } else {
                    theme::aurora_cell(elapsed, i, bar_w, 1400)
                };
                spans.push(Span::styled(ch.to_string(), Style::default().fg(c)));
            }
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
/// Max soft-wrapped rows shown in the composer before scrolling.
const INPUT_VIEW_MAX: usize = 8;

fn draw_input(f: &mut Frame, app: &mut App, area: Rect) {
    let tick = app.spinner_epoch.elapsed();
    // Border is calm-but-alive when ready for input (slow whole-border aurora
    // shimmer), and quietly dim while a turn runs or a modal owns focus.
    let active_border = !app.busy && app.approval.is_none();
    let border_color = if active_border {
        theme::aurora_cell(tick, 0, 1, 3200)
    } else {
        theme::BORDER
    };

    // Active provider chrome (tracks /login) — not a hard-coded product brand.
    let provider = crate::config::active_provider_chrome(&app.cfg);
    let title = if app.busy {
        let t = if app.queue.is_empty() {
            format!(" {provider} · working ")
        } else {
            format!(" {provider} · working · {} queued ", app.queue.len())
        };
        Span::styled(t, theme::style_faint())
    } else {
        Span::styled(
            format!(" {provider} "),
            Style::default()
                .fg(theme::aurora_cell(tick, 3, 6, 3200))
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

    // A single bright node scans along the top edge when ready — subtle life.
    if active_border && area.width > 4 {
        let inner_w = area.width.saturating_sub(2) as usize;
        let cycle = 2000u128;
        let t = theme::ease_out((tick.as_millis() % cycle) as f64 / cycle as f64);
        let hx = ((t * inner_w as f64) as usize).min(inner_w.saturating_sub(1)) as u16;
        let buf = f.buffer_mut();
        buf[(area.x + 1 + hx, area.y)]
            .set_char('━')
            .set_style(
                Style::default()
                    .fg(theme::BLUE_050)
                    .bg(theme::SURFACE)
                    .add_modifier(Modifier::BOLD),
            );
    }

    let focused = app.approval.is_none() && app.picker.is_none() && app.login.is_none();
    let sel = app.input.selection_range();
    let sel_style = Style::default()
        .fg(theme::BG)
        .bg(theme::META_BLUE_SKY)
        .add_modifier(Modifier::BOLD);
    let normal = Style::default().fg(theme::FG);
    let chip_style = Style::default()
        .fg(theme::META_BLUE)
        .bg(theme::SURFACE)
        .add_modifier(Modifier::BOLD);
    let chip_sel_style = Style::default()
        .fg(theme::BG)
        .bg(theme::META_BLUE)
        .add_modifier(Modifier::BOLD);

    // Reverse history search (Ctrl+R) takes over the composer body so the user
    // sees the query and the matched entry before accepting it.
    if app.input.search_is_active() {
        let query = app.input.search_query().unwrap_or("").to_string();
        let mut spans = vec![
            Span::styled(
                "❯ ".to_string(),
                Style::default()
                    .fg(theme::META_BLUE)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("(reverse-search)`{query}`: "), theme::style_faint()),
        ];
        match app.input.search_match_text() {
            Some(m) => {
                // Collapse to a single-line preview; the raw entry is what gets
                // accepted into the composer on Enter.
                let first = m.lines().next().unwrap_or("");
                let preview = if m.contains('\n') {
                    format!("{first} …")
                } else {
                    first.to_string()
                };
                spans.push(Span::styled(preview, normal));
            }
            None => spans.push(Span::styled(
                "(no match — Esc cancels)".to_string(),
                theme::style_faint(),
            )),
        }
        f.render_widget(
            Paragraph::new(vec![Line::from(spans)]).style(theme::style_surface()),
            inner,
        );
        if focused {
            f.set_cursor_position((inner.x + 2, inner.y));
        }
        app.input_area = area;
        app.input_inner = inner;
        return;
    }

    // Content width after "❯ " / "  " prefix.
    let usable = (inner.width as usize).saturating_sub(3).max(1);
    app.input_usable_w = usable;
    let vrows = app.input.visual_rows(usable);
    let vcount = vrows.len().max(1);
    let h = (inner.height as usize).max(1).min(INPUT_VIEW_MAX);
    app.input_view_h = h;
    let max_top = vcount.saturating_sub(h);
    if app.input_scroll_top > max_top {
        app.input_scroll_top = max_top;
    }
    let top = app.input_scroll_top;
    // Soft-wrap replaces horizontal pan for normal text; keep x_off at 0.
    app.input_x_off = 0;

    let mut lines: Vec<Line> = Vec::new();
    if app.input.is_empty() {
        let hint = if app.busy {
            "type a follow-up — Enter queues it"
        } else {
            "plan, build, debug  ·  paste large text → chip  ·  wheel scrolls draft"
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
        let cursor = app.input.cursor_index();
        let buf_len = app.input.text().chars().count();
        for (vi, row) in vrows.iter().enumerate().skip(top).take(h) {
            let prefix = if vi == 0 { "❯ " } else { "  " };
            let mut spans = vec![Span::styled(
                prefix.to_string(),
                Style::default()
                    .fg(theme::META_BLUE)
                    .add_modifier(Modifier::BOLD),
            )];
            let mut run = String::new();
            let mut run_sel = false;
            let flush = |run: &mut String, run_sel: bool, spans: &mut Vec<Span>| {
                if run.is_empty() {
                    return;
                }
                let style = if run_sel { sel_style } else { normal };
                spans.push(Span::styled(std::mem::take(run), style));
            };
            let mut i = row.abs_start;
            while i < row.abs_end {
                let is_sel = sel.map(|(lo, hi)| i >= lo && i < hi).unwrap_or(false);
                let caret_on = focused && i == cursor;
                if let Some(label) = app.input.chip_label_at(i) {
                    flush(&mut run, run_sel, &mut spans);
                    let style = if caret_on {
                        if is_sel {
                            chip_sel_style
                        } else {
                            Style::default()
                                .fg(theme::BG)
                                .bg(theme::META_BLUE)
                                .add_modifier(Modifier::BOLD)
                        }
                    } else if is_sel {
                        chip_sel_style
                    } else {
                        chip_style
                    };
                    spans.push(Span::styled(label, style));
                    i += 1;
                    continue;
                }
                let Some(ch) = app.input.char_at(i) else {
                    break;
                };
                if caret_on {
                    flush(&mut run, run_sel, &mut spans);
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
                    run.push(ch);
                } else {
                    if run.is_empty() {
                        run_sel = is_sel;
                    }
                    run.push(ch);
                }
                i += 1;
            }
            flush(&mut run, run_sel, &mut spans);
            // Caret at end of buffer on the last visual row, or on an empty row.
            let caret_at_end = focused
                && cursor == buf_len
                && cursor == row.abs_end
                && (vi + 1 == vcount || row.abs_start == row.abs_end);
            if caret_at_end {
                spans.push(Span::styled(" ".to_string(), theme::style_cursor_on()));
            }
            lines.push(Line::from(spans));
        }
    }

    // Scroll indicator in the title area when content overflows.
    if max_top > 0 {
        let shown_from = top + 1;
        let shown_to = (top + h).min(vcount);
        let ind = format!(" {shown_from}–{shown_to}/{vcount} ");
        let ind_w = ind.width() as u16;
        if area.width > ind_w + 8 {
            let x = area.right().saturating_sub(ind_w + 1);
            let buf = f.buffer_mut();
            for (i, ch) in ind.chars().enumerate() {
                let cell = &mut buf[(x + i as u16, area.y)];
                cell.set_char(ch);
                cell.set_style(theme::style_faint());
            }
        }
    }

    f.render_widget(
        Paragraph::new(lines).style(theme::style_surface()),
        inner,
    );

    if focused && !app.input.is_empty() {
        let (vr, vc) = app.input.cursor_visual_pos(usable);
        if vr >= top && vr < top + h {
            let cx = inner.x + 2 + vc as u16;
            let cy = inner.y + (vr - top) as u16;
            if cx < inner.right() && cy < inner.bottom() {
                f.set_cursor_position((cx, cy));
            }
        }
    } else if focused && app.input.is_empty() {
        f.set_cursor_position((inner.x + 2, inner.y));
    }

    app.input_area = area;
    app.input_inner = inner;
    app.input_scroll_top = top;
}

// ── statusline ─────────────────────────────────────────────────────────────
fn draw_statusline(f: &mut Frame, app: &App, area: Rect) {
    let u = &app.u_session;
    let ctx_used = app.u_last.input_tokens + app.u_last.output_tokens;
    let ctx_win = app.cfg.context_window;
    let ctx_pct = if ctx_win > 0 {
        (ctx_used as f64 / ctx_win as f64 * 100.0).min(100.0)
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
    // Separators slowly cycle the aurora ring so the whole strip feels alive.
    let statick = app.spinner_epoch.elapsed();
    let sep = || {
        Span::styled(
            "  ·  ".to_string(),
            Style::default().fg(theme::dim(theme::aurora_cell(statick, 0, 1, 4000), 0.25)),
        )
    };

    // Dollar values are list-price estimates (~). Wide terminals also show
    // context as used/window so the % isn't a black box.
    let cost = u.estimated_cost_usd();
    let cost_label = if cost > 0.0 || u.total_tokens > 0 {
        format!("~${:.4}", cost)
    } else {
        "~$0".into()
    };
    let wide = area.width >= 96;
    let tok_label = if wide && (u.input_tokens > 0 || u.output_tokens > 0) {
        format!(
            "{} tok ({}↑ {}↓)",
            fmt_num(u.total_tokens),
            fmt_num(u.input_tokens),
            fmt_num(u.output_tokens)
        )
    } else {
        format!("{} tok", fmt_num(u.total_tokens))
    };
    let ctx_label = if wide && ctx_win > 0 {
        format!(
            "ctx {ctx_pct:.0}% {}/{}",
            fmt_num(ctx_used),
            fmt_num(ctx_win)
        )
    } else {
        format!("ctx {ctx_pct:.0}%")
    };

    let left = vec![
        Span::raw(" ".to_string()),
        state_dot,
        Span::styled(tok_label, Style::default().fg(theme::BLUE_200)),
        sep(),
        Span::styled(cost_label, Style::default().fg(theme::TEAL)),
        sep(),
        Span::styled(ctx_label, ctx_style),
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
    let area = f.area();
    let mut rect = fit_modal_rect(area, 60, content + 4, 34, 4);
    rect.x = input_area
        .x
        .saturating_add(1)
        .min(area.right().saturating_sub(rect.width));
    rect.y = input_area
        .y
        .saturating_sub(rect.height)
        .max(area.y);
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
    let vis = inner.height as usize;
    // Clamp palette_scroll so the keyboard selection is always visible.
    let mut start = app.palette_scroll;
    start = start.max(sel.saturating_sub(vis.saturating_sub(1)));
    let max_scroll = matches.len().saturating_sub(vis);
    start = start.min(max_scroll);
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
    let rect = fit_modal_rect(area, 78, content + 4, 48, 7);
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

/// Tools whose transcript card renders a green/red diff.
fn is_edit_tool(name: &str) -> bool {
    matches!(name, "write_file" | "edit_file" | "multi_edit" | "apply_patch")
}

/// Full edit/write content for an **expanded** transcript card (vs short
/// `approval_preview` used when collapsed / in the approval modal).
fn transcript_edit_diff(tool: &str, args: &str) -> Vec<String> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(args) else {
        return pretty_args(args)
            .lines()
            .map(|s| s.to_string())
            .take(80)
            .collect();
    };
    match tool {
        "edit_file" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("?");
            let old = v.get("old_string").and_then(|x| x.as_str()).unwrap_or("");
            let new = v.get("new_string").and_then(|x| x.as_str()).unwrap_or("");
            let mut out = vec![format!("path {path}")];
            out.extend(mini_unified_diff(old, new, 80));
            out
        }
        "write_file" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("?");
            let content = v.get("content").and_then(|x| x.as_str()).unwrap_or("");
            let n = content.lines().count();
            let mut out = vec![format!("path {path}  (write · {n} lines)")];
            for l in content.lines().take(100) {
                out.push(format!("+{l}"));
            }
            if n > 100 {
                out.push(format!("… +{} more lines", n - 100));
            }
            out
        }
        "multi_edit" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("?");
            let mut out = vec![format!("path {path}  (multi_edit)")];
            if let Some(edits) = v.get("edits").and_then(|e| e.as_array()) {
                out.push(format!("@@ {} edit(s)", edits.len()));
                for (i, e) in edits.iter().enumerate() {
                    let old = e.get("old_string").and_then(|x| x.as_str()).unwrap_or("");
                    let new = e.get("new_string").and_then(|x| x.as_str()).unwrap_or("");
                    out.push(format!("── edit {} ──", i + 1));
                    out.extend(mini_unified_diff(old, new, 24));
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
            patch.lines().take(120).map(|s| s.to_string()).collect()
        }
        _ => approval_preview(tool, args),
    }
}

/// `error: tool error: foo` → `foo` for a clean transcript failure line.
fn strip_tool_error_prefix(s: &str) -> String {
    let mut t = s.trim();
    for p in ["error: tool error: ", "error: ", "tool error: "] {
        if let Some(rest) = t.strip_prefix(p) {
            t = rest.trim();
        }
    }
    t.to_string()
}

/// Classify a unified-diff line.
enum DiffKind {
    Add,
    Del,
    Meta,
    Context,
}

fn diff_kind(l: &str) -> DiffKind {
    if l.starts_with('+') && !l.starts_with("+++") {
        DiffKind::Add
    } else if l.starts_with('-') && !l.starts_with("---") {
        DiffKind::Del
    } else if l.starts_with("@@")
        || l.starts_with("path ")
        || l.starts_with("cmd ")
        || l.starts_with("── ")
    {
        DiffKind::Meta
    } else {
        DiffKind::Context
    }
}

/// Count added / removed lines in a diff preview.
fn diff_counts(lines: &[String]) -> (usize, usize) {
    let mut add = 0;
    let mut del = 0;
    for l in lines {
        match diff_kind(l) {
            DiffKind::Add => add += 1,
            DiffKind::Del => del += 1,
            _ => {}
        }
    }
    (add, del)
}

/// Render one diff line as a full-width Claude-Code style band: a coloured
/// gutter bar, `+`/`-`/space sign, tinted text, and a subtle background so
/// added/removed rows read as blocks. `indent` is the left margin (spaces).
fn diff_line(l: &str, indent: usize, width: usize) -> Line<'static> {
    let pad = " ".repeat(indent);
    match diff_kind(l) {
        DiffKind::Add => {
            let body = l.strip_prefix('+').unwrap_or(l);
            let text = pad_to(&format!("{body}"), width.saturating_sub(indent + 2));
            Line::from(vec![
                Span::raw(pad),
                Span::styled("▎".to_string(), Style::default().fg(theme::SUCCESS)),
                Span::styled(
                    format!("+{text}"),
                    Style::default().fg(theme::DIFF_ADD_FG).bg(theme::DIFF_ADD_BG),
                ),
            ])
        }
        DiffKind::Del => {
            let body = l.strip_prefix('-').unwrap_or(l);
            let text = pad_to(&format!("{body}"), width.saturating_sub(indent + 2));
            Line::from(vec![
                Span::raw(pad),
                Span::styled("▎".to_string(), Style::default().fg(theme::ERROR)),
                Span::styled(
                    format!("-{text}"),
                    Style::default().fg(theme::DIFF_DEL_FG).bg(theme::DIFF_DEL_BG),
                ),
            ])
        }
        DiffKind::Meta => Line::from(vec![
            Span::raw(pad),
            Span::styled(
                truncate(l, width.saturating_sub(indent)),
                Style::default().fg(theme::DIFF_META).add_modifier(Modifier::BOLD),
            ),
        ]),
        DiffKind::Context => Line::from(vec![
            Span::raw(pad),
            Span::styled("  ".to_string(), Style::default()),
            Span::styled(truncate(l, width.saturating_sub(indent + 2)), theme::style_faint()),
        ]),
    }
}

/// Right-pad a string to `w` display columns so diff band backgrounds fill the row.
fn pad_to(s: &str, w: usize) -> String {
    let cur = s.width();
    if cur >= w {
        // Truncate to width for a clean band edge.
        truncate(s, w)
    } else {
        format!("{s}{}", " ".repeat(w - cur))
    }
}

/// Stable-ish fingerprint so wrap cache can skip finished cells.
fn cell_wrap_key(cell: &Cell, spin_i: u64) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    match cell {
        Cell::Banner => {
            1u8.hash(&mut h);
            // Banner gradient shimmers — re-wrap each spinner frame.
            spin_i.hash(&mut h);
        }
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
        Cell::Queued { text } => {
            9u8.hash(&mut h);
            text.hash(&mut h);
        }
        Cell::Graph { lines, live } => {
            10u8.hash(&mut h);
            lines.hash(&mut h);
            live.hash(&mut h);
            if *live {
                spin_i.hash(&mut h);
            }
        }
        Cell::Swarm { live, detail } => {
            11u8.hash(&mut h);
            live.hash(&mut h);
            detail.hash(&mut h);
            if *live {
                // Reads the registry every frame; the spinner tick is the clock.
                spin_i.hash(&mut h);
            } else {
                // Frozen: fingerprint the table so a later run still repaints.
                for run in crate::agent::swarm::snapshot() {
                    run.id.hash(&mut h);
                    format!("{:?}", run.state).hash(&mut h);
                    run.tools_done.hash(&mut h);
                    run.tokens.hash(&mut h);
                }
            }
        }
        Cell::Error(t) => {
            8u8.hash(&mut h);
            t.hash(&mut h);
        }
    }
    h.finish()
}

/// Full write/edit content for the peek dialogue (not the short card preview).
pub fn tool_file_peek_body(name: &str, args: &str, result: Option<&str>) -> String {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(args) else {
        return format!("tool: {name}\nargs: {args}\n---\n{}", result.unwrap_or(""));
    };
    let mut s = String::new();
    s.push_str(&format!("tool: {name}\n"));
    match name {
        "write_file" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("?");
            let content = v.get("content").and_then(|x| x.as_str()).unwrap_or("");
            let n = content.lines().count();
            s.push_str(&format!("path: {path}\nlines: {n}\n---\n"));
            s.push_str(content);
            if !content.ends_with('\n') {
                s.push('\n');
            }
        }
        "edit_file" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("?");
            let old = v.get("old_string").and_then(|x| x.as_str()).unwrap_or("");
            let new = v.get("new_string").and_then(|x| x.as_str()).unwrap_or("");
            s.push_str(&format!("path: {path}\n--- old ---\n{old}\n--- new ---\n{new}\n"));
        }
        "multi_edit" => {
            let path = v.get("path").and_then(|x| x.as_str()).unwrap_or("?");
            s.push_str(&format!("path: {path}\n"));
            if let Some(edits) = v.get("edits").and_then(|e| e.as_array()) {
                s.push_str(&format!("edits: {}\n", edits.len()));
                for (i, e) in edits.iter().enumerate() {
                    let old = e.get("old_string").and_then(|x| x.as_str()).unwrap_or("");
                    let new = e.get("new_string").and_then(|x| x.as_str()).unwrap_or("");
                    s.push_str(&format!("\n── edit {} ──\n- old:\n{old}\n+ new:\n{new}\n", i + 1));
                }
            }
        }
        "apply_patch" => {
            let patch = v
                .get("patch")
                .or_else(|| v.get("input"))
                .and_then(|x| x.as_str())
                .unwrap_or(args);
            s.push_str("---\n");
            s.push_str(patch);
        }
        _ => {
            s.push_str(&format!("args: {args}\n"));
        }
    }
    if let Some(r) = result {
        if !r.trim().is_empty() {
            s.push_str("\n--- result ---\n");
            s.push_str(r);
        }
    }
    s
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

fn draw_ctx_menu(f: &mut Frame, app: &mut App) {
    use super::app::CTX_ACTIONS;
    let area = f.area();
    // content rows + 2 border + 2 inner padding, so the shared ornate frame fits.
    let desired_h: u16 = CTX_ACTIONS.len() as u16 + 4;
    let fitted = fit_modal_rect(area, 34, desired_h, 20, 4);
    let menu_w = fitted.width;
    let menu_h = fitted.height;
    // Anchor to where the menu OPENED (fixed), not the live cursor — so
    // wheeling through the options never drifts the box.
    let (ax, ay) = app.ctx_menu.as_ref().map(|m| m.anchor).unwrap_or((0, 0));
    let x = ax.min(area.right().saturating_sub(menu_w).saturating_sub(1));
    let y = ay.min(area.bottom().saturating_sub(menu_h).saturating_sub(1));
    let frame = Rect::new(x, y, menu_w, menu_h);

    f.render_widget(Clear, frame);
    f.render_widget(
        Block::default().style(Style::default().bg(theme::SURFACE_2)),
        frame,
    );
    let phase = modal_phase(app);
    draw_modal_frame(
        f,
        frame,
        phase,
        theme::META_BLUE,
        " prompt ",
        None,
        "  ↑↓/wheel move  ·  ↵ choose  ·  esc  ",
    );
    let inner = modal_inner(frame);

    let sel = app.ctx_menu.as_ref().map(|m| m.selected).unwrap_or(0);
    let mut actions = Vec::new();
    for (i, (glyph, label)) in CTX_ACTIONS.iter().enumerate() {
        let ar = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
        let selected = i == sel;
        let (fg, bg) = if selected {
            (theme::BG, theme::META_BLUE)
        } else {
            (theme::FG, theme::SURFACE_2)
        };
        let marker = if selected { "❯ " } else { "  " };
        let line = Line::from(vec![
            Span::styled(
                format!("{marker}{glyph}  "),
                Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{label:<width$}", width = inner.width.saturating_sub(6) as usize),
                Style::default().fg(fg).bg(bg),
            ),
        ]);
        f.render_widget(
            Paragraph::new(line).style(Style::default().bg(bg)),
            ar,
        );
        actions.push((i, ar));
    }

    if let Some(menu) = &mut app.ctx_menu {
        menu.hit.frame = frame;
        menu.hit.actions = actions;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── swarm card ───────────────────────────────────────────────────────

    /// Render the card and return its plain-text rows.
    fn swarm_rows(width: usize, live: bool, detail: bool) -> Vec<String> {
        let mut out = Vec::new();
        swarm_card(width, live, detail, Duration::from_millis(0), &mut out);
        out.iter().map(line_to_plain).collect()
    }

    /// Seed the registry with a representative fan-out.
    fn seed_swarm(n: usize) {
        use crate::agent::swarm::{self, RunState};
        swarm::reset();
        let tasks = [
            ("explore", "map every call site of run_subagent"),
            ("general", "port the grid engine tests to the new API"),
            ("plan", "design the /swarm card layout"),
            ("explore", "find where images enter the chat body"),
            ("general", "run the failing local-model repro"),
            ("explore", "audit provider catalog for local entries"),
        ];
        for i in 0..n {
            let (kind, task) = tasks[i % tasks.len()];
            let id = swarm::begin(kind, task);
            for t in 0..(i + 2) {
                swarm::tool_start(id, ["grep", "read_file", "bash", "glob"][t % 4]);
                swarm::tool_end(id, t % 5 != 4);
            }
            match i % 3 {
                0 => {} // still running
                1 => swarm::finish(id, RunState::Done, 4200 * (i as u64 + 1)),
                _ => swarm::finish(id, RunState::Failed, 900),
            }
        }
    }

    /// Not an assertion — `cargo test swarm_preview -- --ignored --nocapture`
    /// prints the card at a few sizes so the layout can be eyeballed.
    #[test]
    #[ignore]
    fn swarm_preview() {
        let _g = crate::agent::swarm::test_lock();
        for (n, width) in [
            (1usize, 96usize),
            (3, 96),
            (5, 120),
            (8, 150),
            (4, 60),
            (5, 40),
            (5, 26),
            (5, 20),
        ] {
            seed_swarm(n);
            println!("\n──── {n} agents @ {width} cols ────");
            for row in swarm_rows(width, true, false) {
                println!("{row}");
            }
        }
    }

    #[test]
    fn swarm_card_is_empty_but_helpful_with_no_subagents() {
        let _g = crate::agent::swarm::test_lock();
        crate::agent::swarm::reset();
        let rows = swarm_rows(100, true, false);
        assert!(rows[1].contains("swarm"));
        assert!(
            rows.iter().any(|r| r.contains("no subagents yet")),
            "empty state must explain itself: {rows:?}"
        );
    }

    #[test]
    fn swarm_panes_tile_to_the_full_card_width() {
        let _g = crate::agent::swarm::test_lock();
        for n in 1..=6 {
            seed_swarm(n);
            for width in [60usize, 97, 140] {
                let rows = swarm_rows(width, true, false);
                // Grid rows are the ones drawn by the canvas: every one of them
                // is exactly `width` columns, which is what keeps borders aligned.
                let grid_rows: Vec<&String> = rows
                    .iter()
                    .filter(|r| r.starts_with('╭') || r.starts_with('│') || r.starts_with('╰'))
                    .collect();
                assert!(!grid_rows.is_empty(), "n={n} w={width} drew no panes");
                for row in grid_rows {
                    assert_eq!(
                        UnicodeWidthStr::width(row.as_str()),
                        width,
                        "n={n} w={width} row {row:?} must fill the card"
                    );
                }
            }
        }
    }

    #[test]
    fn swarm_rollup_counts_every_state() {
        let _g = crate::agent::swarm::test_lock();
        seed_swarm(6);
        let rows = swarm_rows(120, true, false);
        let header = &rows[1];
        assert!(header.contains("2 running"), "header: {header}");
        assert!(header.contains("2 done"), "header: {header}");
        assert!(header.contains("2 ended early"), "header: {header}");
        assert!(header.contains("tok"), "header: {header}");
    }

    #[test]
    fn swarm_shows_running_agents_first_and_flags_overflow() {
        let _g = crate::agent::swarm::test_lock();
        seed_swarm(12);
        let rows = swarm_rows(160, true, false);
        let body = rows.join("\n");
        assert!(body.contains("+4 more"), "overflow must be disclosed:\n{body}");
        // Every still-running agent survives the truncation to MAX_PANES.
        let running = crate::agent::swarm::running_count();
        // Panes sit side by side on shared rows, so count glyphs, not rows.
        let live_panes: usize = rows
            .iter()
            .map(|r| {
                theme::SPINNER
                    .iter()
                    .map(|frame| r.matches(frame).count())
                    .sum::<usize>()
            })
            .sum();
        assert!(
            live_panes >= running,
            "expected ≥{running} live panes, found {live_panes}"
        );
    }

    #[test]
    fn swarm_detail_mode_adds_the_activity_row() {
        let _g = crate::agent::swarm::test_lock();
        seed_swarm(1);
        let plain = swarm_rows(90, true, false);
        let detailed = swarm_rows(90, true, true);
        assert_eq!(
            detailed.len(),
            plain.len() + 1,
            "detail adds exactly one row per grid row"
        );
        assert!(
            detailed.iter().any(|r| r.contains("read_file"))
                && detailed.iter().filter(|r| r.contains("read_file")).count() > plain
                    .iter()
                    .filter(|r| r.contains("read_file"))
                    .count(),
            "detail surfaces the status line as well as the tool"
        );
    }

    /// Resizing must never produce a row wider than the viewport — that is what
    /// wraps a card into visual garbage — and must never panic, at any width
    /// the TUI will hand us (`draw` already refuses terminals under 20 cols).
    #[test]
    fn swarm_card_fits_every_width_from_tiny_to_wide() {
        let _g = crate::agent::swarm::test_lock();
        for agents in [1usize, 3, 7, 12] {
            seed_swarm(agents);
            for width in 4..=200usize {
                for detail in [false, true] {
                    let rows = swarm_rows(width, true, detail);
                    assert!(!rows.is_empty(), "agents={agents} w={width} drew nothing");
                    for row in &rows {
                        assert!(
                            UnicodeWidthStr::width(row.as_str()) <= width,
                            "agents={agents} w={width} detail={detail} overflowed: {row:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn narrow_terminals_drop_the_frames_for_a_compact_list() {
        let _g = crate::agent::swarm::test_lock();
        seed_swarm(4);
        let rows = swarm_rows(COMPACT_BELOW - 1, true, false);
        assert!(
            !rows.iter().any(|r| r.contains('╭')),
            "no frames below the compact threshold: {rows:?}"
        );
        // One row per agent, still showing state + id.
        assert!(rows.iter().filter(|r| r.contains('⚒')).count() >= 1);

        // One column wider and the framed grid comes back.
        seed_swarm(4);
        let framed = swarm_rows(COMPACT_BELOW, true, false);
        assert!(framed.iter().any(|r| r.starts_with('╭')));
    }

    #[test]
    fn a_narrow_window_shows_fewer_panes_rather_than_a_tall_stack() {
        let _g = crate::agent::swarm::test_lock();
        seed_swarm(12);
        let narrow = swarm_rows(30, true, false);
        let wide = swarm_rows(180, true, false);
        assert!(
            narrow.len() <= wide.len(),
            "narrow card ({} rows) must not be taller than the wide one ({} rows)",
            narrow.len(),
            wide.len()
        );
        let framed_rows = narrow.iter().filter(|r| r.starts_with('╭')).count();
        assert!(
            framed_rows * PANE_H as usize <= MAX_CARD_ROWS,
            "the grid must respect its height budget"
        );
        assert!(
            narrow.iter().any(|r| r.contains("more")),
            "the panes it dropped must be disclosed: {narrow:?}"
        );
    }

    #[test]
    fn frozen_swarm_card_says_how_to_resume() {
        let _g = crate::agent::swarm::test_lock();
        seed_swarm(2);
        let rows = swarm_rows(100, false, false);
        assert!(rows[1].contains("swarm") && !rows[1].contains("live"));
        assert!(rows.last().unwrap().contains("/swarm to resume"));
    }

    #[test]
    fn strip_tool_error_prefix_cleans_double_prefix() {
        assert_eq!(
            strip_tool_error_prefix("error: tool error: old_string not found in file"),
            "old_string not found in file"
        );
        assert_eq!(
            strip_tool_error_prefix("tool error: path missing"),
            "path missing"
        );
        assert_eq!(strip_tool_error_prefix("plain"), "plain");
    }

    #[test]
    fn transcript_edit_diff_edit_file_includes_path_and_hunk() {
        let args = r#"{"path":"a.rs","old_string":"foo","new_string":"bar"}"#;
        let lines = transcript_edit_diff("edit_file", args);
        assert!(lines.iter().any(|l| l.contains("path a.rs")));
        assert!(lines.iter().any(|l| l.starts_with("-foo") || l == "-foo"));
        assert!(lines.iter().any(|l| l.starts_with("+bar") || l == "+bar"));
    }

    #[test]
    fn diff_kinds_and_counts() {
        let diff = vec![
            "path src/x.rs".to_string(),
            "@@ -3 +4 @@".to_string(),
            "-old line".to_string(),
            "+new line".to_string(),
            "+another".to_string(),
            " context".to_string(),
        ];
        assert_eq!(diff_counts(&diff), (2, 1));
        assert!(matches!(diff_kind("+add"), DiffKind::Add));
        assert!(matches!(diff_kind("-del"), DiffKind::Del));
        // Unified-diff file markers are NOT add/del rows.
        assert!(matches!(diff_kind("+++ b/x"), DiffKind::Meta | DiffKind::Context));
        assert!(matches!(diff_kind("--- a/x"), DiffKind::Meta | DiffKind::Context));
        assert!(matches!(diff_kind("@@ -1 +1 @@"), DiffKind::Meta));
    }

    #[test]
    fn edit_tools_are_diffed() {
        for t in ["write_file", "edit_file", "multi_edit", "apply_patch"] {
            assert!(is_edit_tool(t), "{t} should render a diff");
        }
        for t in ["bash", "read_file", "grep", "web_fetch"] {
            assert!(!is_edit_tool(t));
        }
    }

    #[test]
    fn diff_band_fills_to_width() {
        // Added/removed bands must be exactly `width` cols so the bg fills the row.
        let add = diff_line("+hello", 2, 40);
        let del = diff_line("-bye", 2, 40);
        let wsum = |l: &Line| -> usize { l.spans.iter().map(|s| s.content.width()).sum() };
        assert_eq!(wsum(&add), 40);
        assert_eq!(wsum(&del), 40);
    }

    #[test]
    fn modal_rects_stay_inside_small_terminal_after_resize() {
        let area = Rect::new(0, 0, 38, 10);
        let rect = fit_modal_rect(area, 82, 40, 54, 8);
        assert!(rect.right() <= area.right());
        assert!(rect.bottom() <= area.bottom());
        assert_eq!(rect.width, 36);
        assert_eq!(rect.height, 8);

        let tiny = Rect::new(0, 0, 18, 5);
        let rect = fit_modal_rect(tiny, 82, 40, 54, 8);
        assert!(rect.right() <= tiny.right());
        assert!(rect.bottom() <= tiny.bottom());
        assert_eq!(rect.width, 16);
        assert_eq!(rect.height, 3);
    }

    #[test]
    fn frozen_peek_rect_is_constrained_after_resize() {
        let area = Rect::new(0, 0, 32, 8);
        let rect = constrain_rect(area, Rect::new(20, 10, 80, 28));
        assert!(rect.right() <= area.right());
        assert!(rect.bottom() <= area.bottom());
        assert_eq!(rect.width, 30);
        assert_eq!(rect.height, 6);
    }

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
