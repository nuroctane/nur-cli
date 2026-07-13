//! Interactive TUI: streaming transcript, slash-command palette, tool
//! approval modals, and a persistent usage statusline (bottom-left).

use crate::agent::{
    self, AgentEvent, AgentRunner, ApprovalDecision, PermissionMode, Session, SharedMode,
    SharedPermissions, SharedTodos,
};
use crate::theme::{self, Tone};
use crate::tools::ToolHost;
use crate::api::MetaClient;
use crate::config::Config;
use crate::error::Result;
use crate::tui::input::InputState;
use crate::tui::scrollbar::{Hit, ScrollMetrics};
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
use std::io::{stdout, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

pub const COMMANDS: &[(&str, &str)] = &[
    ("/help", "commands + keyboard shortcuts"),
    ("/commands", "commands + keyboard shortcuts"),
    ("/clear", "clear the transcript display"),
    ("/new", "start a fresh session"),
    ("/compact", "summarize conversation, free context"),
    ("/cd", "change working directory: /cd <path>  (tools sandbox here)"),
    ("/pwd", "print the current working directory"),
    ("/mode", "permission: manual | plan | auto  (or Shift+Tab)"),
    ("/plan", "switch to plan mode (read-only)"),
    ("/manual", "switch to manual mode (approve tools)"),
    ("/auto", "switch to auto-approve mode"),
    ("/todos", "show session task list"),
    ("/memory", "show ~/.meta/memory.md excerpt"),
    ("/skills", "list installed skills"),
    ("/graphify", "knowledge graph: status | query | path | explain | extract"),
    ("/plur", "shared engram memory: status | learn | recall | inject"),
    ("/ruflo", "vector memory / swarm: status | search | store"),
    ("/ecosystem", "graphify · plur · ruflo readiness"),
    ("/usage", "token usage + cost for this session  (/cost)"),
    ("/budget", "session spend ceiling: /budget [cost <usd>|tokens <n>|clear|save]"),
    ("/poor", "toggle cost-saver: skip PLUR inject + skills catalog + long memory in prompt"),
    ("/context", "context-window utilization for this session"),
    ("/status", "session snapshot: model · mode · cwd · tokens"),
    ("/doctor", "health check: version · auth · ecosystem · shell"),
    ("/permissions", "show or reload allow/deny/ask rules (permissions.toml)"),
    ("/hooks", "show local tool hook status (hooks.toml)"),
    ("/model", "show or switch model"),
    ("/effort", "reasoning effort: minimal|low|medium|high|xhigh"),
    ("/sessions", "browse & open past sessions  (same as /resume · Ctrl+R)"),
    ("/resume", "browse & open past sessions  (same as /sessions · Ctrl+R)"),
    ("/init", "generate a META.md project guide"),
    ("/login", "enter / replace your Meta API key"),
    ("/logout", "clear the stored API key"),
    ("/config", "show config + data paths"),
    ("/bug", "how to report an issue"),
    ("/exit", "quit"),
];

pub enum Cell {
    Banner,
    User(String),
    Assistant { text: String, streaming: bool },
    /// Model reasoning stream. Collapsed by default when finished — click or
    /// click to expand. `duration` set when the thought ends.
    Thinking {
        text: String,
        active: bool,
        started: Instant,
        duration: Option<Duration>,
        expanded: bool,
    },
    /// Tool / bash / command card. Header always visible with duration;
    /// body expands for full output.
    Tool {
        name: String,
        args: String,
        result: Option<String>,
        ok: Option<bool>,
        started: Instant,
        duration: Option<Duration>,
        expanded: bool,
    },
    /// End-of-turn timing strip — always includes wall time + thought time.
    TurnDone {
        duration: Duration,
        /// Sum of model thinking time during this turn (0 if none).
        thought: Duration,
        interrupted: bool,
    },
    /// System notice. `tone` picks the colour + glyph so a mode switch, a plan,
    /// a todo update and a usage dump don't all read as the same blue blob.
    Info { text: String, tone: Tone },
    Error(String),
}

impl Cell {
    /// Whether this cell can be expanded/collapsed in the transcript.
    pub fn is_collapsible(&self) -> bool {
        matches!(self, Cell::Thinking { .. } | Cell::Tool { .. })
    }

    /// Hover peek / expand target — thoughts, tools/bash, and turn timing strips.
    pub fn is_peekable(&self) -> bool {
        matches!(
            self,
            Cell::Thinking { .. } | Cell::Tool { .. } | Cell::TurnDone { .. }
        )
    }

    pub fn expanded(&self) -> bool {
        match self {
            Cell::Thinking { expanded, .. } | Cell::Tool { expanded, .. } => *expanded,
            _ => false,
        }
    }

    pub fn toggle_expanded(&mut self) {
        match self {
            Cell::Thinking { expanded, .. } | Cell::Tool { expanded, .. } => {
                *expanded = !*expanded;
            }
            _ => {}
        }
    }

    /// Title for the hover dialogue.
    pub fn peek_title(&self) -> Option<String> {
        match self {
            Cell::Thinking {
                active,
                duration,
                started,
                ..
            } => {
                let d = if *active {
                    theme::fmt_elapsed_live(started.elapsed())
                } else {
                    duration
                        .map(theme::fmt_duration)
                        .unwrap_or_else(|| "—".into())
                };
                Some(if *active {
                    format!("thought · {d} (live)")
                } else {
                    format!("thought · took {d}")
                })
            }
            Cell::Tool {
                name,
                ok,
                duration,
                started,
                ..
            } => {
                let d = if ok.is_none() {
                    theme::fmt_elapsed_live(started.elapsed())
                } else {
                    duration
                        .map(theme::fmt_duration)
                        .unwrap_or_else(|| "—".into())
                };
                let status = match ok {
                    None => "running",
                    Some(true) => "ok",
                    Some(false) => "failed",
                };
                Some(format!("{name} · {status} · {d}"))
            }
            Cell::TurnDone {
                duration,
                thought,
                interrupted,
            } => {
                let t = theme::fmt_duration(*duration);
                let th = theme::fmt_duration(*thought);
                Some(if *interrupted {
                    format!("turn cancelled · {t} · thought {th}")
                } else {
                    format!("turn · took {t} · thought {th}")
                })
            }
            _ => None,
        }
    }

    /// Full body for the hover dialogue (and in-place expand).
    pub fn peek_body(&self) -> Option<String> {
        match self {
            Cell::Thinking { text, active, .. } => {
                if text.trim().is_empty() {
                    Some(if *active {
                        "…thinking".into()
                    } else {
                        "(empty thought)".into()
                    })
                } else {
                    Some(text.clone())
                }
            }
            Cell::Tool {
                name,
                args,
                result,
                ok,
                duration,
                started,
                ..
            } => {
                let mut s = String::new();
                s.push_str(&format!("tool: {name}\n"));
                s.push_str(&format!("args: {args}\n"));
                let d = if ok.is_none() {
                    theme::fmt_elapsed_live(started.elapsed())
                } else {
                    duration
                        .map(theme::fmt_duration)
                        .unwrap_or_else(|| "—".into())
                };
                s.push_str(&format!("duration: {d}\n"));
                s.push_str("---\n");
                match result {
                    None => s.push_str("…running"),
                    Some(r) if r.trim().is_empty() => s.push_str("(no output)"),
                    Some(r) => s.push_str(r),
                }
                Some(s)
            }
            Cell::TurnDone {
                duration,
                thought,
                interrupted,
            } => Some(if *interrupted {
                format!(
                    "This turn was cancelled after {} (thought {}).",
                    theme::fmt_duration(*duration),
                    theme::fmt_duration(*thought)
                )
            } else {
                format!(
                    "This turn completed in {}.\nThought time: {}.\n\nClick a thought/tool card to peek full content. Click ▸ to expand in place. Click ↓ End to jump to latest.",
                    theme::fmt_duration(*duration),
                    theme::fmt_duration(*thought)
                )
            }),
            _ => None,
        }
    }
}

#[derive(PartialEq)]
enum TurnMode {
    Chat,
    Compact,
}

#[derive(Clone)]
pub struct CtxMenuHit {
    pub frame: ratatui::layout::Rect,
    pub actions: Vec<(usize, ratatui::layout::Rect)>,
}

pub struct CtxMenu {
    pub cell_idx: usize,
    pub selected: usize,
    pub hit: CtxMenuHit,
    /// Where the menu was anchored when it opened (col,row). Fixed for the
    /// menu's lifetime so wheeling through options doesn't drift the box.
    pub anchor: (u16, u16),
    /// Coalesce trackpad/OS wheel floods so one notch moves **one** row
    /// (Fork → Revert → Copy) instead of jumping over Revert.
    pub last_step_at: Instant,
}

/// Prompt context-menu actions, in display order. Single source of truth for
/// both the renderer and the confirm handler so the row you pick always runs
/// the action it shows.
pub const CTX_ACTIONS: &[(&str, &str)] = &[
    ("⑂", "Fork"),   // branch: a new session seeded up to this prompt
    ("↺", "Revert"), // rewind this session to just before this prompt
    ("⧉", "Copy"),   // copy the prompt text
];

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

/// Character position in the wrapped transcript (line + display column).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextPos {
    pub line: usize,
    pub col: usize,
}

/// Ordered selection range in the transcript.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextRange {
    pub start: TextPos,
    pub end: TextPos,
}

impl TextRange {
    pub fn normalized(self) -> (TextPos, TextPos) {
        let a = self.start;
        let b = self.end;
        if a.line < b.line || (a.line == b.line && a.col <= b.col) {
            (a, b)
        } else {
            (b, a)
        }
    }

    pub fn is_empty(self) -> bool {
        self.start == self.end
    }
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

/// Outcome of a click in the transcript body.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum TranscriptClick {
    /// Expand/collapse a collapsible card in place.
    ToggleExpand(usize),
    /// Open (pin) the floating peek dialogue for a peekable cell.
    PinPeek(usize),
    /// Dismiss any pinned peek.
    Dismiss,
}

/// Pure resolution of a transcript click — the single source of truth for what
/// clicking a line does. Kept side-effect-free so it can be unit-tested (the
/// physical mouse can't be injected through the test harness).
///
/// - `chevron`: the click landed in the left gutter (~3 cols).
/// - `header`: the collapsible card whose header this line is, if any.
/// - `peekable`: the peekable cell this line belongs to (incl. the finished-turn
///   timing strip), if any.
/// - `pinned`: the currently pinned peek cell.
/// - `target_collapsible`: whether the resolved target can expand in place.
pub fn resolve_transcript_click(
    chevron: bool,
    header: Option<usize>,
    peekable: Option<usize>,
    pinned: Option<usize>,
    target_collapsible: bool,
) -> TranscriptClick {
    // Left-gutter click on a collapsible header → expand/collapse.
    if chevron {
        if let Some(h) = header {
            return TranscriptClick::ToggleExpand(h);
        }
    }
    if let Some(idx) = peekable.or(header) {
        // Second click on the already-pinned card: collapsible cards toggle
        // expand-in-place; everything else (e.g. the turn strip) just closes.
        if pinned == Some(idx) {
            return if target_collapsible {
                TranscriptClick::ToggleExpand(idx)
            } else {
                TranscriptClick::Dismiss
            };
        }
        // First click → open the peek.
        return TranscriptClick::PinPeek(idx);
    }
    // Empty space → dismiss.
    TranscriptClick::Dismiss
}

pub struct ApprovalState {
    pub name: String,
    pub args: String,
    pub respond: Option<oneshot::Sender<ApprovalDecision>>,
}

/// Secure in-TUI API-key entry (`/login`). The key is captured masked and
/// never enters the transcript or the persisted input history.
pub struct LoginModal {
    /// The key characters typed/pasted so far (rendered as dots).
    pub buf: String,
    /// True when replacing an existing key (shows a slightly different hint).
    pub replacing: bool,
    /// Transient error to show under the field (e.g. "key too short").
    pub error: Option<String>,
}

/// One row of the unified sessions picker (`/sessions` · `/resume` · Ctrl+R).
#[derive(Debug, Clone)]
pub struct SessionRow {
    pub id: String,
    /// Relative time label ("2h ago", "just now").
    pub when: String,
    pub messages: usize,
    pub tokens: u64,
    pub cost: f64,
    pub cwd: String,
    /// First user prompt, single-line.
    pub preview: String,
    /// Session belongs to the current workspace.
    pub here: bool,
}

/// Interactive sessions browser — open with `/sessions`, `/resume`, or Ctrl+R.
pub struct SessionPicker {
    pub rows: Vec<SessionRow>,
    /// Selected entry (absolute index into `visible()`).
    pub idx: usize,
    /// First visible entry — only advances/retreats by 1 when selection leaves the window.
    pub scroll: usize,
    /// How many entries fit in the body (set by last draw).
    pub vis_page: usize,
    /// Only show sessions from this workspace.
    pub this_cwd_only: bool,
    /// Hit-test geometry filled by the last draw (screen coords).
    pub hit: PickerHit,
    /// Coalesce trackpad/OS wheel floods to one step per tick.
    pub last_step_at: Instant,
}

/// Click targets for the sessions modal (updated each frame while open).
#[derive(Debug, Clone, Default)]
pub struct PickerHit {
    pub frame: ratatui::layout::Rect,
    /// Top-right close control (✕).
    pub close: ratatui::layout::Rect,
    /// List body (rows).
    #[allow(dead_code)]
    pub body: ratatui::layout::Rect,
    /// Scope chip ("here" / "all") — click to toggle.
    pub scope: ratatui::layout::Rect,
    /// Visible row index → screen rect (for click-to-select).
    pub rows: Vec<(usize, ratatui::layout::Rect)>,
}

impl SessionPicker {
    pub fn visible(&self) -> Vec<&SessionRow> {
        self.rows
            .iter()
            .filter(|r| !self.this_cwd_only || r.here)
            .collect()
    }

    pub fn count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| !self.this_cwd_only || r.here)
            .count()
    }

    /// Clamp idx/scroll after page-size or filter changes (never jumps more than needed).
    pub fn clamp_scroll(&mut self) {
        let count = self.count();
        if count == 0 {
            self.idx = 0;
            self.scroll = 0;
            return;
        }
        self.idx = self.idx.min(count - 1);
        let page = self.vis_page.max(1);
        let max_scroll = count.saturating_sub(page);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
        // Selection above window → pull scroll up (min steps).
        if self.idx < self.scroll {
            self.scroll = self.idx;
        }
        // Selection below window → push scroll down (min steps).
        let last_vis = self.scroll + page - 1;
        if self.idx > last_vis {
            self.scroll = self.idx + 1 - page;
        }
    }

    /// Move selection by exactly one entry (sign of `dir`). Viewport shifts by at most 1.
    pub fn step(&mut self, dir: i32) {
        if dir == 0 {
            return;
        }
        let count = self.count();
        if count == 0 {
            return;
        }
        let page = self.vis_page.max(1);
        if dir < 0 {
            if self.idx == 0 {
                return;
            }
            self.idx -= 1;
            // If we walked above the window, scroll up by exactly one.
            if self.idx < self.scroll {
                self.scroll = self.idx;
            }
        } else {
            if self.idx + 1 >= count {
                return;
            }
            self.idx += 1;
            // If we walked past the bottom of the window, scroll down by exactly one.
            let last_vis = self.scroll + page - 1;
            if self.idx > last_vis {
                self.scroll += 1;
            }
        }
        let max_scroll = count.saturating_sub(page);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    /// Wheel: one step max every 45ms so OS/trackpad floods don't skip items.
    pub fn wheel_step(&mut self, dir: i32) {
        let now = Instant::now();
        if now.duration_since(self.last_step_at) < Duration::from_millis(45) {
            return;
        }
        self.last_step_at = now;
        self.step(dir.signum());
    }

    /// Arrows / j-k: always one entry, no throttle.
    pub fn move_by(&mut self, delta: i32) {
        if delta == 0 {
            return;
        }
        let steps = delta.unsigned_abs() as usize;
        let dir = if delta < 0 { -1 } else { 1 };
        for _ in 0..steps {
            let before = self.idx;
            self.step(dir);
            if self.idx == before {
                break;
            }
        }
    }

    pub fn set_idx(&mut self, i: usize) {
        let count = self.count();
        if count == 0 {
            self.idx = 0;
            self.scroll = 0;
            return;
        }
        self.idx = i.min(count - 1);
        // Bring into view with minimal scroll (may jump if click far — intentional).
        self.clamp_scroll();
    }
}

/// Compact relative timestamp for the sessions picker.
fn relative_when(dt: chrono::DateTime<chrono::Utc>) -> String {
    let secs = chrono::Utc::now()
        .signed_duration_since(dt)
        .num_seconds()
        .max(0);
    if secs < 45 {
        "just now".into()
    } else if secs < 90 {
        "1m ago".into()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 3600 * 36 {
        format!("{}h ago", secs / 3600)
    } else if secs < 3600 * 24 * 14 {
        format!("{}d ago", secs / (3600 * 24))
    } else {
        dt.format("%b %d").to_string()
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
    pub permissions: SharedPermissions,
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
    /// Floating "↓ End" jump chip (hit-tested on click). Empty when hidden.
    pub jump_chip: ratatui::layout::Rect,
    /// Sticky prompt banner rect (top overlay) and the User cell it represents,
    /// so right/double-click on the header opens that prompt's menu too.
    pub sticky_banner: ratatui::layout::Rect,
    pub sticky_cell: Option<usize>,
    /// True while the user is dragging the scrollbar thumb.
    pub scrollbar_drag: bool,
    /// Mouse is over the scrollbar rail (hover affordance — thumb widens).
    pub scrollbar_hover: bool,
    /// Subcell offset between the grab point and the thumb's top edge, so
    /// dragging keeps the exact spot under the pointer (no jump-to-centre).
    pub scrollbar_grab: usize,
    /// Scroll offset (rows) inside the pinned peek dialogue.
    pub peek_scroll: u16,
    /// Total content rows of the current peek body (set at draw; drives clamping).
    pub peek_rows: u16,
    /// Peek cell the scroll offset belongs to — reset when the target changes.
    pub peek_scroll_cell: Option<usize>,
    /// Terminal graphics picker (protocol + font size) for inline image peeks.
    pub img_picker: Option<ratatui_image::picker::Picker>,
    /// Decoded image protocols keyed by path — encoding is expensive, cache it.
    pub img_cache: HashMap<String, ratatui_image::protocol::StatefulProtocol>,
    /// True while drag-selecting transcript text (not scrollbar).
    pub selecting: bool,
    /// Left button is held — some hosts emit `Moved` instead of `Drag` while held.
    pub mouse_left_down: bool,
    /// Down position before we know if this is a click or a drag-select.
    pub select_anchor: Option<TextPos>,
    /// Active text selection in the transcript (plain drag — no Shift needed).
    pub selection: Option<TextRange>,
    /// Plain text of every wrapped transcript line (for copy). Rebuilt each draw.
    pub plain_lines: Vec<String>,
    /// Per-cell wrap cache — avoids re-wrapping the whole transcript every frame.
    pub wrap_cache_width: u16,
    pub wrap_cache_keys: Vec<u64>,
    pub wrap_cache_parts: Vec<Vec<ratatui::text::Line<'static>>>,
    /// Per wrapped transcript line: `Some(cell_idx)` when that line is a
    /// collapsible card header (click to expand/collapse).
    pub hit_headers: Vec<Option<usize>>,
    /// Per wrapped line → owning peekable cell (hover dialogue).
    pub line_cells: Vec<Option<usize>>,
    /// Per wrapped line → owning cell index (ALL cell types, for right-click hit-testing).
    pub line_cell_all: Vec<Option<usize>>,
    /// First visible wrapped-line index in the transcript body (for hit-tests).
    pub transcript_top: u16,
    /// Brief highlight after toggle: (cell_idx, when).
    pub expand_flash: Option<(usize, Instant)>,
    /// Cell under the mouse (all-motion tracking) for free hover peek.
    pub hover_cell: Option<usize>,
    /// Click-pinned peek — stays open until Esc / click outside / ✕.
    pub peek_pinned: Option<usize>,
    /// Bounds of the pinned peek box (for click-outside dismissal). Set each draw.
    pub peek_box: ratatui::layout::Rect,
    /// Clickable ✕ close rect on the peek box.
    pub peek_close: ratatui::layout::Rect,
    /// Right-click / double-click context menu on a User prompt.
    pub ctx_menu: Option<CtxMenu>,
    /// Last left-button press (cell idx, time) — for double-click detection.
    pub last_click: Option<(usize, Instant)>,
    /// Last known mouse position (for anchoring the peek box).
    pub mouse_col: u16,
    pub mouse_row: u16,

    pub input: InputState,
    pub queue: VecDeque<String>,

    pub busy: bool,
    /// True after Esc/Ctrl+C until Done arrives — spinners show "cancelling…".
    pub cancelling: bool,
    turn_kind: TurnMode,
    pub turn_started: Instant,
    /// Accumulated model-thinking time for the current turn (for end-of-output strip).
    thought_accum: Duration,
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
    pub palette_scroll: usize,
    pub palette_last_step: std::time::Instant,
    pub quit_armed: Option<Instant>,

    tx: mpsc::UnboundedSender<AgentEvent>,
    rx: mpsc::UnboundedReceiver<AgentEvent>,
    cancel: Option<CancellationToken>,
    should_quit: bool,
    /// Window title locked to the session's first user prompt.
    title_from_prompt: bool,
    /// Base text for the (animated) window title — the current prompt or "ready".
    window_base: String,
    /// Secure API-key entry modal (`/login`), when open.
    pub login: Option<LoginModal>,
    /// Whether an API key is available. `/logout` flips this false and blocks
    /// turns until `/login` provides a new key.
    authed: bool,
}

struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        disable_mouse();
        let _ = stdout().execute(Show);
        let _ = disable_raw_mode();
        let _ = stdout().execute(DisableBracketedPaste);
        let _ = stdout().execute(LeaveAlternateScreen);
    }
}

/// Capture mouse for drag-select, scrollbar, wheel, click-peek (always on).
///
/// Mode 1002 (button-event tracking) reports motion only while a button is held
/// — exactly what drag-select and scrollbar-drag need — plus 1000 (clicks) and
/// 1006 (SGR coords). We deliberately do NOT enable 1003 (any-motion): it floods
/// a motion event for every cell the pointer crosses, which — combined with the
/// ambient repaint — backs up the event queue and makes drags/clicks lag. The
/// cost is free (no-click) hover-peek; click-to-peek stays the primary path.
fn enable_mouse() {
    let _ = stdout().execute(EnableMouseCapture);
    let mut out = stdout();
    let _ = write!(out, "\x1b[?1000h\x1b[?1002h\x1b[?1006h");
    let _ = out.flush();
}

fn disable_mouse() {
    let mut out = stdout();
    let _ = write!(out, "\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?1006l");
    let _ = out.flush();
    let _ = stdout().execute(DisableMouseCapture);
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
    workspace_note: Option<String>,
) -> Result<()> {
    // Fail clearly if stdin isn't a real console (redirects / dead pipes).
    if !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
        return Err(crate::error::MuseError::Other(
            "meta needs an interactive terminal (stdin is not a TTY).\n\
             Run `meta` from a normal shell window, not a redirected pipe."
                .into(),
        ));
    }
    enable_raw_mode().map_err(|e| {
        crate::error::MuseError::Other(format!(
            "cannot enter raw mode (TUI): {e}\n\
             Try a different terminal, or close other full-screen console apps."
        ))
    })?;
    stdout()
        .execute(EnterAlternateScreen)
        .map_err(|e| crate::error::MuseError::Other(format!("alternate screen: {e}")))?;
    stdout().execute(EnableBracketedPaste)?;
    enable_mouse();
    // Hardware cursor hidden — we paint a Meta blue block caret ourselves.
    stdout().execute(Hide)?;
    let _guard = TermGuard;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|e| crate::error::MuseError::Other(format!("terminal init: {e}")))?;

    // Query the terminal's graphics protocol + font size for inline image
    // peeks (sixel / kitty / iTerm2, halfblocks fallback). 1s timeout inside;
    // any failure degrades to a sane halfblocks picker, never an error.
    let img_picker = Some(
        ratatui_image::picker::Picker::from_query_stdio()
            .unwrap_or_else(|_| ratatui_image::picker::Picker::from_fontsize((9, 18))),
    );

    let (tx, rx) = mpsc::unbounded_channel();
    let u_session = usage.session_usage().clone();
    let session_id = session.id.clone();
    let mode_label = permission_mode.get().label().to_string();

    // Host tab title from first prompt (prefer CLI seed, else resume history).
    let seed_prompt = initial_prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or_else(|| {
            session
                .messages
                .iter()
                .find(|m| m.role == "user")
                .map(|m| m.content.clone())
        });
    let title_from_prompt = seed_prompt.is_some();
    crate::ade::set_terminal_title(&crate::ade::session_window_title(
        seed_prompt.as_deref().unwrap_or("ready"),
    ));

    let permissions = SharedPermissions::load(&cwd);
    let mut app = App {
        client,
        cfg,
        cwd,
        permission_mode,
        approved_tools: Arc::new(Mutex::new(HashSet::new())),
        tool_host: ToolHost::default(),
        permissions,
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
        jump_chip: ratatui::layout::Rect::default(),
        sticky_banner: ratatui::layout::Rect::default(),
        sticky_cell: None,
        scrollbar_drag: false,
        scrollbar_hover: false,
        scrollbar_grab: 0,
        peek_scroll: 0,
        peek_rows: 0,
        peek_scroll_cell: None,
        img_picker,
        img_cache: HashMap::new(),
        selecting: false,
        mouse_left_down: false,
        select_anchor: None,
        selection: None,
        plain_lines: Vec::new(),
        wrap_cache_width: 0,
        wrap_cache_keys: Vec::new(),
        wrap_cache_parts: Vec::new(),
        hit_headers: Vec::new(),
        line_cells: Vec::new(),
        line_cell_all: Vec::new(),
        transcript_top: 0,
        expand_flash: None,
        hover_cell: None,
        peek_pinned: None,
        peek_box: ratatui::layout::Rect::default(),
        peek_close: ratatui::layout::Rect::default(),
        ctx_menu: None,
        last_click: None,
        mouse_col: 0,
        mouse_row: 0,
        input: InputState::new(),
        queue: VecDeque::new(),
        busy: false,
        cancelling: false,
        turn_kind: TurnMode::Chat,
        turn_started: Instant::now(),
        thought_accum: Duration::ZERO,
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
        palette_scroll: 0,
        palette_last_step: std::time::Instant::now(),
        quit_armed: None,
        tx,
        rx,
        cancel: None,
        should_quit: false,
        title_from_prompt,
        window_base: seed_prompt.clone().unwrap_or_else(|| "ready".into()),
        login: None,
        authed: true,
    };

    app.replay_session_tail(8);
    if let Some(note) = workspace_note {
        app.push_note(Tone::Session, note);
    }
    app.push_info(format!(
        "mode · {mode_label}  ·  Shift+Tab cycles  manual → plan → auto  ·  /mode"
    ));
    app.push_note(
        Tone::Mode,
        "drag text to select (auto-copies)  ·  drag right scrollbar to scroll  ·  /help".into(),
    );
    if !ecosystem_summary.is_empty() {
        app.push_note(Tone::Skill, ecosystem_summary);
    }

    // Started without any API key → sign-in required before the first turn.
    if crate::auth::resolve_api_key().is_err() {
        app.authed = false;
        app.push_note(
            Tone::Mode,
            "no API key found — press any key, then /login to sign in (or set META_API_KEY)".into(),
        );
        app.open_login();
    }

    if let Some(p) = initial_prompt {
        if !p.trim().is_empty() {
            app.submit_text(&p);
        }
    }

    // Input-first event loop: always process mouse/keys BEFORE paint so
    // scrollbar-drag and text-select never lag behind the ambient repaint
    // (especially while a turn is streaming at ~30fps).
    const FRAME_BUSY_MS: u64 = 33; // ~30fps under load
    const FRAME_IDLE_MS: u64 = 90; // ~11fps ambient shimmer
    let mut dirty = true;
    let mut last_draw = Instant::now();
    let mut last_title = Instant::now();
    let mut title_animating = false;
    // Re-assert mouse modes occasionally — OSC title spam / hosts can drop them.
    let mut last_mouse_rearm = Instant::now();
    loop {
        // 1) Agent events (streaming text/tools).
        while let Ok(ev) = app.rx.try_recv() {
            app.on_agent_event(ev);
            dirty = true;
        }

        let frame_ms = if app.busy
            || app.picker.is_some()
            || app.approval.is_some()
            || app.login.is_some()
            || app.scrollbar_drag
            || app.selecting
            || app.mouse_left_down
        {
            FRAME_BUSY_MS
        } else {
            FRAME_IDLE_MS
        };

        // 2) Input FIRST — drain the whole queue every tick.
        //    Wait up to one frame for the first event when idle; never draw
        //    before handling a pending Down/Drag/Up (that was the post-submit lag).
        let wait = if dirty {
            Duration::ZERO
        } else {
            Duration::from_millis(frame_ms)
        };
        if event::poll(wait)? {
            loop {
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
                        if let Some(m) = &mut app.login {
                            m.buf.push_str(s.trim());
                            dirty = true;
                        } else if app.approval.is_none() && app.picker.is_none() {
                            app.input.insert_str(&s);
                            dirty = true;
                        }
                    }
                    Event::Resize(_, _) => dirty = true,
                    _ => {}
                }
                if app.should_quit {
                    break;
                }
                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }

        if app.should_quit {
            break;
        }

        // 3) Ambient / animation dirty flags.
        if last_draw.elapsed().as_millis() as u64 >= frame_ms {
            dirty = true;
        }
        if let Some((_, t)) = app.expand_flash {
            if t.elapsed().as_millis() >= theme::SETTLE_MS + 20 {
                app.expand_flash = None;
            } else {
                dirty = true;
            }
        }
        if app.busy {
            if last_title.elapsed().as_millis() >= 110 {
                last_title = Instant::now();
                crate::ade::set_terminal_title(&crate::ade::running_window_title(
                    app.spinner_epoch.elapsed(),
                    &app.window_base,
                ));
            }
            title_animating = true;
        } else if title_animating {
            title_animating = false;
            crate::ade::set_terminal_title(&crate::ade::session_window_title(&app.window_base));
        }
        // Mouse capture can be clobbered by title OSC / host quirks mid-session.
        if last_mouse_rearm.elapsed().as_secs() >= 2 {
            enable_mouse();
            last_mouse_rearm = Instant::now();
        }

        // 4) Paint once after input has been applied.
        if dirty {
            terminal.draw(|f| super::ui::draw(f, &mut app))?;
            last_draw = Instant::now();
            dirty = false;
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

    /// Move palette selection by exactly one entry. Viewport shifts by at most 1.
    fn palette_step(&mut self, dir: i32) {
        if dir == 0 || !self.palette_visible() {
            return;
        }
        let n = self.palette_matches().len();
        if n == 0 {
            return;
        }
        let page = 10usize;
        if dir < 0 {
            if self.palette_idx == 0 {
                return;
            }
            self.palette_idx -= 1;
            if self.palette_idx < self.palette_scroll {
                self.palette_scroll = self.palette_idx;
            }
        } else {
            if self.palette_idx + 1 >= n {
                return;
            }
            self.palette_idx += 1;
            let last_vis = self.palette_scroll + page - 1;
            if self.palette_idx > last_vis {
                self.palette_scroll += 1;
            }
        }
        let max_scroll = n.saturating_sub(page);
        if self.palette_scroll > max_scroll {
            self.palette_scroll = max_scroll;
        }
    }

    /// Wheel: one step max every 45ms so OS/trackpad floods don't skip items.
    fn palette_wheel_step(&mut self, dir: i32) {
        let now = std::time::Instant::now();
        if now.duration_since(self.palette_last_step) < std::time::Duration::from_millis(45) {
            return;
        }
        self.palette_last_step = now;
        self.palette_step(dir.signum());
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
        // Prefer live wrapped-line count so scrollbar math stays correct while
        // a turn is streaming (view_total is only refreshed at draw time).
        let total = (self.plain_lines.len() as u16).max(self.view_total);
        let h = self.view_h.max(1);
        total.saturating_sub(h)
    }

    pub fn scroll_up(&mut self, n: u16) {
        let max = self.max_scroll();
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(n).min(max);
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

    /// Absolute jump used by scrollbar thumb drag (0 = latest, max = oldest).
    pub fn set_scroll_from_bottom(&mut self, v: u16) {
        self.scroll_from_bottom = v.min(self.max_scroll());
    }

    // ── prompt context menu (right-click / double-click a User prompt) ───
    //
    // No keyboard shortcuts: wheel or ↑/↓ move the highlight, Enter or click
    // chooses, Esc or an outside click dismisses. Styled like every other
    // dialogue (shared modal frame).

    fn open_ctx_menu(&mut self, cell_idx: usize) {
        self.ctx_menu = Some(CtxMenu {
            cell_idx,
            selected: 0,
            hit: CtxMenuHit {
                frame: ratatui::layout::Rect::default(),
                actions: Vec::new(),
            },
            // Anchor to the cursor once; the box stays put while you wheel.
            anchor: (self.mouse_col, self.mouse_row),
            last_step_at: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
        });
    }

    fn close_ctx_menu(&mut self) {
        self.ctx_menu = None;
    }

    fn ctx_move(&mut self, delta: isize) {
        if let Some(menu) = &mut self.ctx_menu {
            let n = CTX_ACTIONS.len() as isize;
            if n > 0 {
                let cur = menu.selected as isize;
                menu.selected = (cur + delta).clamp(0, n - 1) as usize;
            }
        }
    }

    /// Wheel / trackpad: one menu row per notch (45ms coalesce), same as the
    /// sessions picker — without this, OS multi-fire events jump Fork→Copy
    /// and land past Revert.
    fn ctx_wheel_step(&mut self, dir: i32) {
        let Some(menu) = &mut self.ctx_menu else { return };
        let now = Instant::now();
        if now.duration_since(menu.last_step_at) < Duration::from_millis(45) {
            return;
        }
        menu.last_step_at = now;
        let n = CTX_ACTIONS.len() as isize;
        if n == 0 {
            return;
        }
        let cur = menu.selected as isize;
        menu.selected = (cur + dir.signum() as isize).clamp(0, n - 1) as usize;
    }

    fn on_ctx_menu_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => self.close_ctx_menu(),
            KeyCode::Up => self.ctx_move(-1),
            KeyCode::Down => self.ctx_move(1),
            KeyCode::Enter => self.ctx_confirm(),
            _ => {} // deliberately no letter shortcuts
        }
    }

    fn on_ctx_menu_mouse(&mut self, m: event::MouseEvent) {
        let Some(menu) = &self.ctx_menu else { return };
        let hit = menu.hit.clone();
        match m.kind {
            MouseEventKind::ScrollUp => self.ctx_wheel_step(-1),
            MouseEventKind::ScrollDown => self.ctx_wheel_step(1),
            // Hovering the menu moves the highlight (feels like a real menu).
            MouseEventKind::Moved => {
                for (i, r) in &hit.actions {
                    if rect_contains(*r, m.column, m.row) {
                        if let Some(menu) = &mut self.ctx_menu {
                            menu.selected = *i;
                        }
                        break;
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                for (i, r) in &hit.actions {
                    if rect_contains(*r, m.column, m.row) {
                        if let Some(menu) = &mut self.ctx_menu {
                            menu.selected = *i;
                        }
                        self.ctx_confirm();
                        return;
                    }
                }
                // Click off the menu → dismiss.
                if !rect_contains(hit.frame, m.column, m.row) {
                    self.close_ctx_menu();
                }
            }
            MouseEventKind::Down(MouseButton::Right) => self.close_ctx_menu(),
            _ => {}
        }
    }

    fn ctx_confirm(&mut self) {
        let sel = self.ctx_menu.as_ref().map(|m| m.selected).unwrap_or(0);
        // Index order must match CTX_ACTIONS: 0 Fork · 1 Revert · 2 Copy.
        match sel {
            0 => self.ctx_fork(),
            1 => self.ctx_revert(),
            2 => self.ctx_copy(),
            _ => {}
        }
        self.close_ctx_menu();
    }

    /// The selected prompt's text plus its position counted **from the end** of
    /// the user prompts (1 = last prompt, 2 = second-to-last, …). Counting from
    /// the end makes revert/fork correct even when the transcript only shows a
    /// resumed tail: the displayed prompts are always the suffix of the session.
    fn ctx_prompt(&self) -> Option<(String, usize)> {
        let idx = self.ctx_menu.as_ref()?.cell_idx;
        let text = match self.cells.get(idx)? {
            Cell::User(t) => t.clone(),
            _ => return None,
        };
        let displayed = self
            .cells
            .iter()
            .filter(|c| matches!(c, Cell::User(_)))
            .count();
        let before = self.cells[..idx]
            .iter()
            .filter(|c| matches!(c, Cell::User(_)))
            .count();
        let from_end = displayed.saturating_sub(before); // 1-based from the end
        Some((text, from_end))
    }

    fn ctx_copy(&mut self) {
        if let Some((text, _)) = self.ctx_prompt() {
            let n = text.chars().count();
            clipboard_set(&text);
            self.push_note(Tone::Neutral, format!("copied prompt · {n} chars"));
        }
    }

    /// Revert: rewind THIS session to just before the selected prompt, then load
    /// the prompt back into the input box (edit and resend at will). Everything
    /// from that prompt onward is dropped from the transcript and session.
    fn ctx_revert(&mut self) {
        if self.busy {
            self.push_error("wait for the current turn to finish, then revert".into());
            return;
        }
        let Some((prompt, from_end)) = self.ctx_prompt() else { return };
        let idx = self.ctx_menu.as_ref().map(|m| m.cell_idx).unwrap_or(0);

        self.cells.truncate(idx);
        if let Some(session) = &mut self.session {
            truncate_session_before_prompt(session, from_end);
            let _ = session.save();
        }
        self.reset_transcript_interaction();
        self.input.set_text(&prompt);
        self.scroll_to_bottom();
        self.push_note(
            Tone::Session,
            "reverted — dropped this prompt and everything after; it's back in the input to edit or resend".into(),
        );
    }

    /// Fork: branch into a NEW session seeded with the conversation up to (but
    /// not including) the selected prompt. The original session is left intact
    /// on disk; the prompt is placed in the input, ready to send down the fork.
    fn ctx_fork(&mut self) {
        if self.busy {
            self.push_error("wait for the current turn to finish, then fork".into());
            return;
        }
        let Some((prompt, from_end)) = self.ctx_prompt() else { return };
        let idx = self.ctx_menu.as_ref().map(|m| m.cell_idx).unwrap_or(0);

        // Persist the original before branching.
        if let Some(s) = &self.session {
            let _ = s.save();
        }
        // Clone current session → new id, truncated to before this prompt.
        let mut forked = match &self.session {
            Some(s) => (**s).clone(),
            None => Session::new(&self.cfg.model, &self.cwd.display().to_string()),
        };
        forked.id = uuid::Uuid::new_v4().to_string();
        forked.cwd = self.cwd.display().to_string();
        truncate_session_before_prompt(&mut forked, from_end);
        let _ = forked.save();

        self.session_id = forked.id.clone();
        self.u_session = forked.usage.clone();
        self.u_last = TokenUsage::default();
        let mut usage = UsageTracker::new(
            forked.id.clone(),
            self.cfg.model.clone(),
            self.cwd.clone(),
        );
        usage.seed_session(forked.usage.clone());
        self.session = Some(Box::new(forked));
        self.usage = Some(Box::new(usage));

        // Transcript shows the shared history up to the fork point.
        self.cells.truncate(idx);
        self.reset_transcript_interaction();
        self.input.set_text(&prompt);
        self.scroll_to_bottom();
        self.push_note(
            Tone::Session,
            format!(
                "forked → {} · branched from this prompt (original kept) — prompt is in the input, send to continue the fork",
                &self.session_id[..8.min(self.session_id.len())]
            ),
        );
    }

    /// Clear transient transcript interaction state after cells change.
    fn reset_transcript_interaction(&mut self) {
        self.peek_pinned = None;
        self.hover_cell = None;
        self.selection = None;
        self.select_anchor = None;
        self.selecting = false;
        self.expand_flash = None;
    }

    // ── keys ───────────────────────────────────────────────────────────
    fn on_key(&mut self, key: event::KeyEvent) {
        // Secure login modal swallows all keys (masked key entry).
        if self.login.is_some() {
            self.on_login_key(key);
            return;
        }
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
        // Context menu swallows keys while open.
        if self.ctx_menu.is_some() {
            self.on_ctx_menu_key(key.code);
            return;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            // Ctrl+C: copy selection (transcript first, then input) → else
            // interrupt / clear / double-tap quit.
            KeyCode::Char('c') if ctrl => {
                if let Some(t) = self.selected_transcript_text() {
                    if !t.is_empty() {
                        clipboard_set(&t);
                        return;
                    }
                }
                if let Some(t) = self.input.selected_text() {
                    if !t.is_empty() {
                        clipboard_set(&t);
                        return;
                    }
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
            // Ctrl+V: paste into the input (replaces any input selection).
            KeyCode::Char('v') if ctrl => {
                if let Some(t) = clipboard_get() {
                    self.input.insert_str(&t);
                }
                return;
            }
            // Ctrl+X: cut input selection (or whole input if none).
            KeyCode::Char('x') if ctrl => {
                if self.input.has_selection() {
                    if let Some(t) = self.input.selected_text() {
                        clipboard_set(&t);
                        self.input.delete_selection();
                    }
                } else if !self.input.is_empty() {
                    clipboard_set(&self.input.text());
                    self.input.clear();
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
                // Peek closes first — same priority as a modal.
                if self.peek_pinned.is_some() {
                    self.peek_pinned = None;
                } else if self.busy {
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
                    self.palette_scroll = 0;
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
                    self.palette_scroll = 0;
                }
            }
            // Arrows scroll the transcript. They only move the caret when you
            // are actually editing a multi-line draft; prompt history lives on
            // Ctrl+P/N (and Alt+↑/↓) so reading back through the chat is the
            // default, not a surprise recall into the input box.
            KeyCode::Up if alt => self.input.history_prev(),
            KeyCode::Down if alt => self.input.history_next(),
            KeyCode::Up => match self.arrow_action(true) {
                ArrowAction::Palette => self.palette_step(-1),
                ArrowAction::Caret => self.input.move_up_line(),
                ArrowAction::Scroll => self.scroll_up(1),
            },
            KeyCode::Down => match self.arrow_action(false) {
                ArrowAction::Palette => self.palette_step(1),
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
            // Ctrl+A: select all input text; if input empty, select whole transcript.
            KeyCode::Char('a') if ctrl => {
                if !self.input.is_empty() {
                    self.input.select_all();
                } else if !self.plain_lines.is_empty() {
                    let last = self.plain_lines.len().saturating_sub(1);
                    let end_col = self.plain_lines[last].chars().count();
                    self.selection = Some(TextRange {
                        start: TextPos { line: 0, col: 0 },
                        end: TextPos {
                            line: last,
                            col: end_col,
                        },
                    });
                    if let Some(t) = self.selected_transcript_text() {
                        if !t.trim().is_empty() {
                            clipboard_set(&t);
                        }
                    }
                }
            }
            KeyCode::Char('e') if ctrl => self.input.move_line_end(),
            KeyCode::Char('u') if ctrl => self.input.delete_to_line_start(),
            KeyCode::Char('w') if ctrl => self.input.delete_word_back(),
            KeyCode::Char('j') if ctrl => self.input.insert_char('\n'),
            // No bare/Alt letter shortcuts for peek/expand — those used to eat the
            // first keystroke of normal typing ("e"xplain, "p"lease). Use click.
            KeyCode::Char(c) if !ctrl && !alt => {
                self.input.insert_char(c);
                self.palette_idx = 0;
                self.palette_scroll = 0;
            }
            _ => {}
        }
    }

    /// Mouse:
    /// - drag on transcript → select text (auto-copy on release)
    /// - drag on right scrollbar → scrub history
    /// - click card → peek; click ▸ → expand; click input → caret
    ///
    /// Works while a turn is streaming. Approval/login modals no longer kill
    /// an in-progress scrollbar drag or wheel scroll.
    fn on_mouse(&mut self, m: event::MouseEvent) {
        if self.picker.is_some() {
            // Don't clear left-down state for the main transcript — picker is modal.
            self.scrollbar_drag = false;
            self.selecting = false;
            self.select_anchor = None;
            self.mouse_left_down = false;
            self.on_picker_mouse(m);
            return;
        }
        // Login is fully modal (masked key entry) — no transcript interaction.
        if self.login.is_some() {
            return;
        }

        self.mouse_col = m.column;
        self.mouse_row = m.row;
        // Hover affordance: the thumb widens when the pointer is on the rail.
        self.scrollbar_hover = self.scrollbar_drag || self.hit_scrollbar(m.column, m.row);

        // Context menu is modal — forward all mouse events.
        if self.ctx_menu.is_some() {
            self.scrollbar_drag = false;
            self.selecting = false;
            self.select_anchor = None;
            self.mouse_left_down = false;
            self.on_ctx_menu_mouse(m);
            return;
        }

        // Approval is a modal *overlay* but must not brick scroll/select forever.
        // Allow wheel + continue an in-progress scrollbar drag; new clicks on
        // the transcript are ignored until the modal is dismissed.
        let approval_open = self.approval.is_some();

        match m.kind {
            MouseEventKind::ScrollUp => {
                if self.palette_visible() {
                    self.palette_wheel_step(-1);
                } else if self.wheel_over_pinned_peek(m.column, m.row) {
                    // Wheel inside a pinned peek scrolls its body, not the page.
                    self.peek_scroll = self.peek_scroll.saturating_sub(3);
                } else {
                    // Always works — including during streaming and under approval.
                    self.scroll_up(3);
                }
                if !approval_open {
                    self.update_hover_from_mouse();
                }
            }
            MouseEventKind::ScrollDown => {
                if self.palette_visible() {
                    self.palette_wheel_step(1);
                } else if self.wheel_over_pinned_peek(m.column, m.row) {
                    self.peek_scroll = self
                        .peek_scroll
                        .saturating_add(3)
                        .min(self.peek_rows.saturating_sub(1));
                } else {
                    self.scroll_down(3);
                }
                if !approval_open {
                    self.update_hover_from_mouse();
                }
            }
            MouseEventKind::Moved => {
                // Some terminals report button-held motion as Moved, not Drag.
                if self.mouse_left_down {
                    self.apply_mouse_drag(m.column, m.row);
                } else if !approval_open {
                    self.update_hover_from_mouse();
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                self.mouse_left_down = true;
                if approval_open {
                    // Only allow grabbing the scrollbar to read context.
                    if self.hit_scrollbar(m.column, m.row) {
                        self.selecting = false;
                        self.select_anchor = None;
                        self.selection = None;
                        self.scrollbar_press(m.row);
                    }
                    return;
                }
                // Pinned peek acts like a popup: the ✕ or a click anywhere
                // OUTSIDE the box closes it (and consumes the click); a click
                // inside keeps it open. This is consistent on every side —
                // including below the box.
                if self.peek_pinned.is_some() {
                    if peek_click_dismisses(self.peek_close, self.peek_box, m.column, m.row) {
                        self.peek_pinned = None;
                    }
                    return;
                }
                // "↓ N · End" chip — one click jumps to latest.
                if self.hit_jump_chip(m.column, m.row) {
                    self.scroll_to_bottom();
                    self.selection = None;
                    self.select_anchor = None;
                    self.selecting = false;
                    self.scrollbar_drag = false;
                    self.last_click = None;
                    return;
                }
                // Double-click a User prompt (in the transcript OR the sticky
                // header) → open its context menu. A single click on a prompt
                // still does nothing, so this never fights normal clicking.
                let over_prompt = self.prompt_cell_at_mouse();
                if let Some(idx) = over_prompt {
                    let dbl = self
                        .last_click
                        .map(|(ci, t)| ci == idx && t.elapsed() < Duration::from_millis(450))
                        .unwrap_or(false);
                    if dbl {
                        self.last_click = None;
                        self.selecting = false;
                        self.select_anchor = None;
                        self.selection = None;
                        self.open_ctx_menu(idx);
                        return;
                    }
                    self.last_click = Some((idx, Instant::now()));
                } else {
                    self.last_click = None;
                }
                if self.hit_scrollbar(m.column, m.row) {
                    self.selecting = false;
                    self.select_anchor = None;
                    self.selection = None;
                    self.scrollbar_press(m.row);
                } else if self.in_transcript(m.column, m.row) {
                    self.scrollbar_drag = false;
                    // Begin potential drag-select; click actions fire on Up if
                    // the pointer barely moved (so drag never fights peek/expand).
                    if let Some(pos) = self.pos_at(m.column, m.row) {
                        self.select_anchor = Some(pos);
                        self.selecting = false;
                        self.selection = Some(TextRange {
                            start: pos,
                            end: pos,
                        });
                    }
                } else {
                    self.scrollbar_drag = false;
                    self.select_anchor = None;
                    self.selecting = false;
                    self.selection = None;
                    self.peek_pinned = None;
                    self.click_input(m.column, m.row);
                }
                self.update_hover_from_mouse();
            }
            MouseEventKind::Down(MouseButton::Right) => {
                if approval_open {
                    return;
                }
                self.scrollbar_drag = false;
                self.selecting = false;
                self.select_anchor = None;
                self.update_hover_from_mouse();
                self.ctx_menu = None;
                if let Some(idx) = self.prompt_cell_at_mouse() {
                    self.open_ctx_menu(idx);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                self.mouse_left_down = true;
                self.apply_mouse_drag(m.column, m.row);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                self.mouse_left_down = false;
                if self.scrollbar_drag {
                    self.scrollbar_drag = false;
                } else if approval_open {
                    // Don't run transcript click handlers under the modal.
                    self.selecting = false;
                    self.select_anchor = None;
                } else if self.selecting {
                    // A drag that actually covered text → finalize + auto-copy.
                    // A drag that covered nothing (tiny jitter during a click) →
                    // treat as a click so peek/expand still fire.
                    let selected = self
                        .selected_transcript_text()
                        .filter(|t| !t.trim().is_empty());
                    match selected {
                        Some(t) => clipboard_set(&t),
                        None => {
                            self.selection = None;
                            self.click_transcript(m.column, m.row);
                        }
                    }
                    self.selecting = false;
                    self.select_anchor = None;
                } else if self.select_anchor.is_some() {
                    // Click without drag → peek / expand / etc.
                    let col = m.column;
                    let row = m.row;
                    self.select_anchor = None;
                    self.selection = None;
                    self.click_transcript(col, row);
                }
                self.selecting = false;
            }
            MouseEventKind::Up(_) => {
                self.mouse_left_down = false;
            }
            _ => {
                if !approval_open {
                    self.update_hover_from_mouse();
                }
            }
        }
    }

    /// Shared path for `Drag` and button-held `Moved` (scrollbar + text select).
    fn apply_mouse_drag(&mut self, col: u16, row: u16) {
        if self.scrollbar_drag {
            self.scrollbar_drag_to(row);
            return;
        }
        if self.approval.is_some() {
            return;
        }
        let Some(anchor) = self.select_anchor else {
            return;
        };
        let Some(pos) = self.pos_at(col, row) else {
            return;
        };
        // Threshold: more than 1 cell → real drag-select.
        let moved = pos.line != anchor.line || pos.col.abs_diff(anchor.col) > 1;
        if moved {
            self.selecting = true;
            self.selection = Some(TextRange {
                start: anchor,
                end: pos,
            });
            self.hover_cell = None;
        }
    }

    fn in_transcript(&self, col: u16, row: u16) -> bool {
        let body = self.transcript_body;
        body.width > 0
            && col >= body.x
            && col < body.right()
            && row >= body.y
            && row < body.bottom()
    }

    /// Map screen coords → absolute wrapped-line TextPos.
    fn pos_at(&self, col: u16, row: u16) -> Option<TextPos> {
        let body = self.transcript_body;
        if !self.in_transcript(col, row) {
            return None;
        }
        let local_y = row.saturating_sub(body.y) as usize;
        let line = self.transcript_top as usize + local_y;
        if line >= self.plain_lines.len() {
            return None;
        }
        let local_x = col.saturating_sub(body.x) as usize;
        let plain = &self.plain_lines[line];
        let col_idx = display_col_to_char_idx(plain, local_x);
        Some(TextPos {
            line,
            col: col_idx,
        })
    }

    /// Selected transcript text (normalized range), if any.
    pub fn selected_transcript_text(&self) -> Option<String> {
        let sel = self.selection?;
        if sel.is_empty() {
            return None;
        }
        let (a, b) = sel.normalized();
        if self.plain_lines.is_empty() {
            return None;
        }
        let mut out = String::new();
        for li in a.line..=b.line.min(self.plain_lines.len().saturating_sub(1)) {
            let line = &self.plain_lines[li];
            let chars: Vec<char> = line.chars().collect();
            let (from, to) = if a.line == b.line {
                (a.col.min(chars.len()), b.col.min(chars.len()))
            } else if li == a.line {
                (a.col.min(chars.len()), chars.len())
            } else if li == b.line {
                (0, b.col.min(chars.len()))
            } else {
                (0, chars.len())
            };
            if from < to {
                out.extend(chars[from..to].iter());
            }
            if li < b.line {
                out.push('\n');
            }
        }
        Some(out)
    }

    /// Resolve `hover_cell` from mouse position over the transcript body.
    fn update_hover_from_mouse(&mut self) {
        self.hover_cell = self.cell_at_mouse();
    }

    fn cell_at_mouse(&self) -> Option<usize> {
        let body = self.transcript_body;
        if body.width == 0
            || body.height == 0
            || self.mouse_col < body.x
            || self.mouse_col >= body.right()
            || self.mouse_row < body.y
            || self.mouse_row >= body.bottom()
        {
            return None;
        }
        let local_y = self.mouse_row.saturating_sub(body.y) as usize;
        let line_idx = self.transcript_top as usize + local_y;
        self.line_cells.get(line_idx).copied().flatten()
    }

    fn cell_at_mouse_any(&self) -> Option<usize> {
        let body = self.transcript_body;
        if body.width == 0
            || body.height == 0
            || self.mouse_col < body.x
            || self.mouse_col >= body.right()
            || self.mouse_row < body.y
            || self.mouse_row >= body.bottom()
        {
            return None;
        }
        let local_y = self.mouse_row.saturating_sub(body.y) as usize;
        let line_idx = self.transcript_top as usize + local_y;
        self.line_cell_all.get(line_idx).copied().flatten()
    }

    /// The User-prompt cell under the mouse — from a transcript line OR the
    /// sticky prompt banner at the top. Single entry point for right-click and
    /// double-click so both open the fork/revert/copy menu, header included.
    fn prompt_cell_at_mouse(&self) -> Option<usize> {
        // Sticky prompt banner overlays the top rows; check it first.
        if rect_contains(self.sticky_banner, self.mouse_col, self.mouse_row) {
            return self.sticky_cell;
        }
        self.cell_at_mouse_any()
            .filter(|&i| matches!(self.cells.get(i), Some(Cell::User(_))))
    }

    /// Active peek target: pinned click wins, else free hover.
    pub fn active_peek_cell(&self) -> Option<usize> {
        self.peek_pinned.or(self.hover_cell)
    }

    /// Decoded terminal-graphics protocol for an image path, lazily built and
    /// cached (encoding is expensive; re-doing it per frame would melt the UI).
    pub fn image_protocol(
        &mut self,
        path: &str,
    ) -> Option<&mut ratatui_image::protocol::StatefulProtocol> {
        if !self.img_cache.contains_key(path) {
            let picker = self.img_picker.as_ref()?;
            let meta = std::fs::metadata(path).ok()?;
            if meta.len() > 20 * 1024 * 1024 {
                return None; // don't decode huge files on the UI thread
            }
            let img = image::ImageReader::open(path).ok()?.decode().ok()?;
            if self.img_cache.len() >= 4 {
                self.img_cache.clear();
            }
            self.img_cache
                .insert(path.to_string(), picker.new_resize_protocol(img));
        }
        self.img_cache.get_mut(path)
    }

    fn hit_scrollbar(&self, col: u16, row: u16) -> bool {
        let t = self.scrollbar_track;
        if t.width == 0 || t.height == 0 {
            return false;
        }
        // Generous hit target: full track height + 2 columns left of the rail.
        let left = t.x.saturating_sub(2);
        col >= left && col < t.right() && row >= t.y && row < t.bottom()
    }

    /// Wheel events over an open pinned peek scroll the peek body.
    fn wheel_over_pinned_peek(&self, col: u16, row: u16) -> bool {
        self.peek_pinned.is_some() && rect_contains(self.peek_box, col, row)
    }

    fn hit_jump_chip(&self, col: u16, row: u16) -> bool {
        let t = self.jump_chip;
        t.width > 0
            && t.height > 0
            && col >= t.x
            && col < t.right()
            && row >= t.y
            && row < t.bottom()
    }

    /// Subcell scrollbar geometry for the current transcript (same fractional
    /// model the renderer uses, so hit-tests match what's on screen).
    fn scrollbar_metrics(&self) -> ScrollMetrics {
        let total = (self.plain_lines.len() as u16).max(self.view_total);
        ScrollMetrics::new(
            total as usize,
            self.view_h as usize,
            self.transcript_top as usize,
            self.scrollbar_track.height,
        )
    }

    /// Press on the rail — GUI-standard feel:
    /// on the thumb → start a drag, remembering where inside the thumb you
    /// grabbed (so it never jumps to centre under the pointer);
    /// on the open track → page toward the click.
    fn scrollbar_press(&mut self, row: u16) {
        let t = self.scrollbar_track;
        if t.height == 0 {
            return;
        }
        let m = self.scrollbar_metrics();
        if m.max_offset() == 0 {
            return;
        }
        let pos = ScrollMetrics::subcell_at_row(row.saturating_sub(t.y));
        match m.hit_test(pos) {
            Hit::Thumb => {
                self.scrollbar_drag = true;
                self.scrollbar_grab = pos.saturating_sub(m.thumb_start());
            }
            Hit::Track => {
                let page = (m.viewport_len().saturating_sub(1).max(1)) as u16;
                if pos < m.thumb_start() {
                    self.scroll_up(page);
                } else {
                    self.scroll_down(page);
                }
            }
        }
    }

    /// Drag the thumb: its top edge follows (pointer − grab offset).
    fn scrollbar_drag_to(&mut self, row: u16) {
        let t = self.scrollbar_track;
        if t.height == 0 {
            return;
        }
        let m = self.scrollbar_metrics();
        if m.max_offset() == 0 {
            return;
        }
        let rel = row.clamp(t.y, t.bottom().saturating_sub(1)).saturating_sub(t.y);
        let pos = ScrollMetrics::subcell_at_row(rel);
        let thumb_start = pos.saturating_sub(self.scrollbar_grab);
        // `offset` counts lines from the top; scroll_from_bottom from the end.
        let offset = m.offset_for_thumb_start(thumb_start) as u16;
        let max = m.max_offset() as u16;
        self.set_scroll_from_bottom(max.saturating_sub(offset));
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

    // ── session picker (`/sessions` · `/resume` · Ctrl+R) ─────────────
    fn open_session_picker(&mut self) {
        if self.busy {
            self.push_error("wait for the current turn to finish".into());
            return;
        }
        // Lightweight summaries (no input_items) from ~/.meta + legacy ~/.muse.
        let sessions = match crate::agent::session::list_session_summaries() {
            Ok(s) => s,
            Err(e) => {
                self.push_error(format!("could not list sessions: {e}"));
                return;
            }
        };
        let here_key = self.cwd.display().to_string().to_lowercase();
        let mut rows: Vec<SessionRow> = sessions
            .into_iter()
            // Current session isn't a resume target.
            .filter(|s| s.id != self.session_id)
            .take(120)
            .map(|s| {
                let preview = if !s.preview.is_empty() {
                    s.preview.chars().take(100).collect()
                } else if s.messages == 0 {
                    "(empty session)".into()
                } else {
                    "(no prompt)".into()
                };
                SessionRow {
                    id: s.id,
                    when: relative_when(s.updated_at),
                    messages: s.messages,
                    tokens: s.total_tokens,
                    cost: s.estimated_cost_usd,
                    cwd: s.cwd.clone(),
                    preview,
                    here: s.cwd.to_lowercase() == here_key,
                }
            })
            .collect();

        // Hide empty sessions (0 messages) when real chats exist.
        let has_real = rows.iter().any(|r| r.messages > 0);
        if has_real {
            rows.retain(|r| r.messages > 0);
        }

        if rows.is_empty() {
            self.push_note(
                Tone::Session,
                "no past sessions yet — keep chatting, then /sessions to jump back\n\
                 (searched ~/.meta/sessions and legacy ~/.muse/sessions)"
                    .into(),
            );
            return;
        }
        // Default to **all** sessions. A "here only" default hid expensive chats
        // opened from another cwd (e.g. C:\ vs a project folder) and looked like
        // data loss. Toggle with Tab / click the scope chip.
        self.picker = Some(SessionPicker {
            rows,
            idx: 0,
            scroll: 0,
            vis_page: 6,
            this_cwd_only: false,
            hit: PickerHit::default(),
            last_step_at: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
        });
    }

    fn close_picker(&mut self) {
        self.picker = None;
    }

    fn picker_confirm(&mut self) {
        let id = self
            .picker
            .as_ref()
            .and_then(|p| p.visible().get(p.idx).map(|r| r.id.clone()));
        self.picker = None;
        if let Some(id) = id {
            self.cmd_resume(&id);
        }
    }

    fn picker_toggle_scope(&mut self) {
        if let Some(p) = &mut self.picker {
            p.this_cwd_only = !p.this_cwd_only;
            p.idx = 0;
            p.scroll = 0;
        }
    }

    fn on_picker_key(&mut self, code: KeyCode) {
        let Some(p) = &mut self.picker else { return };
        let count = p.count();
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_picker(),
            // One entry per key — same path the wheel uses.
            KeyCode::Up | KeyCode::Char('k') => p.step(-1),
            KeyCode::Down | KeyCode::Char('j') => p.step(1),
            KeyCode::PageUp => p.move_by(-(p.vis_page.max(1) as i32)),
            KeyCode::PageDown => p.move_by(p.vis_page.max(1) as i32),
            KeyCode::Home => {
                p.idx = 0;
                p.scroll = 0;
            }
            KeyCode::End => {
                if count > 0 {
                    p.idx = count - 1;
                    p.clamp_scroll();
                }
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Char('a') | KeyCode::Char(' ') => {
                self.picker_toggle_scope();
            }
            KeyCode::Enter => self.picker_confirm(),
            _ => {}
        }
    }

    /// Mouse while the sessions modal is open: wheel, rows, scope chip, close.
    fn on_picker_mouse(&mut self, m: event::MouseEvent) {
        self.mouse_col = m.column;
        self.mouse_row = m.row;
        match m.kind {
            // Same as ↑ / ↓ — one entry. Coalesce OS wheel floods so one notch ≈ one key.
            MouseEventKind::ScrollUp => {
                if let Some(p) = &mut self.picker {
                    p.wheel_step(-1);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(p) = &mut self.picker {
                    p.wheel_step(1);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(p) = &self.picker else { return };
                let hit = p.hit.clone();
                let col = m.column;
                let row = m.row;
                if rect_contains(hit.close, col, row) {
                    self.close_picker();
                    return;
                }
                if rect_contains(hit.scope, col, row) {
                    self.picker_toggle_scope();
                    return;
                }
                for (i, r) in &hit.rows {
                    if rect_contains(*r, col, row) {
                        let same = self.picker.as_ref().map(|p| p.idx == *i).unwrap_or(false);
                        if let Some(p) = &mut self.picker {
                            p.set_idx(*i);
                        }
                        if same {
                            self.picker_confirm();
                        }
                        return;
                    }
                }
                if !rect_contains(hit.frame, col, row) {
                    self.close_picker();
                }
            }
            _ => {}
        }
    }

    // ── secure login ───────────────────────────────────────────────────
    fn open_login(&mut self) {
        let replacing = crate::auth::resolve_api_key().is_ok();
        self.login = Some(LoginModal {
            buf: String::new(),
            replacing,
            error: None,
        });
    }

    fn on_login_key(&mut self, key: event::KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.login = None;
            }
            KeyCode::Enter => self.submit_login(),
            KeyCode::Backspace => {
                if let Some(m) = &mut self.login {
                    m.buf.pop();
                }
            }
            // Paste a key with Ctrl+V (keys are usually pasted, not typed).
            KeyCode::Char('v') if ctrl => {
                if let (Some(t), Some(m)) = (clipboard_get(), self.login.as_mut()) {
                    m.buf.push_str(t.trim());
                }
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(m) = &mut self.login {
                    m.buf.clear();
                }
            }
            KeyCode::Char(c) if !ctrl && !c.is_control() => {
                if let Some(m) = &mut self.login {
                    m.buf.push(c);
                }
            }
            _ => {}
        }
    }

    fn submit_login(&mut self) {
        let key = match &self.login {
            Some(m) => m.buf.trim().to_string(),
            None => return,
        };
        match crate::auth::save_api_key(&key) {
            Ok(()) => {
                // Hot-swap the client so the new key takes effect next turn.
                match crate::api::MetaClient::new(&self.cfg.base_url, &key) {
                    Ok(client) => {
                        self.client = client;
                        self.authed = true;
                        self.login = None;
                        self.push_note(
                            Tone::Mode,
                            format!(
                                "signed in · key {} · saved to {}",
                                crate::auth::key_fingerprint(&key),
                                crate::config::auth_path().display()
                            ),
                        );
                    }
                    Err(e) => {
                        if let Some(m) = &mut self.login {
                            m.error = Some(format!("client error: {e}"));
                        }
                    }
                }
            }
            Err(e) => {
                if let Some(m) = &mut self.login {
                    m.error = Some(e.to_string());
                }
            }
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
        // Claude-Code-style quick memory: a line starting with `#` (but not a
        // `##` markdown heading) is saved to ~/.meta/memory.md without a turn.
        if let Some(rest) = text.strip_prefix('#') {
            if !rest.starts_with('#') {
                let note = rest.trim();
                if note.is_empty() {
                    self.push_error("nothing to remember — type your note after #".into());
                } else if let Err(e) = crate::agent::memory::append_memory(note) {
                    self.push_error(format!("could not save memory: {e}"));
                } else {
                    self.push_note(Tone::Memory, format!("remembered · {note}"));
                }
                return;
            }
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
        if !self.authed {
            self.push_error(
                "signed out — run /login to enter an API key before sending a message".into(),
            );
            return;
        }
        let (Some(session), Some(usage)) = (self.session.take(), self.usage.take()) else {
            self.push_error("internal: session busy".into());
            return;
        };
        self.cells.push(Cell::User(prompt.to_string()));
        // First user prompt of the session owns the window/tab title text; the
        // loop animates its marker orb while the turn runs.
        if !self.title_from_prompt {
            self.window_base = prompt.to_string();
            self.title_from_prompt = true;
        }
        // Sending always snaps you back to the live end of the conversation.
        self.scroll_to_bottom();
        // Clear any stale drag state from the previous idle frame.
        self.scrollbar_drag = false;
        self.selecting = false;
        self.select_anchor = None;
        self.mouse_left_down = false;
        // Re-assert mouse capture — hosts sometimes drop modes after heavy I/O.
        enable_mouse();
        self.busy = true;
        self.cancelling = false;
        self.turn_kind = TurnMode::Chat;
        self.turn_started = Instant::now();
        self.thought_accum = Duration::ZERO;
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
            permissions: self.permissions.clone(),
            hooks: agent::hooks::HooksConfig::load(),
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
                Cell::Thinking {
                    active,
                    text,
                    started,
                    duration,
                    ..
                } => {
                    if *active {
                        let d = started.elapsed();
                        *active = false;
                        *duration = Some(d);
                        self.thought_accum = self.thought_accum.saturating_add(d);
                        if !text.is_empty() {
                            text.push_str("  · cancelled");
                        }
                    }
                }
                Cell::Tool {
                    result,
                    ok,
                    started,
                    duration,
                    ..
                } => {
                    if result.is_none() {
                        *result = Some("cancelled".into());
                        *ok = Some(false);
                        *duration = Some(started.elapsed());
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
                if let Some(Cell::Thinking {
                    text,
                    active: true,
                    ..
                }) = self.cells.last_mut()
                {
                    text.push_str(&d);
                } else {
                    self.cells.push(Cell::Thinking {
                        text: d,
                        active: true,
                        started: Instant::now(),
                        duration: None,
                        // Always start collapsed — user clicks ▸ to open body.
                        expanded: false,
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
                    started: Instant::now(),
                    duration: None,
                    // Always start collapsed — user clicks ▸ to open full output.
                    expanded: false,
                });
                self.tool_cells.insert(id, self.cells.len() - 1);
                self.status = "running tool".into();
            }
            AgentEvent::ToolEnd {
                id, result, ok, ..
            } => {
                if let Some(&idx) = self.tool_cells.get(&id) {
                    if let Some(Cell::Tool {
                        result: r,
                        ok: o,
                        started,
                        duration,
                        expanded,
                        ..
                    }) = self.cells.get_mut(idx)
                    {
                        *r = Some(result);
                        *o = Some(ok);
                        *duration = Some(started.elapsed());
                        // Stay collapsed unless the user already opened it.
                        let _ = expanded;
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
                let turn_dur = self.turn_started.elapsed();
                self.u_session = usage.session_usage().clone();
                self.session_id = session.id.clone();
                self.session = Some(session);
                self.usage = Some(usage);
                self.busy = false;
                self.cancelling = false;
                self.cancel = None;
                self.status = "idle".into();
                // Turn done — restore mouse modes in case title OSC / host dropped them.
                enable_mouse();
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
                        self.push_turn_done(turn_dur, true);
                    }
                    (TurnMode::Compact, Ok(summary), _) => {
                        self.push_info(format!(
                            "context compacted — summary:\n{summary}"
                        ));
                        self.push_turn_done(turn_dur, false);
                    }
                    (TurnMode::Compact, Err(e), _) => {
                        self.push_error(format!("compaction failed: {e}"))
                    }
                    (TurnMode::Chat, Err(e), _) => {
                        // Interrupted surfaces as Err("interrupted") sometimes.
                        let was_interrupt = e.contains("interrupted");
                        if !was_interrupt {
                            self.push_error(e);
                        }
                        self.push_turn_done(turn_dur, was_interrupt);
                    }
                    (TurnMode::Chat, Ok(_), _) => {
                        // Always post turn + thought timers at end of finished output.
                        self.push_turn_done(turn_dur, false);
                    }
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
            if let Cell::Thinking {
                active,
                started,
                duration,
                expanded,
                ..
            } = c
            {
                if *active {
                    let d = started.elapsed();
                    *active = false;
                    *duration = Some(d);
                    self.thought_accum = self.thought_accum.saturating_add(d);
                    // Never auto-expand — duration lives on the header chip.
                    *expanded = false;
                }
                break;
            }
        }
    }

    fn push_turn_done(&mut self, duration: Duration, interrupted: bool) {
        // Ensure any still-open thought is closed and counted.
        self.finish_thinking();
        self.cells.push(Cell::TurnDone {
            duration,
            thought: self.thought_accum,
            interrupted,
        });
        // Snap to latest so the timing strip is always visible at end of output.
        if !interrupted {
            self.scroll_to_bottom();
        }
    }

    /// Toggle expand on a collapsible cell (thinking / tool / bash).
    pub fn toggle_cell_expand(&mut self, cell_idx: usize) {
        if let Some(c) = self.cells.get_mut(cell_idx) {
            if c.is_collapsible() {
                c.toggle_expanded();
                self.expand_flash = Some((cell_idx, Instant::now()));
            }
        }
    }

    /// Map a click in the transcript body.
    ///
    /// - Click left edge / chevron (first ~3 cells): expand/collapse in place.
    /// - Click anywhere else on a peekable card: pin the full-content dialogue
    ///   (works without free mouse-move — many hosts never emit hover events).
    /// - Second click on the same pinned card expands (if collapsible).
    fn click_transcript(&mut self, col: u16, row: u16) {
        let body = self.transcript_body;
        if body.width == 0 || body.height == 0 {
            return;
        }
        if col < body.x || col >= body.right() || row < body.y || row >= body.bottom() {
            return;
        }
        let local_y = row.saturating_sub(body.y) as usize;
        let local_x = col.saturating_sub(body.x);
        let line_idx = self.transcript_top as usize + local_y;

        let header = self.hit_headers.get(line_idx).copied().flatten();
        let peekable = self.line_cells.get(line_idx).copied().flatten();
        let chevron = local_x <= 3;
        let target_collapsible = peekable
            .or(header)
            .and_then(|i| self.cells.get(i))
            .map(|c| c.is_collapsible())
            .unwrap_or(false);

        match resolve_transcript_click(chevron, header, peekable, self.peek_pinned, target_collapsible)
        {
            TranscriptClick::ToggleExpand(i) => {
                self.toggle_cell_expand(i);
                self.peek_pinned = None;
            }
            TranscriptClick::PinPeek(i) => self.peek_pinned = Some(i),
            TranscriptClick::Dismiss => self.peek_pinned = None,
        }
    }

    // ── slash commands ──────────────────────────────────────────────────
    fn run_command(&mut self, raw: &str) {
        let mut parts = raw.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim().to_string();

        match cmd {
            "/exit" | "/quit" => self.should_quit = true,
            "/help" | "/commands" => self.cmd_help(),
            "/clear" => {
                self.cells.retain(|c| matches!(c, Cell::Banner));
                self.scroll_from_bottom = 0;
            }
            "/new" => self.cmd_new(),
            "/compact" => self.cmd_compact(),
            "/cd" => self.cmd_cd(&arg),
            "/pwd" => self.push_info(format!("cwd  {}", self.cwd.display())),
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
                        "no skills found — add ~/.meta/skills/<name>/SKILL.md\n\
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
            "/budget" => self.cmd_budget(&arg),
            "/poor" => self.cmd_poor(),
            "/permissions" => self.cmd_permissions(&arg),
            "/hooks" => self.push_note(
                Tone::Skill,
                agent::hooks::HooksConfig::load().summary(),
            ),
            "/context" => self.cmd_context(),
            "/status" => self.cmd_status(),
            "/doctor" => self.cmd_doctor(),
            "/model" => self.cmd_model(&arg),
            "/effort" => self.cmd_effort(&arg),
            // /sessions and /resume are the same interactive picker.
            "/sessions" | "/resume" => {
                if arg.is_empty() {
                    self.open_session_picker();
                } else {
                    // Still accept /resume <id> for scripts / muscle memory.
                    self.cmd_resume(&arg);
                }
            }
            "/config" => self.cmd_config(),
            "/init" => {
                self.submit_text(
                    "Analyze this codebase (structure, build/test commands, conventions, \
                     architecture) and create a META.md file at the workspace root that future \
                     agent sessions can use as project instructions. Keep it under 120 lines.",
                );
            }
            "/graphify" => self.cmd_graphify(&arg),
            "/plur" => self.cmd_plur(&arg),
            "/ruflo" => self.cmd_ruflo(&arg),
            "/ecosystem" => {
                self.push_note(Tone::Skill, crate::ecosystem::quick_status());
            }
            "/login" => self.open_login(),
            "/logout" => self.cmd_logout(),
            "/bug" => self.push_note(
                Tone::Neutral,
                "report an issue (unofficial community project)\n  \
                 https://github.com/nuroctane/meta-cli/issues\n  \
                 include: meta --version, OS, and steps to reproduce".into(),
            ),
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

    fn cmd_logout(&mut self) {
        match crate::auth::logout() {
            Ok(()) => {
                self.authed = false;
                self.push_note(
                    Tone::Mode,
                    "signed out — cleared the stored API key.\n  \
                     /login to enter a new key. (env keys like META_API_KEY still apply on restart)"
                        .into(),
                );
            }
            Err(e) => self.push_error(format!("logout failed: {e}")),
        }
    }

    fn cmd_help(&mut self) {
        let m = self.permission_mode.get();
        let mut s = String::new();
        s.push_str(&format!(
            "help  ·  mode {} — {}\n  model  {}\n\n",
            m.badge(),
            m.description(),
            self.cfg.model,
        ));

        s.push_str("keyboard\n");
        // Two-column: shortcut (left, fixed) · action
        let keys: &[(&str, &str)] = &[
            ("↑ ↓  ·  wheel", "scroll transcript"),
            ("drag scrollbar", "scrub history"),
            ("drag text", "select + auto-copy"),
            ("click ↓ End", "jump to latest"),
            ("click card  ·  ▸", "peek  ·  expand"),
            ("right/2×-click prompt", "menu: fork · revert · copy"),
            ("Ctrl+A", "select all (input, or transcript if empty)"),
            ("Ctrl+C", "copy selection  ·  else cancel / double-tap quit"),
            ("Ctrl+V  ·  Ctrl+X", "paste  ·  cut"),
            ("Ctrl+P / N", "prompt history  (also Alt+↑/↓)"),
            ("Enter", "send  ·  \\+Enter or Ctrl+J = newline"),
            ("Shift+Tab", "cycle permission mode"),
            ("Ctrl+R", "sessions browser  (/sessions · /resume)"),
            ("Esc", "close peek  →  cancel turn  →  clear input"),
            ("Ctrl+L", "clear transcript view"),
            ("y  ·  a  ·  n", "approve once  ·  always  ·  deny"),
        ];
        for (k, v) in keys {
            s.push_str(&format!("  {k:<22}  {v}\n"));
        }

        s.push_str("\nsessions browser  (Ctrl+R)\n");
        for (k, v) in [
            ("↑ ↓  ·  wheel", "move selection"),
            ("Enter", "open session"),
            ("Tab  ·  Space", "toggle this workspace / all"),
            ("click row", "select  ·  click again to open"),
            ("click ✕  ·  Esc", "close"),
        ] {
            s.push_str(&format!("  {k:<22}  {v}\n"));
        }

        s.push_str("\ncommands\n");
        for (name, desc) in COMMANDS {
            s.push_str(&format!("  {name:<12}  {desc}\n"));
        }
        s.push_str("\n  #<note>       quick-save to memory (no turn)\n");
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
        self.title_from_prompt = false;
        crate::ade::set_terminal_title(&crate::ade::session_window_title("ready"));
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
        self.thought_accum = Duration::ZERO;
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

    fn cmd_usage(&mut self) {
        let u = &self.u_session;
        let cost_cap = self
            .cfg
            .max_session_cost_usd
            .map(|c| format!("${c:.4}"))
            .unwrap_or_else(|| "∞".into());
        let tok_cap = self
            .cfg
            .max_session_tokens
            .map(|t| fmt_num(t))
            .unwrap_or_else(|| "∞".into());
        self.push_note(
            Tone::Usage,
            format!(
            "session usage\n  input    {} tok ({} cached)\n  output   {} tok ({} reasoning)\n  \
             total    {} tok  (cap {tok_cap})\n  est cost ${:.4}  (cap {cost_cap})\n  status   {}\n  \
             set a ceiling with /budget",
            fmt_num(u.input_tokens),
            fmt_num(u.cached_tokens),
            fmt_num(u.output_tokens),
            fmt_num(u.reasoning_tokens),
            fmt_num(u.total_tokens),
            u.estimated_cost_usd(),
            crate::config::status_path().display(),
        ));
    }

    fn cmd_poor(&mut self) {
        self.cfg.poor_mode = !self.cfg.poor_mode;
        if self.cfg.poor_mode {
            self.push_note(
                Tone::Usage,
                "poor mode ON · skipping PLUR auto-inject, skills catalog, and long memory in the system prompt \
                 (tools still available). /poor again to restore. /budget save does not persist this — \
                 set poor_mode=true in config.toml if you want it permanent."
                    .into(),
            );
        } else {
            self.push_note(Tone::Usage, "poor mode OFF · full prompt context restored".into());
        }
    }

    fn cmd_permissions(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg == "reload" {
            self.permissions.reload(&self.cwd);
            self.push_note(
                Tone::Skill,
                format!(
                    "permissions reloaded\n{}",
                    self.permissions.summary()
                ),
            );
            return;
        }
        self.push_note(
            Tone::Skill,
            format!(
                "{}\n  path   {}\n  /permissions reload  re-read files",
                self.permissions.summary(),
                crate::agent::permissions::home_permissions_path().display(),
            ),
        );
    }

    /// Session spend ceiling: show, set cost/tokens, clear, or save to config.toml.
    fn cmd_budget(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() || arg == "show" {
            let cost = self
                .cfg
                .max_session_cost_usd
                .map(|c| format!("${c:.4}"))
                .unwrap_or_else(|| "unlimited".into());
            let toks = self
                .cfg
                .max_session_tokens
                .map(|t| fmt_num(t))
                .unwrap_or_else(|| "unlimited".into());
            let used_c = self.u_session.estimated_cost_usd();
            let used_t = self.u_session.total_tokens;
            self.push_note(
                Tone::Usage,
                format!(
                    "budget\n  cost    {cost}  · spent ${used_c:.4}\n  tokens  {toks}  · used {}\n  \
                     /budget cost <usd>   e.g. /budget cost 2.5\n  \
                     /budget tokens <n>   e.g. /budget tokens 500000\n  \
                     /budget clear        remove ceilings this process\n  \
                     /budget save         write current ceilings to config.toml",
                    fmt_num(used_t),
                ),
            );
            return;
        }
        let mut parts = arg.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        match cmd {
            "clear" => {
                self.cfg.max_session_cost_usd = None;
                self.cfg.max_session_tokens = None;
                self.push_note(Tone::Usage, "budget cleared (unlimited for this process)".into());
            }
            "save" => match crate::config::save_config(&self.cfg) {
                Ok(()) => self.push_note(
                    Tone::Usage,
                    format!(
                        "budget saved to {}",
                        crate::config::config_path().display()
                    ),
                ),
                Err(e) => self.push_error(format!("could not save config: {e}")),
            },
            "cost" => {
                let Some(v) = parts.next() else {
                    self.push_error("usage: /budget cost <usd>".into());
                    return;
                };
                match v.parse::<f64>() {
                    Ok(n) if n.is_finite() && n >= 0.0 => {
                        self.cfg.max_session_cost_usd = Some(n);
                        self.push_note(
                            Tone::Usage,
                            format!("budget cost set to ${n:.4} (this process · /budget save to persist)"),
                        );
                    }
                    _ => self.push_error("cost must be a non-negative number".into()),
                }
            }
            "tokens" => {
                let Some(v) = parts.next() else {
                    self.push_error("usage: /budget tokens <n>".into());
                    return;
                };
                match v.parse::<u64>() {
                    Ok(n) if n > 0 => {
                        self.cfg.max_session_tokens = Some(n);
                        self.push_note(
                            Tone::Usage,
                            format!(
                                "budget tokens set to {} (this process · /budget save to persist)",
                                fmt_num(n)
                            ),
                        );
                    }
                    _ => self.push_error("tokens must be a positive integer".into()),
                }
            }
            _ => self.push_error(
                "usage: /budget [cost <usd>|tokens <n>|clear|save]".into(),
            ),
        }
    }

    /// Change the workspace the agent's tools are sandboxed to. `~` expands to
    /// home; relative paths resolve against the current cwd. Filesystem roots
    /// are refused (the tool layer already blocks them). The session + usage
    /// tracker are re-homed so status.json and future tool calls follow.
    fn cmd_cd(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() {
            self.push_info(format!(
                "cwd  {}\n  usage: /cd <path>  (~ ok · relative to here · absolute ok)",
                self.cwd.display()
            ));
            return;
        }
        // Expand a leading ~ / ~/… to the home directory.
        let expanded: PathBuf = if arg == "~" {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from(arg))
        } else if let Some(rest) = arg.strip_prefix("~/").or_else(|| arg.strip_prefix("~\\")) {
            dirs::home_dir()
                .map(|h| h.join(rest))
                .unwrap_or_else(|| PathBuf::from(arg))
        } else {
            let p = PathBuf::from(arg);
            if p.is_absolute() {
                p
            } else {
                self.cwd.join(p)
            }
        };
        // Canonicalize so `..`, symlinks, and case resolve to a real path.
        let target = match expanded.canonicalize() {
            Ok(p) => p,
            Err(e) => {
                self.push_error(format!("cd: {}: {e}", expanded.display()));
                return;
            }
        };
        if !target.is_dir() {
            self.push_error(format!("cd: not a directory: {}", target.display()));
            return;
        }
        if crate::tools::is_dangerous_workspace(&target) {
            self.push_error(
                "cd: refusing a filesystem root — tools need a real project directory".into(),
            );
            return;
        }
        // Strip Windows \\?\ verbatim prefix for a clean display path.
        let clean = target.to_string_lossy().replace(r"\\?\", "");
        let target = PathBuf::from(&clean);
        let from = self.cwd.display().to_string();
        self.cwd = target.clone();
        if let Some(s) = &mut self.session {
            s.cwd = clean.clone();
        }
        if let Some(u) = &mut self.usage {
            u.set_cwd(target);
        }
        self.push_note(
            Tone::Session,
            format!("cd\n  from  {from}\n  to    {clean}  · tools sandboxed here"),
        );
    }

    /// Context-window utilization — how full the model's context is this turn.
    fn cmd_context(&mut self) {
        let used = self.u_last.input_tokens + self.u_last.output_tokens;
        let window = self.cfg.context_window;
        let pct = if window > 0 {
            (used as f64 / window as f64 * 100.0).min(100.0)
        } else {
            0.0
        };
        // Simple 20-cell bar so the fill is legible at a glance.
        let cells = 20usize;
        let filled = ((pct / 100.0) * cells as f64).round() as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(cells.saturating_sub(filled));
        self.push_note(
            Tone::Usage,
            format!(
                "context window\n  {bar}  {pct:.0}%\n  used     {} tok (last turn: {} in · {} out)\n  \
                 window   {} tok\n  cached   {} tok\n  /compact frees context when this climbs",
                fmt_num(used),
                fmt_num(self.u_last.input_tokens),
                fmt_num(self.u_last.output_tokens),
                fmt_num(window),
                fmt_num(self.u_last.cached_tokens),
            ),
        );
    }

    /// One-glance session snapshot (model · effort · mode · session · cwd · tokens).
    fn cmd_status(&mut self) {
        let mode = self.permission_mode.get();
        let ctx_used = self.u_last.input_tokens + self.u_last.output_tokens;
        let ctx_pct = if self.cfg.context_window > 0 {
            (ctx_used as f64 / self.cfg.context_window as f64 * 100.0).min(100.0)
        } else {
            0.0
        };
        let auth = if self.authed { "signed in" } else { "no key — /login" };
        self.push_note(
            Tone::Session,
            format!(
                "status\n  version  meta v{}\n  model    {}  · effort {}\n  mode     {}  ({})\n  \
                 session  {}\n  cwd      {}\n  auth     {}\n  tokens   {} session · ctx {ctx_pct:.0}%  · ${:.4}",
                env!("CARGO_PKG_VERSION"),
                self.cfg.model,
                self.cfg.reasoning_effort,
                mode.label(),
                mode.description(),
                &self.session_id[..8.min(self.session_id.len())],
                self.cwd.display(),
                auth,
                fmt_num(self.u_session.total_tokens),
                self.u_session.estimated_cost_usd(),
            ),
        );
    }

    /// Inline health check — the interactive cousin of `meta doctor`.
    fn cmd_doctor(&mut self) {
        let sh = crate::tools::shell_backend();
        let auth = if self.authed { "signed in" } else { "no key — /login" };
        let mut lines = format!(
            "doctor · meta v{}\n  model    {}\n  cwd      {}\n  auth     {}\n  shell    {}\n",
            env!("CARGO_PKG_VERSION"),
            self.cfg.model,
            self.cwd.display(),
            auth,
            sh.label,
        );
        lines.push_str("\n");
        lines.push_str(&crate::ecosystem::quick_status());
        self.push_note(Tone::Skill, lines);
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

    fn cmd_resume(&mut self, arg: &str) {
        if self.busy {
            self.push_error("wait for the current turn to finish".into());
            return;
        }
        if arg.is_empty() {
            self.open_session_picker();
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
                // Window title = first user prompt of the resumed session.
                if let Some(first) = loaded.messages.iter().find(|m| m.role == "user") {
                    crate::ade::set_terminal_title(&crate::ade::session_window_title(
                        &first.content,
                    ));
                    self.title_from_prompt = true;
                }
                self.session = Some(Box::new(loaded));
                self.usage = Some(Box::new(tracker));
                self.cells.retain(|c| matches!(c, Cell::Banner));
                let short = &self.session_id[..8.min(self.session_id.len())];
                match from_elsewhere {
                    Some(old) => self.push_note(
                        Tone::Session,
                        format!(
                            "opened {short}\n  was  {old}\n  now  {}  · tools sandboxed here",
                            self.cwd.display()
                        ),
                    ),
                    None => self.push_note(Tone::Session, format!("opened {short}")),
                }
                self.replay_session_tail(20);
            }
            Err(e) => self.push_error(format!("could not open session: {e}")),
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

/// Rewind a session to just before a user prompt, identified by its position
/// **from the end** (1 = last prompt). Drops that prompt's message + API items
/// and everything after, in both the display messages and the Responses
/// `input_items` history, so the model's context is genuinely rewound.
pub fn truncate_session_before_prompt(session: &mut crate::agent::Session, from_end: usize) {
    if from_end == 0 {
        return;
    }
    // Chat messages (user + assistant).
    let user_msgs: Vec<usize> = session
        .messages
        .iter()
        .enumerate()
        .filter(|(_, m)| m.role == "user")
        .map(|(i, _)| i)
        .collect();
    if from_end <= user_msgs.len() {
        let cut = user_msgs[user_msgs.len() - from_end];
        session.messages.truncate(cut);
    }
    // API input_items — each user turn begins with a `role: "user"` item.
    let user_items: Vec<usize> = session
        .input_items
        .iter()
        .enumerate()
        .filter(|(_, it)| it.get("role").and_then(|r| r.as_str()) == Some("user"))
        .map(|(i, _)| i)
        .collect();
    if from_end <= user_items.len() {
        let cut = user_items[user_items.len() - from_end];
        session.input_items.truncate(cut);
    }
    session.updated_at = chrono::Utc::now();
}

/// A pinned peek behaves like a popup: a left-click dismisses it when it lands
/// on the ✕ or **anywhere outside** the box — the same on every side (this is
/// the fix for "clicking below the box didn't close it"). A click inside keeps
/// it open.
pub fn peek_click_dismisses(
    close: ratatui::layout::Rect,
    box_: ratatui::layout::Rect,
    col: u16,
    row: u16,
) -> bool {
    rect_contains(close, col, row) || !rect_contains(box_, col, row)
}

fn rect_contains(r: ratatui::layout::Rect, col: u16, row: u16) -> bool {
    r.width > 0
        && r.height > 0
        && col >= r.x
        && col < r.x.saturating_add(r.width)
        && row >= r.y
        && row < r.y.saturating_add(r.height)
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

/// Map a display column (terminal cells) to a char index in `plain`.
pub fn display_col_to_char_idx(plain: &str, target_col: usize) -> usize {
    let mut used = 0usize;
    for (i, ch) in plain.chars().enumerate() {
        let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
        if used + w > target_col {
            return i;
        }
        used += w;
        if used >= target_col {
            return i + 1;
        }
    }
    plain.chars().count()
}

/// Flatten a ratatui Line to plain text.
pub fn line_to_plain(line: &ratatui::text::Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::input::InputState;

    fn sess_with_turns(prompts: &[&str]) -> crate::agent::Session {
        let mut s = crate::agent::Session::new("m", "/tmp");
        for (i, p) in prompts.iter().enumerate() {
            s.messages.push(crate::agent::session::SessionMessage {
                role: "user".into(),
                content: (*p).into(),
                ts: chrono::Utc::now(),
            });
            s.input_items
                .push(crate::api::types::user_text_item(p));
            // assistant reply for the turn
            s.messages.push(crate::agent::session::SessionMessage {
                role: "assistant".into(),
                content: format!("reply {i}"),
                ts: chrono::Utc::now(),
            });
            s.input_items.push(serde_json::json!({
                "role": "assistant",
                "content": [{"type": "output_text", "text": format!("reply {i}")}]
            }));
        }
        s
    }

    #[test]
    fn revert_rewinds_to_before_the_chosen_prompt() {
        // 3 turns: prompts A,B,C. Revert to B (from_end = 2) keeps only A's turn.
        let mut s = sess_with_turns(&["A", "B", "C"]);
        assert_eq!(s.messages.len(), 6);
        assert_eq!(s.input_items.len(), 6);
        truncate_session_before_prompt(&mut s, 2);
        // Only A's user+assistant remain, in messages AND the API items.
        assert_eq!(s.messages.iter().filter(|m| m.role == "user").count(), 1);
        assert_eq!(s.messages.last().unwrap().content, "reply 0");
        assert_eq!(
            s.input_items
                .iter()
                .filter(|it| it.get("role").and_then(|r| r.as_str()) == Some("user"))
                .count(),
            1
        );
    }

    #[test]
    fn revert_last_prompt_keeps_all_prior() {
        let mut s = sess_with_turns(&["A", "B", "C"]);
        truncate_session_before_prompt(&mut s, 1); // the last prompt (C)
        assert_eq!(s.messages.iter().filter(|m| m.role == "user").count(), 2);
    }

    #[test]
    fn revert_first_prompt_clears_everything() {
        let mut s = sess_with_turns(&["A", "B"]);
        truncate_session_before_prompt(&mut s, 2); // from_end 2 == first of two
        assert!(s.messages.is_empty());
        assert!(s.input_items.is_empty());
    }

    #[test]
    fn clicking_finished_turn_strip_opens_the_peek() {
        // The turn-done strip is peekable but NOT collapsible and has no header.
        // A click anywhere on it (gutter or body) must PIN its peek.
        let strip = Some(7);
        // body click
        assert_eq!(
            resolve_transcript_click(false, None, strip, None, false),
            TranscriptClick::PinPeek(7)
        );
        // gutter click (no header) still pins — never a dead click.
        assert_eq!(
            resolve_transcript_click(true, None, strip, None, false),
            TranscriptClick::PinPeek(7)
        );
        // second click closes it (non-collapsible → dismiss, not toggle).
        assert_eq!(
            resolve_transcript_click(false, None, strip, Some(7), false),
            TranscriptClick::Dismiss
        );
    }

    #[test]
    fn clicking_collapsible_card_pins_then_expands() {
        let card = Some(3);
        assert_eq!(
            resolve_transcript_click(false, card, card, None, true),
            TranscriptClick::PinPeek(3)
        );
        // gutter click on its header → expand in place.
        assert_eq!(
            resolve_transcript_click(true, card, card, None, true),
            TranscriptClick::ToggleExpand(3)
        );
        // second body click on the pinned collapsible card → expand in place.
        assert_eq!(
            resolve_transcript_click(false, card, card, Some(3), true),
            TranscriptClick::ToggleExpand(3)
        );
    }

    #[test]
    fn pinned_peek_dismisses_on_every_side_and_close() {
        use ratatui::layout::Rect;
        // Box at (10,5) 30x12 → spans cols 10..40, rows 5..17. ✕ at top-right.
        let box_ = Rect::new(10, 5, 30, 12);
        let close = Rect::new(box_.x + box_.width - 4, box_.y, 3, 1); // (36,5) 3x1
        // Inside → stays open.
        assert!(!peek_click_dismisses(close, box_, 20, 10));
        // The ✕ → closes.
        assert!(peek_click_dismisses(close, box_, 37, 5));
        // Every outside direction closes — including BELOW (the reported bug).
        assert!(peek_click_dismisses(close, box_, 20, 20), "below must close");
        assert!(peek_click_dismisses(close, box_, 20, 2), "above must close");
        assert!(peek_click_dismisses(close, box_, 2, 10), "left must close");
        assert!(peek_click_dismisses(close, box_, 50, 10), "right must close");
    }

    #[test]
    fn clicking_empty_space_dismisses() {
        assert_eq!(
            resolve_transcript_click(false, None, None, Some(2), false),
            TranscriptClick::Dismiss
        );
    }

    #[test]
    fn ctx_action_indices_match_labels() {
        // The confirm handler switches on these indices; keep them pinned.
        assert_eq!(CTX_ACTIONS[0].1, "Fork");
        assert_eq!(CTX_ACTIONS[1].1, "Revert");
        assert_eq!(CTX_ACTIONS[2].1, "Copy");
        // Three rows: wheel must be able to land on each (0/1/2), not skip 1.
        assert_eq!(CTX_ACTIONS.len(), 3);
    }

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

    fn test_picker(n: usize, page: usize) -> SessionPicker {
        let rows = (0..n)
            .map(|i| SessionRow {
                id: format!("id-{i}"),
                when: "now".into(),
                messages: 1,
                tokens: 0,
                cost: 0.0,
                cwd: "/x".into(),
                preview: format!("prompt {i}"),
                here: true,
            })
            .collect();
        SessionPicker {
            rows,
            idx: 0,
            scroll: 0,
            vis_page: page,
            this_cwd_only: true,
            hit: PickerHit::default(),
            last_step_at: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
        }
    }

    #[test]
    fn picker_step_moves_one_entry_at_a_time() {
        let mut p = test_picker(10, 4);
        assert_eq!((p.idx, p.scroll), (0, 0));
        p.step(1);
        assert_eq!((p.idx, p.scroll), (1, 0));
        p.step(1);
        p.step(1);
        p.step(1);
        // idx 4 → past last visible (0..3), scroll advances by exactly 1
        assert_eq!((p.idx, p.scroll), (4, 1));
        p.step(1);
        assert_eq!((p.idx, p.scroll), (5, 2));
        p.step(-1);
        assert_eq!((p.idx, p.scroll), (4, 2)); // still in view, scroll holds
        p.step(-1);
        p.step(-1);
        p.step(-1);
        // idx 1, still in [2,5]? 1 < 2 → scroll becomes 1
        assert_eq!(p.idx, 1);
        assert_eq!(p.scroll, 1);
        p.step(-1);
        assert_eq!((p.idx, p.scroll), (0, 0));
    }

    #[test]
    fn picker_wheel_matches_arrow_step() {
        let mut a = test_picker(8, 3);
        let mut b = test_picker(8, 3);
        for _ in 0..5 {
            a.step(1);
            // Simulate time between notches so throttle doesn't drop events.
            b.last_step_at = Instant::now()
                .checked_sub(Duration::from_millis(100))
                .unwrap_or_else(Instant::now);
            b.wheel_step(1);
            assert_eq!((a.idx, a.scroll), (b.idx, b.scroll));
        }
        for _ in 0..3 {
            a.step(-1);
            b.last_step_at = Instant::now()
                .checked_sub(Duration::from_millis(100))
                .unwrap_or_else(Instant::now);
            b.wheel_step(-1);
            assert_eq!((a.idx, a.scroll), (b.idx, b.scroll));
        }
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
