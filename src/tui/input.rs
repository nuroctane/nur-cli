//! Multi-line input editor with cursor movement + persistent history.

use std::fs;
use std::path::PathBuf;

pub struct InputState {
    /// Buffer with embedded '\n' for multi-line prompts.
    buffer: Vec<char>,
    /// Cursor as a char index into `buffer`.
    cursor: usize,
    history: Vec<String>,
    hist_idx: Option<usize>,
    stash: String,
}

fn history_path() -> PathBuf {
    crate::config::muse_home().join("history.jsonl")
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
            history,
            hist_idx: None,
            stash: String::new(),
        }
    }

    pub fn text(&self) -> String {
        self.buffer.iter().collect()
    }

    pub fn is_empty(&self) -> bool {
        self.buffer.is_empty()
    }

    pub fn set_text(&mut self, s: &str) {
        self.buffer = s.chars().collect();
        self.cursor = self.buffer.len();
    }

    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
        self.hist_idx = None;
    }

    pub fn insert_char(&mut self, c: char) {
        self.buffer.insert(self.cursor, c);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            // Normalize CRLF pastes.
            if c == '\r' {
                continue;
            }
            self.insert_char(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.buffer.remove(self.cursor);
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.buffer.len() {
            self.buffer.remove(self.cursor);
        }
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor += 1;
        }
    }

    pub fn move_line_home(&mut self) {
        while self.cursor > 0 && self.buffer[self.cursor - 1] != '\n' {
            self.cursor -= 1;
        }
    }

    pub fn move_line_end(&mut self) {
        while self.cursor < self.buffer.len() && self.buffer[self.cursor] != '\n' {
            self.cursor += 1;
        }
    }

    pub fn word_left(&mut self) {
        while self.cursor > 0 && !self.buffer[self.cursor - 1].is_alphanumeric() {
            self.cursor -= 1;
        }
        while self.cursor > 0 && self.buffer[self.cursor - 1].is_alphanumeric() {
            self.cursor -= 1;
        }
    }

    pub fn word_right(&mut self) {
        let n = self.buffer.len();
        while self.cursor < n && !self.buffer[self.cursor].is_alphanumeric() {
            self.cursor += 1;
        }
        while self.cursor < n && self.buffer[self.cursor].is_alphanumeric() {
            self.cursor += 1;
        }
    }

    pub fn delete_word_back(&mut self) {
        let end = self.cursor;
        self.word_left();
        self.buffer.drain(self.cursor..end);
    }

    pub fn delete_to_line_start(&mut self) {
        let end = self.cursor;
        self.move_line_home();
        self.buffer.drain(self.cursor..end);
    }

    /// Line count of the current buffer.
    pub fn line_count(&self) -> usize {
        1 + self.buffer.iter().filter(|c| **c == '\n').count()
    }

    /// (line, col) of the cursor for terminal cursor placement.
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let mut line = 0;
        let mut col = 0;
        for i in 0..self.cursor {
            if self.buffer[i] == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
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
        let (line, col) = self.cursor_line_col();
        if line == 0 {
            return;
        }
        self.move_to_line_col(line - 1, col);
    }

    pub fn move_down_line(&mut self) {
        let (line, col) = self.cursor_line_col();
        if line + 1 >= self.line_count() {
            return;
        }
        self.move_to_line_col(line + 1, col);
    }

    fn move_to_line_col(&mut self, target_line: usize, target_col: usize) {
        let mut line = 0;
        let mut idx = 0;
        // Find start of target line.
        while idx < self.buffer.len() && line < target_line {
            if self.buffer[idx] == '\n' {
                line += 1;
            }
            idx += 1;
        }
        // Advance up to target_col or end of line.
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
                self.stash = self.text();
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

    /// Take the buffer as a submission: records history, clears the editor.
    pub fn submit(&mut self) -> String {
        let text = self.text();
        self.clear();
        let trimmed = text.trim();
        if !trimmed.is_empty() && self.history.last().map(|h| h.as_str()) != Some(trimmed) {
            self.history.push(trimmed.to_string());
            self.persist_history();
        }
        text
    }

    #[cfg(test)]
    fn empty_for_test() -> Self {
        Self {
            buffer: Vec::new(),
            cursor: 0,
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
        let _ = fs::write(history_path(), out);
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
}
