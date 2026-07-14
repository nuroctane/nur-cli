//! Multi-line input editor with cursor movement, paste chips, + persistent history.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Private-use base for paste-chip sentinels in the buffer (one char per chip).
const PASTE_BASE: u32 = 0xE000;
const PASTE_SLOT_COUNT: u32 = 4096;

/// Multi-line or long pastes collapse into a chip when either threshold is hit.
const PASTE_CHIP_MIN_LINES: usize = 2;
const PASTE_CHIP_MIN_CHARS: usize = 200;

#[derive(Clone, Debug)]
pub struct PasteBlock {
    #[allow(dead_code)]
    pub id: u32,
    pub content: String,
}

/// A run of plain text or a single paste chip on a display line.
#[derive(Clone, Debug)]
pub enum DisplaySeg {
    Text(String),
    Chip {
        #[allow(dead_code)]
        id: u32,
        label: String,
    },
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

/// Label shown in the composer for a collapsed paste.
pub fn paste_chip_label(content: &str) -> String {
    let n = paste_line_count(content);
    format!("pasted lines 1-{n}")
}

fn should_chip(s: &str) -> bool {
    let lines = paste_line_count(s);
    let chars = s.chars().filter(|c| *c != '\r').count();
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

    pub fn set_text(&mut self, s: &str) {
        self.drop_all_pastes();
        self.buffer = s.chars().filter(|c| *c != '\r').collect();
        self.cursor = self.buffer.len();
        self.selection_anchor = None;
    }

    pub fn clear(&mut self) {
        self.drop_all_pastes();
        self.buffer.clear();
        self.cursor = 0;
        self.selection_anchor = None;
        self.hist_idx = None;
    }

    fn drop_all_pastes(&mut self) {
        self.pastes.clear();
    }

    fn gc_pastes(&mut self) {
        let live: std::collections::HashSet<u32> = self
            .buffer
            .iter()
            .filter_map(|c| paste_id_of(*c))
            .collect();
        self.pastes.retain(|id, _| live.contains(id));
    }

    fn alloc_paste(&mut self, content: String) -> Option<u32> {
        for _ in 0..PASTE_SLOT_COUNT {
            let id = self.next_paste_id % PASTE_SLOT_COUNT;
            self.next_paste_id = self.next_paste_id.wrapping_add(1);
            if !self.pastes.contains_key(&id)
                && !self.buffer.iter().any(|c| paste_id_of(*c) == Some(id))
            {
                self.pastes.insert(
                    id,
                    PasteBlock {
                        id,
                        content,
                    },
                );
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

    #[allow(dead_code)]
    pub fn paste_at(&self, idx: usize) -> Option<&PasteBlock> {
        let c = *self.buffer.get(idx)?;
        let id = paste_id_of(c)?;
        self.pastes.get(&id)
    }

    /// Display segments for one logical line (no embedded `\n`).
    pub fn line_display_segs(&self, line: usize) -> Vec<DisplaySeg> {
        let text = self.text();
        let line_str = text.split('\n').nth(line).unwrap_or("");
        // Map line_str back to absolute indices via line starts.
        let base = self.line_start_index(line);
        let mut segs = Vec::new();
        let mut plain = String::new();
        for (off, ch) in line_str.chars().enumerate() {
            let abs = base + off;
            if is_paste_sentinel(ch) {
                if !plain.is_empty() {
                    segs.push(DisplaySeg::Text(std::mem::take(&mut plain)));
                }
                let label = self
                    .chip_label_at(abs)
                    .unwrap_or_else(|| "pasted lines 1-1".into());
                let id = paste_id_of(ch).unwrap_or(0);
                segs.push(DisplaySeg::Chip { id, label });
            } else {
                plain.push(ch);
            }
        }
        if !plain.is_empty() {
            segs.push(DisplaySeg::Text(plain));
        }
        segs
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
        for c in s.chars() {
            if c == '\r' || is_paste_sentinel(c) {
                continue;
            }
            self.buffer.insert(self.cursor, c);
            self.cursor += 1;
        }
        self.selection_anchor = None;
    }

    /// Paste from clipboard / bracketed paste — chips large multi-line bodies.
    pub fn insert_paste(&mut self, s: &str) {
        let normalized = normalize_paste(s);
        if normalized.is_empty() {
            return;
        }
        if !should_chip(&normalized) {
            self.insert_str(&normalized);
            return;
        }
        self.delete_selection();
        let Some(id) = self.alloc_paste(normalized) else {
            // Slot exhaustion — fall back to raw insert.
            self.insert_str(s);
            return;
        };
        let ch = paste_sentinel(id);
        self.buffer.insert(self.cursor, ch);
        self.cursor += 1;
        self.selection_anchor = None;
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

    /// Place the caret from a mouse click at (line, **display** column).
    pub fn click_at(&mut self, line: usize, display_col: usize) {
        self.selection_anchor = None;
        self.cursor = self.index_at_display_col(line, display_col);
    }

    /// Start a drag-select: anchor + caret at (line, display col).
    pub fn select_start_at(&mut self, line: usize, display_col: usize) {
        let idx = self.index_at_display_col(line, display_col);
        self.selection_anchor = Some(idx);
        self.cursor = idx;
    }

    /// Extend selection end to (line, display col); keeps anchor.
    pub fn select_drag_to(&mut self, line: usize, display_col: usize) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.cursor = self.index_at_display_col(line, display_col);
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

    /// Line count of the current buffer (chips count as one char on one line).
    pub fn line_count(&self) -> usize {
        1 + self.buffer.iter().filter(|c| **c == '\n').count()
    }

    /// (line, **buffer** col) of the cursor — col counts chips as 1.
    pub fn cursor_line_col(&self) -> (usize, usize) {
        self.line_col_of_index(self.cursor)
    }

    /// (line, **display** col) of the caret for rendering.
    pub fn cursor_display_line_col(&self) -> (usize, usize) {
        let (line, _) = self.cursor_line_col();
        let col = self.display_col_of_index(self.cursor);
        (line, col)
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
        }
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
        i.click_at(1, 2); // w|orld (display col 2)
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
        assert_eq!(label, "pasted lines 1-3");
        let submitted = i.submit();
        assert_eq!(submitted, body);
        assert!(i.is_empty());
    }

    #[test]
    fn long_single_line_paste_chips() {
        let mut i = InputState::empty_for_test();
        let body = "x".repeat(250);
        i.insert_paste(&body);
        assert_eq!(i.buffer.len(), 1);
        assert!(is_paste_sentinel(i.buffer[0]));
        assert_eq!(i.text_expanded(), body);
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
        i.select_start_at(0, 1);
        i.select_drag_to(0, 4);
        assert_eq!(i.selected_text().as_deref(), Some("bcd"));
    }
}
