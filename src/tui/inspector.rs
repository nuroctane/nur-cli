//! One read-only home for session details. No filesystem or provider I/O in draw.
use super::app::{App, Cell};
use crate::theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Copy)]
pub enum Target {
    Cell(usize),
    Agent(u64),
}

type DetailRow = (String, Option<Target>, ratatui::style::Color);

#[derive(Default)]
pub struct Inspector {
    pub open: bool,
    pub tab: usize,
    pub scroll: usize,
    pub area: Rect,
    pub tabs: Vec<Rect>,
    pub rows: Vec<(Rect, Target)>,
    pub max_scroll: usize,
    cache_key: u64,
    cache_rows: Vec<DetailRow>,
}

pub const TABS: [&str; 4] = ["Changes", "Tools", "Agents", "Context"];

/// Keep both context and filename when a path or command exceeds the panel.
fn fit_label(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let budget = width - 1;
    let mut left = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let size = ch.width().unwrap_or(0);
        if used + size > budget / 2 {
            break;
        }
        left.push(ch);
        used += size;
    }
    let mut right = Vec::new();
    for ch in text.chars().rev() {
        let size = ch.width().unwrap_or(0);
        if used + size > budget {
            break;
        }
        right.push(ch);
        used += size;
    }
    format!("{left}…{}", right.into_iter().rev().collect::<String>())
}

pub fn is_change(name: &str) -> bool {
    matches!(
        name,
        "apply_patch" | "edit_file" | "write_file" | "multi_edit"
    )
}

fn detail_rows(app: &App) -> Vec<DetailRow> {
    let mut rows = Vec::new();
    match app.inspector.tab {
        0 | 1 => {
            for (index, cell) in app.cells.iter().enumerate().rev() {
                if let Cell::Tool {
                    name,
                    args,
                    ok,
                    duration,
                    ..
                } = cell
                {
                    if app.inspector.tab == 0 && !is_change(name) {
                        continue;
                    }
                    let (mark, hue) = match ok {
                        Some(true) => ("✓", theme::SUCCESS()),
                        Some(false) => ("✗", theme::ERROR()),
                        None => ("◌", theme::FG()),
                    };
                    let elapsed = duration
                        .map(theme::fmt_duration)
                        .unwrap_or_else(|| "running".into());
                    rows.push((
                        format!("{mark} {name} · {elapsed}"),
                        Some(Target::Cell(index)),
                        hue,
                    ));
                    let args: serde_json::Value = serde_json::from_str(args).unwrap_or_default();
                    let label = ["path", "file_path", "command", "query"]
                        .iter()
                        .find_map(|key| args.get(key).and_then(|value| value.as_str()))
                        .unwrap_or("Open to inspect arguments and result");
                    rows.push((
                        format!("  {}", label.lines().next().unwrap_or_default()),
                        Some(Target::Cell(index)),
                        theme::FG(),
                    ));
                    rows.push((String::new(), None, theme::FG()));
                }
            }
        }
        2 => {
            for run in crate::agent::swarm::snapshot().iter().rev() {
                rows.push((
                    format!("{:?} · {}", run.state, run.task),
                    Some(Target::Agent(run.id)),
                    theme::FG(),
                ));
                rows.push((
                    format!("{} · {}", run.provider, run.model),
                    Some(Target::Agent(run.id)),
                    theme::dim(theme::FG(), 0.2),
                ));
                rows.push((
                    run.activity.clone(),
                    Some(Target::Agent(run.id)),
                    theme::FG(),
                ));
                rows.push((String::new(), None, theme::FG()));
            }
        }
        _ => {
            for text in [
                format!("Provider  {}", app.cfg.provider),
                format!("Model     {}", app.cfg.model),
                format!("Session   {}", app.session_id),
                String::new(),
                format!("Input     {} tokens", app.u_session.input_tokens),
                format!("Output    {} tokens", app.u_session.output_tokens),
                format!("Cached    {} tokens", app.u_session.cached_tokens),
                String::new(),
                format!("Queued    {} messages", app.queue.len()),
                format!("Draft     {} images", app.draft_image_indices().len()),
                "Token counts are session totals.".into(),
                "Use /context for context details.".into(),
            ] {
                rows.push((text, None, theme::FG()));
            }
        }
    }
    if rows.is_empty() {
        rows.push((
            match app.inspector.tab {
                0 => "No file-edit tool calls yet.",
                1 => "No tool activity yet.",
                _ => "No agent activity yet.",
            }
            .into(),
            None,
            theme::dim(theme::FG(), 0.2),
        ));
    }
    rows
}

pub fn draw(f: &mut Frame, app: &mut App, area: Rect) {
    if app.inspector.tab < 2 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        app.inspector.tab.hash(&mut hash);
        // Running spinners and streamed prose do not invalidate tool metadata.
        // Only structural tool changes require parsing the arguments again.
        for (index, cell) in app.cells.iter().enumerate() {
            if let Cell::Tool {
                name,
                args,
                ok,
                duration,
                ..
            } = cell
            {
                (index, name, args, ok, duration).hash(&mut hash);
            }
        }
        for color in [
            theme::FG(),
            theme::dim(theme::FG(), 0.2),
            theme::SUCCESS(),
            theme::ERROR(),
        ] {
            color.hash(&mut hash);
        }
        let key = hash.finish();
        if key != app.inspector.cache_key || app.inspector.cache_rows.is_empty() {
            app.inspector.cache_rows = detail_rows(app);
            app.inspector.cache_key = key;
        }
        let rows = std::mem::take(&mut app.inspector.cache_rows);
        draw_rows(f, &mut app.inspector, &rows, area);
        app.inspector.cache_rows = rows;
    } else {
        let rows = detail_rows(app);
        draw_rows(f, &mut app.inspector, &rows, area);
    }
}

pub fn draw_rows(f: &mut Frame, inspector: &mut Inspector, rows: &[DetailRow], area: Rect) {
    inspector.area = area;
    inspector.tabs.clear();
    inspector.rows.clear();
    f.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::dim(theme::FG(), 0.2)))
        .style(theme::style_surface())
        .title(Span::styled(
            format!(" {} · F6 close ", TABS[inspector.tab]),
            Style::default().fg(theme::dim(theme::FG(), 0.2)),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.height < 3 || inner.width == 0 {
        return;
    }
    let tab_width = inner.width / 4;
    for (index, name) in TABS.iter().enumerate() {
        let rect = Rect::new(inner.x + index as u16 * tab_width, inner.y, tab_width, 1);
        inspector.tabs.push(rect);
        let selected = index == inspector.tab;
        let label = if tab_width < 7 {
            format!("{}", index + 1)
        } else {
            name.to_string()
        };
        let style = if selected {
            Style::default()
                .fg(theme::FG())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::dim(theme::FG(), 0.2))
        };
        f.render_widget(Paragraph::new(Span::styled(label, style)), rect);
    }
    let height = inner.height.saturating_sub(3) as usize;
    inspector.max_scroll = rows.len().saturating_sub(height);
    inspector.scroll = inspector.scroll.min(inspector.max_scroll);
    for (offset, (text, cell, hue)) in rows.iter().skip(inspector.scroll).take(height).enumerate() {
        let rect = Rect::new(inner.x, inner.y + 2 + offset as u16, inner.width, 1);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                fit_label(text, inner.width as usize),
                Style::default().fg(*hue),
            ))),
            rect,
        );
        if let Some(cell) = cell {
            inspector.rows.push((rect, *cell));
        }
    }
    f.render_widget(
        Paragraph::new(Span::styled(
            "Alt+1-4 tabs · Alt+PgUp/Dn scroll",
            Style::default().fg(theme::dim(theme::FG(), 0.2)),
        )),
        Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};
    #[test]
    fn truncation_preserves_filename_and_display_width() {
        let text = "  src/very/long/module/路径/session.rs";
        let clipped = fit_label(text, 28);
        assert!(clipped.ends_with("session.rs"));
        assert!(clipped.width() <= 28);
        for width in 0..30 {
            assert!(fit_label("目录/🦀/session.rs", width).width() <= width);
        }
    }
    #[test]
    fn rows_and_tabs_stay_inside_panel_after_resize() {
        let rows: Vec<_> = (0..50)
            .map(|i| (format!("tool {i}"), Some(Target::Cell(i)), theme::FG()))
            .collect();
        let mut inspector = Inspector {
            scroll: usize::MAX,
            ..Default::default()
        };
        for (width, height) in [(42, 20), (20, 8), (20, 3), (1, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|f| draw_rows(f, &mut inspector, &rows, f.area()))
                .unwrap();
            assert!(inspector
                .rows
                .iter()
                .all(|(r, _)| r.right() <= width && r.bottom() <= height));
            assert!(inspector
                .tabs
                .iter()
                .all(|r| r.right() <= width && r.bottom() <= height));
        }
    }
}
