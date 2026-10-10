//! Transcript cards: every piece of transcript output (prompt, answer,
//! thought, tool, notice, error, ...) sits in a rounded frame coloured from
//! the active theme's role for its kind, so the transcript reads as a column
//! of distinct, colour-coded pieces instead of one stream.
//!
//! The frame is drawn *around* the content, never into it. Rows are laid out
//! at the content width exactly as before, and `draw_transcript` publishes
//! the content rect as `transcript_body`, so every column-based hitbox
//! (chevrons, "click to peek", links, swarm panes) and the selection/copy
//! mapping stay in content columns. A card's top and bottom edges are blank
//! content rows that [`paint_row`] draws over.
//!
//! Consecutive one-line finished tools share one frame (a tool run): a frame
//! each would triple their height and say nothing more. Each row's sides keep
//! that tool's own family colour.
//!
//! The NUR banner is a locked design and stays unframed.

use super::app::Cell;
use super::row_index::RowIndex;
use super::wrap::LinkSpan;
use crate::theme::{self, FrameWeight};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::Line,
};

/// Columns a frame takes on each side of the content: the edge glyph and one
/// column of padding.
pub const SIDE: u16 = 2;

/// How one cell's rows sit in its frame. Stored per cell beside its wrapped
/// rows, so a frame is resolved once per content change, not per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CardFrame {
    /// Edge colour, already resolved against the active theme. `None` for
    /// unframed cells (the banner).
    pub hue: Option<Color>,
    /// Row 0 is this card's top edge (false inside a tool run).
    pub top: bool,
    /// The last row is this card's bottom edge (false inside a tool run).
    pub bottom: bool,
}

/// What a framed row is to its card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Side,
    Bottom,
}

impl CardFrame {
    /// Edge role of row `row` of a card that is `rows` tall.
    pub fn edge(&self, row: usize, rows: usize) -> Option<Edge> {
        self.hue?;
        Some(if self.top && row == 0 {
            Edge::Top
        } else if self.bottom && row + 1 == rows {
            Edge::Bottom
        } else {
            Edge::Side
        })
    }
}

/// Whether `cell` gets a frame at all.
pub fn is_framed(cell: &Cell) -> bool {
    !matches!(cell, Cell::Banner | Cell::Image { queued: true, .. })
}

/// A one-line finished tool: the only kind that shares its neighbours' frame.
fn joins_run(cell: &Cell) -> bool {
    matches!(
        cell,
        Cell::Tool {
            ok: Some(true),
            expanded: false,
            ..
        }
    )
}

/// Which edges cell `idx` owns: `(top, bottom)`. A tool-run member gives up
/// the edge it shares with an adjacent run member. Queued draft images are
/// skipped because they never reach the transcript.
pub fn edges(cells: &[Cell], idx: usize) -> (bool, bool) {
    let Some(cell) = cells.get(idx) else {
        return (false, false);
    };
    if !is_framed(cell) {
        return (false, false);
    }
    if !joins_run(cell) {
        return (true, true);
    }
    let shown = |c: &&Cell| !matches!(c, Cell::Image { queued: true, .. });
    let above = cells[..idx].iter().rev().find(shown).is_some_and(joins_run);
    let below = cells[idx + 1..].iter().find(shown).is_some_and(joins_run);
    (!above, !below)
}

/// The theme role and weight for a cell's frame. Settled history is quiet,
/// work in progress and answers are normal, prompts and errors are strong.
pub fn frame_hue(cell: &Cell) -> Option<Color> {
    use FrameWeight::{Normal, Quiet, Strong};
    let (hue, weight) = match cell {
        Cell::Banner | Cell::Image { queued: true, .. } => return None,
        Cell::User(_) => (theme::NUR_GOLD(), Strong),
        Cell::Image { .. } => (theme::NUR_GOLD(), Normal),
        Cell::Assistant { .. } => (theme::SEAFOAM(), Normal),
        Cell::Thinking { active: true, .. } => (theme::VIOLET(), Normal),
        Cell::Thinking { .. } => (theme::VIOLET(), Quiet),
        Cell::Tool {
            ok: Some(false), ..
        } => (theme::ERROR(), Normal),
        Cell::Tool { name, ok: None, .. } => (theme::tool_color(name), Normal),
        Cell::Tool {
            name,
            expanded: true,
            ..
        } => (theme::tool_color(name), Normal),
        Cell::Tool { name, .. } => (theme::tool_color(name), Quiet),
        Cell::TurnDone {
            interrupted: true, ..
        } => (theme::WARN(), Quiet),
        Cell::TurnDone { .. } => (theme::SUCCESS(), Quiet),
        Cell::Info { tone, .. } => (tone.color(), Quiet),
        Cell::Queued { .. } => (theme::WARN(), Normal),
        Cell::Graph { .. } | Cell::Swarm { .. } => (theme::NUR_GOLD(), Normal),
        Cell::Error(_) => (theme::ERROR(), Strong),
    };
    Some(theme::card_frame(hue, weight))
}

/// Whether transcript row `row` is a card's bottom edge, read from the last
/// draw: its row index, per-cell frames and per-cell row counts.
pub fn is_bottom_edge(
    rows: &RowIndex,
    frames: &[CardFrame],
    height: impl Fn(usize) -> usize,
    row: usize,
) -> bool {
    let Some((Some(cell), i)) = rows.get(row) else {
        return false;
    };
    frames.get(cell).and_then(|card| card.edge(i, height(cell))) == Some(Edge::Bottom)
}

/// A row with nothing to show: whitespace only, and no band behind it (a
/// blank row inside a code block keeps its background and is content).
fn is_spacer(line: &Line<'_>) -> bool {
    line.spans
        .iter()
        .all(|s| s.content.trim().is_empty() && s.style.bg.is_none())
        && line.style.bg.is_none()
}

/// Lay a cell's wrapped rows into its frame. The spacer rows renderers lead
/// or end with are dropped (edges now separate cards), then a blank row is
/// added for each edge the card owns. A card with no content keeps no rows,
/// so an empty answer never shows as an empty box. Links move with their rows.
pub fn fit_rows(
    rows: &mut Vec<Line<'static>>,
    links: &mut Vec<Vec<LinkSpan>>,
    top: bool,
    bottom: bool,
) {
    links.resize_with(rows.len(), Vec::new);
    let lead = rows.iter().take_while(|l| is_spacer(l)).count();
    if lead == rows.len() {
        rows.clear();
        links.clear();
        return;
    }
    let trail = rows.iter().rev().take_while(|l| is_spacer(l)).count();
    rows.truncate(rows.len() - trail);
    links.truncate(rows.len());
    rows.drain(..lead);
    links.drain(..lead);
    if top {
        rows.insert(0, Line::default());
        links.insert(0, Vec::new());
    }
    if bottom {
        rows.push(Line::default());
        links.push(Vec::new());
    }
}

/// Paint one row of frame chrome. `outer` is the row's full frame width: its
/// first column is the left edge, its last the right edge, and the content
/// sits [`SIDE`] columns inside. Side rows touch only the edge columns, so
/// the text already rendered between them is left exactly as it was.
pub fn paint_row(buf: &mut Buffer, outer: Rect, edge: Edge, hue: Color) {
    let area = outer.intersection(buf.area);
    if area.width < 2 || area.height == 0 || area.x != outer.x || area.width != outer.width {
        return;
    }
    let style = Style::default().fg(hue).bg(theme::BG());
    let (y, x0, x1) = (outer.y, outer.x, outer.right() - 1);
    let (left, fill, right) = match edge {
        Edge::Top => ('╭', Some('─'), '╮'),
        Edge::Bottom => ('╰', Some('─'), '╯'),
        Edge::Side => ('│', None, '│'),
    };
    buf[(x0, y)].set_char(left).set_style(style);
    buf[(x1, y)].set_char(right).set_style(style);
    if let Some(fill) = fill {
        for x in x0 + 1..x1 {
            buf[(x, y)].set_char(fill).set_style(style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;
    use ratatui::text::Span;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn tool(ok: Option<bool>, expanded: bool) -> Cell {
        Cell::Tool {
            name: "read_file".into(),
            args: "{}".into(),
            result: Some("x".into()),
            ok,
            started: Instant::now(),
            duration: Some(Duration::from_millis(5)),
            expanded,
        }
    }

    fn text(rows: &[Line<'static>]) -> Vec<String> {
        rows.iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    // Spacers around the content become the edges; links stay on their text.
    #[test]
    fn fitting_replaces_spacers_with_edges_and_keeps_links_on_their_rows() {
        let mut rows = vec![
            Line::default(),
            Line::from("first"),
            Line::from("second"),
            Line::from("  "),
        ];
        let link: LinkSpan = (0, 6, Arc::from("https://x.test"));
        let mut links = vec![vec![], vec![], vec![link.clone()], vec![]];
        fit_rows(&mut rows, &mut links, true, true);
        assert_eq!(text(&rows), ["", "first", "second", ""]);
        assert_eq!(links, [vec![], vec![], vec![link], vec![]]);
    }

    // A blank row with a code band behind it is content, not a spacer.
    #[test]
    fn banded_blank_rows_are_content() {
        let band = Line::from(Span::styled(
            "    ",
            Style::default().bg(Color::Rgb(1, 2, 3)),
        ));
        let mut rows = vec![Line::from("code"), band.clone()];
        let mut links = Vec::new();
        fit_rows(&mut rows, &mut links, false, false);
        assert_eq!(rows.len(), 2);
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn a_card_with_nothing_to_show_takes_no_rows() {
        let mut rows = vec![Line::default(), Line::from(" ")];
        let mut links = vec![vec![], vec![]];
        fit_rows(&mut rows, &mut links, true, true);
        assert!(rows.is_empty() && links.is_empty());
    }

    // Run members drop the edge they share; anything else breaks the run.
    #[test]
    fn finished_one_line_tools_share_one_frame() {
        let cells = vec![
            Cell::Banner,
            tool(Some(true), false),
            tool(Some(true), false),
            Cell::Image {
                path: "draft.png".into(),
                label: "image".into(),
                queued: true,
            },
            tool(Some(true), false),
            tool(Some(true), true),
            tool(None, false),
            tool(Some(true), false),
            tool(Some(false), false),
        ];
        let got: Vec<_> = (0..cells.len()).map(|i| edges(&cells, i)).collect();
        assert_eq!(
            got,
            [
                (false, false), // banner: unframed
                (true, false),  // run opens
                (false, false), // middle (the draft image is not shown)
                (false, false), // draft image: unframed
                (false, true),  // run closes before the expanded card
                (true, true),   // expanded: its own card
                (true, true),   // running: its own card
                (true, true),   // alone between two non-run cards
                (true, true),   // failed: its own card
            ]
        );
    }

    #[test]
    fn edge_roles_follow_the_owned_edges() {
        let hue = Some(Color::Rgb(9, 9, 9));
        let card = CardFrame {
            hue,
            top: true,
            bottom: true,
        };
        let roles: Vec<_> = (0..3).map(|r| card.edge(r, 3)).collect();
        assert_eq!(
            roles,
            [Some(Edge::Top), Some(Edge::Side), Some(Edge::Bottom)]
        );
        let middle = CardFrame {
            hue,
            top: false,
            bottom: false,
        };
        assert_eq!(middle.edge(0, 1), Some(Edge::Side));
        assert_eq!(CardFrame::default().edge(0, 1), None);
    }

    // Side rows must leave the content between the edges untouched.
    #[test]
    fn painting_draws_edges_without_touching_content() {
        let hue = Color::Rgb(10, 200, 150);
        let mut buf = Buffer::empty(Rect::new(0, 0, 12, 3));
        buf.set_string(2, 1, "content!", Style::default());
        let row = |y| Rect::new(0, y, 12, 1);
        paint_row(&mut buf, row(0), Edge::Top, hue);
        paint_row(&mut buf, row(1), Edge::Side, hue);
        paint_row(&mut buf, row(2), Edge::Bottom, hue);
        let line = |y: u16| -> String { (0..12).map(|x| buf[(x, y)].symbol()).collect() };
        assert_eq!(line(0), "╭──────────╮");
        assert_eq!(line(1), "│ content! │");
        assert_eq!(line(2), "╰──────────╯");
        assert_eq!(buf[(0, 1)].fg, hue);
        assert_eq!(buf[(11, 1)].fg, hue);
    }

    // A frame clipped by the buffer is skipped, never drawn half or panicking.
    #[test]
    fn clipped_frames_are_not_painted() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 6, 1));
        paint_row(
            &mut buf,
            Rect::new(2, 0, 10, 1),
            Edge::Top,
            Color::Rgb(1, 1, 1),
        );
        paint_row(
            &mut buf,
            Rect::new(0, 4, 6, 1),
            Edge::Side,
            Color::Rgb(1, 1, 1),
        );
        assert!((0..6).all(|x| buf[(x, 0)].symbol() == " "));
    }
}
