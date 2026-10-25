//! Cumulative row heights: one entry per cell, not per wrapped transcript row.

/// Where a scrolled-back reader is looking: the top visible row as a cell and
/// a row offset within it (negative for a gap row just above the cell), plus
/// the scroll offset it was taken at. Re-resolving it after a relayout keeps
/// the same text on the top row however much output arrived or reflowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollAnchor {
    pub cell: usize,
    pub offset: isize,
    pub scroll_from_bottom: usize,
    /// The top row's text. A cell that reflows (a streamed answer becoming
    /// rendered markdown, a resize) moves text between rows, so the row
    /// offset alone would land on other words; the text finds them again.
    pub text: String,
}

/// The row of `cell` that best continues an anchor: the row whose text starts
/// with the anchored row's leading words, nearest to `guess`, else `guess`.
pub fn settle(index: &RowIndex, rows: &[String], anchor: &ScrollAnchor) -> Option<usize> {
    let guess = index.row_of(anchor.cell, anchor.offset)?;
    let (start, end) = index.cell_rows(anchor.cell)?;
    let key: String = anchor
        .text
        .split_whitespace()
        .take(4)
        .collect::<Vec<_>>()
        .join(" ");
    if key.is_empty() || anchor.offset < 0 {
        return Some(guess);
    }
    let words = |row: &String| row.split_whitespace().collect::<Vec<_>>().join(" ");
    (start..end.min(rows.len()))
        .filter(|&row| words(&rows[row]).contains(&key))
        .min_by_key(|&row| row.abs_diff(guess))
        .or(Some(guess))
}

#[derive(Default)]
pub struct RowIndex {
    spans: Vec<Span>,
    len: usize,
}
struct Span {
    start: usize,
    end: usize,
    cell: Option<usize>,
    owner: Option<usize>,
    head_rows: usize,
}
impl RowIndex {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn push(
        &mut self,
        cell: Option<usize>,
        rows: usize,
        owner: Option<usize>,
        head_rows: usize,
    ) {
        if rows == 0 {
            return;
        }
        let end = self.len + rows;
        self.spans.push(Span {
            start: self.len,
            end,
            cell,
            owner,
            head_rows: head_rows.min(rows),
        });
        self.len = end;
    }
    fn span_at(&self, row: usize) -> Option<&Span> {
        self.spans
            .get(self.spans.partition_point(|span| span.end <= row))
    }
    pub fn row(&self, row: usize) -> (Option<usize>, usize) {
        self.get(row).expect("row inside viewport")
    }
    /// `(cell, row within cell)`, or `None` past the end.
    pub fn get(&self, row: usize) -> Option<(Option<usize>, usize)> {
        let span = self.span_at(row)?;
        Some((span.cell, row - span.start))
    }
    /// The anchor for `row`: its cell and row within it, or, for a gap row
    /// that belongs to no cell, the next cell at a negative offset.
    pub fn anchor_at(&self, row: usize) -> Option<(usize, isize)> {
        let index = self.spans.partition_point(|span| span.end <= row);
        let span = self.spans.get(index)?;
        if let Some(cell) = span.cell {
            return Some((cell, (row - span.start) as isize));
        }
        self.spans[index..]
            .iter()
            .find_map(|next| {
                next.cell
                    .map(|cell| (cell, row as isize - next.start as isize))
            })
            .or_else(|| {
                self.spans[..index].iter().rev().find_map(|prev| {
                    prev.cell
                        .map(|cell| (cell, row as isize - prev.start as isize))
                })
            })
    }

    /// `[start, end)` rows of `cell` in this layout.
    pub fn cell_rows(&self, cell: usize) -> Option<(usize, usize)> {
        self.spans
            .iter()
            .find(|span| span.cell == Some(cell))
            .map(|span| (span.start, span.end))
    }

    /// The row an anchor points at in this layout, if its cell still exists.
    pub fn row_of(&self, cell: usize, offset: isize) -> Option<usize> {
        let span = self.spans.iter().find(|span| span.cell == Some(cell))?;
        let last = span.end.saturating_sub(1) as isize;
        Some((span.start as isize + offset).clamp(0, last.max(0)) as usize)
    }

    pub fn sticky_owner(&self, lo: usize, hi: usize) -> Option<usize> {
        let owner = self.span_at(lo)?.owner?;
        let first = self.spans.partition_point(|span| span.end <= lo);
        let visible_head = self.spans[first..]
            .iter()
            .take_while(|span| span.start < hi)
            .any(|span| {
                span.owner == Some(owner) && span.start + span.head_rows > lo && span.head_rows > 0
            });
        if visible_head {
            None
        } else {
            Some(owner)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skips_empty_cells_and_maps_boundaries() {
        let mut index = RowIndex::default();
        index.push(Some(0), 3, None, 0);
        index.push(Some(1), 0, None, 0);
        index.push(Some(2), 4, Some(0), 3);
        index.push(Some(3), 100_000, Some(0), 0);
        assert_eq!(index.spans.len(), 3);
        assert_eq!(index.row(3), (Some(2), 0));
        assert_eq!(index.row(100_006), (Some(3), 99_999));
        assert_eq!(index.sticky_owner(0, 2), None);
        assert_eq!(index.sticky_owner(3, 9), None);
        assert_eq!(index.sticky_owner(7, 15), Some(0));
    }

    /// An anchor taken in one layout finds the same text in the next, when
    /// rows were appended below, when a cell above grew, and from a gap row.
    #[test]
    fn anchors_survive_growth_above_and_below() {
        let layout = |first: usize, last: usize| {
            let mut index = RowIndex::default();
            index.push(Some(0), first, None, 0);
            index.push(None, 1, None, 0);
            index.push(Some(1), 10, None, 0);
            index.push(Some(2), last, None, 0);
            index
        };
        let before = layout(5, 3);
        let (cell, offset) = before.anchor_at(8).unwrap();
        assert_eq!((cell, offset), (1, 2));
        assert_eq!(
            layout(5, 300).row_of(cell, offset),
            Some(8),
            "output appended below"
        );
        assert_eq!(
            layout(9, 3).row_of(cell, offset),
            Some(12),
            "a cell above grew by 4"
        );
        let (cell, offset) = before.anchor_at(5).unwrap();
        assert_eq!(
            (cell, offset),
            (1, -1),
            "a gap row anchors to the next cell"
        );
        assert_eq!(layout(7, 3).row_of(cell, offset), Some(7));
        assert_eq!(before.row_of(9, 0), None, "a removed cell has no row");
    }

    /// A cell that reflowed (streamed lines joined into a paragraph) keeps
    /// the reader on the same words, not on the same row number.
    #[test]
    fn a_reflowed_cell_keeps_the_same_words_on_top() {
        let mut streamed = RowIndex::default();
        streamed.push(Some(0), 6, None, 0);
        let mut joined = RowIndex::default();
        joined.push(Some(0), 2, None, 0);
        let rows: Vec<String> = ["L1 a L2 b L3 c", "L4 d L5 e L6 f"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let anchor = ScrollAnchor {
            cell: 0,
            offset: 3,
            scroll_from_bottom: 5,
            text: "L4 d".into(),
        };
        assert_eq!(streamed.row_of(0, 3), Some(3));
        assert_eq!(
            settle(&joined, &rows, &anchor),
            Some(1),
            "L4 sits on row 1 now"
        );
        let gone = ScrollAnchor {
            text: "missing words".into(),
            ..anchor.clone()
        };
        assert_eq!(
            settle(&joined, &rows, &gone),
            Some(1),
            "falls back to the clamped offset"
        );
    }

    #[test]
    fn indexed_sticky_headers_match_row_by_row_oracle() {
        let mut index = RowIndex::default();
        let mut owners = Vec::new();
        let mut heads = Vec::new();
        for (cell, rows, owner, head_rows) in [
            (0, 3, None, 0),
            (1, 4, Some(0), 3),
            (2, 20, Some(0), 0),
            (3, 1, Some(1), 0),
            (4, 4, Some(1), 3),
            (5, 20, Some(1), 0),
        ] {
            index.push(Some(cell), rows, owner, head_rows);
            owners.extend(std::iter::repeat_n(owner, rows));
            heads.extend((0..rows).map(|row| row < head_rows));
        }
        for lo in 0..owners.len() {
            for height in 1..15 {
                let hi = (lo + height).min(owners.len());
                let expected = owners[lo]
                    .filter(|owner| !(lo..hi).any(|row| heads[row] && owners[row] == Some(*owner)));
                assert_eq!(index.sticky_owner(lo, hi), expected, "viewport {lo}..{hi}");
            }
        }
    }
}
