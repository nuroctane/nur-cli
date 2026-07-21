//! Multi-line input editor with cursor movement, paste chips, + persistent history.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Private-use base for paste-chip sentinels in the buffer (one char per chip).
const PASTE_BASE: u32 = 0xE000;
const PASTE_SLOT_COUNT: u32 = 4096;

/// Multi-line or long pastes collapse into a chip when either threshold is hit.
/// Keep these low — walls of text must never dump into the composer (Claude-style).
const PASTE_CHIP_MIN_LINES: usize = 2;
const PASTE_CHIP_MIN_CHARS: usize = 40;

#[derive(Clone, Debug)]
pub struct PasteBlock {
    #[allow(dead_code)]
    pub id: u32,
    pub content: String,
}

/// One soft-wrapped row in the input viewport (scroll unit = one of these).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualRow {
    /// Inclusive start buffer index.
    pub abs_start: usize,
    /// Exclusive end buffer index (may equal start for empty row after `\n`).
    pub abs_end: usize,
}

/// Active Ctrl+R reverse-history-search session.
struct ReverseSearch {
    /// The incremental search query the user is typing.
    query: String,
    /// Index into `history` of the current match, if any.
    match_idx: Option<usize>,
    /// Buffer text before search began — restored on cancel.
    restore: String,
}

pub struct InputState {
    /// Buffer with embedded '\n' for multi-line prompts.
    /// Large pastes may appear as a single private-use sentinel char.
    buffer: Vec<char>,
    /// Cursor as a char index into `buffer`.
    cursor: usize,
    /// Selection anchor (other end of the selection). `None` = no selection.
    /// The range is always between `selection_anchor` and `cursor` (inclusive of
    /// the lower end, exclusive of the upper — standard half-open).
    selection_anchor: Option<usize>,
    /// Full bodies for paste chips keyed by id.
    pastes: HashMap<u32, PasteBlock>,
    next_paste_id: u32,
    history: Vec<String>,
    hist_idx: Option<usize>,
    stash: String,
    /// Some while a Ctrl+R reverse search is in progress.
    search: Option<ReverseSearch>,
}

/// Most-recent history entry (searching newest → oldest) at an index `< before`
/// whose text contains `query` (case-insensitive). `before = None` searches the
/// whole list. Empty query or no hit → `None`.
fn reverse_search_match(history: &[String], query: &str, before: Option<usize>) -> Option<usize> {
    if query.is_empty() {
        return None;
    }
    let q = query.to_lowercase();
    let end = before.unwrap_or(history.len()).min(history.len());
    history[..end]
        .iter()
        .enumerate()
        .rev()
        .find(|(_, h)| h.to_lowercase().contains(&q))
        .map(|(i, _)| i)
}

fn history_path() -> PathBuf {
    crate::config::muse_home().join("history.jsonl")
}

pub fn is_paste_sentinel(c: char) -> bool {
    let u = c as u32;
    (PASTE_BASE..PASTE_BASE + PASTE_SLOT_COUNT).contains(&u)
}

fn paste_id_of(c: char) -> Option<u32> {
    if is_paste_sentinel(c) {
        Some(c as u32 - PASTE_BASE)
    } else {
        None
    }
}

fn paste_sentinel(id: u32) -> char {
    char::from_u32(PASTE_BASE + id).expect("paste id in private-use range")
}

fn normalize_paste(s: &str) -> String {
    s.chars().filter(|c| *c != '\r').collect()
}

fn paste_line_count(content: &str) -> usize {
    if content.is_empty() {
        return 1;
    }
    let n = content.lines().count();
    n.max(1)
}

/// Label shown in the composer for a collapsed paste (Claude-style).
pub fn paste_chip_label(content: &str) -> String {
    let n = paste_line_count(content);
    format!("[pasted 1-{n} lines]")
}

pub(crate) fn should_chip(s: &str) -> bool {
    let lines = paste_line_count(s);
    let chars = s.chars().filter(|c| *c != '\r').count();
    // Any multi-line paste, or anything longer than a short phrase.
    lines >= PASTE_CHIP_MIN_LINES || chars >= PASTE_CHIP_MIN_CHARS
}

impl InputState {
    pub fn new() -> Self {
        let mut history = Vec::new();
        if let Ok(text) = fs::read_to_string(history_path()) {
            for line in text.lines() {
                if let Ok(s) = serde_json::from_str::<String>(line) {
                    history.push(s);
                }
            }
        }
        Self {
            buffer: Vec::new(),
            cursor: 0,
            selection_anchor: None,
            pastes: HashMap::new(),
            next_paste_id: 0,
            history,
            hist_idx: None,
            stash: String::new(),
            search: None,
        }
    }

    /// Raw buffer text (may contain paste sentinels). Prefer for layout math.
    pub fn text(&self) -> String {
        self.buffer.iter().collect()
    }

    /// Expand paste chips to full content (what the model / history should see).
    pub fn text_expanded(&self) -> String {
        self.expand_range(0, self.buffer.len())
    }

    fn expand_range(&self, lo: usize, hi: usize) -> String {
        let mut out = String::new();
        for &c in &self.buffer[lo..hi.min(self.buffer.len())] {
            if let Some(id) = paste_id_of(c) {
                if let Some(p) = self.pastes.get(&id) {
                    out.push_str(&p.content);
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Replace buffer. Large multi-line / long blobs are re-chipped so history
    /// recall does not dump raw paste soup back into the composer.
    pub fn set_text(&mut self, s: &str) {
        let normalized = normalize_paste(s);
        self.drop_all_pastes();
        self.selection_anchor = None;
        if should_chip(&normalized) {
            if let Some(id) = self.alloc_paste(normalized.clone()) {
                self.buffer = vec![paste_sentinel(id)];
                self.cursor = 1;
                return;
            }
        }
        self.buffer = normalized.chars().collect();
        self.cursor = self.buffer.len();
    }

    pub fn clear(&mut self) {
        self.drop_all_pastes();
        self.buffer.clear();
        self.cursor = 0;
        self.selection_anchor = None;
        self.hist_idx = None;
        // Clearing the input always exits reverse search — the stashed draft it
        // would restore is gone, so never leave a dangling search session.
        self.search = None;
    }

    fn drop_all_pastes(&mut self) {
        self.pastes.clear();
    }

    fn gc_pastes(&mut self) {
        let live: std::collections::HashSet<u32> =
            self.buffer.iter().filter_map(|c| paste_id_of(*c)).collect();
        self.pastes.retain(|id, _| live.contains(id));
    }

    fn alloc_paste(&mut self, content: String) -> Option<u32> {
        for _ in 0..PASTE_SLOT_COUNT {
            let id = self.next_paste_id % PASTE_SLOT_COUNT;
            self.next_paste_id = self.next_paste_id.wrapping_add(1);
            if !self.pastes.contains_key(&id)
                && !self.buffer.iter().any(|c| paste_id_of(*c) == Some(id))
            {
                self.pastes.insert(id, PasteBlock { id, content });
                return Some(id);
            }
        }
        None
    }

    /// Label for a paste sentinel, if any.
    pub fn chip_label_at(&self, idx: usize) -> Option<String> {
        let c = *self.buffer.get(idx)?;
        let id = paste_id_of(c)?;
        let p = self.pastes.get(&id)?;
        Some(paste_chip_label(&p.content))
    }

    /// Id of paste chip at buffer index, if any.
    pub fn paste_id_at(&self, idx: usize) -> Option<u32> {
        let c = *self.buffer.get(idx)?;
        paste_id_of(c)
    }

    /// Append text to an existing paste block (for merging consecutive drips).
    pub fn append_to_paste(&mut self, id: u32, s: &str) -> bool {
        let normalized = normalize_paste(s);
        if normalized.is_empty() {
            return false;
        }
        if let Some(p) = self.pastes.get_mut(&id) {
            p.content.push_str(&normalized);
            return true;
        }
        false
    }

    /// Delete a range [lo, hi) from the buffer.
    pub fn delete_range(&mut self, lo: usize, hi: usize) {
        if lo >= hi || lo >= self.buffer.len() {
            return;
        }
        let hi = hi.min(self.buffer.len());
        self.buffer.drain(lo..hi);
        if self.cursor > hi {
            self.cursor -= hi - lo;
        } else if self.cursor > lo {
            self.cursor = lo;
        }
        self.selection_anchor = None;
        self.gc_pastes();
    }

    /// Check if buffer contains a paste chip with this id.
    pub fn has_paste_id(&self, id: u32) -> bool {
        self.pastes.contains_key(&id)
    }

    #[allow(dead_code)]
    pub fn paste_at(&self, idx: usize) -> Option<&PasteBlock> {
        let c = *self.buffer.get(idx)?;
        let id = paste_id_of(c)?;
        self.pastes.get(&id)
    }

    fn line_start_index(&self, target_line: usize) -> usize {
        let mut line = 0;
        let mut idx = 0;
        while idx < self.buffer.len() && line < target_line {
            if self.buffer[idx] == '\n' {
                line += 1;
            }
            idx += 1;
        }
        idx
    }

    /// Display cell width of buffer char at `idx` (chips expand to label width).
    pub fn display_width_at(&self, idx: usize) -> usize {
        let Some(&c) = self.buffer.get(idx) else {
            return 0;
        };
        if is_paste_sentinel(c) {
            self.chip_label_at(idx)
                .map(|l| l.width())
                .unwrap_or(1)
                .max(1)
        } else if c == '\n' {
            0
        } else {
            UnicodeWidthChar::width(c).unwrap_or(1).max(1)
        }
    }

    /// Convert a display column on `line` to a buffer char index (start of that cell).
    pub fn index_at_display_col(&self, line: usize, display_col: usize) -> usize {
        let start = self.line_start_index(line);
        let mut idx = start;
        let mut used = 0usize;
        while idx < self.buffer.len() && self.buffer[idx] != '\n' {
            let w = self.display_width_at(idx);
            if used + w > display_col {
                break;
            }
            used += w;
            idx += 1;
            if used >= display_col {
                break;
            }
        }
        idx
    }

    /// Display column of buffer index on its line (start of that char/chip).
    pub fn display_col_of_index(&self, abs: usize) -> usize {
        let (line, _) = self.line_col_of_index(abs);
        let start = self.line_start_index(line);
        let mut col = 0usize;
        let mut i = start;
        while i < abs && i < self.buffer.len() && self.buffer[i] != '\n' {
            col += self.display_width_at(i);
            i += 1;
        }
        col
    }

    fn line_col_of_index(&self, abs: usize) -> (usize, usize) {
        let mut line = 0;
        let mut col = 0;
        let end = abs.min(self.buffer.len());
        for i in 0..end {
            if self.buffer[i] == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    /// Select the entire buffer (Ctrl+A).
    pub fn select_all(&mut self) {
        if self.buffer.is_empty() {
            self.selection_anchor = None;
            return;
        }
        self.selection_anchor = Some(0);
        self.cursor = self.buffer.len();
    }

    /// Half-open selection range, if any.
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let a = self.selection_anchor?;
        let (lo, hi) = if a <= self.cursor {
            (a, self.cursor)
        } else {
            (self.cursor, a)
        };
        if lo == hi {
            None
        } else {
            Some((lo, hi))
        }
    }

    pub fn has_selection(&self) -> bool {
        self.selection_range().is_some()
    }

    /// Selected text with paste chips expanded.
    pub fn selected_text(&self) -> Option<String> {
        let (lo, hi) = self.selection_range()?;
        Some(self.expand_range(lo, hi))
    }

    /// Delete the current selection; returns true if something was removed.
    pub fn delete_selection(&mut self) -> bool {
        let Some((lo, hi)) = self.selection_range() else {
            return false;
        };
        self.buffer.drain(lo..hi);
        self.cursor = lo;
        self.selection_anchor = None;
        self.gc_pastes();
        true
    }

    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    pub fn insert_char(&mut self, c: char) {
        if c == '\r' {
            return;
        }
        // Don't let users type private-use sentinels.
        if is_paste_sentinel(c) {
            return;
        }
        self.delete_selection();
        self.buffer.insert(self.cursor, c);
        self.cursor += 1;
        self.selection_anchor = None;
    }

    pub fn insert_str(&mut self, s: &str) {
        self.delete_selection();
        let chars: Vec<char> = s
            .chars()
            .filter(|c| *c != '\r' && !is_paste_sentinel(*c))
            .collect();
        if chars.is_empty() {
            return;
        }
        // O(n) splice — never per-char insert (that "types out" large pastes slowly).
        let mut new_buf = Vec::with_capacity(self.buffer.len() + chars.len());
        new_buf.extend_from_slice(&self.buffer[..self.cursor]);
        new_buf.extend_from_slice(&chars);
        new_buf.extend_from_slice(&self.buffer[self.cursor..]);
        self.cursor += chars.len();
        self.buffer = new_buf;
        self.selection_anchor = None;
    }

    /// Paste from clipboard / bracketed paste. Large blobs → one chip.
    /// Returns Some(id) if chip was created, None if raw inserted.
    #[allow(dead_code)]
    pub fn insert_paste(&mut self, s: &str) -> Option<u32> {
        let normalized = normalize_paste(s);
        if normalized.is_empty() {
            return None;
        }
        if !should_chip(&normalized) {
            self.insert_str(&normalized);
            return None;
        }
        self.start_paste_chip(&normalized)
    }

    /// Create a new paste chip at the caret; returns its id.
    pub fn start_paste_chip(&mut self, s: &str) -> Option<u32> {
        let normalized = normalize_paste(s);
        if normalized.is_empty() {
            return None;
        }
        self.delete_selection();
        let id = self.alloc_paste(normalized)?;
        let ch = paste_sentinel(id);
        let mut new_buf = Vec::with_capacity(self.buffer.len() + 1);
        new_buf.extend_from_slice(&self.buffer[..self.cursor]);
        new_buf.push(ch);
        new_buf.extend_from_slice(&self.buffer[self.cursor..]);
        self.buffer = new_buf;
        self.cursor += 1;
        self.selection_anchor = None;
        Some(id)
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buffer.remove(self.cursor);
            self.gc_pastes();
        }
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
            self.gc_pastes();
        }
    }

    pub fn move_left(&mut self) {
        self.selection_anchor = None;
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        self.selection_anchor = None;
        if self.cursor < self.buffer.len() {
            self.cursor += 1;
        }
    }

    /// Shift+Left: extend or start selection, then move caret left.
    pub fn extend_left(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Shift+Right: extend or start selection, then move caret right.
    pub fn extend_right(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        if self.cursor < self.buffer.len() {
            self.cursor += 1;
        }
    }

    pub fn extend_up_line(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        let (line, _) = self.cursor_line_col();
        if line == 0 {
            return;
        }
        let dcol = self.display_col_of_index(self.cursor);
        self.cursor = self.index_at_display_col(line - 1, dcol);
    }

    pub fn extend_down_line(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        let (line, _) = self.cursor_line_col();
        if line + 1 >= self.line_count() {
            return;
        }
        let dcol = self.display_col_of_index(self.cursor);
        self.cursor = self.index_at_display_col(line + 1, dcol);
    }

    pub fn cursor_index(&self) -> usize {
        self.cursor
    }

    pub fn char_at(&self, idx: usize) -> Option<char> {
        self.buffer.get(idx).copied()
    }

    pub fn set_cursor_index(&mut self, idx: usize) {
        self.selection_anchor = None;
        self.cursor = idx.min(self.buffer.len());
    }

    pub fn select_start_at_index(&mut self, idx: usize) {
        let idx = idx.min(self.buffer.len());
        self.selection_anchor = Some(idx);
        self.cursor = idx;
    }

    pub fn select_drag_to_index(&mut self, idx: usize) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.cursor = idx.min(self.buffer.len());
    }

    pub fn move_line_home(&mut self) {
        self.selection_anchor = None;
        while self.cursor > 0 && self.buffer[self.cursor - 1] != '\n' {
            self.cursor -= 1;
        }
    }

    pub fn move_line_end(&mut self) {
        self.selection_anchor = None;
        while self.cursor < self.buffer.len() && self.buffer[self.cursor] != '\n' {
            self.cursor += 1;
        }
    }

    pub fn word_left(&mut self) {
        self.selection_anchor = None;
        // Skip chip as one unit.
        if self.cursor > 0 && is_paste_sentinel(self.buffer[self.cursor - 1]) {
            self.cursor -= 1;
            return;
        }
        while self.cursor > 0 && !self.buffer[self.cursor - 1].is_alphanumeric() {
            if is_paste_sentinel(self.buffer[self.cursor - 1]) {
                break;
            }
            self.cursor -= 1;
        }
        while self.cursor > 0 && self.buffer[self.cursor - 1].is_alphanumeric() {
            self.cursor -= 1;
        }
    }

    pub fn word_right(&mut self) {
        self.selection_anchor = None;
        let n = self.buffer.len();
        if self.cursor < n && is_paste_sentinel(self.buffer[self.cursor]) {
            self.cursor += 1;
            return;
        }
        while self.cursor < n && !self.buffer[self.cursor].is_alphanumeric() {
            if is_paste_sentinel(self.buffer[self.cursor]) {
                break;
            }
            self.cursor += 1;
        }
        while self.cursor < n && self.buffer[self.cursor].is_alphanumeric() {
            self.cursor += 1;
        }
    }

    /// True if char index `i` lies inside the active selection.
    #[allow(dead_code)]
    pub fn is_selected(&self, i: usize) -> bool {
        self.selection_range()
            .map(|(lo, hi)| i >= lo && i < hi)
            .unwrap_or(false)
    }

    pub fn delete_word_back(&mut self) {
        let end = self.cursor;
        self.word_left();
        self.buffer.drain(self.cursor..end);
        self.gc_pastes();
    }

    pub fn delete_to_line_start(&mut self) {
        let end = self.cursor;
        self.move_line_home();
        self.buffer.drain(self.cursor..end);
        self.gc_pastes();
    }

    /// Hard line count of the buffer (`\n` separators; chips count as one char).
    pub fn line_count(&self) -> usize {
        1 + self.buffer.iter().filter(|c| **c == '\n').count()
    }

    /// Soft-wrapped visual rows for a content width in terminal cells.
    /// This is what the input viewport scrolls through — one notch = one row.
    pub fn visual_rows(&self, width: usize) -> Vec<VisualRow> {
        let width = width.max(1);
        let mut rows = Vec::new();
        let mut row_start = 0usize;
        let mut col = 0usize;
        let n = self.buffer.len();

        if n == 0 {
            return vec![VisualRow {
                abs_start: 0,
                abs_end: 0,
            }];
        }

        let mut i = 0usize;
        while i < n {
            let c = self.buffer[i];
            if c == '\n' {
                rows.push(VisualRow {
                    abs_start: row_start,
                    abs_end: i,
                });
                i += 1;
                row_start = i;
                col = 0;
                continue;
            }
            let w = self.display_width_at(i).max(1);
            // Wrap before placing if this glyph won't fit (keep at least one per row).
            if col > 0 && col + w > width {
                rows.push(VisualRow {
                    abs_start: row_start,
                    abs_end: i,
                });
                row_start = i;
                col = 0;
            }
            col += w;
            i += 1;
            // If a single glyph is wider than the pane, still put it alone.
            if col >= width {
                rows.push(VisualRow {
                    abs_start: row_start,
                    abs_end: i,
                });
                row_start = i;
                col = 0;
            }
        }
        // Trailing partial row (or empty row after final newline).
        if row_start < n || rows.is_empty() || self.buffer.last() == Some(&'\n') {
            rows.push(VisualRow {
                abs_start: row_start,
                abs_end: n,
            });
        }
        rows
    }

    pub fn visual_line_count(&self, width: usize) -> usize {
        self.visual_rows(width).len().max(1)
    }

    /// Which visual row contains buffer index `abs` (clamped).
    /// Caret at a wrap boundary belongs to the **next** row when one starts there;
    /// caret at end-of-buffer belongs to the last row.
    pub fn visual_row_of_index(&self, abs: usize, width: usize) -> usize {
        let rows = self.visual_rows(width);
        if rows.is_empty() {
            return 0;
        }
        let abs = abs.min(self.buffer.len());
        for (ri, r) in rows.iter().enumerate() {
            if abs < r.abs_end {
                return ri;
            }
            // Empty row (after newline): caret at abs_start == abs_end
            if r.abs_start == r.abs_end && abs == r.abs_start {
                return ri;
            }
        }
        // abs == end of buffer (or end of last non-empty row)
        rows.len() - 1
    }

    /// Display column of `abs` within its visual row (0-based cells).
    pub fn visual_col_of_index(&self, abs: usize, width: usize) -> usize {
        let rows = self.visual_rows(width);
        let ri = self.visual_row_of_index(abs, width);
        let row = &rows[ri.min(rows.len().saturating_sub(1))];
        let mut col = 0usize;
        let mut i = row.abs_start;
        let end = abs.min(row.abs_end).min(self.buffer.len());
        while i < end {
            col += self.display_width_at(i);
            i += 1;
        }
        col
    }

    /// Map a click on visual row `vrow` at display col `dcol` → buffer index.
    pub fn index_at_visual(&self, vrow: usize, dcol: usize, width: usize) -> usize {
        let rows = self.visual_rows(width);
        if rows.is_empty() {
            return 0;
        }
        let row = &rows[vrow.min(rows.len() - 1)];
        let mut col = 0usize;
        let mut i = row.abs_start;
        while i < row.abs_end && i < self.buffer.len() {
            let w = self.display_width_at(i).max(1);
            if col + w > dcol {
                return i;
            }
            col += w;
            i += 1;
            if col >= dcol {
                return i;
            }
        }
        row.abs_end.min(self.buffer.len())
    }

    /// (line, **buffer** col) of the cursor — col counts chips as 1.
    pub fn cursor_line_col(&self) -> (usize, usize) {
        self.line_col_of_index(self.cursor)
    }

    /// (visual_row, col_in_row) of the caret for soft-wrapped viewports.
    pub fn cursor_visual_pos(&self, width: usize) -> (usize, usize) {
        let row = self.visual_row_of_index(self.cursor, width);
        let col = self.visual_col_of_index(self.cursor, width);
        (row, col)
    }

    /// Is the cursor on the first / last line? (for history vs line nav)
    pub fn on_first_line(&self) -> bool {
        self.cursor_line_col().0 == 0
    }

    pub fn on_last_line(&self) -> bool {
        let (line, _) = self.cursor_line_col();
        line + 1 == self.line_count()
    }

    pub fn move_up_line(&mut self) {
        self.selection_anchor = None;
        let (line, _) = self.cursor_line_col();
        if line == 0 {
            return;
        }
        let dcol = self.display_col_of_index(self.cursor);
        self.cursor = self.index_at_display_col(line - 1, dcol);
    }

    pub fn move_down_line(&mut self) {
        self.selection_anchor = None;
        let (line, _) = self.cursor_line_col();
        if line + 1 >= self.line_count() {
            return;
        }
        let dcol = self.display_col_of_index(self.cursor);
        self.cursor = self.index_at_display_col(line + 1, dcol);
    }

    #[allow(dead_code)]
    pub fn move_to_line_col(&mut self, target_line: usize, target_col: usize) {
        // target_col is **buffer** columns (legacy); prefer display helpers for mouse.
        let mut line = 0;
        let mut idx = 0;
        while idx < self.buffer.len() && line < target_line {
            if self.buffer[idx] == '\n' {
                line += 1;
            }
            idx += 1;
        }
        let mut col = 0;
        while idx < self.buffer.len() && self.buffer[idx] != '\n' && col < target_col {
            idx += 1;
            col += 1;
        }
        self.cursor = idx;
    }

    pub fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }
        match self.hist_idx {
            None => {
                self.stash = self.text_expanded();
                self.hist_idx = Some(self.history.len() - 1);
            }
            Some(0) => {}
            Some(i) => self.hist_idx = Some(i - 1),
        }
        if let Some(i) = self.hist_idx {
            let s = self.history[i].clone();
            self.set_text(&s);
        }
    }

    pub fn history_next(&mut self) {
        match self.hist_idx {
            None => {}
            Some(i) if i + 1 < self.history.len() => {
                self.hist_idx = Some(i + 1);
                let s = self.history[i + 1].clone();
                self.set_text(&s);
            }
            Some(_) => {
                self.hist_idx = None;
                let s = self.stash.clone();
                self.set_text(&s);
            }
        }
    }

    /// Enter reverse history search (Ctrl+R). Stashes the current buffer so a
    /// cancel can restore it. If already searching, steps to the next older
    /// match (Ctrl+R pressed again).
    pub fn search_begin(&mut self) {
        if self.search.is_some() {
            self.search_prev();
            return;
        }
        self.search = Some(ReverseSearch {
            query: String::new(),
            match_idx: None,
            restore: self.text_expanded(),
        });
    }

    pub fn search_is_active(&self) -> bool {
        self.search.is_some()
    }

    /// The current incremental-search query, if searching.
    pub fn search_query(&self) -> Option<&str> {
        self.search.as_ref().map(|s| s.query.as_str())
    }

    /// The history entry currently matched by the search, if any.
    pub fn search_match_text(&self) -> Option<String> {
        let s = self.search.as_ref()?;
        s.match_idx.map(|i| self.history[i].clone())
    }

    /// Append a char to the search query and re-match from the newest entry.
    pub fn search_push(&mut self, c: char) {
        let Some(s) = self.search.as_mut() else {
            return;
        };
        s.query.push(c);
        s.match_idx = reverse_search_match(&self.history, &s.query, None);
    }

    /// Remove the last query char and re-match from the newest entry.
    pub fn search_backspace(&mut self) {
        let Some(s) = self.search.as_mut() else {
            return;
        };
        s.query.pop();
        s.match_idx = reverse_search_match(&self.history, &s.query, None);
    }

    /// Ctrl+R again: step to the next older match, keeping the current match if
    /// there is nothing older.
    pub fn search_prev(&mut self) {
        let Some(s) = self.search.as_mut() else {
            return;
        };
        match s.match_idx {
            Some(cur) => {
                if let Some(older) = reverse_search_match(&self.history, &s.query, Some(cur)) {
                    s.match_idx = Some(older);
                }
            }
            None => s.match_idx = reverse_search_match(&self.history, &s.query, None),
        }
    }

    /// Accept the current match into the buffer and leave search mode. With no
    /// match, leaves search mode with the pre-search buffer intact.
    pub fn search_accept(&mut self) {
        if let Some(s) = self.search.take() {
            let text = match s.match_idx {
                Some(i) => self.history[i].clone(),
                None => s.restore,
            };
            self.set_text(&text);
        }
    }

    /// Abort search and restore the buffer as it was before Ctrl+R.
    pub fn search_cancel(&mut self) {
        if let Some(s) = self.search.take() {
            self.set_text(&s.restore);
        }
    }

    /// Take the buffer as a submission: expands chips, records history, clears.
    pub fn submit(&mut self) -> String {
        let text = self.text_expanded();
        self.clear();
        let trimmed = text.trim();
        if !trimmed.is_empty() && self.history.last().map(|h| h.as_str()) != Some(trimmed) {
            self.history.push(trimmed.to_string());
            self.persist_history();
        }
        text
    }

    #[cfg(test)]
    pub(crate) fn empty_for_test() -> Self {
        Self {
            buffer: Vec::new(),
            cursor: 0,
            selection_anchor: None,
            pastes: HashMap::new(),
            next_paste_id: 0,
            history: Vec::new(),
            hist_idx: None,
            stash: String::new(),
            search: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn seed_history_for_test(&mut self, items: &[&str]) {
        self.history = items.iter().map(|s| s.to_string()).collect();
    }

    fn persist_history(&self) {
        let _ = crate::config::ensure_dirs();
        let tail: Vec<&String> = self.history.iter().rev().take(200).collect();
        let mut out = String::new();
        for s in tail.into_iter().rev() {
            if let Ok(line) = serde_json::to_string(s) {
                out.push_str(&line);
                out.push('\n');
            }
        }
        let _ = crate::config::atomic_write(&history_path(), out.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_and_cursor_math() {
        let mut i = InputState::empty_for_test();
        i.insert_str("hello\nworld");
        assert_eq!(i.line_count(), 2);
        assert_eq!(i.cursor_line_col(), (1, 5));
        i.move_line_home();
        assert_eq!(i.cursor_line_col(), (1, 0));
        i.move_up_line();
        assert_eq!(i.cursor_line_col(), (0, 0));
        i.move_line_end();
        assert_eq!(i.cursor_line_col(), (0, 5));
        i.delete_word_back();
        assert_eq!(i.text(), "\nworld");
    }

    #[test]
    fn word_navigation() {
        let mut i = InputState::empty_for_test();
        i.insert_str("foo bar-baz");
        i.word_left();
        assert_eq!(i.cursor_line_col().1, 8); // start of "baz"
        i.word_left();
        assert_eq!(i.cursor_line_col().1, 4); // start of "bar"
        i.word_right();
        assert_eq!(i.cursor_line_col().1, 7); // end of "bar"
    }

    #[test]
    fn unicode_safe() {
        let mut i = InputState::empty_for_test();
        i.insert_str("héllo 日本");
        i.move_left();
        i.backspace();
        assert_eq!(i.text(), "héllo 本");
        i.delete();
        assert_eq!(i.text(), "héllo ");
    }

    #[test]
    fn select_all_and_click() {
        let mut i = InputState::empty_for_test();
        i.insert_str("hello\nworld");
        i.select_all();
        assert_eq!(i.selected_text().as_deref(), Some("hello\nworld"));
        i.set_cursor_index(i.index_at_display_col(1, 2)); // w|orld (display col 2)
        assert!(!i.has_selection());
        assert_eq!(i.cursor_line_col(), (1, 2));
        i.select_all();
        i.insert_str("x");
        assert_eq!(i.text(), "x");
    }

    #[test]
    fn small_paste_stays_raw() {
        let mut i = InputState::empty_for_test();
        i.insert_paste("hello");
        assert_eq!(i.text(), "hello");
        assert!(i.pastes.is_empty());
    }

    #[test]
    fn multi_line_paste_becomes_chip() {
        let mut i = InputState::empty_for_test();
        let body = "line1\nline2\nline3";
        i.insert_paste(body);
        assert_eq!(i.line_count(), 1);
        assert_eq!(i.buffer.len(), 1);
        assert!(is_paste_sentinel(i.buffer[0]));
        assert_eq!(i.text_expanded(), body);
        let label = i.chip_label_at(0).unwrap();
        assert_eq!(label, "[pasted 1-3 lines]");
        let submitted = i.submit();
        assert_eq!(submitted, body);
        assert!(i.is_empty());
    }

    #[test]
    fn long_single_line_paste_chips() {
        let mut i = InputState::empty_for_test();
        let body = "x".repeat(80);
        i.insert_paste(&body);
        assert_eq!(
            i.buffer.len(),
            1,
            "long single-line paste must be one chip, not raw text"
        );
        assert!(is_paste_sentinel(i.buffer[0]));
        assert_eq!(i.text_expanded(), body);
        assert_eq!(i.chip_label_at(0).as_deref(), Some("[pasted 1-1 lines]"));
    }

    #[test]
    fn wall_of_text_is_one_chip_not_n_chars() {
        let mut i = InputState::empty_for_test();
        let mut body = String::new();
        for n in 1..=50 {
            body.push_str(&format!("line {n} of a giant paste wall\n"));
        }
        i.insert_paste(&body);
        assert_eq!(i.buffer.len(), 1);
        assert_eq!(
            i.line_count(),
            1,
            "chip is a single buffer glyph / one hard line"
        );
        assert!(i.text_expanded().starts_with("line 1"));
        assert!(i.text_expanded().contains("line 50"));
        assert_eq!(i.chip_label_at(0).as_deref(), Some("[pasted 1-50 lines]"));
    }

    #[test]
    fn two_pastes_make_two_independent_chips() {
        let mut i = InputState::empty_for_test();
        i.insert_paste("a\nb\nc");
        i.insert_str(" mid ");
        i.insert_paste("d\ne\nf");
        assert_eq!(
            i.buffer.iter().filter(|c| is_paste_sentinel(**c)).count(),
            2
        );
        assert_eq!(i.text_expanded(), "a\nb\nc mid d\ne\nf");
    }

    #[test]
    fn backspace_removes_whole_chip() {
        let mut i = InputState::empty_for_test();
        i.insert_str("hi ");
        i.insert_paste("a\nb\nc");
        assert_eq!(i.buffer.len(), 4); // h i space chip
        i.backspace();
        assert_eq!(i.text(), "hi ");
        assert!(i.pastes.is_empty());
    }

    #[test]
    fn select_copy_expands_chip() {
        let mut i = InputState::empty_for_test();
        i.insert_str("pre ");
        i.insert_paste("one\ntwo");
        i.insert_str(" post");
        i.select_all();
        assert_eq!(i.selected_text().as_deref(), Some("pre one\ntwo post"));
    }

    #[test]
    fn drag_select_across_text() {
        let mut i = InputState::empty_for_test();
        i.insert_str("abcdef");
        i.select_start_at_index(i.index_at_display_col(0, 1));
        i.select_drag_to_index(i.index_at_display_col(0, 4));
        assert_eq!(i.selected_text().as_deref(), Some("bcd"));
    }

    #[test]
    fn set_text_rechips_large_history() {
        let mut i = InputState::empty_for_test();
        let body = "alpha\nbeta\ngamma\ndelta";
        i.set_text(body);
        assert_eq!(i.buffer.len(), 1);
        assert!(is_paste_sentinel(i.buffer[0]));
        assert_eq!(i.text_expanded(), body);
    }

    #[test]
    fn set_text_keeps_small_raw() {
        let mut i = InputState::empty_for_test();
        i.set_text("hi");
        assert_eq!(i.text(), "hi");
        assert!(i.pastes.is_empty());
    }

    #[test]
    fn shift_extend_selection() {
        let mut i = InputState::empty_for_test();
        i.insert_str("abcd");
        i.move_left();
        i.move_left(); // caret at 'c'
        i.extend_right();
        i.extend_right();
        assert_eq!(i.selected_text().as_deref(), Some("cd"));
    }

    #[test]
    fn index_at_display_col_with_chip() {
        let mut i = InputState::empty_for_test();
        i.insert_str("ab");
        i.insert_paste("one\ntwo\nthree");
        i.insert_str("cd");
        // "ab" + chip + "cd"
        assert_eq!(i.buffer.len(), 5);
        let chip_w = i.display_width_at(2);
        assert!(chip_w > 5);
        // Click in middle of chip → chip index
        let mid = 2 + chip_w / 2;
        assert_eq!(i.index_at_display_col(0, mid), 2);
        // Past chip → 'c'
        assert_eq!(i.index_at_display_col(0, 2 + chip_w), 3);
    }

    #[test]
    fn soft_wrap_produces_multiple_visual_rows() {
        let mut i = InputState::empty_for_test();
        // 30 'a' chars with width 10 → 3 visual rows
        i.insert_str(&"a".repeat(30));
        let rows = i.visual_rows(10);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].abs_start, 0);
        assert_eq!(rows[0].abs_end, 10);
        assert_eq!(rows[1].abs_start, 10);
        assert_eq!(rows[1].abs_end, 20);
        assert_eq!(rows[2].abs_start, 20);
        assert_eq!(rows[2].abs_end, 30);
    }

    #[test]
    fn soft_wrap_newlines_and_scroll_units() {
        let mut i = InputState::empty_for_test();
        i.insert_str("one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten");
        // Narrow width still one visual row per hard line (short words).
        let rows = i.visual_rows(40);
        assert_eq!(rows.len(), 10);
        // Scroll max with view of 8: max_top = 2 — intermediate positions exist.
        let max_top = rows.len().saturating_sub(8);
        assert_eq!(max_top, 2);
        assert_eq!(i.index_at_visual(3, 0, 40), rows[3].abs_start);
    }

    #[test]
    fn visual_row_of_cursor_tracks_wrap() {
        let mut i = InputState::empty_for_test();
        i.insert_str(&"x".repeat(25));
        // Caret at end
        assert_eq!(i.visual_row_of_index(25, 10), 2);
        // Caret mid second row
        assert_eq!(i.visual_row_of_index(15, 10), 1);
    }

    #[test]
    fn append_to_paste_merges_drips_into_one_chip() {
        let mut i = InputState::empty_for_test();
        let first = "line one of many
line two
";
        let id = i.start_paste_chip(first).expect("chip");
        assert_eq!(i.buffer.len(), 1);
        assert_eq!(i.paste_id_at(0), Some(id));
        for _ in 0..100 {
            let ok = i.append_to_paste(
                id,
                "more line
",
            );
            assert!(ok);
        }
        assert_eq!(i.buffer.len(), 1, "must stay ONE chip, not 100");
        let expanded = i.text_expanded();
        assert!(expanded.contains("line one"));
        assert!(expanded.lines().count() > 50);
        let label = i.chip_label_at(0).unwrap();
        assert!(label.starts_with("[pasted 1-"));
    }

    #[test]
    fn delete_range_enables_raw_to_chip_conversion() {
        let mut i = InputState::empty_for_test();
        i.insert_str("hello");
        assert_eq!(i.text(), "hello");
        i.delete_range(0, 5);
        assert_eq!(i.text(), "");
        let combined = "hello world this is now a very long paste that should become a chip because it exceeds threshold";
        let id = i.start_paste_chip(combined).unwrap();
        assert_eq!(i.buffer.len(), 1);
        assert!(is_paste_sentinel(i.buffer[0]));
        assert_eq!(i.paste_id_at(0), Some(id));
        assert_eq!(i.text_expanded(), combined);
    }

    #[test]
    fn reverse_search_match_picks_most_recent() {
        let hist: Vec<String> = [
            "git status",
            "cargo build",
            "git commit -m wip",
            "cargo test",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(reverse_search_match(&hist, "git", None), Some(2));
        assert_eq!(reverse_search_match(&hist, "git", Some(2)), Some(0));
        assert_eq!(reverse_search_match(&hist, "git", Some(0)), None);
        assert_eq!(reverse_search_match(&hist, "CARGO", None), Some(3)); // case-insensitive
        assert_eq!(reverse_search_match(&hist, "", None), None);
        assert_eq!(reverse_search_match(&hist, "docker", None), None);
    }

    #[test]
    fn reverse_search_accept_sets_buffer() {
        let mut i = InputState::empty_for_test();
        i.seed_history_for_test(&["git status", "cargo build", "git commit"]);
        i.insert_str("wip");
        i.search_begin();
        assert!(i.search_is_active());
        for c in "git".chars() {
            i.search_push(c);
        }
        assert_eq!(i.search_query(), Some("git"));
        assert_eq!(i.search_match_text().as_deref(), Some("git commit"));
        i.search_accept();
        assert!(!i.search_is_active());
        assert_eq!(i.text_expanded(), "git commit");
    }

    #[test]
    fn reverse_search_cancel_restores_buffer() {
        let mut i = InputState::empty_for_test();
        i.seed_history_for_test(&["git status", "cargo build"]);
        i.insert_str("draft text");
        i.search_begin();
        i.search_push('c');
        i.search_push('a');
        assert_eq!(i.search_match_text().as_deref(), Some("cargo build"));
        i.search_cancel();
        assert!(!i.search_is_active());
        assert_eq!(i.text_expanded(), "draft text");
    }

    #[test]
    fn reverse_search_steps_to_older_matches() {
        let mut i = InputState::empty_for_test();
        i.seed_history_for_test(&["git status", "make all", "git commit"]);
        i.search_begin();
        for c in "git".chars() {
            i.search_push(c);
        }
        assert_eq!(i.search_match_text().as_deref(), Some("git commit"));
        i.search_prev();
        assert_eq!(i.search_match_text().as_deref(), Some("git status"));
        i.search_prev(); // nothing older → keep the last good match
        assert_eq!(i.search_match_text().as_deref(), Some("git status"));
    }

    #[test]
    fn reverse_search_backspace_rematches() {
        let mut i = InputState::empty_for_test();
        i.seed_history_for_test(&["git status", "grep foo", "git commit"]);
        i.search_begin();
        for c in "gitx".chars() {
            i.search_push(c);
        }
        assert_eq!(i.search_match_text(), None); // "gitx" matches nothing
        i.search_backspace(); // back to "git"
        assert_eq!(i.search_match_text().as_deref(), Some("git commit"));
    }

    #[test]
    fn thousand_small_pastes_merged_via_append() {
        let mut i = InputState::empty_for_test();
        let big = (0..1000)
            .map(|n| {
                format!(
                    "line {}
",
                    n
                )
            })
            .collect::<String>();
        let id = i.start_paste_chip(&big).unwrap();
        for _ in 0..999 {
            let _ = i.append_to_paste(id, "x");
        }
        assert_eq!(
            i.buffer.iter().filter(|c| is_paste_sentinel(**c)).count(),
            1,
            "must be ONE chip even after 1000 appends"
        );
        assert!(i.text_expanded().len() >= 1000);
        assert_eq!(i.buffer.len(), 1);
    }
}
