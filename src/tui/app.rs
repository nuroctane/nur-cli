//! Interactive TUI: streaming transcript, slash-command palette, tool
//! approval modals, and a persistent usage statusline (bottom-left).

use crate::agent::{
    self, AgentEvent, AgentRunner, ApprovalDecision, PermissionMode, Session, SharedMode,
    SharedTodos,
};
use crate::theme::Tone;
use crate::tools::ToolHost;
use crate::api::MetaClient;
use crate::config::Config;
use crate::error::Result;
use crate::tui::input::InputState;
use crate::usage::{TokenUsage, UsageTracker};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::stdout;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "commands + keyboard shortcuts"),
    ("/clear", "clear the transcript display"),
    ("/new", "start a fresh session"),
    ("/compact", "summarize conversation, free context"),
    ("/mode", "permission: manual | plan | auto  (or Shift+Tab)"),
    ("/plan", "switch to plan mode (read-only)"),
    ("/manual", "switch to manual mode (approve tools)"),
    ("/auto", "switch to auto-approve mode"),
    ("/todos", "show session task list"),
    ("/memory", "show ~/.muse/memory.md excerpt"),
    ("/skills", "list installed skills"),
    ("/graphify", "knowledge graph: status | query | path | explain | extract"),
    ("/plur", "shared engram memory: status | learn | recall | inject"),
    ("/ruflo", "vector memory / swarm: status | search | store"),
    ("/ecosystem", "graphify · plur · ruflo readiness"),
    ("/usage", "token usage + cost for this session"),
    ("/cost", "alias for /usage"),
    ("/model", "show or switch model"),
    ("/effort", "reasoning effort: minimal|low|medium|high|xhigh"),
    ("/sessions", "list recent sessions"),
    ("/resume", "pick a past session to return to  (Ctrl+R)"),
    ("/init", "generate a MUSE.md project guide"),
    ("/mouse", "wheel scrolling ⇄ text selection"),
    ("/config", "show config + data paths"),
    ("/exit", "quit"),
];

pub enum Cell {
    Banner,
    User(String),
    Assistant { text: String, streaming: bool },
    Thinking { text: String, active: bool },
    Tool {
        name: String,
        args: String,
        result: Option<String>,
        ok: Option<bool>,
    },
    /// System notice. `tone` picks the colour + glyph so a mode switch, a plan,
    /// a todo update and a usage dump don't all read as the same blue blob.
    Info { text: String, tone: Tone },
    Error(String),
}

#[derive(PartialEq)]
enum TurnMode {
    Chat,
    Compact,
}

/// What ↑/↓ do in the current UI state. Never history — history lives on
/// Ctrl+P / Ctrl+N, because a past prompt jumping into the input box unbidden
/// is exactly the behavior we removed.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ArrowAction {
    /// Move the selection in the slash-command palette.
    Palette,
    /// Move the caret inside a multi-line draft.
    Caret,
    /// Scroll the transcript.
    Scroll,
}

/// The whole arrow-key policy, as a pure function — single source of truth for
/// both `App::arrow_action` and its tests.
///
/// Reading the chat is the common case, so arrows scroll. They only move the
/// caret when you are genuinely mid-draft on a multi-line prompt.
pub fn decide_arrow_action(input_empty: bool, on_edge: bool, palette_open: bool) -> ArrowAction {
    if palette_open {
        ArrowAction::Palette
    } else if input_empty || on_edge {
        ArrowAction::Scroll
    } else {
        ArrowAction::Caret
    }
}

pub struct ApprovalState {
    pub name: String,
    pub args: String,
    pub respond: Option<oneshot::Sender<ApprovalDecision>>,
}

/// One row of the session picker.
pub struct SessionRow {
    pub id: String,
    pub when: String,
    pub messages: usize,
    pub tokens: u64,
    pub cwd: String,
    pub preview: String,
    /// Session belongs to the current workspace.
    pub here: bool,
}

/// Interactive `/resume` picker: arrow through recent sessions, Enter loads.
pub struct SessionPicker {
    pub rows: Vec<SessionRow>,
    pub idx: usize,
    /// Only show sessions from this workspace.
    pub this_cwd_only: bool,
}

impl SessionPicker {
    pub fn visible(&self) -> Vec<&SessionRow> {
        self.rows
            .iter()
            .filter(|r| !self.this_cwd_only || r.here)
            .collect()
    }
}

pub struct App {
    pub client: MetaClient,
    pub cfg: Config,
    pub cwd: PathBuf,
    /// Live permission mode (manual / plan / auto) — Arc, mid-turn safe.
    pub permission_mode: SharedMode,
    pub approved_tools: Arc<Mutex<HashSet<String>>>,
    pub tool_host: ToolHost,
    pub todos: SharedTodos,

    pub cells: Vec<Cell>,
    tool_cells: HashMap<u64, usize>,
    /// Lines scrolled back from the newest line (0 = following the bottom).
    pub scroll_from_bottom: u16,
    /// Transcript viewport height + wrapped line count, refreshed each draw so
    /// PageUp/Home can scroll in real pages instead of guessing.
    pub view_h: u16,
    pub view_total: u16,
    /// Inner text area of the input box (updated every draw) for click-to-caret.
    pub input_inner: ratatui::layout::Rect,
    /// First visible input line (vertical scroll) + horizontal scroll offset.
    pub input_scroll_top: usize,
    pub input_x_off: u16,
    /// Transcript body area (excluding sticky banner) for scrollbar hit-testing.
    pub transcript_body: ratatui::layout::Rect,
    /// Right-edge scrollbar track (1 column).
    pub scrollbar_track: ratatui::layout::Rect,
    /// True while the user is dragging the scrollbar thumb.
    pub scrollbar_drag: bool,

    pub input: InputState,
    pub queue: VecDeque<String>,

    pub busy: bool,
    /// True after Esc/Ctrl+C until Done arrives — spinners show "cancelling…".
    pub cancelling: bool,
    turn_kind: TurnMode,
    pub turn_started: Instant,
    pub status: String,
    pub spinner_epoch: Instant,

    pub session: Option<Box<Session>>,
    pub usage: Option<Box<UsageTracker>>,
    pub session_id: String,
    pub u_session: TokenUsage,
    pub u_last: TokenUsage,

    pub approval: Option<ApprovalState>,
    pub picker: Option<SessionPicker>,
    pub palette_idx: usize,
    pub quit_armed: Option<Instant>,

    tx: mpsc::UnboundedSender<AgentEvent>,
    rx: mpsc::UnboundedReceiver<AgentEvent>,
    cancel: Option<CancellationToken>,
    should_quit: bool,
}

struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = stdout().execute(Show);
        let _ = disable_raw_mode();
        let _ = stdout().execute(DisableMouseCapture);
        let _ = stdout().execute(DisableBracketedPaste);
        let _ = stdout().execute(LeaveAlternateScreen);
    }
}

pub async fn run_tui(
    client: MetaClient,
    cfg: Config,
    cwd: PathBuf,
    permission_mode: SharedMode,
    session: Session,
    usage: UsageTracker,
    initial_prompt: Option<String>,
    ecosystem_summary: String,
) -> Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableBracketedPaste)?;
    // Always capture the mouse so clicks place the caret in the input box and
    // the wheel scrolls the transcript. Native click-drag selection in the
    // terminal is replaced by in-app selection (Ctrl+A / Shift-style select) +
    // Ctrl+C/V clipboard; Shift+drag still selects in many terminals.
    stdout().execute(EnableMouseCapture)?;
    // Hardware cursor hidden — we paint a Meta blue block caret ourselves.
    stdout().execute(Hide)?;
    let _guard = TermGuard;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let (tx, rx) = mpsc::unbounded_channel();
    let u_session = usage.session_usage().clone();
    let session_id = session.id.clone();
    let mode_label = permission_mode.get().label().to_string();

    let mut app = App {
        client,
        cfg,
        cwd,
        permission_mode,
        approved_tools: Arc::new(Mutex::new(HashSet::new())),
        tool_host: ToolHost::default(),
        todos: agent::shared_empty(),
        cells: vec![Cell::Banner],
        tool_cells: HashMap::new(),
        scroll_from_bottom: 0,
        view_h: 20,
        view_total: 0,
        input_inner: ratatui::layout::Rect::default(),
        input_scroll_top: 0,
        input_x_off: 0,
        transcript_body: ratatui::layout::Rect::default(),
        scrollbar_track: ratatui::layout::Rect::default(),
        scrollbar_drag: false,
        input: InputState::new(),
        queue: VecDeque::new(),
        busy: false,
        cancelling: false,
        turn_kind: TurnMode::Chat,
        turn_started: Instant::now(),
        status: "idle".into(),
        spinner_epoch: Instant::now(),
        session: Some(Box::new(session)),
        usage: Some(Box::new(usage)),
        session_id,
        u_session,
        u_last: TokenUsage::default(),
        approval: None,
        picker: None,
        palette_idx: 0,
        quit_armed: None,
        tx,
        rx,
        cancel: None,
        should_quit: false,
    };

    app.replay_session_tail(8);
    app.push_info(format!(
        "mode · {mode_label}  ·  Shift+Tab cycles  manual → plan → auto  ·  /mode"
    ));
    if !ecosystem_summary.is_empty() {
        app.push_note(Tone::Skill, ecosystem_summary);
    }

    if let Some(p) = initial_prompt {
        if !p.trim().is_empty() {
            app.submit_text(&p);
        }
    }

    // Redraw only when something actually changed (or while an animation is
    // running). Repainting every tick makes the whole UI shimmer and pins a CPU
    // core for nothing.
    let mut dirty = true;
    loop {
        // Drain agent events first so the frame is fresh.
        while let Ok(ev) = app.rx.try_recv() {
            app.on_agent_event(ev);
            dirty = true;
        }

        // Spinners / streaming caret only animate while a turn is in flight.
        if app.busy {
            dirty = true;
        }

        if dirty {
            terminal.draw(|f| super::ui::draw(f, &mut app))?;
            dirty = false;
        }

        if app.should_quit {
            break;
        }

        // ~30fps ceiling while animating; idle just parks on the poll.
        if event::poll(Duration::from_millis(32))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    app.on_key(key);
                    dirty = true;
                }
                Event::Mouse(m) => {
                    app.on_mouse(m);
                    dirty = true;
                }
                Event::Paste(s) => {
                    if app.approval.is_none() && app.picker.is_none() {
                        app.input.insert_str(&s);
                        dirty = true;
                    }
                }
                Event::Resize(_, _) => dirty = true,
                _ => {}
            }
        }
    }

    // Persist on exit.
    if let Some(s) = &app.session {
        let _ = s.save();
    }
    Ok(())
}

impl App {
    // ── palette ────────────────────────────────────────────────────────
    pub fn palette_matches(&self) -> Vec<(&'static str, &'static str)> {
        let text = self.input.text();
        if !text.starts_with('/') || text.contains('\n') {
            return Vec::new();
        }
        let token = text.split_whitespace().next().unwrap_or("");
        // Once a full command + space is typed, hide the palette.
        if text.contains(' ') {
            return Vec::new();
        }
        COMMANDS
            .iter()
            .filter(|(name, _)| name.starts_with(token))
            .copied()
            .collect()
    }

    fn palette_visible(&self) -> bool {
        !self.palette_matches().is_empty()
    }

    // ── arrow-key policy ───────────────────────────────────────────────
    fn arrow_action(&self, up: bool) -> ArrowAction {
        let on_edge = if up {
            self.input.on_first_line()
        } else {
            self.input.on_last_line()
        };
        decide_arrow_action(self.input.is_empty(), on_edge, self.palette_visible())
    }

    // ── transcript scrolling ───────────────────────────────────────────
    /// One page = a viewport minus two lines of overlap for context.
    fn page(&self) -> u16 {
        self.view_h.saturating_sub(2).max(1)
    }

    fn max_scroll(&self) -> u16 {
        self.view_total.saturating_sub(self.view_h)
    }

    pub fn scroll_up(&mut self, n: u16) {
        self.scroll_from_bottom = self
            .scroll_from_bottom
            .saturating_add(n)
            .min(self.max_scroll());
    }

    pub fn scroll_down(&mut self, n: u16) {
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(n);
    }

    fn scroll_to_top(&mut self) {
        self.scroll_from_bottom = self.max_scroll();
    }

    fn scroll_to_bottom(&mut self) {
        self.scroll_from_bottom = 0;
    }

    // ── keys ───────────────────────────────────────────────────────────
    fn on_key(&mut self, key: event::KeyEvent) {
        // Approval modal swallows all keys.
        if self.approval.is_some() {
            self.on_approval_key(key.code);
            return;
        }
        // Session picker swallows all keys while open.
        if self.picker.is_some() {
            self.on_picker_key(key.code);
            return;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            // Ctrl+C: copy selection if any; else interrupt busy turn; else
            // clear input; else double-tap to quit (never steal OS copy when
            // the user has selected text in the editor).
            KeyCode::Char('c') if ctrl => {
                if self.input.has_selection() {
                    if let Some(t) = self.input.selected_text() {
                        clipboard_set(&t);
                    }
                    return;
                }
                if self.busy {
                    self.interrupt();
                } else if !self.input.is_empty() {
                    self.input.clear();
                } else if self
                    .quit_armed
                    .map(|t| t.elapsed() < Duration::from_secs(2))
                    .unwrap_or(false)
                {
                    self.should_quit = true;
                } else {
                    self.quit_armed = Some(Instant::now());
                }
                return;
            }
            KeyCode::Char('d') if ctrl && self.input.is_empty() => {
                self.should_quit = true;
                return;
            }
            // Ctrl+V: paste system clipboard into the input (bracketed paste
            // also works for terminal pastes).
            KeyCode::Char('v') if ctrl => {
                if let Some(t) = clipboard_get() {
                    self.input.insert_str(&t);
                }
                return;
            }
            // Ctrl+X: cut selection to clipboard.
            KeyCode::Char('x') if ctrl => {
                if let Some(t) = self.input.selected_text() {
                    clipboard_set(&t);
                    self.input.delete_selection();
                }
                return;
            }
            // Claude Code pattern: Shift+Tab cycles permission modes immediately.
            KeyCode::BackTab => {
                self.cycle_permission_mode();
                return;
            }
            _ => {}
        }
        self.quit_armed = None;

        match key.code {
            KeyCode::Esc => {
                if self.busy {
                    self.interrupt();
                } else if self.palette_visible() {
                    self.input.clear();
                } else if !self.input.is_empty() {
                    self.input.clear();
                }
            }
            KeyCode::Enter if alt || ctrl => self.input.insert_char('\n'),
            KeyCode::Enter => {
                if self.palette_visible() {
                    let matches = self.palette_matches();
                    let idx = self.palette_idx.min(matches.len().saturating_sub(1));
                    let (name, _) = matches[idx];
                    let text = self.input.text();
                    // Exact match or unique completion → run it.
                    if text.trim() == name || matches.len() == 1 || idx > 0 {
                        self.input.clear();
                        let cmd = name.to_string();
                        self.submit_text(&cmd);
                    } else {
                        self.input.set_text(&format!("{name} "));
                    }
                    self.palette_idx = 0;
                    return;
                }
                let text = self.input.text();
                if text.ends_with('\\') {
                    // Trailing backslash → literal newline.
                    self.input.backspace();
                    self.input.insert_char('\n');
                    return;
                }
                if text.trim().is_empty() {
                    return;
                }
                let submitted = self.input.submit();
                self.submit_text(&submitted);
            }
            KeyCode::Tab => {
                if self.palette_visible() {
                    let matches = self.palette_matches();
                    let idx = self.palette_idx.min(matches.len().saturating_sub(1));
                    self.input.set_text(&format!("{} ", matches[idx].0));
                    self.palette_idx = 0;
                }
            }
            // Arrows scroll the transcript. They only move the caret when you
            // are actually editing a multi-line draft; prompt history lives on
            // Ctrl+P/N (and Alt+↑/↓) so reading back through the chat is the
            // default, not a surprise recall into the input box.
            KeyCode::Up if alt => self.input.history_prev(),
            KeyCode::Down if alt => self.input.history_next(),
            KeyCode::Up => match self.arrow_action(true) {
                ArrowAction::Palette => self.palette_idx = self.palette_idx.saturating_sub(1),
                ArrowAction::Caret => self.input.move_up_line(),
                ArrowAction::Scroll => self.scroll_up(1),
            },
            KeyCode::Down => match self.arrow_action(false) {
                ArrowAction::Palette => {
                    let n = self.palette_matches().len();
                    if self.palette_idx + 1 < n {
                        self.palette_idx += 1;
                    }
                }
                ArrowAction::Caret => self.input.move_down_line(),
                ArrowAction::Scroll => self.scroll_down(1),
            },
            KeyCode::Char('p') if ctrl => self.input.history_prev(),
            KeyCode::Char('n') if ctrl => self.input.history_next(),
            KeyCode::Left if ctrl => self.input.word_left(),
            KeyCode::Right if ctrl => self.input.word_right(),
            KeyCode::Left => self.input.move_left(),
            KeyCode::Right => self.input.move_right(),
            // Home/End edit the draft when there is one, else jump the transcript.
            KeyCode::Home => {
                if self.input.is_empty() {
                    self.scroll_to_top();
                } else {
                    self.input.move_line_home();
                }
            }
            KeyCode::End => {
                if self.input.is_empty() {
                    self.scroll_to_bottom();
                } else {
                    self.input.move_line_end();
                }
            }
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::PageUp => self.scroll_up(self.page()),
            KeyCode::PageDown => self.scroll_down(self.page()),
            KeyCode::Char('l') if ctrl => {
                self.cells.retain(|c| matches!(c, Cell::Banner));
                self.scroll_from_bottom = 0;
            }
            KeyCode::Char('r') if ctrl => self.open_session_picker(),
            // Ctrl+A: select all in the input box (standard editor, not line-home).
            KeyCode::Char('a') if ctrl => self.input.select_all(),
            KeyCode::Char('e') if ctrl => self.input.move_line_end(),
            KeyCode::Char('u') if ctrl => self.input.delete_to_line_start(),
            KeyCode::Char('w') if ctrl => self.input.delete_word_back(),
            KeyCode::Char('j') if ctrl => self.input.insert_char('\n'),
            KeyCode::Char(c) if !ctrl && !alt => {
                self.input.insert_char(c);
                self.palette_idx = 0;
            }
            _ => {}
        }
    }

    /// Mouse: click places caret in the input; wheel scrolls the transcript;
    /// drag the right-edge scrollbar thumb to scrub history.
    fn on_mouse(&mut self, m: event::MouseEvent) {
        if self.approval.is_some() || self.picker.is_some() {
            self.scrollbar_drag = false;
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp => self.scroll_up(3),
            MouseEventKind::ScrollDown => self.scroll_down(3),
            MouseEventKind::Down(MouseButton::Left) => {
                if self.hit_scrollbar(m.column, m.row) {
                    self.scrollbar_drag = true;
                    self.scroll_from_scrollbar_y(m.row);
                } else {
                    self.scrollbar_drag = false;
                    self.click_input(m.column, m.row);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                if self.scrollbar_drag {
                    self.scroll_from_scrollbar_y(m.row);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.scrollbar_drag = false;
            }
            _ => {}
        }
    }

    fn hit_scrollbar(&self, col: u16, row: u16) -> bool {
        let t = self.scrollbar_track;
        t.width > 0
            && col >= t.x
            && col < t.right()
            && row >= t.y
            && row < t.bottom()
    }

    /// Map a Y position on the scrollbar track to `scroll_from_bottom`.
    ///
    /// Track top = oldest (max scroll_from_bottom); track bottom = latest (0).
    fn scroll_from_scrollbar_y(&mut self, row: u16) {
        let t = self.scrollbar_track;
        if t.height == 0 {
            return;
        }
        let max = self.max_scroll();
        if max == 0 {
            self.scroll_from_bottom = 0;
            return;
        }
        let y = row.saturating_sub(t.y).min(t.height.saturating_sub(1));
        // Fraction from top of track (0 = top/oldest, 1 = bottom/newest).
        let frac = y as f64 / t.height.saturating_sub(1).max(1) as f64;
        // scroll_from_bottom is high when looking at old content (top).
        self.scroll_from_bottom = ((1.0 - frac) * max as f64).round() as u16;
        self.scroll_from_bottom = self.scroll_from_bottom.min(max);
    }

    /// Map a terminal (column, row) click onto the input buffer caret.
    fn click_input(&mut self, col: u16, row: u16) {
        let inner = self.input_inner;
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        // Outside the input pane — ignore (transcript clicks don't move caret).
        if col < inner.x || col >= inner.right() || row < inner.y || row >= inner.bottom() {
            return;
        }
        // Content starts after the "❯ " / "  " prefix (2 cells).
        let prefix_w: u16 = 2;
        let local_x = col.saturating_sub(inner.x).saturating_sub(prefix_w) as usize
            + self.input_x_off as usize;
        let local_y = row.saturating_sub(inner.y) as usize + self.input_scroll_top;
        // Convert display column → char column on that line (unicode-width).
        let text = self.input.text();
        let line_str = text.split('\n').nth(local_y).unwrap_or("");
        let mut used = 0usize;
        let mut char_col = 0usize;
        for ch in line_str.chars() {
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
            if used + w > local_x {
                break;
            }
            used += w;
            char_col += 1;
            if used >= local_x {
                break;
            }
        }
        self.input.click_at(local_y, char_col);
    }

    // ── session picker ─────────────────────────────────────────────────
    fn open_session_picker(&mut self) {
        if self.busy {
            self.push_error("wait for the current turn to finish".into());
            return;
        }
        let sessions = match crate::agent::session::list_sessions() {
            Ok(s) => s,
            Err(e) => {
                self.push_error(format!("could not list sessions: {e}"));
                return;
            }
        };
        let here_key = self.cwd.display().to_string().to_lowercase();
        let rows: Vec<SessionRow> = sessions
            .iter()
            .filter(|s| s.id != self.session_id) // current session isn't a resume target
            // Every launch mints a session; empty ones are noise, not history.
            .filter(|s| !s.messages.is_empty())
            .take(50)
            .map(|s| SessionRow {
                id: s.id.clone(),
                when: s.updated_at.format("%m-%d %H:%M").to_string(),
                messages: s.messages.len(),
                tokens: s.usage.total_tokens,
                cwd: s.cwd.clone(),
                preview: s
                    .messages
                    .iter()
                    .find(|m| m.role == "user")
                    .map(|m| m.content.replace('\n', " ").chars().take(90).collect())
                    .unwrap_or_else(|| "(no prompt)".into()),
                here: s.cwd.to_lowercase() == here_key,
            })
            .collect();

        if rows.is_empty() {
            self.push_info("no other sessions to resume".into());
            return;
        }
        // Default to this-workspace-only if any session is from here.
        let any_here = rows.iter().any(|r| r.here);
        self.picker = Some(SessionPicker {
            rows,
            idx: 0,
            this_cwd_only: any_here,
        });
    }

    fn on_picker_key(&mut self, code: KeyCode) {
        let Some(p) = &mut self.picker else { return };
        let count = p.visible().len();
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.picker = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                p.idx = p.idx.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if count > 0 && p.idx + 1 < count {
                    p.idx += 1;
                }
            }
            KeyCode::PageUp => p.idx = p.idx.saturating_sub(5),
            KeyCode::PageDown => {
                if count > 0 {
                    p.idx = (p.idx + 5).min(count - 1);
                }
            }
            KeyCode::Home => p.idx = 0,
            KeyCode::End => p.idx = count.saturating_sub(1),
            // Toggle "this workspace only" / all workspaces.
            KeyCode::Tab | KeyCode::Char('a') => {
                p.this_cwd_only = !p.this_cwd_only;
                p.idx = 0;
            }
            KeyCode::Enter => {
                let id = p.visible().get(p.idx).map(|r| r.id.clone());
                self.picker = None;
                if let Some(id) = id {
                    self.cmd_resume(&id);
                }
            }
            _ => {}
        }
    }

    fn on_approval_key(&mut self, code: KeyCode) {
        let decision = match code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                Some(ApprovalDecision::Approve)
            }
            KeyCode::Char('a') | KeyCode::Char('A') => Some(ApprovalDecision::ApproveAlways),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                Some(ApprovalDecision::Deny)
            }
            _ => None,
        };
        if let Some(d) = decision {
            if let Some(mut a) = self.approval.take() {
                if let Some(respond) = a.respond.take() {
                    let _ = respond.send(d);
                }
            }
        }
    }

    // ── submission ─────────────────────────────────────────────────────
    fn submit_text(&mut self, text: &str) {
        let text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if text.starts_with('/') {
            self.run_command(&text);
            return;
        }
        if self.busy {
            self.queue.push_back(text.clone());
            self.push_info(format!("queued ({} waiting)", self.queue.len()));
            return;
        }
        self.start_turn(&text);
    }

    fn start_turn(&mut self, prompt: &str) {
        let (Some(session), Some(usage)) = (self.session.take(), self.usage.take()) else {
            self.push_error("internal: session busy".into());
            return;
        };
        self.cells.push(Cell::User(prompt.to_string()));
        // Sending always snaps you back to the live end of the conversation.
        self.scroll_to_bottom();
        self.busy = true;
        self.cancelling = false;
        self.turn_kind = TurnMode::Chat;
        self.turn_started = Instant::now();
        self.status = format!("thinking · {}", self.permission_mode.get().label());
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        let runner = Arc::new(self.make_runner());
        agent::spawn_turn(
            runner,
            *session,
            *usage,
            prompt.to_string(),
            self.tx.clone(),
            cancel,
        );
    }

    fn make_runner(&self) -> AgentRunner {
        let host = ToolHost {
            todos: self.todos.clone(),
            plan: self.tool_host.plan.clone(),
        };
        AgentRunner {
            client: self.client.clone(),
            config: self.cfg.clone(),
            cwd: self.cwd.clone(),
            permission_mode: self.permission_mode.clone(),
            verbose: false,
            approved_tools: self.approved_tools.clone(),
            tools: host,
            is_subagent: false,
        }
    }

    /// Apply mode immediately (even mid-turn). Shared with AgentRunner via Arc.
    fn set_permission_mode(&mut self, mode: PermissionMode) {
        self.permission_mode.set(mode);
        // If an approval modal is open and we switched to Auto, resolve it as approve.
        if mode.auto_approves() {
            if let Some(mut a) = self.approval.take() {
                if let Some(respond) = a.respond.take() {
                    let _ = respond.send(ApprovalDecision::Approve);
                }
            }
        }
        // Plan mode: pending approvals become denies.
        if mode.is_read_only_enforced() {
            if let Some(mut a) = self.approval.take() {
                if let Some(respond) = a.respond.take() {
                    let _ = respond.send(ApprovalDecision::Deny);
                }
            }
        }
        self.push_note(
            Tone::Mode,
            format!(
                "mode · {} — {}{}",
                mode.badge(),
                mode.description(),
                if self.busy {
                    "  ·  applies to next tool now"
                } else {
                    ""
                }
            ),
        );
    }

    fn cycle_permission_mode(&mut self) {
        let next = self.permission_mode.cycle();
        // cycle() already stored it; run shared side effects (approval resolution + info).
        self.set_permission_mode(next);
    }

    fn interrupt(&mut self) {
        if self.cancelling {
            // Already cancelling — keep calm UI.
            return;
        }
        if let Some(c) = &self.cancel {
            c.cancel();
        }
        // Unblock approval wait so the agent loop can exit.
        if let Some(mut a) = self.approval.take() {
            if let Some(respond) = a.respond.take() {
                let _ = respond.send(ApprovalDecision::Deny);
            }
        }
        self.cancelling = true;
        self.status = "cancelling…".into();
        // Stop "live" animations that look like work is progressing.
        self.freeze_live_cells_as_cancelled();
        self.push_info("cancelled — waiting for in-flight work to stop".into());
    }

    /// Mark streaming/thinking/running-tool cells so the UI stops looking "active".
    fn freeze_live_cells_as_cancelled(&mut self) {
        for c in self.cells.iter_mut().rev() {
            match c {
                Cell::Assistant { streaming, .. } => {
                    *streaming = false;
                }
                Cell::Thinking { active, text } => {
                    if *active {
                        *active = false;
                        if !text.is_empty() {
                            text.push_str("  · cancelled");
                        }
                    }
                }
                Cell::Tool {
                    result, ok, ..
                } => {
                    if result.is_none() {
                        *result = Some("cancelled".into());
                        *ok = Some(false);
                    }
                }
                _ => {}
            }
        }
    }

    // ── agent events ───────────────────────────────────────────────────
    fn on_agent_event(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Status(s) => {
                if self.cancelling {
                    self.status = "cancelling…".into();
                } else {
                    self.status = s;
                }
            }
            AgentEvent::ReasoningDelta(d) => {
                if self.cancelling {
                    return;
                }
                if let Some(Cell::Thinking { text, active: true }) = self.cells.last_mut() {
                    text.push_str(&d);
                } else {
                    self.cells.push(Cell::Thinking {
                        text: d,
                        active: true,
                    });
                }
            }
            AgentEvent::TextDelta(d) => {
                // Ignore late deltas after Esc so the stream caret stops "typing".
                if self.cancelling {
                    return;
                }
                self.finish_thinking();
                if let Some(Cell::Assistant {
                    text,
                    streaming: true,
                }) = self.cells.last_mut()
                {
                    text.push_str(&d);
                } else {
                    self.cells.push(Cell::Assistant {
                        text: d,
                        streaming: true,
                    });
                }
            }
            AgentEvent::AssistantMessage(m) => {
                self.finish_thinking();
                self.finish_streaming();
                if !m.trim().is_empty() {
                    self.cells.push(Cell::Assistant {
                        text: m,
                        streaming: false,
                    });
                }
            }
            AgentEvent::ToolStart { id, name, args } => {
                if self.cancelling {
                    // Don't start new "running" chrome after cancel.
                    return;
                }
                self.finish_thinking();
                self.finish_streaming();
                self.cells.push(Cell::Tool {
                    name,
                    args,
                    result: None,
                    ok: None,
                });
                self.tool_cells.insert(id, self.cells.len() - 1);
                self.status = "running tool".into();
            }
            AgentEvent::ToolEnd {
                id, result, ok, ..
            } => {
                if let Some(&idx) = self.tool_cells.get(&id) {
                    if let Some(Cell::Tool {
                        result: r, ok: o, ..
                    }) = self.cells.get_mut(idx)
                    {
                        *r = Some(result);
                        *o = Some(ok);
                    }
                }
            }
            AgentEvent::ApprovalRequest {
                name,
                args,
                respond,
            } => {
                self.approval = Some(ApprovalState {
                    name,
                    args,
                    respond: Some(respond),
                });
            }
            AgentEvent::Usage { session, last } => {
                self.u_session = session;
                self.u_last = last;
            }
            AgentEvent::TodosChanged(text) => {
                self.push_note(Tone::Todos, format!("todos\n{text}"));
            }
            AgentEvent::PlanSubmitted(text) => {
                self.push_note(
                    Tone::Plan,
                    format!("plan saved — Shift+Tab to manual/auto, then implement\n{text}"),
                );
            }
            AgentEvent::Done {
                session,
                usage,
                result,
                interrupted,
            } => {
                self.finish_thinking();
                self.finish_streaming();
                // Ensure no cell still looks "running".
                self.freeze_live_cells_as_cancelled();
                self.u_session = usage.session_usage().clone();
                self.session_id = session.id.clone();
                self.session = Some(session);
                self.usage = Some(usage);
                self.busy = false;
                self.cancelling = false;
                self.cancel = None;
                self.status = "idle".into();
                match (&self.turn_kind, result, interrupted) {
                    (_, _, true) => {
                        // Already pushed "cancelled" on Esc; keep a quiet final line.
                        if !self
                            .cells
                            .iter()
                            .rev()
                            .take(3)
                            .any(|c| matches!(c, Cell::Info { text, .. } if text.contains("cancelled")))
                        {
                            self.push_info("cancelled".into());
                        }
                    }
                    (TurnMode::Compact, Ok(summary), _) => {
                        self.push_info(format!(
                            "context compacted — summary:\n{summary}"
                        ));
                    }
                    (TurnMode::Compact, Err(e), _) => {
                        self.push_error(format!("compaction failed: {e}"))
                    }
                    (TurnMode::Chat, Err(e), _) => {
                        // Interrupted surfaces as Err("interrupted") sometimes.
                        if e.contains("interrupted") {
                            // quiet — UI already shows cancelled
                        } else {
                            self.push_error(e);
                        }
                    }
                    (TurnMode::Chat, Ok(_), _) => {}
                }
                self.turn_kind = TurnMode::Chat;
                // Drop queued prompts after cancel so we don't surprise-run them.
                if interrupted {
                    self.queue.clear();
                } else if let Some(next) = self.queue.pop_front() {
                    self.submit_text(&next);
                }
            }
        }
    }

    fn finish_streaming(&mut self) {
        if let Some(Cell::Assistant { streaming, .. }) = self.cells.last_mut() {
            *streaming = false;
        }
    }

    fn finish_thinking(&mut self) {
        for c in self.cells.iter_mut().rev() {
            if let Cell::Thinking { active, .. } = c {
                *active = false;
                break;
            }
        }
    }

    // ── slash commands ──────────────────────────────────────────────────
    fn run_command(&mut self, raw: &str) {
        let mut parts = raw.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim().to_string();

        match cmd {
            "/exit" | "/quit" => self.should_quit = true,
            "/help" => self.cmd_help(),
            "/clear" => {
                self.cells.retain(|c| matches!(c, Cell::Banner));
                self.scroll_from_bottom = 0;
            }
            "/new" => self.cmd_new(),
            "/compact" => self.cmd_compact(),
            "/mode" => self.cmd_mode(&arg),
            "/plan" => self.set_permission_mode(PermissionMode::Plan),
            "/manual" => self.set_permission_mode(PermissionMode::Manual),
            "/auto" => self.set_permission_mode(PermissionMode::Auto),
            "/todos" => {
                let t = self
                    .todos
                    .lock()
                    .map(|g| g.render())
                    .unwrap_or_else(|_| "(lock error)".into());
                self.push_note(Tone::Todos, format!("todos\n{t}"));
            }
            "/memory" => {
                self.push_note(
                    Tone::Memory,
                    format!(
                    "memory\n{}",
                    agent::memory::read_memory()
                        .chars()
                        .rev()
                        .take(2000)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                    ),
                );
            }
            "/skills" => {
                let skills = agent::skills::load_skills(&self.cwd);
                if skills.is_empty() {
                    self.push_note(
                        Tone::Skill,
                        "no skills found — add ~/.muse/skills/<name>/SKILL.md\n\
                         or ~/.agents/skills/<name>/SKILL.md  (graphify install --platform agents)\n\
                         the agent can also load them itself via the `skill` tool"
                            .into(),
                    );
                } else {
                    let mut s = String::from("skills (agent loads via `skill` tool)\n");
                    for sk in skills {
                        s.push_str(&format!("  · {} — {}\n", sk.name, sk.description));
                    }
                    self.push_note(Tone::Skill, s);
                }
            }
            "/usage" | "/cost" => self.cmd_usage(),
            "/model" => self.cmd_model(&arg),
            "/effort" => self.cmd_effort(&arg),
            "/sessions" => self.cmd_sessions(),
            "/resume" => {
                if arg.is_empty() {
                    self.open_session_picker();
                } else {
                    self.cmd_resume(&arg);
                }
            }
            "/config" => self.cmd_config(),
            "/init" => {
                self.submit_text(
                    "Analyze this codebase (structure, build/test commands, conventions, \
                     architecture) and create a MUSE.md file at the workspace root that future \
                     agent sessions can use as project instructions. Keep it under 120 lines.",
                );
            }
            "/mouse" => self.cmd_mouse(),
            "/graphify" => self.cmd_graphify(&arg),
            "/plur" => self.cmd_plur(&arg),
            "/ruflo" => self.cmd_ruflo(&arg),
            "/ecosystem" => {
                self.push_note(Tone::Skill, crate::ecosystem::quick_status());
            }
            other => self.push_error(format!("unknown command: {other} — try /help")),
        }
    }

    fn cmd_plur(&mut self, arg: &str) {
        let arg = arg.trim();
        let json = if arg.is_empty() || arg == "status" || arg == "help" {
            r#"{"action":"status"}"#.to_string()
        } else {
            let mut parts = arg.splitn(2, char::is_whitespace);
            let action = parts.next().unwrap_or("status").trim();
            let rest = parts.next().unwrap_or("").trim();
            match action {
                "learn" => {
                    if rest.is_empty() {
                        self.push_error("usage: /plur learn <statement>".into());
                        return;
                    }
                    serde_json::json!({"action":"learn","statement": rest}).to_string()
                }
                "recall" | "search" => {
                    if rest.is_empty() {
                        self.push_error("usage: /plur recall <query>".into());
                        return;
                    }
                    serde_json::json!({"action":"recall","query": rest}).to_string()
                }
                "inject" => {
                    let task = if rest.is_empty() { "coding task" } else { rest };
                    serde_json::json!({"action":"inject","task": task}).to_string()
                }
                "list" => r#"{"action":"list"}"#.to_string(),
                "capture" => {
                    if rest.is_empty() {
                        self.push_error("usage: /plur capture <summary>".into());
                        return;
                    }
                    serde_json::json!({"action":"capture","summary": rest}).to_string()
                }
                "timeline" => r#"{"action":"timeline"}"#.to_string(),
                "status" | "help" => r#"{"action":"status"}"#.to_string(),
                other => {
                    // Free text → learn
                    serde_json::json!({"action":"learn","statement": format!("{other} {rest}").trim()})
                        .to_string()
                }
            }
        };
        let host = ToolHost::default();
        let ctx = crate::tools::ToolContext {
            cwd: self.cwd.clone(),
            cancel: CancellationToken::new(),
        };
        match host.dispatch("plur", &json, &ctx) {
            Ok(s) => self.push_note(Tone::Memory, s),
            Err(e) => self.push_error(e.to_string()),
        }
    }

    fn cmd_ruflo(&mut self, arg: &str) {
        let arg = arg.trim();
        let json = if arg.is_empty() || arg == "status" || arg == "help" {
            r#"{"action":"status"}"#.to_string()
        } else {
            let mut parts = arg.splitn(2, char::is_whitespace);
            let action = parts.next().unwrap_or("status").trim();
            let rest = parts.next().unwrap_or("").trim();
            match action {
                "search" | "memory_search" => {
                    if rest.is_empty() {
                        self.push_error("usage: /ruflo search <query>".into());
                        return;
                    }
                    serde_json::json!({"action":"memory_search","query": rest}).to_string()
                }
                "store" | "memory_store" => {
                    // /ruflo store key=value or /ruflo store key value
                    let (k, v) = if let Some((a, b)) = rest.split_once('=') {
                        (a.trim(), b.trim())
                    } else {
                        let mut sp = rest.splitn(2, char::is_whitespace);
                        (sp.next().unwrap_or("").trim(), sp.next().unwrap_or("").trim())
                    };
                    if k.is_empty() || v.is_empty() {
                        self.push_error("usage: /ruflo store <key> <value>".into());
                        return;
                    }
                    serde_json::json!({"action":"memory_store","key": k, "value": v}).to_string()
                }
                "stats" => r#"{"action":"memory_stats"}"#.to_string(),
                "list" => r#"{"action":"memory_list"}"#.to_string(),
                "agents" | "agent_list" => r#"{"action":"agent_list"}"#.to_string(),
                "swarm" => r#"{"action":"swarm_status"}"#.to_string(),
                "doctor" => r#"{"action":"doctor"}"#.to_string(),
                "status" => r#"{"action":"status"}"#.to_string(),
                other => {
                    serde_json::json!({"action":"memory_search","query": format!("{other} {rest}").trim()})
                        .to_string()
                }
            }
        };
        let host = ToolHost::default();
        let ctx = crate::tools::ToolContext {
            cwd: self.cwd.clone(),
            cancel: CancellationToken::new(),
        };
        match host.dispatch("ruflo", &json, &ctx) {
            Ok(s) => self.push_note(Tone::Skill, s),
            Err(e) => self.push_error(e.to_string()),
        }
    }

    /// Run graphify CLI actions from the TUI without going through the model.
    fn cmd_graphify(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() || arg == "status" || arg == "help" || arg == "-h" || arg == "--help" {
            // Always show status; if empty also print usage.
            let host = ToolHost::default();
            let ctx = crate::tools::ToolContext {
                cwd: self.cwd.clone(),
                cancel: CancellationToken::new(),
            };
            match host.dispatch("graphify", r#"{"action":"status"}"#, &ctx) {
                Ok(s) => {
                    let mut msg = s;
                    if arg.is_empty() || arg == "help" || arg == "-h" || arg == "--help" {
                        msg.push_str(
                            "\n\nusage\n  \
                             /graphify                         status (CLI + graph present?)\n  \
                             /graphify query <question>        BFS over graph.json\n  \
                             /graphify path <A> <B>            shortest path\n  \
                             /graphify explain <node>          node + neighbors\n  \
                             /graphify report                  GRAPH_REPORT.md excerpt\n  \
                             /graphify extract [path]          build local code AST graph\n  \
                             /graphify update [path]           re-extract changed code\n\n\
                             install:  uv tool install graphifyy\n\
                                       graphify install --platform agents\n\
                             skill:    skill(action=read, name=graphify)  or  /skills",
                        );
                    }
                    self.push_note(Tone::Skill, msg);
                }
                Err(e) => self.push_error(e.to_string()),
            }
            return;
        }

        let mut parts = arg.splitn(2, char::is_whitespace);
        let action = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();

        let json = match action {
            "query" | "q" => {
                if rest.is_empty() {
                    self.push_error("usage: /graphify query <question>".into());
                    return;
                }
                serde_json::json!({"action": "query", "question": rest}).to_string()
            }
            "path" => {
                let mut ab = rest.split_whitespace();
                let from = ab.next().unwrap_or("");
                let to = ab.next().unwrap_or("");
                if from.is_empty() || to.is_empty() {
                    self.push_error("usage: /graphify path <A> <B>".into());
                    return;
                }
                serde_json::json!({"action": "path", "from": from, "to": to}).to_string()
            }
            "explain" => {
                if rest.is_empty() {
                    self.push_error("usage: /graphify explain <node>".into());
                    return;
                }
                serde_json::json!({"action": "explain", "node": rest}).to_string()
            }
            "affected" => {
                if rest.is_empty() {
                    self.push_error("usage: /graphify affected <node>".into());
                    return;
                }
                serde_json::json!({"action": "affected", "node": rest}).to_string()
            }
            "report" => r#"{"action":"report"}"#.to_string(),
            "extract" | "build" => {
                let path = if rest.is_empty() { "." } else { rest };
                serde_json::json!({"action": "extract", "path": path}).to_string()
            }
            "update" => {
                let path = if rest.is_empty() { "." } else { rest };
                serde_json::json!({"action": "update", "path": path}).to_string()
            }
            "status" => r#"{"action":"status"}"#.to_string(),
            other => {
                // Treat free text as a query (fast path when graph exists).
                serde_json::json!({"action": "query", "question": format!("{other} {rest}").trim()})
                    .to_string()
            }
        };

        self.push_note(Tone::Skill, format!("graphify · {action}…"));
        let host = ToolHost::default();
        let ctx = crate::tools::ToolContext {
            cwd: self.cwd.clone(),
            cancel: CancellationToken::new(),
        };
        match host.dispatch("graphify", &json, &ctx) {
            Ok(s) => self.push_note(Tone::Skill, s),
            Err(e) => self.push_error(e.to_string()),
        }
    }

    fn cmd_mode(&mut self, arg: &str) {
        if arg.is_empty() {
            let m = self.permission_mode.get();
            self.push_note(
                Tone::Mode,
                format!(
                    "mode · {} — {}\n  Shift+Tab cycles  manual → plan → auto\n  /mode manual|plan|auto",
                    m.badge(),
                    m.description()
                ),
            );
            return;
        }
        match PermissionMode::parse(arg) {
            Some(m) => self.set_permission_mode(m),
            None => self.push_error(format!(
                "unknown mode '{arg}' — use manual, plan, or auto"
            )),
        }
    }

    fn cmd_help(&mut self) {
        let mut s = String::from("commands\n");
        for (name, desc) in COMMANDS {
            s.push_str(&format!("  {name:<10} {desc}\n"));
        }
        let m = self.permission_mode.get();
        s.push_str(&format!(
            "\npermission mode now: {} — {}\n\
             keys\n  \
             ↑/↓ · wheel · drag scrollbar   scroll the chat\n  \
             click in input                 place caret where you click\n  \
             Ctrl+A / Ctrl+C / Ctrl+V / Ctrl+X   select-all · copy · paste · cut\n  \
             Ctrl+P/Ctrl+N  prompt history    (also Alt+↑/↓)\n  \
             Enter          send              (\\+Enter or Ctrl+J for a newline)\n  \
             Shift+Tab      cycle modes  ·  Ctrl+R resume a session\n  \
             Esc            cancel turn   ·  Ctrl+C (no selection) twice quit  ·  Ctrl+L clear\n  \
             approvals (manual): y once · a always · n deny",
            m.badge(),
            m.description()
        ));
        self.push_info(s);
    }

    fn cmd_new(&mut self) {
        if self.busy {
            self.push_error("wait for the current turn to finish".into());
            return;
        }
        if let Some(s) = &self.session {
            let _ = s.save();
        }
        let session = Session::new(&self.cfg.model, &self.cwd.display().to_string());
        self.session_id = session.id.clone();
        let usage = UsageTracker::new(
            session.id.clone(),
            self.cfg.model.clone(),
            self.cwd.clone(),
        );
        self.session = Some(Box::new(session));
        self.usage = Some(Box::new(usage));
        self.u_session = TokenUsage::default();
        self.u_last = TokenUsage::default();
        self.cells.retain(|c| matches!(c, Cell::Banner));
        self.push_info(format!(
            "new session {}",
            &self.session_id[..8.min(self.session_id.len())]
        ));
    }

    fn cmd_compact(&mut self) {
        if self.busy {
            self.push_error("wait for the current turn to finish".into());
            return;
        }
        let (Some(session), Some(usage)) = (self.session.take(), self.usage.take()) else {
            return;
        };
        self.busy = true;
        self.cancelling = false;
        self.turn_kind = TurnMode::Compact;
        self.turn_started = Instant::now();
        self.status = "compacting".into();
        let runner = self.make_runner();
        let tx = self.tx.clone();
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        tokio::spawn(async move {
            let mut session = *session;
            let mut usage = *usage;
            let res = tokio::select! {
                _ = cancel.cancelled() => Err(crate::error::MuseError::Interrupted),
                r = agent::compact_session(&runner, &mut session, &mut usage) => r,
            };
            let interrupted = matches!(res, Err(crate::error::MuseError::Interrupted));
            let _ = tx.send(AgentEvent::Done {
                session: Box::new(session),
                usage: Box::new(usage),
                result: res.map_err(|e| e.to_string()),
                interrupted,
            });
        });
    }

    /// Toggle whether wheel-scroll is aggressive; mouse is always captured for
    /// click-to-caret + scrollbar drag. Kept for config compatibility.
    fn cmd_mouse(&mut self) {
        self.cfg.mouse = !self.cfg.mouse;
        // Always re-assert capture — click/scrollbar depend on it.
        let _ = stdout().execute(EnableMouseCapture);
        let _ = crate::config::save_config(&self.cfg);
        self.push_note(
            Tone::Mode,
            format!(
                "mouse capture always on for caret + scrollbar\n  \
                 click input to place caret  ·  drag right-edge thumb to scroll\n  \
                 Ctrl+A select all · Ctrl+C copy · Ctrl+V paste · Ctrl+X cut\n  \
                 wheel pref stored: {}",
                if self.cfg.mouse { "primary" } else { "secondary" }
            ),
        );
    }

    fn cmd_usage(&mut self) {
        let u = &self.u_session;
        self.push_note(
            Tone::Usage,
            format!(
            "session usage\n  input    {} tok ({} cached)\n  output   {} tok ({} reasoning)\n  \
             total    {} tok\n  est cost ${:.4}\n  status   {}",
            fmt_num(u.input_tokens),
            fmt_num(u.cached_tokens),
            fmt_num(u.output_tokens),
            fmt_num(u.reasoning_tokens),
            fmt_num(u.total_tokens),
            u.estimated_cost_usd(),
            crate::config::status_path().display(),
        ));
    }

    fn cmd_model(&mut self, arg: &str) {
        if arg.is_empty() {
            self.push_info(format!(
                "model: {} · effort: {} · /model <id> to switch",
                self.cfg.model, self.cfg.reasoning_effort
            ));
            return;
        }
        self.cfg.model = arg.to_string();
        if let Some(s) = &mut self.session {
            s.model = arg.to_string();
        }
        if let Some(u) = &mut self.usage {
            u.set_model(arg.to_string());
        }
        self.push_info(format!("model → {arg}"));
    }

    fn cmd_effort(&mut self, arg: &str) {
        const LEVELS: &[&str] = &["minimal", "low", "medium", "high", "xhigh"];
        if arg.is_empty() {
            self.push_info(format!(
                "effort: {} · /effort <{}>",
                self.cfg.reasoning_effort,
                LEVELS.join("|")
            ));
            return;
        }
        if !LEVELS.contains(&arg) {
            self.push_error(format!("invalid effort '{arg}' — use {}", LEVELS.join("|")));
            return;
        }
        self.cfg.reasoning_effort = arg.to_string();
        self.push_info(format!("reasoning effort → {arg}"));
    }

    fn cmd_sessions(&mut self) {
        match crate::agent::session::list_sessions() {
            Ok(sessions) => {
                if sessions.is_empty() {
                    self.push_info("no sessions yet".into());
                    return;
                }
                let mut s = String::from("recent sessions (/resume <id>)\n");
                for sess in sessions.iter().take(10) {
                    s.push_str(&format!(
                        "  {}  {}  {:>4} msgs  {:>8} tok  {}\n",
                        &sess.id[..8.min(sess.id.len())],
                        sess.updated_at.format("%m-%d %H:%M"),
                        sess.messages.len(),
                        fmt_num(sess.usage.total_tokens),
                        sess.cwd
                    ));
                }
                self.push_info(s.trim_end().to_string());
            }
            Err(e) => self.push_error(format!("could not list sessions: {e}")),
        }
    }

    fn cmd_resume(&mut self, arg: &str) {
        if self.busy {
            self.push_error("wait for the current turn to finish".into());
            return;
        }
        if arg.is_empty() {
            self.push_error("usage: /resume <session-id-prefix>".into());
            return;
        }
        match Session::load(arg) {
            Ok(mut loaded) => {
                if let Some(s) = &self.session {
                    let _ = s.save();
                }
                // Tools stay sandboxed to the *current* workspace, so a session
                // resumed from elsewhere is re-homed here — say so plainly.
                let from_elsewhere = {
                    let here = self.cwd.display().to_string();
                    (!loaded.cwd.eq_ignore_ascii_case(&here)).then(|| loaded.cwd.clone())
                };
                loaded.cwd = self.cwd.display().to_string();
                self.session_id = loaded.id.clone();
                let mut tracker = UsageTracker::new(
                    loaded.id.clone(),
                    self.cfg.model.clone(),
                    self.cwd.clone(),
                );
                tracker.seed_session(loaded.usage.clone());
                self.u_session = loaded.usage.clone();
                self.session = Some(Box::new(loaded));
                self.usage = Some(Box::new(tracker));
                self.cells.retain(|c| matches!(c, Cell::Banner));
                let short = &self.session_id[..8.min(self.session_id.len())];
                match from_elsewhere {
                    Some(old) => self.push_info(format!(
                        "resumed session {short}\n  was: {old}\n  now: {}  (tools stay sandboxed here)",
                        self.cwd.display()
                    )),
                    None => self.push_info(format!("resumed session {short}")),
                }
                self.replay_session_tail(20);
            }
            Err(e) => self.push_error(format!("resume failed: {e}")),
        }
    }

    fn cmd_config(&mut self) {
        self.push_info(format!(
            "config ({})\n  model           {}\n  base_url        {}\n  effort          {}\n  \
             max_turns       {}\n  stream          {}\n  context_window  {}\n\npaths\n  \
             home     {}\n  status   {}\n  usage    {}\n  sessions {}",
            crate::config::config_path().display(),
            self.cfg.model,
            self.cfg.base_url,
            self.cfg.reasoning_effort,
            self.cfg.max_turns,
            self.cfg.stream,
            fmt_num(self.cfg.context_window),
            crate::config::muse_home().display(),
            crate::config::status_path().display(),
            crate::config::usage_log_path().display(),
            crate::config::sessions_dir().display(),
        ));
    }

    // ── helpers ────────────────────────────────────────────────────────
    fn replay_session_tail(&mut self, n: usize) {
        let Some(session) = &self.session else { return };
        let msgs: Vec<(String, String)> = session
            .messages
            .iter()
            .rev()
            .take(n)
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();
        if msgs.is_empty() {
            return;
        }
        self.cells.push(Cell::Info {
            text: format!("history · {} messages", session.messages.len()),
            tone: Tone::Session,
        });
        for (role, content) in msgs.into_iter().rev() {
            match role.as_str() {
                "user" => self.cells.push(Cell::User(content)),
                _ => self.cells.push(Cell::Assistant {
                    text: content,
                    streaming: false,
                }),
            }
        }
    }

    fn push_info(&mut self, s: String) {
        self.cells.push(Cell::Info {
            text: s,
            tone: Tone::Neutral,
        });
    }

    /// Notice with a semantic colour/glyph (mode, plan, todos, usage, …).
    fn push_note(&mut self, tone: Tone, s: String) {
        self.cells.push(Cell::Info { text: s, tone });
    }

    fn push_error(&mut self, s: String) {
        self.cells.push(Cell::Error(s));
    }
}

// ── system clipboard (Ctrl+C / Ctrl+V / Ctrl+X) ───────────────────────────

fn clipboard_set(text: &str) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_text(text.to_string());
    }
}

fn clipboard_get() -> Option<String> {
    arboard::Clipboard::new()
        .ok()
        .and_then(|mut cb| cb.get_text().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::input::InputState;

    /// Calls the same `decide_arrow_action` the App does — no mirrored logic,
    /// so the tests cannot drift from the behavior they claim to pin.
    fn arrow_action(input: &InputState, palette: bool, up: bool) -> ArrowAction {
        let on_edge = if up {
            input.on_first_line()
        } else {
            input.on_last_line()
        };
        decide_arrow_action(input.is_empty(), on_edge, palette)
    }

    #[test]
    fn empty_input_arrows_scroll_the_chat() {
        let i = InputState::empty_for_test();
        assert_eq!(arrow_action(&i, false, true), ArrowAction::Scroll);
        assert_eq!(arrow_action(&i, false, false), ArrowAction::Scroll);
    }

    #[test]
    fn single_line_draft_still_scrolls() {
        // A draft in the box must not turn ↑ into "replace my draft with history".
        let mut i = InputState::empty_for_test();
        i.insert_str("hello draft");
        assert_eq!(arrow_action(&i, false, true), ArrowAction::Scroll);
        assert_eq!(arrow_action(&i, false, false), ArrowAction::Scroll);
    }

    #[test]
    fn multi_line_draft_moves_the_caret_inside_it() {
        let mut i = InputState::empty_for_test();
        i.insert_str("line one\nline two");
        // Cursor on the last line: ↑ edits, ↓ falls through to scroll.
        assert_eq!(arrow_action(&i, false, true), ArrowAction::Caret);
        assert_eq!(arrow_action(&i, false, false), ArrowAction::Scroll);
        i.move_up_line();
        // Now on the first line: ↓ edits, ↑ falls through to scroll.
        assert_eq!(arrow_action(&i, false, false), ArrowAction::Caret);
        assert_eq!(arrow_action(&i, false, true), ArrowAction::Scroll);
    }

    #[test]
    fn palette_owns_the_arrows_while_open() {
        let mut i = InputState::empty_for_test();
        i.insert_str("/mo");
        assert_eq!(arrow_action(&i, true, true), ArrowAction::Palette);
        assert_eq!(arrow_action(&i, true, false), ArrowAction::Palette);
    }

    #[test]
    fn history_is_reachable_only_from_explicit_keys() {
        // Ctrl+P / Alt+↑ call history_prev directly; the arrow policy never does.
        let mut i = InputState::empty_for_test();
        i.insert_str("first");
        let _ = i.submit();
        i.history_prev();
        assert_eq!(i.text(), "first");
    }
}

pub fn fmt_num(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
