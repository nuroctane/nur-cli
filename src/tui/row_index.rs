//! Cumulative row heights: one entry per cell, not per wrapped transcript row.
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
        let span = self.span_at(row).expect("row inside viewport");
        (span.cell, row - span.start)
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
