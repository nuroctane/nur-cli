//! Zone-grid layout engine for inline TUI cards.
//!
//! A port of the FancyZones grid model (PowerToys' `GridLayoutModel` /
//! `GridData`, MIT) to terminal cells, in the same spirit as hermes-agent's
//! `pane-shell/tree/grid-model.ts`:
//!
//!  - a layout is `rows × columns` of **percent tracks** summing to
//!    [`MULTIPLIER`], plus a `cell_child_map` assigning every cell to a zone;
//!    a zone spanning several cells is the same index in adjacent cells,
//!  - zones are rectangles in the `0..MULTIPLIER` coordinate space, recovered
//!    from the cell map by prefix sums,
//!  - [`is_guillotine`] checks the arrangement can be produced by recursive
//!    full-length cuts — the property that makes a grid renderable as nested
//!    splits (and rules out interlocking pinwheels).
//!
//! On top of that sits the terminal half: [`zones_to_rects`] snaps the percent
//! space onto integer character cells with no gaps and no double-drawn columns,
//! and [`Canvas`] is a tiny styled character buffer the cards paint panes into
//! before it is flattened to ratatui [`Line`]s.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthChar;

/// Row/column percents sum to this, as in FancyZones.
pub const MULTIPLIER: u32 = 10_000;

/// A `rows × columns` percent grid with a cell→zone assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridLayout {
    pub rows: usize,
    pub columns: usize,
    pub row_percents: Vec<u32>,
    pub column_percents: Vec<u32>,
    /// `cell_child_map[row][col]` = zone index. Spans repeat the index.
    pub cell_child_map: Vec<Vec<usize>>,
}

/// A zone rectangle in the `0..MULTIPLIER` space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Zone {
    pub index: usize,
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

/// `result[k]` is the sum of the first `k` entries — track edges from widths.
fn prefix_sum(list: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(list.len() + 1);
    let mut sum = 0;
    out.push(0);
    for v in list {
        sum += *v;
        out.push(sum);
    }
    out
}

/// Split `MULTIPLIER` into `n` tracks, giving the remainder to the first ones
/// so the tracks always sum to exactly `MULTIPLIER`.
pub fn even_percents(n: usize) -> Vec<u32> {
    if n == 0 {
        return Vec::new();
    }
    let base = MULTIPLIER / n as u32;
    let mut out = vec![base; n];
    let rem = MULTIPLIER - base * n as u32;
    for slot in out.iter_mut().take(rem as usize) {
        *slot += 1;
    }
    out
}

impl GridLayout {
    /// A plain `rows × columns` grid, one zone per cell, even tracks.
    pub fn uniform(rows: usize, columns: usize) -> Self {
        let cell_child_map = (0..rows)
            .map(|r| (0..columns).map(|c| r * columns + c).collect())
            .collect();
        Self {
            rows,
            columns,
            row_percents: even_percents(rows),
            column_percents: even_percents(columns),
            cell_child_map,
        }
    }

    /// Build from an explicit cell map (spans allowed), with even tracks.
    pub fn from_map(map: &[&[usize]]) -> Self {
        let rows = map.len();
        let columns = map.first().map(|r| r.len()).unwrap_or(0);
        Self {
            rows,
            columns,
            row_percents: even_percents(rows),
            column_percents: even_percents(columns),
            cell_child_map: map.iter().map(|r| r.to_vec()).collect(),
        }
    }

    /// Number of distinct zones this layout declares.
    pub fn zone_count(&self) -> usize {
        self.cell_child_map
            .iter()
            .flatten()
            .copied()
            .max()
            .map(|m| m + 1)
            .unwrap_or(0)
    }
}

/// Recover zone rectangles from the cell map (`GridData.ModelToZones`).
///
/// Returns `None` when the map is not a valid zone set: more zones than cells,
/// or a zone whose cells do not form a solid rectangle.
pub fn model_to_zones(model: &GridLayout) -> Option<Vec<Zone>> {
    let zone_count = model.zone_count();
    if zone_count == 0 || zone_count > model.rows * model.columns {
        return None;
    }
    if model.cell_child_map.len() != model.rows
        || model
            .cell_child_map
            .iter()
            .any(|r| r.len() != model.columns)
        || model.row_percents.len() != model.rows
        || model.column_percents.len() != model.columns
    {
        return None;
    }

    let row_edges = prefix_sum(&model.row_percents);
    let col_edges = prefix_sum(&model.column_percents);

    let mut zones: Vec<Zone> = (0..zone_count)
        .map(|index| Zone {
            index,
            left: u32::MAX,
            top: u32::MAX,
            right: 0,
            bottom: 0,
        })
        .collect();
    let mut cells_seen = vec![0usize; zone_count];

    for row in 0..model.rows {
        for col in 0..model.columns {
            let i = model.cell_child_map[row][col];
            let z = &mut zones[i];
            z.left = z.left.min(col_edges[col]);
            z.right = z.right.max(col_edges[col + 1]);
            z.top = z.top.min(row_edges[row]);
            z.bottom = z.bottom.max(row_edges[row + 1]);
            cells_seen[i] += 1;
        }
    }

    // Every zone must be solid: its bounding box must contain exactly the cells
    // that claim it. An L-shape would pass the bounding-box pass above.
    for (i, zone) in zones.iter().enumerate() {
        if cells_seen[i] == 0 {
            return None;
        }
        let mut spanned = 0usize;
        for row in 0..model.rows {
            for col in 0..model.columns {
                let inside = col_edges[col] >= zone.left
                    && col_edges[col + 1] <= zone.right
                    && row_edges[row] >= zone.top
                    && row_edges[row + 1] <= zone.bottom;
                if inside {
                    if model.cell_child_map[row][col] != i {
                        return None;
                    }
                    spanned += 1;
                }
            }
        }
        if spanned != cells_seen[i] {
            return None;
        }
    }

    Some(zones)
}

/// Can this zone set be produced by recursive full-length cuts?
///
/// Every practical template is guillotine-cuttable; interlocking pinwheels are
/// not, and cannot be rendered as nested splits.
pub fn is_guillotine(zones: &[Zone]) -> bool {
    if zones.len() <= 1 {
        return true;
    }
    for horizontal in [false, true] {
        let mut cuts: Vec<u32> = zones
            .iter()
            .map(|z| if horizontal { z.top } else { z.left })
            .collect();
        cuts.sort_unstable();
        cuts.dedup();
        for at in cuts.into_iter().skip(1) {
            let clean = zones.iter().all(|z| {
                let (lo, hi) = if horizontal {
                    (z.top, z.bottom)
                } else {
                    (z.left, z.right)
                };
                hi <= at || lo >= at
            });
            if !clean {
                continue;
            }
            let (a, b): (Vec<Zone>, Vec<Zone>) = zones.iter().partition(|z| {
                let hi = if horizontal { z.bottom } else { z.right };
                hi <= at
            });
            if a.is_empty() || b.is_empty() {
                continue;
            }
            return is_guillotine(&a) && is_guillotine(&b);
        }
    }
    false
}

/// Map percent-space zones onto integer character rects inside `area`.
///
/// Edges are rounded from the same percent scale, so neighbouring zones share
/// an exact boundary: no 1-cell gaps, no overlaps, and the union is `area`.
pub fn zones_to_rects(zones: &[Zone], area: Rect) -> Vec<Rect> {
    let map = |v: u32, span: u16, origin: u16| -> u16 {
        origin + ((v as u64 * span as u64 + MULTIPLIER as u64 / 2) / MULTIPLIER as u64) as u16
    };
    zones
        .iter()
        .map(|z| {
            let x0 = map(z.left, area.width, area.x);
            let x1 = map(z.right, area.width, area.x);
            let y0 = map(z.top, area.height, area.y);
            let y1 = map(z.bottom, area.height, area.y);
            Rect {
                x: x0,
                y: y0,
                width: x1.saturating_sub(x0),
                height: y1.saturating_sub(y0),
            }
        })
        .collect()
}

/// Pick a layout for `n` panes that fits `width` columns.
///
/// Curated templates give small counts a deliberate shape (a hero pane on top
/// of a pair reads better than three equal strips); beyond that a balanced
/// grid is generated. `min_pane_width` clamps the column count so panes never
/// shrink below something legible on a narrow terminal.
pub fn layout_for(n: usize, width: u16, min_pane_width: u16) -> GridLayout {
    let n = n.max(1);
    let fit = (width / min_pane_width.max(1)).max(1) as usize;

    let template = match n {
        1 => Some(GridLayout::uniform(1, 1)),
        2 => Some(GridLayout::uniform(1, 2)),
        // Hero on top, pair below — the running agent usually leads.
        3 => Some(GridLayout::from_map(&[&[0, 0], &[1, 2]])),
        4 => Some(GridLayout::uniform(2, 2)),
        5 => Some(GridLayout::from_map(&[&[0, 1, 2], &[3, 3, 4]])),
        6 => Some(GridLayout::uniform(2, 3)),
        _ => None,
    };

    // A template is only used when it fits the terminal *and* is a legal,
    // cuttable zone set — so editing the table above can never hand the
    // renderer a layout it cannot tile.
    let usable = template.filter(|t| {
        t.columns <= fit
            && model_to_zones(t)
                .map(|z| z.len() == n && is_guillotine(&z))
                .unwrap_or(false)
    });

    match usable {
        Some(t) => t,
        None => {
            let columns = n.min(fit).max(1);
            let rows = n.div_ceil(columns);
            // Trailing row may be short: widen its last pane to fill the row.
            let mut map: Vec<Vec<usize>> = Vec::with_capacity(rows);
            for r in 0..rows {
                let mut row = Vec::with_capacity(columns);
                for c in 0..columns {
                    let i = r * columns + c;
                    row.push(i.min(n - 1));
                }
                map.push(row);
            }
            let mut layout = GridLayout {
                rows,
                columns,
                row_percents: even_percents(rows),
                column_percents: even_percents(columns),
                cell_child_map: map,
            };
            // Re-index so zones are 0..k contiguous even when the last row is short.
            let mut seen: Vec<usize> = Vec::new();
            for row in layout.cell_child_map.iter_mut() {
                for cell in row.iter_mut() {
                    let pos = seen.iter().position(|v| v == cell).unwrap_or_else(|| {
                        seen.push(*cell);
                        seen.len() - 1
                    });
                    *cell = pos;
                }
            }
            layout
        }
    }
}

// ── styled character canvas ──────────────────────────────────────────────

/// A fixed-size grid of styled characters that cards paint into before being
/// flattened to ratatui lines. Blitting panes into one buffer is what lets the
/// tiles share borders and stay aligned regardless of wrapping.
pub struct Canvas {
    width: usize,
    height: usize,
    cells: Vec<(char, Style)>,
}

impl Canvas {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![(' ', Style::default()); width * height],
        }
    }

    pub fn set(&mut self, x: usize, y: usize, ch: char, style: Style) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x] = (ch, style);
        }
    }

    /// Write `text` at `(x, y)`, clipped to `max_w` columns and to the canvas.
    /// Returns the number of columns advanced. Wide glyphs are counted by their
    /// display width so box borders never drift.
    pub fn text(&mut self, x: usize, y: usize, text: &str, style: Style, max_w: usize) -> usize {
        let mut cx = x;
        let limit = x + max_w;
        for ch in text.chars() {
            let w = ch.width().unwrap_or(0);
            if w == 0 {
                continue;
            }
            if cx + w > limit || cx >= self.width {
                break;
            }
            self.set(cx, y, ch, style);
            // Blank the continuation column of a wide glyph so the next write
            // cannot land inside it.
            for pad in 1..w {
                self.set(cx + pad, y, '\0', style);
            }
            cx += w;
        }
        cx - x
    }

    /// Like [`Canvas::text`], but marks a truncation with an ellipsis so a
    /// clipped label never reads as a real (shorter) value.
    pub fn text_clipped(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        style: Style,
        max_w: usize,
    ) -> usize {
        let full: usize = text.chars().map(|c| c.width().unwrap_or(0)).sum();
        if full <= max_w {
            return self.text(x, y, text, style, max_w);
        }
        if max_w == 0 {
            return 0;
        }
        let used = self.text(x, y, text, style, max_w.saturating_sub(1));
        self.text(x + used, y, "…", style, 1);
        used + 1
    }

    /// Write `text` right-aligned so it ends at `end_x` (exclusive).
    pub fn text_right(&mut self, end_x: usize, y: usize, text: &str, style: Style) {
        let w: usize = text.chars().map(|c| c.width().unwrap_or(0)).sum();
        let start = end_x.saturating_sub(w);
        self.text(start, y, text, style, w);
    }

    /// Draw a rounded box border around `rect`, optionally weaving a title into
    /// the top edge and a right-aligned badge next to it.
    pub fn frame(&mut self, rect: Rect, style: Style, title: Option<(&str, Style)>) {
        let (x0, y0) = (rect.x as usize, rect.y as usize);
        let (w, h) = (rect.width as usize, rect.height as usize);
        if w < 2 || h < 2 {
            return;
        }
        let (x1, y1) = (x0 + w - 1, y0 + h - 1);
        self.set(x0, y0, '╭', style);
        self.set(x1, y0, '╮', style);
        self.set(x0, y1, '╰', style);
        self.set(x1, y1, '╯', style);
        for x in x0 + 1..x1 {
            self.set(x, y0, '─', style);
            self.set(x, y1, '─', style);
        }
        for y in y0 + 1..y1 {
            self.set(x0, y, '│', style);
            self.set(x1, y, '│', style);
        }
        if let Some((label, label_style)) = title {
            let room = w.saturating_sub(6);
            if room >= 3 {
                self.text(x0 + 2, y0, " ", style, 1);
                let used = self.text(x0 + 3, y0, label, label_style, room);
                self.text(x0 + 3 + used, y0, " ", style, 1);
            }
        }
    }

    /// Flatten to ratatui lines, merging runs of equal style into single spans.
    pub fn into_lines(self) -> Vec<Line<'static>> {
        let mut out = Vec::with_capacity(self.height);
        for y in 0..self.height {
            let row = &self.cells[y * self.width..(y + 1) * self.width];
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut buf = String::new();
            let mut cur: Option<Style> = None;
            for (ch, style) in row {
                if *ch == '\0' {
                    continue; // continuation column of a wide glyph
                }
                match cur {
                    Some(s) if s == *style => buf.push(*ch),
                    Some(s) => {
                        spans.push(Span::styled(std::mem::take(&mut buf), s));
                        buf.push(*ch);
                        cur = Some(*style);
                    }
                    None => {
                        buf.push(*ch);
                        cur = Some(*style);
                    }
                }
            }
            if let Some(s) = cur {
                spans.push(Span::styled(buf, s));
            }
            out.push(Line::from(spans));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zones_of(model: &GridLayout) -> Vec<Zone> {
        model_to_zones(model).expect("valid layout")
    }

    #[test]
    fn even_percents_always_sum_to_the_multiplier() {
        for n in 1..=17 {
            assert_eq!(even_percents(n).iter().sum::<u32>(), MULTIPLIER, "n = {n}");
        }
    }

    #[test]
    fn uniform_grid_yields_one_zone_per_cell_tiling_the_space() {
        let zones = zones_of(&GridLayout::uniform(2, 3));
        assert_eq!(zones.len(), 6);
        let area: u64 = zones
            .iter()
            .map(|z| (z.right - z.left) as u64 * (z.bottom - z.top) as u64)
            .sum();
        assert_eq!(area, MULTIPLIER as u64 * MULTIPLIER as u64);
    }

    #[test]
    fn spans_merge_adjacent_cells_into_one_zone() {
        // Hero row on top of a pair.
        let zones = zones_of(&GridLayout::from_map(&[&[0, 0], &[1, 2]]));
        assert_eq!(zones.len(), 3);
        let hero = zones[0];
        assert_eq!((hero.left, hero.right), (0, MULTIPLIER));
        assert_eq!(hero.top, 0);
        assert_eq!(zones[1].top, hero.bottom);
    }

    #[test]
    fn non_rectangular_zones_are_rejected() {
        // Zone 0 is an L-shape — not a legal zone.
        let model = GridLayout::from_map(&[&[0, 0], &[0, 1]]);
        assert!(model_to_zones(&model).is_none());
    }

    #[test]
    fn templates_are_guillotine_cuttable() {
        for n in 1..=6 {
            let model = layout_for(n, 200, 20);
            let zones = zones_of(&model);
            assert_eq!(zones.len(), n, "template {n} must expose {n} zones");
            assert!(is_guillotine(&zones), "template {n} must be cuttable");
        }
    }

    #[test]
    fn pinwheel_is_not_guillotine_cuttable() {
        // Classic interlocking pinwheel: no full-length cut on either axis.
        let zones = vec![
            Zone {
                index: 0,
                left: 0,
                top: 0,
                right: 6000,
                bottom: 4000,
            },
            Zone {
                index: 1,
                left: 6000,
                top: 0,
                right: 10000,
                bottom: 6000,
            },
            Zone {
                index: 2,
                left: 4000,
                top: 6000,
                right: 10000,
                bottom: 10000,
            },
            Zone {
                index: 3,
                left: 0,
                top: 4000,
                right: 4000,
                bottom: 10000,
            },
            Zone {
                index: 4,
                left: 4000,
                top: 4000,
                right: 6000,
                bottom: 6000,
            },
        ];
        assert!(!is_guillotine(&zones));
    }

    #[test]
    fn generated_layouts_cover_every_pane_without_gaps() {
        for n in 1..=12 {
            let model = layout_for(n, 120, 30);
            let zones = zones_of(&model);
            assert!(is_guillotine(&zones), "n = {n}");
            let area: u64 = zones
                .iter()
                .map(|z| (z.right - z.left) as u64 * (z.bottom - z.top) as u64)
                .sum();
            assert_eq!(area, MULTIPLIER as u64 * MULTIPLIER as u64, "n = {n}");
        }
    }

    #[test]
    fn narrow_terminals_fall_back_to_fewer_columns() {
        // 6 panes at 60 cols with a 30-col minimum = at most 2 across.
        let model = layout_for(6, 60, 30);
        assert!(model.columns <= 2, "columns = {}", model.columns);
        assert_eq!(model.zone_count(), 6);
    }

    #[test]
    fn rects_tile_the_area_with_no_gaps_or_overlaps() {
        let area = Rect {
            x: 0,
            y: 0,
            width: 97,
            height: 13,
        };
        for n in 1..=8 {
            let zones = zones_of(&layout_for(n, area.width, 24));
            let rects = zones_to_rects(&zones, area);
            let mut covered = vec![0u8; (area.width as usize) * (area.height as usize)];
            for r in &rects {
                for y in r.y..r.y + r.height {
                    for x in r.x..r.x + r.width {
                        covered[y as usize * area.width as usize + x as usize] += 1;
                    }
                }
            }
            assert!(covered.iter().all(|c| *c == 1), "n = {n} must tile exactly");
        }
    }

    #[test]
    fn canvas_merges_runs_and_keeps_width() {
        let mut c = Canvas::new(10, 2);
        let s = Style::default();
        c.text(0, 0, "abc", s, 10);
        c.text(3, 0, "de", s, 10);
        let lines = c.into_lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans.len(), 1, "equal styles merge into one span");
        let rendered: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(rendered, "abcde     ");
    }

    #[test]
    fn canvas_text_is_clipped_to_max_width() {
        let mut c = Canvas::new(10, 1);
        let used = c.text(0, 0, "abcdefghijklm", Style::default(), 4);
        assert_eq!(used, 4);
        let rendered: String = c.into_lines()[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(rendered, "abcd      ");
    }

    #[test]
    fn frame_draws_a_closed_border_with_a_title() {
        let mut c = Canvas::new(12, 3);
        c.frame(
            Rect {
                x: 0,
                y: 0,
                width: 12,
                height: 3,
            },
            Style::default(),
            Some(("hi", Style::default())),
        );
        let lines = c.into_lines();
        let row: Vec<String> = lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        assert!(row[0].starts_with("╭─ hi "), "got {:?}", row[0]);
        assert!(row[0].ends_with('╮'));
        assert!(row[1].starts_with('│') && row[1].ends_with('│'));
        assert!(row[2].starts_with('╰') && row[2].ends_with('╯'));
    }
}
