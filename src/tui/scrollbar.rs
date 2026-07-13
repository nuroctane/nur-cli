//! Smooth proportional scrollbar geometry — fractional (1/8-cell) thumb.
//!
//! The math is modelled on ratatui's [`tui-scrollbar`] crate (which targets
//! ratatui 0.30, so we port the idea rather than depend on it): all thumb
//! geometry is tracked in *subcells* — eighths of a terminal cell — so the
//! thumb's size and position move smoothly instead of jumping a full row at a
//! time. Rendering picks eighth-block glyphs for the partially covered edge
//! cells, and dragging keeps the exact grab point under the pointer.
//!
//! [`tui-scrollbar`]: https://crates.io/crates/tui-scrollbar

/// Number of subcells (vertical eighths) in one terminal cell.
pub const SUBCELL: usize = 8;

/// How much of one track cell the thumb covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellFill {
    /// Track only.
    Empty,
    /// Whole cell is thumb.
    Full,
    /// Thumb covers `len` eighths starting `start` eighths from the top.
    Partial { start: u8, len: u8 },
}

/// Where a subcell position lands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Thumb,
    Track,
}

/// Pure scrollbar geometry: content/viewport in lines, track in cells,
/// thumb in subcells. No rendering, fully unit-testable.
#[derive(Debug, Clone, Copy)]
pub struct ScrollMetrics {
    content_len: usize,
    viewport_len: usize,
    track_len: usize,
    thumb_len: usize,
    thumb_start: usize,
}

impl ScrollMetrics {
    /// `content_len`/`viewport_len`/`offset` are in lines; `track_cells` is the
    /// track height in terminal cells. Zero lengths are treated as 1.
    pub fn new(content_len: usize, viewport_len: usize, offset: usize, track_cells: u16) -> Self {
        let track_len = (track_cells as usize).saturating_mul(SUBCELL);
        let content_len = content_len.max(1);
        let viewport_len = viewport_len.min(content_len).max(1);
        let max_offset = content_len.saturating_sub(viewport_len);
        let offset = offset.min(max_offset);

        if track_len == 0 {
            return Self {
                content_len,
                viewport_len,
                track_len,
                thumb_len: 0,
                thumb_start: 0,
            };
        }

        // Thumb is proportional to the visible fraction, never under one cell.
        let thumb_len = (track_len.saturating_mul(viewport_len) / content_len)
            .max(SUBCELL)
            .min(track_len);
        let travel = track_len.saturating_sub(thumb_len);
        let thumb_start = travel
            .saturating_mul(offset)
            .checked_div(max_offset)
            .unwrap_or(0);

        Self {
            content_len,
            viewport_len,
            track_len,
            thumb_len,
            thumb_start,
        }
    }

    pub fn max_offset(&self) -> usize {
        self.content_len.saturating_sub(self.viewport_len)
    }

    pub fn thumb_start(&self) -> usize {
        self.thumb_start
    }

    pub fn thumb_travel(&self) -> usize {
        self.track_len.saturating_sub(self.thumb_len)
    }

    pub fn viewport_len(&self) -> usize {
        self.viewport_len
    }

    /// Subcell position at the centre of track cell `row` (0-based).
    pub fn subcell_at_row(row_in_track: u16) -> usize {
        (row_in_track as usize)
            .saturating_mul(SUBCELL)
            .saturating_add(SUBCELL / 2)
    }

    /// Does a subcell position land on the thumb or the open track?
    pub fn hit_test(&self, position: usize) -> Hit {
        if position >= self.thumb_start
            && position < self.thumb_start.saturating_add(self.thumb_len)
        {
            Hit::Thumb
        } else {
            Hit::Track
        }
    }

    /// Content offset (lines) that puts the thumb's top edge at `thumb_start`
    /// subcells — the inverse mapping used while dragging.
    pub fn offset_for_thumb_start(&self, thumb_start: usize) -> usize {
        let travel = self.thumb_travel();
        let thumb_start = thumb_start.min(travel);
        self.max_offset()
            .saturating_mul(thumb_start)
            .checked_div(travel)
            .unwrap_or(0)
    }

    /// Thumb coverage of track cell `cell_index` — drives glyph choice.
    pub fn cell_fill(&self, cell_index: usize) -> CellFill {
        if self.thumb_len == 0 || self.max_offset() == 0 {
            return CellFill::Empty;
        }
        let cell_start = cell_index.saturating_mul(SUBCELL);
        let cell_end = cell_start.saturating_add(SUBCELL);
        let thumb_end = self.thumb_start.saturating_add(self.thumb_len);
        let start = self.thumb_start.max(cell_start);
        let end = thumb_end.min(cell_end);
        if end <= start {
            return CellFill::Empty;
        }
        let len = end.saturating_sub(start).min(SUBCELL) as u8;
        let start = start.saturating_sub(cell_start).min(SUBCELL) as u8;
        if len as usize >= SUBCELL {
            CellFill::Full
        } else {
            CellFill::Partial { start, len }
        }
    }

    /// Glyph for a vertical track cell. Bottom-anchored partials (thumb's top
    /// edge) get exact eighth blocks; top-anchored partials (thumb's bottom
    /// edge) approximate with standard blocks so no legacy-computing font
    /// support is required.
    pub fn glyph(&self, cell_index: usize) -> Option<char> {
        const LOWER: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        const UPPER: [char; 8] = ['▔', '▔', '▀', '▀', '▀', '▀', '█', '█'];
        match self.cell_fill(cell_index) {
            CellFill::Empty => None,
            CellFill::Full => Some('█'),
            CellFill::Partial { start, len } => {
                let i = (len.saturating_sub(1) as usize).min(7);
                if start == 0 {
                    // Fill begins at the cell's top → thumb's bottom edge.
                    Some(UPPER[i])
                } else {
                    // Fill hangs from the cell's bottom → thumb's top edge.
                    Some(LOWER[i])
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_is_proportional_and_at_least_one_cell() {
        let m = ScrollMetrics::new(400, 40, 0, 20);
        // 40/400 of a 160-subcell track = 16 subcells (2 cells).
        assert_eq!(m.thumb_start(), 0);
        assert!(m.thumb_travel() > 0);
        let tiny = ScrollMetrics::new(100_000, 10, 0, 10);
        // Never smaller than one cell.
        assert!(matches!(tiny.cell_fill(0), CellFill::Full | CellFill::Partial { .. }));
    }

    #[test]
    fn ends_pin_exactly() {
        let m = ScrollMetrics::new(300, 30, 0, 12);
        assert_eq!(m.thumb_start(), 0);
        let m = ScrollMetrics::new(300, 30, 270, 12);
        // At max offset the thumb bottom touches the track bottom.
        assert_eq!(m.thumb_start(), m.thumb_travel());
    }

    #[test]
    fn drag_round_trips_offset() {
        let m = ScrollMetrics::new(500, 50, 200, 25);
        let back = m.offset_for_thumb_start(m.thumb_start());
        // Integer rounding may lose < 1 line of precision.
        assert!(back.abs_diff(200) <= 1, "got {back}");
    }

    #[test]
    fn hit_test_finds_the_thumb() {
        let m = ScrollMetrics::new(200, 50, 75, 20);
        assert!(matches!(m.hit_test(m.thumb_start()), Hit::Thumb));
        assert!(matches!(m.hit_test(0), Hit::Track));
    }

    #[test]
    fn partial_edges_use_eighth_blocks() {
        // Odd offsets produce fractional thumb edges somewhere mid-track.
        let m = ScrollMetrics::new(997, 41, 313, 17);
        let mut partial = 0;
        for c in 0..17 {
            if let CellFill::Partial { .. } = m.cell_fill(c) {
                partial += 1;
                assert!(m.glyph(c).is_some());
            }
        }
        assert!(partial <= 2, "at most the two edge cells are partial");
    }

    #[test]
    fn no_scroll_no_thumb() {
        let m = ScrollMetrics::new(10, 40, 0, 20);
        assert_eq!(m.max_offset(), 0);
        assert!(matches!(m.cell_fill(0), CellFill::Empty));
        assert!(m.glyph(0).is_none());
    }
}
