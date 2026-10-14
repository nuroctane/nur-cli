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

/// Smallest thumb on a track with room for it (two cells).
const MIN_THUMB: usize = SUBCELL * 2;

/// `a / b` rounded to nearest; 0 when `b` is 0.
fn div_round(a: usize, b: usize) -> usize {
    a.saturating_add(b / 2).checked_div(b).unwrap_or(0)
}

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

        // Thumb is proportional to the visible fraction. Floor: two cells when
        // the track has room (a one-cell thumb on a long transcript is too
        // small a target), one cell otherwise.
        let min_thumb = if track_len >= MIN_THUMB * 3 {
            MIN_THUMB
        } else {
            SUBCELL
        };
        let thumb_len = (track_len.saturating_mul(viewport_len) / content_len)
            .max(min_thumb)
            .min(track_len);
        let travel = track_len.saturating_sub(thumb_len);
        let thumb_start = div_round(travel.saturating_mul(offset), max_offset);

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

    /// Content offset that centres the thumb on track cell `row_in_track` -
    /// the "jump to here" mapping for a press or drag on a rail with no grab
    /// point (list pickers).
    pub fn offset_centred_on_row(&self, row_in_track: u16) -> usize {
        let pos = Self::subcell_at_row(row_in_track);
        self.offset_for_thumb_start(pos.saturating_sub(self.thumb_len / 2))
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
        div_round(self.max_offset().saturating_mul(thumb_start), travel)
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

    /// How to paint a vertical track cell. Both thumb edges land on exact
    /// eighths using only the standard lower-block glyphs: the thumb's top
    /// edge is a lower block in the thumb colour; its bottom edge is the
    /// *gap* drawn as a lower block in the track's background over a cell
    /// filled with the thumb colour ([`Glyph::inverted`]). No legacy-computing
    /// font support needed.
    pub fn glyph(&self, cell_index: usize) -> Option<Glyph> {
        const LOWER: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        match self.cell_fill(cell_index) {
            CellFill::Empty => None,
            CellFill::Full => Some(Glyph {
                ch: '█',
                inverted: false,
            }),
            CellFill::Partial { start, len } => {
                if start == 0 {
                    // Fill begins at the cell's top → thumb's bottom edge:
                    // paint the uncovered remainder in background colour.
                    let gap = (SUBCELL as u8).saturating_sub(len).clamp(1, 8) as usize;
                    Some(Glyph {
                        ch: LOWER[gap - 1],
                        inverted: true,
                    })
                } else {
                    // Fill hangs from the cell's bottom → thumb's top edge.
                    let i = (len.saturating_sub(1) as usize).min(7);
                    Some(Glyph {
                        ch: LOWER[i],
                        inverted: false,
                    })
                }
            }
        }
    }
}

/// One thumb cell: the character and whether its colours swap (background
/// in the thumb colour, glyph in the track background).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    pub ch: char,
    pub inverted: bool,
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
        assert!(matches!(
            tiny.cell_fill(0),
            CellFill::Full | CellFill::Partial { .. }
        ));
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
            if let CellFill::Partial { start, len } = m.cell_fill(c) {
                partial += 1;
                let g = m.glyph(c).expect("partial cell paints");
                // Bottom edge (fill from the cell top) is drawn as the gap in
                // background colour; top edge as the fill in thumb colour.
                assert_eq!(g.inverted, start == 0, "cell {c}: {start}+{len}");
                assert_ne!(g.ch, '█', "a partial cell is never a full block");
            }
        }
        assert!(partial <= 2, "at most the two edge cells are partial");
    }

    #[test]
    fn centred_mapping_pins_both_ends() {
        let m = ScrollMetrics::new(69, 12, 0, 12);
        assert_eq!(m.offset_centred_on_row(0), 0);
        assert_eq!(m.offset_centred_on_row(11), m.max_offset());
        let mid = m.offset_centred_on_row(6);
        assert!(mid > 0 && mid < m.max_offset(), "got {mid}");
    }

    #[test]
    fn thumb_floor_is_two_cells_when_the_track_has_room() {
        let long = ScrollMetrics::new(100_000, 10, 0, 30);
        assert_eq!(long.thumb_len, MIN_THUMB);
        // A five-row track cannot spare two cells for the thumb.
        let short = ScrollMetrics::new(100_000, 10, 0, 5);
        assert_eq!(short.thumb_len, SUBCELL);
    }

    #[test]
    fn no_scroll_no_thumb() {
        let m = ScrollMetrics::new(10, 40, 0, 20);
        assert_eq!(m.max_offset(), 0);
        assert!(matches!(m.cell_fill(0), CellFill::Empty));
        assert!(m.glyph(0).is_none());
    }
}
