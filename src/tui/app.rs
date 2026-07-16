//! Interactive TUI: streaming transcript, slash-command palette, tool
//! approval modals, and a persistent usage statusline (bottom-left).

use crate::agent::{
    self, AgentEvent, AgentRunner, ApprovalDecision, PermissionMode, Session, SharedMode,
    SharedPermissions, SharedTodos,
};
use crate::theme::{self, Tone};
use crate::tools::ToolHost;
use crate::api::ApiClient;
use crate::config::Config;
use crate::error::Result;
use crate::tui::input::InputState;
use crate::tui::scrollbar::{Hit, ScrollMetrics};
use crate::usage::{TokenUsage, UsageTracker};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButton, MouseEventKind,
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

// Slash-command handlers (`run_command` + `cmd_*`) live in a child module so
// this file stays focused on state, rendering hooks, and the event loop.
mod commands;

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
    ("/memory", "show ~/.nur/memory.md excerpt"),
    ("/skills", "list installed skills"),
    ("/graphify", "knowledge graph: status | query | path | explain | extract"),
    ("/plur", "shared engram memory: status | learn | recall | inject"),
    ("/ruflo", "vector memory / swarm: status | search | store"),
    ("/ecosystem", "ecosystem readiness (graphify · plur · ruflo · excalidraw · …)"),
    ("/usage", "token usage + cost for this session  (/cost)"),
    ("/budget", "session spend ceiling: /budget [cost <usd>|tokens <n>|clear|save]"),
    ("/poor", "toggle cost-saver: skip PLUR inject + skills catalog + long memory in prompt"),
    ("/undo", "revert the last file edit (write/edit/multi_edit) made this session"),
    ("/receipt", "session receipt: verify what actually ran (models, tools, privacy tiers)"),
    ("/cua", "computer-use desktop driver — /cua on (always-on background control) · off (on-demand) · status"),
    ("/failover", "set up cross-provider failover in the provider picker (space toggles)"),
    ("/fusion", "multi-model debate → one answer: /fusion <question> · panel <ids> · off"),
    ("/local", "run a model locally (llama.cpp): /local up [tier|url] · status · models · down"),
    ("/bench", "benchmark models on your tasks: /bench add|list|run <name> [models]|remove"),
    ("/context", "context-window utilization for this session"),
    ("/status", "session snapshot: model · mode · cwd · tokens"),
    ("/doctor", "health check: version · auth · ecosystem · shell"),
    ("/permissions", "show or reload allow/deny/ask rules (permissions.toml)"),
    ("/hooks", "show local tool hook status (hooks.toml)"),
    ("/model", "show and switch models  (/models)"),
    ("/models", "show and switch models  (alias of /model)"),
    ("/plugins", "browse · install · enable marketplace plugins"),
    ("/effort", "reasoning effort: minimal|low|medium|high|xhigh"),
    ("/sessions", "browse & open past sessions  (same as /resume)"),
    ("/resume", "browse & open past sessions  (same as /sessions)"),
    ("/init", "generate a NUR.md project guide"),
    ("/goal", "set a standing session goal (context on every turn)"),
    ("/btw", "add a one-off note to your next message"),
    ("/codesearch", "fast ripgrep over the workspace  (/cs)"),
    ("/mc", "manage MCP servers via the executor gateway  (/mcp)"),
    ("/login", "provider · API key or browser sign-in"),
    ("/logout", "clear the stored API key"),
    ("/config", "show config + data paths"),
    ("/feedback", "file a GitHub issue from here"),
    ("/tips", "mouse + keyboard interaction tips"),
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
    /// A follow-up the user typed while a turn was running. Shown in the
    /// transcript with clickable **send now** / **dismiss** so it can interject
    /// into context without retyping.
    Queued { text: String },
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

    #[allow(dead_code)]
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
                // write/edit tools: full path + content/diff in the peek (not the
                // short approval preview), so click-to-peek is useful for review.
                if matches!(
                    name.as_str(),
                    "write_file" | "edit_file" | "multi_edit" | "apply_patch"
                ) {
                    return Some(super::ui::tool_file_peek_body(name, args, result.as_deref()));
                }
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
    /// (Fork → Edit → Revert → Copy) instead of jumping over Revert.
    pub last_step_at: Instant,
}

/// Prompt context-menu actions, in display order. Single source of truth for
/// both the renderer and the confirm handler so the row you pick always runs
/// the action it shows.
pub const CTX_ACTIONS: &[(&str, &str)] = &[
    ("⑂", "Fork"),   // branch: a new session seeded up to this prompt
    ("✎", "Edit"),   // load prompt into input (no rewind) — send interjects as a new turn
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
    /// Expand/collapse via the left chevron only.
    ToggleExpand(usize),
    /// Open stable peek — **only** when the pointer is on the literal
    /// `"click to peek"` substring (see `hit_click_to_peek`).
    OpenPeek(usize),
    /// No-op for this click (stable peek is closed only via Esc / outside / ✕).
    None,
}

/// Pure click resolution.
///
/// - `chevron` + `header_cell`: expand/collapse.
/// - `click_to_peek_cell`: mouse is inside the exact "click to peek" text span.
/// - Everything else: no open, no dismiss (dismiss is handled by the peek box).
pub fn resolve_transcript_click(
    chevron: bool,
    header_cell: Option<usize>,
    click_to_peek_cell: Option<usize>,
) -> TranscriptClick {
    if chevron {
        if let Some(h) = header_cell {
            return TranscriptClick::ToggleExpand(h);
        }
    }
    if let Some(c) = click_to_peek_cell {
        return TranscriptClick::OpenPeek(c);
    }
    TranscriptClick::None
}

pub struct ApprovalState {
    pub name: String,
    pub args: String,
    pub respond: Option<oneshot::Sender<ApprovalDecision>>,
}

/// Secure in-TUI sign-in (`/login`). API keys are masked; browser flows never
/// echo tokens. Stages: provider → (optional) method → key | browser wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginStage {
    Provider,
    /// Browser vs API key (only for `provider.browser_auth`).
    Method,
    Key,
    /// Device-code / SSO wait (URL + short code like `hf auth login`).
    Browser,
}

pub struct LoginModal {
    pub stage: LoginStage,
    /// Provider-search filter typed in the picker stage.
    pub filter: String,
    /// Selected row index into the filtered provider list (same role as
    /// `SessionPicker::idx` — one-step ↑↓/wheel).
    pub sel: usize,
    /// First visible row — only moves by 1 when selection leaves the window.
    pub scroll: usize,
    /// How many provider rows fit in the body (set by last draw).
    pub vis_page: usize,
    /// Hit-test geometry filled by the last draw (screen coords).
    pub hit: PickerHit,
    /// Coalesce trackpad/OS wheel floods to one step per tick (same as sessions).
    pub last_step_at: Instant,
    /// Provider id chosen once we leave the provider stage.
    pub provider_id: String,
    /// Method stage selection: 0 = browser, 1 = API key, 2 = import existing (if any).
    pub method_sel: usize,
    /// Whether an existing first-party CLI session can be imported.
    pub can_import: bool,
    /// The key characters typed/pasted so far (rendered as dots).
    pub buf: String,
    /// Transient error to show under the field (e.g. "key too short").
    pub error: Option<String>,
    /// Browser-stage status line.
    pub browser_status: String,
    /// Verification / authorize URL.
    pub browser_url: String,
    /// Short user code for device-code flows (HF / xAI style).
    pub browser_user_code: String,
    /// Progress from the background OAuth thread.
    pub oauth_rx: Option<std::sync::mpsc::Receiver<crate::oauth::BrowserLoginProgress>>,
    pub oauth_cancel: Option<crate::oauth::CancelFlag>,
    /// Failover-manage mode (opened via `/failover`): the picker toggles
    /// providers into `fallback_providers` instead of choosing a new primary,
    /// and never logs the active provider out.
    pub manage_failover: bool,
    /// The Key stage is capturing a per-provider failover key (saved to the
    /// provider-key store), not an active-provider login.
    pub fallback_key: bool,
}

impl LoginModal {
    /// Providers matching the current filter (name / id / note, case-insensitive).
    pub fn filtered(&self) -> Vec<&'static crate::providers::Provider> {
        let f = self.filter.trim().to_lowercase();
        crate::providers::PROVIDERS
            .iter()
            .filter(|p| {
                f.is_empty()
                    || p.name.to_lowercase().contains(&f)
                    || p.id.to_lowercase().contains(&f)
                    || p.note.to_lowercase().contains(&f)
            })
            .collect()
    }

    pub fn count(&self) -> usize {
        self.filtered().len()
    }

    /// Same clamp rules as `SessionPicker` — selection stays in view with min scroll.
    pub fn clamp_scroll(&mut self) {
        let count = self.count();
        if count == 0 {
            self.sel = 0;
            self.scroll = 0;
            return;
        }
        self.sel = self.sel.min(count - 1);
        let page = self.vis_page.max(1);
        let max_scroll = count.saturating_sub(page);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
        if self.sel < self.scroll {
            self.scroll = self.sel;
        }
        let last_vis = self.scroll + page - 1;
        if self.sel > last_vis {
            self.scroll = self.sel + 1 - page;
        }
    }

    /// Move selection by exactly one entry. Viewport shifts by at most 1.
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
            if self.sel == 0 {
                return;
            }
            self.sel -= 1;
            if self.sel < self.scroll {
                self.scroll = self.sel;
            }
        } else {
            if self.sel + 1 >= count {
                return;
            }
            self.sel += 1;
            let last_vis = self.scroll + page - 1;
            if self.sel > last_vis {
                self.scroll += 1;
            }
        }
        let max_scroll = count.saturating_sub(page);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    /// Wheel: one step max every 45ms (identical to sessions picker).
    pub fn wheel_step(&mut self, dir: i32) {
        let now = Instant::now();
        if now.duration_since(self.last_step_at) < Duration::from_millis(45) {
            return;
        }
        self.last_step_at = now;
        self.step(dir.signum());
    }

    pub fn set_idx(&mut self, i: usize) {
        let count = self.count();
        if count == 0 {
            self.sel = 0;
            self.scroll = 0;
            return;
        }
        self.sel = i.min(count - 1);
        self.clamp_scroll();
    }
}

/// Model chooser opened by `/model` (no argument). Fetches the active
/// provider's model list live and lets you filter + pick without knowing ids
/// by hand. Scroll/select contract mirrors [`LoginModal`]'s provider picker.
pub struct ModelPicker {
    /// Provider name for the modal title (e.g. "OpenAI").
    pub provider_name: String,
    /// Model ids fetched from the provider (empty until the fetch lands).
    pub models: Vec<String>,
    /// The currently active model id — marked in the list.
    pub current: String,
    /// Search filter; also usable verbatim as a custom id to switch to.
    pub filter: String,
    /// Selected row into the filtered list.
    pub sel: usize,
    /// First visible row — moves by 1 when selection leaves the window.
    pub scroll: usize,
    /// Rows that fit in the body (set by last draw).
    pub vis_page: usize,
    /// Hit-test geometry filled by the last draw.
    pub hit: PickerHit,
    /// Coalesce wheel floods to one step per tick.
    pub last_step_at: Instant,
    /// True while the background model fetch is in flight.
    pub loading: bool,
    /// Fetch error shown inline (you can still type a custom id + Enter).
    pub error: Option<String>,
    /// Background fetch result channel (ids, or an error string).
    pub rx: Option<std::sync::mpsc::Receiver<std::result::Result<Vec<String>, String>>>,
}

impl ModelPicker {
    /// Models matching the current filter (substring, case-insensitive).
    pub fn filtered(&self) -> Vec<&String> {
        let f = self.filter.trim().to_lowercase();
        self.models
            .iter()
            .filter(|m| f.is_empty() || m.to_lowercase().contains(&f))
            .collect()
    }

    pub fn count(&self) -> usize {
        self.filtered().len()
    }

    /// Same clamp rules as the provider picker.
    pub fn clamp_scroll(&mut self) {
        let count = self.count();
        if count == 0 {
            self.sel = 0;
            self.scroll = 0;
            return;
        }
        self.sel = self.sel.min(count - 1);
        let page = self.vis_page.max(1);
        let max_scroll = count.saturating_sub(page);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
        if self.sel < self.scroll {
            self.scroll = self.sel;
        }
        let last_vis = self.scroll + page - 1;
        if self.sel > last_vis {
            self.scroll = self.sel + 1 - page;
        }
    }

    /// Move selection by exactly one entry. Viewport shifts by at most 1.
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
            if self.sel == 0 {
                return;
            }
            self.sel -= 1;
            if self.sel < self.scroll {
                self.scroll = self.sel;
            }
        } else {
            if self.sel + 1 >= count {
                return;
            }
            self.sel += 1;
            let last_vis = self.scroll + page - 1;
            if self.sel > last_vis {
                self.scroll += 1;
            }
        }
        let max_scroll = count.saturating_sub(page);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    /// Wheel: one step max every 45ms (identical to other pickers).
    pub fn wheel_step(&mut self, dir: i32) {
        let now = Instant::now();
        if now.duration_since(self.last_step_at) < Duration::from_millis(45) {
            return;
        }
        self.last_step_at = now;
        self.step(dir.signum());
    }

    pub fn set_idx(&mut self, i: usize) {
        let count = self.count();
        if count == 0 {
            self.sel = 0;
            self.scroll = 0;
            return;
        }
        self.sel = i.min(count - 1);
        self.clamp_scroll();
    }

    /// The id that Enter would select: the highlighted row, or — when nothing
    /// matches the filter — the raw filter text as a custom id.
    pub fn chosen(&self) -> Option<String> {
        let picks = self.filtered();
        if let Some(m) = picks.get(self.sel) {
            return Some((*m).clone());
        }
        let typed = self.filter.trim();
        if !typed.is_empty() {
            return Some(typed.to_string());
        }
        None
    }
}

/// Marketplace chooser opened by `/plugins`. Same scroll/filter/select contract
/// as the provider picker (`LoginModal`) and `/model` picker.
pub struct PluginPicker {
    /// Live rows (catalog + install state). Refreshed after each install.
    pub rows: Vec<crate::plugins::PluginRow>,
    pub filter: String,
    pub sel: usize,
    pub scroll: usize,
    pub vis_page: usize,
    pub hit: PickerHit,
    pub last_step_at: Instant,
    /// Background install/toggle in flight.
    pub busy: bool,
    /// Status / error under the list.
    pub status: Option<String>,
    /// Install result channel: Ok(message) / Err(message).
    pub rx: Option<std::sync::mpsc::Receiver<std::result::Result<String, String>>>,
}

impl PluginPicker {
    pub fn filtered(&self) -> Vec<&crate::plugins::PluginRow> {
        let f = self.filter.trim().to_lowercase();
        self.rows
            .iter()
            .filter(|r| {
                f.is_empty()
                    || r.name.to_lowercase().contains(&f)
                    || r.id.to_lowercase().contains(&f)
                    || r.description.to_lowercase().contains(&f)
                    || r.category.to_lowercase().contains(&f)
            })
            .collect()
    }

    pub fn count(&self) -> usize {
        self.filtered().len()
    }

    pub fn clamp_scroll(&mut self) {
        let count = self.count();
        if count == 0 {
            self.sel = 0;
            self.scroll = 0;
            return;
        }
        self.sel = self.sel.min(count - 1);
        let page = self.vis_page.max(1);
        let max_scroll = count.saturating_sub(page);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
        if self.sel < self.scroll {
            self.scroll = self.sel;
        }
        let last_vis = self.scroll + page - 1;
        if self.sel > last_vis {
            self.scroll = self.sel + 1 - page;
        }
    }

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
            if self.sel == 0 {
                return;
            }
            self.sel -= 1;
            if self.sel < self.scroll {
                self.scroll = self.sel;
            }
        } else {
            if self.sel + 1 >= count {
                return;
            }
            self.sel += 1;
            let last_vis = self.scroll + page - 1;
            if self.sel > last_vis {
                self.scroll += 1;
            }
        }
        let max_scroll = count.saturating_sub(page);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
    }

    pub fn wheel_step(&mut self, dir: i32) {
        let now = Instant::now();
        if now.duration_since(self.last_step_at) < Duration::from_millis(45) {
            return;
        }
        self.last_step_at = now;
        self.step(dir.signum());
    }

    pub fn set_idx(&mut self, i: usize) {
        let count = self.count();
        if count == 0 {
            self.sel = 0;
            self.scroll = 0;
            return;
        }
        self.sel = i.min(count - 1);
        self.clamp_scroll();
    }

    pub fn refresh_rows(&mut self) {
        self.rows = crate::plugins::marketplace_rows();
        self.clamp_scroll();
    }
}

/// One row of the unified sessions picker (`/sessions` · `/resume`).
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

/// Interactive sessions browser — open with `/sessions` or `/resume`.
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
    pub client: ApiClient,
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
    /// Full input box (border + body) for hit-testing hover/wheel/drag.
    pub input_area: ratatui::layout::Rect,
    /// Inner text area of the input box (updated every draw) for click-to-caret.
    pub input_inner: ratatui::layout::Rect,
    /// First visible input line (vertical scroll) + horizontal scroll offset.
    pub input_scroll_top: usize,
    /// User-controlled horizontal pan (cells). Draw clamps to line width.
    pub input_x_off: u16,
    /// Usable content width inside the input (set each draw) for h-scroll clamp.
    pub input_usable_w: usize,
    /// Visible row count inside the input inner area (set each draw).
    pub input_view_h: usize,
    /// Press is in the input (may become a drag-select).
    pub input_drag: bool,
    /// True only after the pointer moved past the click threshold.
    pub input_selecting: bool,
    /// Origin (line, display_col) of the input press for threshold math.
    pub input_drag_origin: Option<(usize, usize)>,
    /// Coalesced paste buffer. A paste arrives either as one bracketed
    /// `Event::Paste` (handled immediately) or, on terminals that "drip" it,
    /// as a burst of key events queued in the same instant. Burst chars land
    /// here and flush as ONE chip once the input stream goes quiet — so a lone
    /// keystroke is never mistaken for a paste. See `flush_paste_accum`.
    paste_accum: String,
    paste_accum_at: Option<Instant>,
    /// Merged paste session — ensures a large wall of text that the PTY split
    /// across many frames / Event::Paste chunks becomes ONE chip, not N chips.
    /// Any paste arriving within PASTE_MERGE_WINDOW appends to this chip.
    active_paste_id: Option<u32>,
    active_paste_at: Option<Instant>,
    /// Last raw (non-chip) paste insertion, for retroactive conversion.
    /// If a paste arrives as many small raw chunks that together exceed the chip
    /// threshold, we delete the previous raw range and re-chip the combined text.
    last_raw_start: Option<usize>,
    last_raw_len: usize,
    last_raw_text: String,
    /// Standing session goal (`/goal`), prepended to every model turn as context
    /// without appearing in the transcript. Cleared with `/goal clear`.
    session_goal: Option<String>,
    /// One-off side notes (`/btw`) folded into the next turn only.
    pending_btw: Vec<String>,
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
    #[cfg(feature = "image-peek")]
    pub img_picker: Option<ratatui_image::picker::Picker>,
    /// Decoded image protocols keyed by path — encoding is expensive, cache it.
    #[cfg(feature = "image-peek")]
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
    /// Per wrapped line → owning peekable cell (legacy map; hover peeks removed).
    pub line_cells: Vec<Option<usize>>,
    /// Per wrapped line → owning cell index (ALL cell types, for right-click hit-testing).
    pub line_cell_all: Vec<Option<usize>>,
    /// Absolute line → (`cell_idx`, display_col_start, display_col_end) for the
    /// exact `"click to peek"` text span. Only this hitbox opens the dialogue.
    pub hit_click_to_peek: Vec<Option<(usize, usize, usize)>>,
    /// Absolute line → (`cell_idx`, col_lo, col_hi) for expand/collapse phrases
    /// (`▸ expands` / `▾ collapse`) so clicks on those words toggle, not no-op.
    pub hit_expand_phrase: Vec<Option<(usize, usize, usize)>>,
    /// Hitboxes for queued follow-up actions on each wrapped line.
    /// Entries: (cell_idx, col_lo, col_hi, action) where action 0 = send now, 1 = dismiss.
    /// Multiple actions can share a line (send now + dismiss).
    pub hit_queue_actions: Vec<Vec<(usize, usize, usize, u8)>>,
    /// Absolute line → clickable `http(s)://` spans `(col_lo, col_hi, url)`.
    pub hit_urls: Vec<Vec<(usize, usize, String)>>,
    /// First visible wrapped-line index in the transcript body (for hit-tests).
    pub transcript_top: u16,
    /// Brief highlight after toggle: (cell_idx, when).
    pub expand_flash: Option<(usize, Instant)>,
    /// Unused (hover peeks removed — kept so field layout stays simple).
    pub hover_cell: Option<usize>,
    /// Stable click-to-peek: cell index while open.
    pub peek_open: Option<usize>,
    /// Frozen dialogue geometry — set **once** on first draw, never moves.
    pub peek_frozen: Option<ratatui::layout::Rect>,
    /// Bounds used for outside-click / ✕ (equals `peek_frozen` while open).
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
    /// When true, a cancel (send-now interject) keeps the queue so the follow-up
    /// can start immediately after the interrupted turn ends.
    preserve_queue_on_interrupt: bool,

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
    /// Model chooser (`/model`) — live provider model list, when open.
    pub model_picker: Option<ModelPicker>,
    /// Plugin marketplace (`/plugins`) — install / enable / disable.
    pub plugin_picker: Option<PluginPicker>,
    /// Whether an API key is available. `/logout` flips this false and blocks
    /// turns until `/login` provides a new key.
    authed: bool,
}

struct TermGuard;

impl Drop for TermGuard {
    fn drop(&mut self) {
        disable_mouse();
        let _ = stdout().execute(DisableFocusChange);
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
    client: ApiClient,
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
            "nur needs an interactive terminal (stdin is not a TTY).\n\
             Run `nur` from a normal shell window, not a redirected pipe."
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
    // Focus events: when the user releases the mouse *outside* the terminal,
    // we never see MouseUp — FocusLost clears stuck drag/select state.
    let _ = stdout().execute(EnableFocusChange);
    enable_mouse();
    // Hardware cursor hidden — we paint a Nur-gold block caret ourselves.
    stdout().execute(Hide)?;
    let _guard = TermGuard;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)
        .map_err(|e| crate::error::MuseError::Other(format!("terminal init: {e}")))?;

    // Query the terminal's graphics protocol + font size for inline image
    // peeks (sixel / kitty / iTerm2, halfblocks fallback). 1s timeout inside;
    // any failure degrades to a sane halfblocks picker, never an error.
    #[cfg(feature = "image-peek")]
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
        input_area: ratatui::layout::Rect::default(),
        input_inner: ratatui::layout::Rect::default(),
        input_scroll_top: 0,
        input_x_off: 0,
        input_usable_w: 40,
        input_view_h: 4,
        input_drag: false,
        input_selecting: false,
        input_drag_origin: None,
        paste_accum: String::new(),
        paste_accum_at: None,
        active_paste_id: None,
        active_paste_at: None,
        last_raw_start: None,
        last_raw_len: 0,
        last_raw_text: String::new(),
        session_goal: None,
        pending_btw: Vec::new(),
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
        #[cfg(feature = "image-peek")]
        img_picker,
        #[cfg(feature = "image-peek")]
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
        hit_click_to_peek: Vec::new(),
        hit_expand_phrase: Vec::new(),
        hit_queue_actions: Vec::new(),
        hit_urls: Vec::new(),
        transcript_top: 0,
        expand_flash: None,
        hover_cell: None,
        peek_open: None,
        peek_frozen: None,
        peek_box: ratatui::layout::Rect::default(),
        peek_close: ratatui::layout::Rect::default(),
        ctx_menu: None,
        last_click: None,
        mouse_col: 0,
        mouse_row: 0,
        input: InputState::new(),
        queue: VecDeque::new(),
        preserve_queue_on_interrupt: false,
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
        model_picker: None,
        plugin_picker: None,
        authed: true,
    };

    app.replay_session_history();
    if let Some(note) = workspace_note {
        app.push_note(Tone::Session, note);
    }
    // Ecosystem snapshot (graphify / plur / …) then active mode — banner already
    // lists feature groups; these notes stay short so the open screen is clean.
    if !ecosystem_summary.is_empty() {
        app.push_note(Tone::Skill, format!("ecosystem · {ecosystem_summary}"));
    }
    app.push_info(format!(
        "mode · {mode_label}  ·  Shift+Tab  manual → plan → auto  ·  /mode"
    ));

    // Started without any API key → sign-in required before the first turn.
    if crate::auth::resolve_api_key().is_err() {
        app.authed = false;
        app.push_note(
            Tone::Mode,
            "no API key found — press any key, then /login to sign in (or set NUR_API_KEY)".into(),
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
        app.poll_oauth_login();
        app.poll_model_picker();
        app.poll_plugin_picker();
        while let Ok(ev) = app.rx.try_recv() {
            app.on_agent_event(ev);
            dirty = true;
        }

        let frame_ms = if app.busy
            || app.picker.is_some()
            || app.approval.is_some()
            || app.login.is_some()
            || app.model_picker.is_some()
            || app.plugin_picker.is_some()
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
        //
        //    Keys/Paste before mouse: Windows mouse-capture floods Moved/Drag and
        //    can starve ↑↓ / typing in modals. We still run the proven main-prompt
        //    paste-chip coalescer (live poll burst drain) — only the *order*
        //    relative to mouse changes, not the paste algorithm.
        let wait = if dirty {
            Duration::ZERO
        } else {
            Duration::from_millis(frame_ms)
        };
        // Modals own the keyboard: never route their keys through paste-chip
        // coalescing (that path is for the main prompt only).
        let modal_open = app.login.is_some()
            || app.model_picker.is_some()
            || app.plugin_picker.is_some()
            || app.approval.is_some()
            || app.picker.is_some()
            || app.ctx_menu.is_some();
        if event::poll(wait)? {
            // Phase A — drain queue. Preserve Key/Paste relative order; park
            // mouse/focus/resize for later so floods cannot interleave ahead of
            // keyboard work.
            let mut kb_events: Vec<Event> = Vec::new();
            let mut other_events: Vec<Event> = Vec::new();
            let mut first = true;
            loop {
                let ev = if first {
                    first = false;
                    event::read()?
                } else if event::poll(Duration::ZERO)? {
                    event::read()?
                } else {
                    break;
                };
                match ev {
                    Event::Key(key)
                        if key.kind == KeyEventKind::Press || key.kind == KeyEventKind::Repeat =>
                    {
                        kb_events.push(Event::Key(key));
                    }
                    Event::Paste(s) => kb_events.push(Event::Paste(s)),
                    // Drop pure hover motion while a modal is open — bulk of
                    // Windows mouse floods; no modal use.
                    Event::Mouse(m)
                        if modal_open
                            && matches!(
                                m.kind,
                                MouseEventKind::Moved | MouseEventKind::Drag(_)
                            ) => {}
                    other => other_events.push(other),
                }
                // Hard cap on mouse/other so a flood cannot keep us draining forever.
                if kb_events.len() + other_events.len() > 512 {
                    while event::poll(Duration::ZERO)? {
                        match event::read()? {
                            Event::Key(key)
                                if key.kind == KeyEventKind::Press
                                    || key.kind == KeyEventKind::Repeat =>
                            {
                                kb_events.push(Event::Key(key));
                            }
                            Event::Paste(s) => kb_events.push(Event::Paste(s)),
                            _ => {}
                        }
                        if kb_events.len() > 256 {
                            break;
                        }
                    }
                    break;
                }
            }

            // Phase B — keyboard / paste (before mouse).
            if modal_open {
                // Modal: no chip coalescing. Press + Repeat both drive handlers
                // (held arrows). Enter is Press-only (no multi-submit).
                app.flush_paste_accum();
                for ev in kb_events {
                    match ev {
                        Event::Key(key) => {
                            if matches!(key.code, KeyCode::Enter)
                                && key.kind != KeyEventKind::Press
                            {
                                continue;
                            }
                            app.on_key(key);
                            dirty = true;
                        }
                        Event::Paste(s) => {
                            app.on_paste(&s);
                            dirty = true;
                        }
                        _ => {}
                    }
                    if app.should_quit {
                        break;
                    }
                }
            } else {
                // Main prompt: original paste-chip machine.
                // Paste: bracketed Event::Paste → one chip; key drips coalesce into
                // paste_accum only when more *text* presses are already queued (not
                // KeyRelease — Windows always pairs Press+Release, which broke Enter).
                // Enter is never paste: plain Enter submits, Shift+Enter = newline.
                // Live poll(ZERO) during a paste-char burst still peeks the PTY for
                // drips that arrive after Phase A — that is the proven path.
                let mut deferred: Option<Event> = None;
                let mut kb_i = 0usize;
                while kb_i < kb_events.len() || deferred.is_some() {
                    let ev = if let Some(e) = deferred.take() {
                        e
                    } else {
                        let e = kb_events[kb_i].clone();
                        kb_i += 1;
                        e
                    };
                    match ev {
                        Event::Key(key)
                            if key.kind == KeyEventKind::Press
                                || key.kind == KeyEventKind::Repeat =>
                        {
                            if matches!(key.code, KeyCode::Enter) {
                                if !app.paste_accum.is_empty() {
                                    app.paste_accum.push('\n');
                                    app.paste_accum_at = Some(Instant::now());
                                } else if key.kind == KeyEventKind::Press {
                                    app.on_key(key);
                                }
                                dirty = true;
                                continue;
                            }

                            if let Some(c) = key_as_paste_burst_char(&key) {
                                // Drain further paste-text / Event::Paste; skip
                                // KeyRelease (Windows ConPTY interleaves Press/Release
                                // for every char — treating Release as a break made
                                // pastes type out char-by-char and never coalesce).
                                let mut burst = String::new();
                                burst.push(c);
                                // First absorb paste keys / Event::Paste still in
                                // the pre-drained keyboard batch (order-preserving).
                                while kb_i < kb_events.len() {
                                    match &kb_events[kb_i] {
                                        Event::Key(k)
                                            if key_as_paste_burst_char(k).is_some() =>
                                        {
                                            if let Some(ch) = key_as_paste_burst_char(k) {
                                                burst.push(ch);
                                            }
                                            kb_i += 1;
                                        }
                                        Event::Paste(p) => {
                                            burst.push_str(p);
                                            kb_i += 1;
                                        }
                                        _ => break,
                                    }
                                }
                                // Then live-queue peek — same as pre-v0.13.7: PTY
                                // may still be delivering drip chars this frame.
                                loop {
                                    if !event::poll(Duration::ZERO)? {
                                        break;
                                    }
                                    let next = match event::read() {
                                        Ok(ev) => ev,
                                        Err(_) => break,
                                    };
                                    match next {
                                        Event::Key(k) if k.kind == KeyEventKind::Release => {
                                            // Ignore release — keep draining the burst.
                                            continue;
                                        }
                                        Event::Key(k)
                                            if key_as_paste_burst_char(&k).is_some() =>
                                        {
                                            if let Some(ch) = key_as_paste_burst_char(&k) {
                                                burst.push(ch);
                                            }
                                        }
                                        Event::Paste(p) => burst.push_str(&p),
                                        // Mouse/focus during a paste burst: park
                                        // for Phase C (keys-first policy).
                                        other @ (Event::Mouse(_)
                                        | Event::FocusLost
                                        | Event::FocusGained
                                        | Event::Resize(_, _)) => {
                                            other_events.push(other);
                                        }
                                        other => {
                                            deferred = Some(other);
                                            break;
                                        }
                                    }
                                }
                                // Multi-char burst, or mid-paste drip → accumulate.
                                // Single isolated char (burst len 1, empty accum) → type.
                                if burst.chars().count() > 1 || !app.paste_accum.is_empty() {
                                    app.paste_accum.push_str(&burst);
                                    app.paste_accum_at = Some(Instant::now());
                                    dirty = true;
                                    continue;
                                }
                                // Lone character: normal typing via on_key.
                                app.flush_paste_accum();
                                if key.kind == KeyEventKind::Press
                                    || key.kind == KeyEventKind::Repeat
                                {
                                    let mut k = key;
                                    k.code = KeyCode::Char(c);
                                    app.on_key(k);
                                }
                                dirty = true;
                                continue;
                            }

                            app.flush_paste_accum();
                            // Press + Repeat (held arrows / shortcuts).
                            app.on_key(key);
                            dirty = true;
                        }
                        Event::Paste(s) => {
                            // Bracketed paste (the clean path) — but the terminal may
                            // split a huge paste into many Event::Paste chunks across
                            // frames. Coalesce them in paste_accum and flush as ONE chip,
                            // which then uses the active_paste_id merge window so even
                            // split flushes become a single chip.
                            app.paste_accum.push_str(&s);
                            app.paste_accum_at = Some(Instant::now());
                            dirty = true;
                        }
                        _ => {}
                    }
                    if app.should_quit {
                        break;
                    }
                }
            }

            // Phase C — mouse / focus / resize (never starves keys above).
            for ev in other_events {
                match ev {
                    Event::Mouse(m) => {
                        app.flush_paste_accum();
                        app.on_mouse(m);
                        dirty = true;
                    }
                    Event::Resize(_, _) => dirty = true,
                    Event::FocusLost => {
                        // Mouse-up outside the window never arrives — reset
                        // drag/select so hover isn't misread as a held button.
                        app.on_focus_lost();
                        dirty = true;
                    }
                    Event::FocusGained => {
                        // Re-arm mouse tracking after host/focus quirks.
                        enable_mouse();
                        last_mouse_rearm = Instant::now();
                    }
                    _ => {}
                }
                if app.should_quit {
                    break;
                }
            }
        }

        if app.should_quit {
            break;
        }

        // Flush a coalesced paste burst once the input stream goes quiet.
        if let Some(at) = app.paste_accum_at {
            if at.elapsed() >= Duration::from_millis(PASTE_FLUSH_MS) {
                app.flush_paste_accum();
                dirty = true;
            }
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
        // Index order must match CTX_ACTIONS: 0 Fork · 1 Edit · 2 Revert · 3 Copy.
        match sel {
            0 => self.ctx_fork(),
            1 => self.ctx_edit(),
            2 => self.ctx_revert(),
            3 => self.ctx_copy(),
            _ => {}
        }
        self.close_ctx_menu();
    }

    /// Edit: load the prompt into the input **without** rewinding history.
    /// Send interjects as a new user turn (full prior context stays in session).
    fn ctx_edit(&mut self) {
        let Some((prompt, _)) = self.ctx_prompt() else { return };
        self.input.set_text(&prompt);
        self.ensure_input_caret_visible();
        self.input_scroll_top = 0;
        self.push_note(
            Tone::Neutral,
            "edit — prompt loaded in input · send to interject as a new turn (history kept)"
                .into(),
        );
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
        self.close_peek();
        self.hover_cell = None;
        self.selection = None;
        self.select_anchor = None;
        self.selecting = false;
        self.expand_flash = None;
    }

    fn close_peek(&mut self) {
        self.peek_open = None;
        self.peek_frozen = None;
        self.peek_box = ratatui::layout::Rect::default();
        self.peek_close = ratatui::layout::Rect::default();
        self.peek_scroll = 0;
        self.peek_scroll_cell = None;
    }

    fn open_stable_peek(&mut self, idx: usize) {
        // Geometry is frozen on the *first draw* of this open (not from mouse).
        self.peek_open = Some(idx);
        self.peek_frozen = None;
        self.peek_scroll = 0;
        self.peek_scroll_cell = Some(idx);
        self.hover_cell = None;
    }

    /// Persist expandable transcript cards so reloads keep thought/tool bodies.
    fn sync_ui_log_to_session(&mut self) {
        let log = cells_to_ui_log(&self.cells);
        if let Some(session) = self.session.as_mut() {
            session.ui_log = log;
        }
    }

    fn save_session_with_ui_log(&mut self) {
        self.sync_ui_log_to_session();
        if let Some(session) = self.session.as_ref() {
            let _ = session.save();
        }
    }

    // ── keys ───────────────────────────────────────────────────────────
    fn on_key(&mut self, key: event::KeyEvent) {
        // Secure login modal swallows all keys (masked key entry).
        if self.login.is_some() {
            self.on_login_key(key);
            return;
        }
        // Model picker swallows all keys while open (type-to-filter).
        if self.model_picker.is_some() {
            self.on_model_picker_key(key);
            return;
        }
        // Plugin marketplace swallows all keys while open.
        if self.plugin_picker.is_some() {
            self.on_plugin_picker_key(key);
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

        // Any normal key (not paste) that reaches here ends the "raw small paste"
        // merge chain. We keep the chip merge window time-based + cursor-checked,
        // but raw small pastes should not retroactively swallow typed chars.
        // Note: paste bursts go via paste_accum, not on_key, so split raw pastes
        // arriving back-to-back will still merge via on_paste.
        if self.paste_accum.is_empty() {
            // Clear only the raw tracker; chip tracking stays for its time window
            // and is cursor-checked inside on_paste.
            if self.last_raw_start.is_some() {
                self.last_raw_start = None;
                self.last_raw_len = 0;
                self.last_raw_text.clear();
            }
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);

        // Reverse history search (Ctrl+R) owns the keyboard while active. A
        // `false` return means it accepted/closed and the key should fall
        // through to normal handling (e.g. an arrow that also moves the caret).
        if self.input.search_is_active() && self.handle_search_key(key, ctrl, alt) {
            return;
        }

        match key.code {
            // Ctrl+C: copy selection (transcript → input → open peek body) → else
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
                // Full thought / shell / tool body from the pinned peek dialogue.
                if let Some(idx) = self.peek_open {
                    if let Some(body) = self.cells.get(idx).and_then(|c| c.peek_body()) {
                        if !body.trim().is_empty() {
                            clipboard_set(&body);
                            return;
                        }
                    }
                }
                if self.busy {
                    self.interrupt();
                } else if !self.input.is_empty() {
                    self.input.clear();
                    self.clear_paste_merge_state();
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
            // Ctrl+V / Shift+Insert: clipboard → exactly one chip, then lock drip.
            KeyCode::Char('v') if ctrl => {
                if let Some(t) = clipboard_get() {
                    self.on_paste(&t);
                }
                return;
            }
            KeyCode::Insert if shift => {
                if let Some(t) = clipboard_get() {
                    self.on_paste(&t);
                }
                return;
            }
            // Ctrl+X: cut input selection (or whole input if none); chips expand.
            KeyCode::Char('x') if ctrl => {
                if self.input.has_selection() {
                    if let Some(t) = self.input.selected_text() {
                        clipboard_set(&t);
                        self.input.delete_selection();
                    }
                } else if !self.input.is_empty() {
                    clipboard_set(&self.input.text_expanded());
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
                if self.peek_open.is_some() {
                    self.close_peek();
                } else if self.busy {
                    self.interrupt();
                } else if self.palette_visible() {
                    self.input.clear();
                    self.clear_paste_merge_state();
                } else if !self.input.is_empty() {
                    self.input.clear();
                    self.clear_paste_merge_state();
                }
            }
            // Shift+Enter → newline. Plain Enter → always submit. Never the reverse.
            KeyCode::Enter if shift && !ctrl && !alt => {
                self.input.insert_char('\n');
                self.ensure_input_caret_visible();
                // Newline is typing — break raw chain (chip merge is cursor-checked).
                self.last_raw_start = None;
                self.last_raw_len = 0;
                self.last_raw_text.clear();
            }
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
                if self.input.text().trim().is_empty() {
                    return;
                }
                let submitted = self.input.submit();
                self.submit_text(&submitted);
                // After submit, start fresh — no dangling paste session.
                self.clear_paste_merge_state();
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
            KeyCode::Up if shift => {
                // Shift+↑ always extends selection in the draft when non-empty.
                if !self.input.is_empty() {
                    self.input.extend_up_line();
                    self.ensure_input_caret_visible();
                } else {
                    self.scroll_up(1);
                }
            }
            KeyCode::Down if shift => {
                if !self.input.is_empty() {
                    self.input.extend_down_line();
                    self.ensure_input_caret_visible();
                } else {
                    self.scroll_down(1);
                }
            }
            KeyCode::Up => match self.arrow_action(true) {
                ArrowAction::Palette => self.palette_step(-1),
                ArrowAction::Caret => {
                    self.input.move_up_line();
                    self.ensure_input_caret_visible();
                }
                ArrowAction::Scroll => self.scroll_up(1),
            },
            KeyCode::Down => match self.arrow_action(false) {
                ArrowAction::Palette => self.palette_step(1),
                ArrowAction::Caret => {
                    self.input.move_down_line();
                    self.ensure_input_caret_visible();
                }
                ArrowAction::Scroll => self.scroll_down(1),
            },
            KeyCode::Char('p') if ctrl => self.input.history_prev(),
            KeyCode::Char('n') if ctrl => self.input.history_next(),
            KeyCode::Left if ctrl => {
                self.input.word_left();
                self.ensure_input_caret_visible();
            }
            KeyCode::Right if ctrl => {
                self.input.word_right();
                self.ensure_input_caret_visible();
            }
            KeyCode::Left if shift => {
                self.input.extend_left();
                self.ensure_input_caret_visible();
            }
            KeyCode::Right if shift => {
                self.input.extend_right();
                self.ensure_input_caret_visible();
            }
            KeyCode::Left => {
                self.input.move_left();
                self.ensure_input_caret_visible();
            }
            KeyCode::Right => {
                self.input.move_right();
                self.ensure_input_caret_visible();
            }
            // Home/End edit the draft when there is one, else jump the transcript.
            KeyCode::Home => {
                if self.input.is_empty() {
                    self.scroll_to_top();
                } else {
                    self.input.move_line_home();
                    self.ensure_input_caret_visible();
                }
            }
            KeyCode::End => {
                if self.input.is_empty() {
                    self.scroll_to_bottom();
                } else {
                    self.input.move_line_end();
                    self.ensure_input_caret_visible();
                }
            }
            KeyCode::Backspace => {
                self.input.backspace();
                self.ensure_input_caret_visible();
            }
            KeyCode::Delete => {
                self.input.delete();
                self.ensure_input_caret_visible();
            }
            KeyCode::PageUp => self.scroll_up(self.page()),
            KeyCode::PageDown => self.scroll_down(self.page()),
            KeyCode::Char('l') if ctrl => {
                self.cells.retain(|c| matches!(c, Cell::Banner));
                self.scroll_from_bottom = 0;
            }
            KeyCode::Char('r') if ctrl => self.input.search_begin(),
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
                self.ensure_input_caret_visible();
                self.palette_idx = 0;
                self.palette_scroll = 0;
            }
            _ => {}
        }
    }

    /// Handle a key while reverse history search (Ctrl+R) is active. Returns
    /// `true` if the key was fully consumed; `false` means the search was
    /// accepted/closed and the caller should process `key` normally.
    fn handle_search_key(&mut self, key: event::KeyEvent, ctrl: bool, alt: bool) -> bool {
        match key.code {
            KeyCode::Char('r') if ctrl => {
                self.input.search_begin(); // step to the next older match
                true
            }
            KeyCode::Esc => {
                self.input.search_cancel();
                true
            }
            KeyCode::Char('c') | KeyCode::Char('g') if ctrl => {
                self.input.search_cancel();
                true
            }
            KeyCode::Backspace => {
                self.input.search_backspace();
                true
            }
            KeyCode::Enter => {
                // Accept the match into the composer (ready to edit or submit).
                self.input.search_accept();
                self.ensure_input_caret_visible();
                true
            }
            KeyCode::Char(c) if !ctrl && !alt => {
                self.input.search_push(c);
                true
            }
            _ => {
                // Any other key (arrows, word ops, …): accept the match and let
                // the key fall through to normal editing.
                self.input.search_accept();
                self.ensure_input_caret_visible();
                false
            }
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
        // Login picker is modal — same wheel/click routing as the sessions picker.
        if self.login.is_some() {
            self.scrollbar_drag = false;
            self.selecting = false;
            self.select_anchor = None;
            self.mouse_left_down = false;
            self.on_login_mouse(m);
            return;
        }
        // Model picker is modal — same wheel/click routing.
        if self.model_picker.is_some() {
            self.scrollbar_drag = false;
            self.selecting = false;
            self.select_anchor = None;
            self.mouse_left_down = false;
            self.on_model_picker_mouse(m);
            return;
        }
        // Plugin marketplace is modal — same wheel/click routing.
        if self.plugin_picker.is_some() {
            self.scrollbar_drag = false;
            self.selecting = false;
            self.select_anchor = None;
            self.mouse_left_down = false;
            self.on_plugin_picker_mouse(m);
            return;
        }

        // Mouse click/drag moves caret → break raw merge chain (same rationale
        // as on_key). Chip merge is cursor-checked inside on_paste.
        if self.paste_accum.is_empty() && self.last_raw_start.is_some() {
            self.last_raw_start = None;
            self.last_raw_len = 0;
            self.last_raw_text.clear();
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
                // Input hitbox wins over palette when the pointer is in the prompt
                // (palette floats above but shouldn't steal draft scroll).
                if self.hit_input_box(m.column, m.row) && !approval_open {
                    self.wheel_input(-1);
                    if self.input_drag {
                        self.note_input_drag_motion(m.column, m.row);
                        self.input_drag_to(m.column, m.row);
                    }
                } else if self.palette_visible() {
                    self.palette_wheel_step(-1);
                } else if self.wheel_over_open_peek(m.column, m.row) {
                    // Wheel inside a pinned peek scrolls its body, not the page.
                    self.peek_scroll = self.peek_scroll.saturating_sub(3);
                } else {
                    // Always works — including during streaming and under approval.
                    self.scroll_up(3);
                    // Keep drag-select alive across scroll (absolute line anchors).
                    if self.mouse_left_down && self.select_anchor.is_some() {
                        self.extend_selection_to(m.column, m.row);
                    }
                }
                if !approval_open {
                    self.update_hover_from_mouse();
                }
            }
            MouseEventKind::ScrollDown => {
                if self.hit_input_box(m.column, m.row) && !approval_open {
                    self.wheel_input(1);
                    if self.input_drag {
                        self.note_input_drag_motion(m.column, m.row);
                        self.input_drag_to(m.column, m.row);
                    }
                } else if self.palette_visible() {
                    self.palette_wheel_step(1);
                } else if self.wheel_over_open_peek(m.column, m.row) {
                    self.peek_scroll = self
                        .peek_scroll
                        .saturating_add(3)
                        .min(self.peek_rows.saturating_sub(1));
                } else {
                    self.scroll_down(3);
                    if self.mouse_left_down && self.select_anchor.is_some() {
                        self.extend_selection_to(m.column, m.row);
                    }
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
                if self.peek_open.is_some() {
                    if peek_click_dismisses(self.peek_close, self.peek_box, m.column, m.row) {
                        self.close_peek();
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
                    self.close_peek();
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
                    self.input_drag = false;
                    self.input_selecting = false;
                    self.input_drag_origin = None;
                    self.scrollbar_press(m.row);
                } else if self.hit_input_box(m.column, m.row) {
                    // Input owns the pointer: arm potential drag-select (threshold on move).
                    self.scrollbar_drag = false;
                    self.select_anchor = None;
                    self.selecting = false;
                    self.selection = None;
                    self.close_peek();
                    self.input_drag = true;
                    self.input_selecting = false;
                    self.input_drag_origin = self.input_pos_at(m.column, m.row);
                    self.input_select_start(m.column, m.row);
                } else if self.in_transcript(m.column, m.row) {
                    self.scrollbar_drag = false;
                    self.input_drag = false;
                    self.input_selecting = false;
                    self.input_drag_origin = None;
                    self.input.clear_selection();
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
                    self.input_drag = false;
                    self.input_selecting = false;
                    self.input_drag_origin = None;
                    self.close_peek();
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
                } else if self.input_drag {
                    // Click (no threshold): place caret only. Real drag: keep selection.
                    let selecting = self.input_selecting;
                    self.input_drag = false;
                    self.input_selecting = false;
                    self.input_drag_origin = None;
                    if !selecting {
                        if let Some((vrow, dcol)) = self.input_pos_at(m.column, m.row) {
                            let w = self.input_usable_w.max(1);
                            let idx = self.input.index_at_visual(vrow, dcol, w);
                            self.input.set_cursor_index(idx);
                        } else {
                            self.input.clear_selection();
                        }
                    }
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
        if self.input_drag {
            self.note_input_drag_motion(col, row);
            self.maybe_autoscroll_input_while_selecting(col, row);
            if self.input_selecting {
                self.input_drag_to(col, row);
            } else if let Some((vrow, dcol)) = self.input_pos_at(col, row) {
                // Still a click — keep selection collapsed under the pointer.
                let w = self.input_usable_w.max(1);
                let idx = self.input.index_at_visual(vrow, dcol, w);
                self.input.select_start_at_index(idx);
            }
            return;
        }
        if self.select_anchor.is_none() {
            return;
        }
        // Edge auto-scroll so the selection can grow past the viewport.
        // Off-screen selected text stays selected (absolute line indices).
        self.maybe_autoscroll_while_selecting(row);
        self.extend_selection_to(col, row);
    }

    /// Scroll toward older/newer content when the pointer is at/past the
    /// transcript edge during a drag-select.
    fn maybe_autoscroll_while_selecting(&mut self, row: u16) {
        let body = self.transcript_body;
        if body.height == 0 {
            return;
        }
        // Outside or in the first/last row of the body → nudge the view.
        if row < body.y.saturating_add(1) {
            self.scroll_up(1);
        } else if row + 1 >= body.bottom() {
            self.scroll_down(1);
        }
    }

    /// Update selection end from pointer (clamped). Marks `selecting` once the
    /// end moves past a one-cell threshold from the anchor.
    fn extend_selection_to(&mut self, col: u16, row: u16) {
        let Some(anchor) = self.select_anchor else {
            return;
        };
        let Some(pos) = self.pos_at_clamped(col, row) else {
            return;
        };
        let moved = pos.line != anchor.line || pos.col.abs_diff(anchor.col) > 1;
        if moved {
            self.selecting = true;
            self.selection = Some(TextRange {
                start: anchor,
                end: pos,
            });
            self.hover_cell = None;
        } else if self.selection.is_some() {
            // Keep end in sync even for tiny moves once a range exists.
            self.selection = Some(TextRange {
                start: anchor,
                end: pos,
            });
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

    /// Map screen coords → absolute wrapped-line TextPos (strict: inside body only).
    fn pos_at(&self, col: u16, row: u16) -> Option<TextPos> {
        if !self.in_transcript(col, row) {
            return None;
        }
        self.pos_at_clamped(col, row)
    }

    /// Visible top line from live scroll state (not only last-draw `transcript_top`).
    /// Needed so edge-scroll during drag maps to the post-scroll viewport.
    fn live_transcript_top(&self) -> usize {
        let total = self.plain_lines.len() as u16;
        let h = self.view_h.max(1);
        let max_scroll = total.saturating_sub(h);
        let sfb = self.scroll_from_bottom.min(max_scroll);
        max_scroll.saturating_sub(sfb) as usize
    }

    /// Like `pos_at`, but clamps outside the transcript body to the nearest
    /// edge of the *visible* slice so drag-select can keep extending while
    /// scrolling. Selection ranges use absolute `plain_lines` indices, so text
    /// that scrolls off-screen stays in the selection.
    fn pos_at_clamped(&self, col: u16, row: u16) -> Option<TextPos> {
        if self.plain_lines.is_empty() {
            return None;
        }
        let body = self.transcript_body;
        if body.width == 0 || body.height == 0 {
            return None;
        }
        let top = self.live_transcript_top();
        let max_line = self.plain_lines.len().saturating_sub(1);
        let vis_last = (top + body.height as usize)
            .saturating_sub(1)
            .min(max_line);
        let max_scroll = self.max_scroll();

        let line = if row < body.y {
            // Above the body: top of viewport (after auto-scroll, older lines).
            // When already at history top, pin absolute line 0.
            if self.scroll_from_bottom >= max_scroll {
                0
            } else {
                top
            }
        } else if row >= body.bottom() {
            // Below the body: bottom of viewport / absolute last when at end.
            if self.scroll_from_bottom == 0 {
                max_line
            } else {
                vis_last
            }
        } else {
            let local_y = row.saturating_sub(body.y) as usize;
            (top + local_y).min(max_line)
        };

        let plain = &self.plain_lines[line];
        let nchars = plain.chars().count();
        let col_idx = if col < body.x {
            0
        } else if col >= body.right() {
            nchars
        } else {
            display_col_to_char_idx(plain, col.saturating_sub(body.x) as usize)
        };
        Some(TextPos {
            line,
            col: col_idx.min(nchars),
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

    /// Hover peeks removed — keep mouse tracking for other UI only.
    fn update_hover_from_mouse(&mut self) {
        self.hover_cell = None;
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

    /// Only the stable click-to-peek dialogue (no hover peeks).
    pub fn active_peek_cell(&self) -> Option<usize> {
        self.peek_open
    }

    /// Decoded terminal-graphics protocol for an image path, lazily built and
    /// cached (encoding is expensive; re-doing it per frame would melt the UI).
    #[cfg(feature = "image-peek")]
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
        // Hit only the painted rail (2 cols). Do NOT steal clicks from the
        // rightmost transcript columns — that was a known UX bug.
        col >= t.x && col < t.right() && row >= t.y && row < t.bottom()
    }

    /// Clear mouse drag / select state when the terminal loses focus
    /// (mouse-up outside the window never fires a MouseUp event).
    fn on_focus_lost(&mut self) {
        self.mouse_left_down = false;
        self.scrollbar_drag = false;
        self.scrollbar_hover = false;
        self.selecting = false;
        self.select_anchor = None;
        // Keep a finished selection for copy; only drop empty/in-progress ranges.
        if self.selection.map(|r| r.is_empty()).unwrap_or(true) {
            self.selection = None;
        }
        self.input_drag = false;
        self.input_selecting = false;
        self.input_drag_origin = None;
    }

    /// Wheel events over an open pinned peek scroll the peek body.
    fn wheel_over_open_peek(&self, col: u16, row: u16) -> bool {
        self.peek_open.is_some() && rect_contains(self.peek_box, col, row)
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

    /// True when the pointer is over the full input box (border + body).
    fn hit_input_box(&self, col: u16, row: u16) -> bool {
        let a = self.input_area;
        a.width > 0
            && a.height > 0
            && col >= a.x
            && col < a.right()
            && row >= a.y
            && row < a.bottom()
    }

    /// Map terminal coords → (visual_row, col_in_row) inside the soft-wrapped input.
    fn input_pos_at(&self, col: u16, row: u16) -> Option<(usize, usize)> {
        let inner = self.input_inner;
        if inner.width == 0 || inner.height == 0 {
            return None;
        }
        let c = col.clamp(inner.x, inner.right().saturating_sub(1));
        let r = row.clamp(inner.y, inner.bottom().saturating_sub(1));
        let prefix_w: u16 = 2;
        let display_col = c.saturating_sub(inner.x).saturating_sub(prefix_w) as usize;
        let vrow = r.saturating_sub(inner.y) as usize + self.input_scroll_top;
        let usable = self.input_usable_w.max(1);
        let vcount = self.input.visual_line_count(usable).max(1);
        let vrow = vrow.min(vcount - 1);
        Some((vrow, display_col))
    }

    fn input_select_start(&mut self, col: u16, row: u16) {
        if let Some((vrow, dcol)) = self.input_pos_at(col, row) {
            let w = self.input_usable_w.max(1);
            let idx = self.input.index_at_visual(vrow, dcol, w);
            // Place caret via absolute index so soft-wrap clicks map correctly.
            self.input.select_start_at_index(idx);
        }
    }

    fn input_drag_to(&mut self, col: u16, row: u16) {
        if let Some((vrow, dcol)) = self.input_pos_at(col, row) {
            let w = self.input_usable_w.max(1);
            let idx = self.input.index_at_visual(vrow, dcol, w);
            self.input.select_drag_to_index(idx);
        }
    }

    /// Scroll the input viewport by `delta` **visual rows** (negative = up).
    /// Always ±1-sized steps from the wheel so intermediate lines stay visible.
    fn scroll_input(&mut self, delta: i32) {
        let h = self.input_view_h.max(1);
        let w = self.input_usable_w.max(1);
        let max_top = self.input.visual_line_count(w).saturating_sub(h);
        if delta < 0 {
            self.input_scroll_top = self
                .input_scroll_top
                .saturating_sub((-delta) as usize)
                .min(max_top);
        } else {
            self.input_scroll_top = (self.input_scroll_top + delta as usize).min(max_top);
        }
    }

    /// Wheel over input: one visual row per notch (smooth, no top↔bottom jumps).
    fn wheel_input(&mut self, dir: i32) {
        self.scroll_input(dir); // dir is ±1 from ScrollUp/Down callers after fix
    }

    /// Cross the click→drag threshold (visual cells or row change).
    fn note_input_drag_motion(&mut self, col: u16, row: u16) {
        if self.input_selecting {
            return;
        }
        let Some(origin) = self.input_drag_origin else {
            return;
        };
        let Some(now) = self.input_pos_at(col, row) else {
            return;
        };
        if now.0 != origin.0 || now.1.abs_diff(origin.1) > 1 {
            self.input_selecting = true;
        }
    }

    /// Keep the caret's **visual** row inside the input viewport after typing.
    fn ensure_input_caret_visible(&mut self) {
        let h = self.input_view_h.max(1);
        let w = self.input_usable_w.max(1);
        let (vrow, _) = self.input.cursor_visual_pos(w);
        let max_top = self.input.visual_line_count(w).saturating_sub(h);
        if vrow < self.input_scroll_top {
            self.input_scroll_top = vrow;
        } else if vrow >= self.input_scroll_top + h {
            self.input_scroll_top = vrow + 1 - h;
        }
        self.input_scroll_top = self.input_scroll_top.min(max_top);
        self.input_x_off = 0;
    }

    /// While drag-selecting, scroll one visual row at a time at the edges.
    fn maybe_autoscroll_input_while_selecting(&mut self, _col: u16, row: u16) {
        if !self.input_selecting {
            return;
        }
        let inner = self.input_inner;
        if inner.height == 0 {
            return;
        }
        // One row per event at the edge — smooth, not a leap to top/bottom.
        if row < inner.y.saturating_add(1) {
            self.scroll_input(-1);
        } else if row + 1 >= inner.bottom() {
            self.scroll_input(1);
        }
    }

    /// Map a terminal (column, row) click onto the input buffer caret.
    #[allow(dead_code)]
    fn click_input(&mut self, col: u16, row: u16) {
        if let Some((vrow, dcol)) = self.input_pos_at(col, row) {
            let w = self.input_usable_w.max(1);
            let idx = self.input.index_at_visual(vrow, dcol, w);
            self.input.set_cursor_index(idx);
            self.ensure_input_caret_visible();
        }
    }

    // ── session picker (`/sessions` · `/resume`) ─────────────
    fn open_session_picker(&mut self) {
        if self.busy {
            self.push_error("wait for the current turn to finish".into());
            return;
        }
        // Lightweight summaries (no input_items) from ~/.nur + legacy ~/.muse.
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
                 (searched ~/.nur/sessions and legacy ~/.muse/sessions)"
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
        // /login clears the prior key/auth up front — a clean slate to pick a
        // provider and enter a fresh key / browser session.
        self.cancel_oauth();
        let _ = crate::auth::logout(false);
        self.authed = false;
        self.login = Some(LoginModal {
            stage: LoginStage::Provider,
            filter: String::new(),
            sel: 0,
            scroll: 0,
            vis_page: 8,
            hit: PickerHit::default(),
            last_step_at: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            provider_id: self.cfg.provider.clone(),
            method_sel: 0,
            can_import: false,
            buf: String::new(),
            error: None,
            browser_status: String::new(),
            browser_url: String::new(),
            browser_user_code: String::new(),
            oauth_rx: None,
            oauth_cancel: None,
            manage_failover: false,
            fallback_key: false,
        });
    }

    /// Open the provider picker in failover-manage mode. Unlike `/login`, this
    /// does **not** log the active provider out — it only edits the failover
    /// chain (`fallback_providers`) and per-provider keys.
    fn open_failover(&mut self) {
        self.login = Some(LoginModal {
            stage: LoginStage::Provider,
            filter: String::new(),
            sel: 0,
            scroll: 0,
            vis_page: 8,
            hit: PickerHit::default(),
            last_step_at: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            provider_id: self.cfg.provider.clone(),
            method_sel: 0,
            can_import: false,
            buf: String::new(),
            error: None,
            browser_status: String::new(),
            browser_url: String::new(),
            browser_user_code: String::new(),
            oauth_rx: None,
            oauth_cancel: None,
            manage_failover: true,
            fallback_key: false,
        });
    }

    /// Cycle the asserted privacy tier of the selected provider and persist it
    /// as an override. Cycling back to the built-in default removes the override.
    fn cycle_privacy_selected(&mut self) {
        let provider = {
            let Some(m) = &self.login else { return };
            if m.stage != LoginStage::Provider {
                return;
            }
            match m.filtered().get(m.sel) {
                Some(p) => **p,
                None => return,
            }
        };
        let cur = crate::providers::effective_privacy(&self.cfg.provider_privacy, provider.id);
        let next = cur.next();
        if next == crate::providers::builtin_privacy(provider.id) {
            self.cfg.provider_privacy.remove(provider.id);
        } else {
            self.cfg
                .provider_privacy
                .insert(provider.id.to_string(), next.as_str().to_string());
        }
        let _ = crate::config::save_config(&self.cfg);
    }

    /// Toggle the currently-selected provider in the failover chain. When newly
    /// added without resolvable credentials, capture them now — Method stage
    /// (browser / key / import) for OAuth-capable providers, Key stage otherwise.
    fn toggle_fallback_selected(&mut self) {
        let provider = {
            let Some(m) = &self.login else { return };
            if m.stage != LoginStage::Provider {
                return;
            }
            let picks = m.filtered();
            match picks.get(m.sel) {
                Some(p) => **p,
                None => return,
            }
        };
        let id = provider.id.to_string();
        if id == self.cfg.provider {
            if let Some(m) = &mut self.login {
                m.error = Some(format!("{} is your active provider", provider.name));
            }
            return;
        }
        let present = self.cfg.fallback_providers.iter().any(|x| x == &id);
        if present {
            self.cfg.fallback_providers.retain(|x| x != &id);
        } else {
            self.cfg.fallback_providers.push(id.clone());
        }
        let _ = crate::config::save_config(&self.cfg);
        if let Some(m) = &mut self.login {
            m.error = None;
        }
        // Newly added and no credentials yet → capture key and/or OAuth now.
        if !present && crate::api::failover::resolve_target_key(&provider).is_none() {
            if let Some(m) = &mut self.login {
                m.provider_id = id;
                m.buf.clear();
                m.fallback_key = true;
                m.error = None;
                if provider.browser_auth {
                    m.can_import = crate::oauth::import_existing_session(provider.id)
                        .ok()
                        .flatten()
                        .is_some();
                    m.method_sel = 0;
                    m.stage = LoginStage::Method;
                } else {
                    m.stage = LoginStage::Key;
                }
            }
        }
    }

    /// Finish a failover-only credential capture (key or OAuth) and return to
    /// the manage-failover picker without switching the active provider.
    fn finish_fallback_credential(&mut self, note: impl Into<String>) {
        if let Some(m) = &mut self.login {
            m.fallback_key = false;
            m.buf.clear();
            m.error = None;
            m.browser_status.clear();
            m.browser_url.clear();
            m.browser_user_code.clear();
            m.oauth_rx = None;
            m.oauth_cancel = None;
            m.stage = LoginStage::Provider;
            m.manage_failover = true;
        }
        self.push_info(note.into());
    }

    /// Open the `/model` chooser and kick off a live model-list fetch for the
    /// active provider in the background (same thread+channel pattern as OAuth).
    pub fn open_model_picker(&mut self) {
        let provider = crate::providers::by_id(&self.cfg.provider)
            .copied()
            .unwrap_or(*crate::providers::default_provider());
        let base_url = self.cfg.base_url.clone();
        // Resolve against the *active* provider so OAuth tokens refresh and
        // catalog env keys (TINKER_API_KEY, XAI_API_KEY, …) are picked up —
        // not a stale generic NUR_API_KEY or empty string.
        let key = crate::auth::resolve_api_key_for(Some(provider.id)).unwrap_or_default();
        let pid = provider.id.to_string();

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(crate::api::fetch_model_ids(&base_url, &key, Some(&pid)));
        });

        self.model_picker = Some(ModelPicker {
            provider_name: provider.name.to_string(),
            models: Vec::new(),
            current: self.cfg.model.clone(),
            filter: String::new(),
            sel: 0,
            scroll: 0,
            vis_page: 12,
            hit: PickerHit::default(),
            last_step_at: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            loading: true,
            error: None,
            rx: Some(rx),
        });
    }

    /// Commit a model switch (from the picker or `/model <id>`): update config,
    /// session, and usage meter, then persist. Returns the id set.
    pub fn apply_model_selection(&mut self, id: &str) {
        let id = id.trim();
        if id.is_empty() {
            return;
        }
        // Anthropic: rewrite retired/short ids so a saved `claude-sonnet-4-…`
        // does not 404 on the first-party Claude API (key or OAuth).
        let id = if self.cfg.provider == "anthropic"
            || self.cfg.base_url.contains("api.anthropic.com")
        {
            crate::api::anthropic::normalize_model_id(id)
        } else {
            id.to_string()
        };
        self.cfg.model = id.clone();
        let _ = crate::config::save_config(&self.cfg);
        if let Some(s) = &mut self.session {
            s.model = id.clone();
        }
        if let Some(u) = &mut self.usage {
            u.set_model(id.to_string());
        }
        self.push_info(format!("model → {id}"));
    }

    /// Drain the background model-list fetch while the picker is open.
    pub fn poll_model_picker(&mut self) {
        let mut result = None;
        if let Some(mp) = &self.model_picker {
            if let Some(rx) = &mp.rx {
                if let Ok(r) = rx.try_recv() {
                    result = Some(r);
                }
            }
        }
        let Some(result) = result else { return };
        if let Some(mp) = &mut self.model_picker {
            mp.loading = false;
            mp.rx = None;
            match result {
                Ok(ids) => {
                    // Land the cursor on the current model if it's in the list.
                    let cur_idx = ids.iter().position(|m| m == &mp.current);
                    mp.models = ids;
                    mp.error = None;
                    if let Some(i) = cur_idx {
                        mp.set_idx(i);
                    }
                }
                Err(e) => {
                    mp.error = Some(e);
                }
            }
            mp.clamp_scroll();
        }
    }

    fn on_model_picker_key(&mut self, key: event::KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.model_picker = None;
            }
            KeyCode::Enter => {
                let chosen = self.model_picker.as_ref().and_then(|m| m.chosen());
                if let Some(id) = chosen {
                    self.model_picker = None;
                    self.apply_model_selection(&id);
                }
            }
            KeyCode::Up => {
                if let Some(m) = &mut self.model_picker {
                    m.step(-1);
                }
            }
            KeyCode::Down => {
                if let Some(m) = &mut self.model_picker {
                    m.step(1);
                }
            }
            KeyCode::PageUp => {
                if let Some(m) = &mut self.model_picker {
                    let page = m.vis_page.max(1) as i32;
                    for _ in 0..page {
                        m.step(-1);
                    }
                }
            }
            KeyCode::PageDown => {
                if let Some(m) = &mut self.model_picker {
                    let page = m.vis_page.max(1) as i32;
                    for _ in 0..page {
                        m.step(1);
                    }
                }
            }
            KeyCode::Home => {
                if let Some(m) = &mut self.model_picker {
                    m.sel = 0;
                    m.scroll = 0;
                }
            }
            KeyCode::End => {
                if let Some(m) = &mut self.model_picker {
                    let count = m.count();
                    if count > 0 {
                        m.set_idx(count - 1);
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(m) = &mut self.model_picker {
                    m.filter.pop();
                    m.sel = 0;
                    m.scroll = 0;
                    m.clamp_scroll();
                }
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(m) = &mut self.model_picker {
                    m.filter.clear();
                    m.sel = 0;
                    m.scroll = 0;
                    m.clamp_scroll();
                }
            }
            KeyCode::Char(c) if !ctrl && !c.is_control() => {
                if let Some(m) = &mut self.model_picker {
                    m.filter.push(c);
                    m.sel = 0;
                    m.scroll = 0;
                    m.clamp_scroll();
                }
            }
            _ => {}
        }
    }

    /// Mouse while the model picker is open — wheel scrolls, click row selects,
    /// second click / Enter confirms, click ✕ closes.
    fn on_model_picker_mouse(&mut self, m: event::MouseEvent) {
        self.mouse_col = m.column;
        self.mouse_row = m.row;
        match m.kind {
            MouseEventKind::ScrollUp => {
                if let Some(mp) = &mut self.model_picker {
                    mp.wheel_step(-1);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(mp) = &mut self.model_picker {
                    mp.wheel_step(1);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let hit = self.model_picker.as_ref().map(|mp| mp.hit.clone());
                let Some(hit) = hit else { return };
                let (col, row) = (m.column, m.row);
                if rect_contains(hit.close, col, row) {
                    self.model_picker = None;
                    return;
                }
                for (i, rect) in &hit.rows {
                    if rect_contains(*rect, col, row) {
                        let same = self
                            .model_picker
                            .as_ref()
                            .map(|mp| mp.sel == *i)
                            .unwrap_or(false);
                        if let Some(mp) = &mut self.model_picker {
                            mp.set_idx(*i);
                        }
                        if same {
                            let chosen =
                                self.model_picker.as_ref().and_then(|mp| mp.chosen());
                            if let Some(id) = chosen {
                                self.model_picker = None;
                                self.apply_model_selection(&id);
                            }
                        }
                        return;
                    }
                }
            }
            _ => {}
        }
    }

    /// Open the `/plugins` marketplace picker (provider-picker UX).
    pub fn open_plugin_picker(&mut self) {
        self.plugin_picker = Some(PluginPicker {
            rows: crate::plugins::marketplace_rows(),
            filter: String::new(),
            sel: 0,
            scroll: 0,
            vis_page: 12,
            hit: PickerHit::default(),
            last_step_at: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
            busy: false,
            status: Some("↵ install / enable / disable  ·  skills land in ~/.nur/skills".into()),
            rx: None,
        });
    }

    /// Drain background plugin install/toggle results.
    pub fn poll_plugin_picker(&mut self) {
        let mut result = None;
        if let Some(pp) = &self.plugin_picker {
            if let Some(rx) = &pp.rx {
                if let Ok(r) = rx.try_recv() {
                    result = Some(r);
                }
            }
        }
        let Some(result) = result else { return };
        if let Some(pp) = &mut self.plugin_picker {
            pp.busy = false;
            pp.rx = None;
            match result {
                Ok(msg) => {
                    pp.status = Some(msg.clone());
                    pp.refresh_rows();
                    self.push_info(msg);
                }
                Err(e) => {
                    pp.status = Some(format!("error: {e}"));
                    self.push_error(e);
                }
            }
        }
    }

    /// Enter on a marketplace row: install if missing, else toggle enable.
    pub fn activate_plugin_selection(&mut self) {
        let Some(pp) = &self.plugin_picker else { return };
        if pp.busy {
            return;
        }
        let Some(row) = pp.filtered().get(pp.sel).map(|r| (*r).clone()) else {
            return;
        };
        // Clone fields we need after we mutably borrow picker again.
        let id = row.id.clone();
        let installed = row.installed;
        let enabled = row.enabled;
        let name = row.name.clone();

        if let Some(pp) = &mut self.plugin_picker {
            pp.busy = true;
            pp.status = Some(if !installed {
                format!("installing {name}…")
            } else if enabled {
                format!("disabling {name}…")
            } else {
                format!("enabling {name}…")
            });
        }

        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = if !installed {
                crate::plugins::install_plugin(&id)
            } else if enabled {
                crate::plugins::set_enabled(&id, false)
                    .map(|_| format!("disabled {name} (skills stay on disk; re-enable anytime)"))
            } else {
                crate::plugins::set_enabled(&id, true).map(|_| {
                    format!("enabled {name} — skills active on next agent turn")
                })
            };
            let _ = tx.send(res);
        });
        if let Some(pp) = &mut self.plugin_picker {
            pp.rx = Some(rx);
        }
    }

    fn on_plugin_picker_key(&mut self, key: event::KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => {
                self.plugin_picker = None;
            }
            KeyCode::Enter => {
                self.activate_plugin_selection();
            }
            KeyCode::Up => {
                if let Some(m) = &mut self.plugin_picker {
                    m.step(-1);
                }
            }
            KeyCode::Down => {
                if let Some(m) = &mut self.plugin_picker {
                    m.step(1);
                }
            }
            KeyCode::PageUp => {
                if let Some(m) = &mut self.plugin_picker {
                    let page = m.vis_page.max(1) as i32;
                    for _ in 0..page {
                        m.step(-1);
                    }
                }
            }
            KeyCode::PageDown => {
                if let Some(m) = &mut self.plugin_picker {
                    let page = m.vis_page.max(1) as i32;
                    for _ in 0..page {
                        m.step(1);
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(m) = &mut self.plugin_picker {
                    m.filter.pop();
                    m.sel = 0;
                    m.scroll = 0;
                    m.clamp_scroll();
                }
            }
            KeyCode::Char('u') if ctrl => {
                if let Some(m) = &mut self.plugin_picker {
                    m.filter.clear();
                    m.sel = 0;
                    m.scroll = 0;
                    m.clamp_scroll();
                }
            }
            KeyCode::Char(c) if !ctrl => {
                if let Some(m) = &mut self.plugin_picker {
                    // Ignore input while installing.
                    if m.busy {
                        return;
                    }
                    m.filter.push(c);
                    m.sel = 0;
                    m.scroll = 0;
                    m.clamp_scroll();
                }
            }
            _ => {}
        }
    }

    fn on_plugin_picker_mouse(&mut self, m: event::MouseEvent) {
        self.mouse_col = m.column;
        self.mouse_row = m.row;
        match m.kind {
            MouseEventKind::ScrollUp => {
                if let Some(mp) = &mut self.plugin_picker {
                    mp.wheel_step(-1);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(mp) = &mut self.plugin_picker {
                    mp.wheel_step(1);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let hit = self.plugin_picker.as_ref().map(|mp| mp.hit.clone());
                let Some(hit) = hit else { return };
                let (col, row) = (m.column, m.row);
                if rect_contains(hit.close, col, row) {
                    self.plugin_picker = None;
                    return;
                }
                for (i, rect) in &hit.rows {
                    if rect_contains(*rect, col, row) {
                        let same = self
                            .plugin_picker
                            .as_ref()
                            .map(|mp| mp.sel == *i)
                            .unwrap_or(false);
                        if let Some(mp) = &mut self.plugin_picker {
                            mp.set_idx(*i);
                        }
                        if same {
                            self.activate_plugin_selection();
                        }
                        return;
                    }
                }
            }
            _ => {}
        }
    }

    fn cancel_oauth(&mut self) {
        if let Some(m) = &self.login {
            if let Some(c) = &m.oauth_cancel {
                c.cancel();
            }
        }
        if let Some(m) = &mut self.login {
            m.oauth_rx = None;
            m.oauth_cancel = None;
        }
    }

    fn on_login_key(&mut self, key: event::KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let stage = self.login.as_ref().map(|m| m.stage);
        match stage {
            Some(LoginStage::Provider) => self.on_login_picker_key(key, ctrl),
            Some(LoginStage::Method) => self.on_login_method_key(key, ctrl),
            Some(LoginStage::Key) => self.on_login_key_entry(key, ctrl),
            Some(LoginStage::Browser) => self.on_login_browser_key(key),
            None => {}
        }
    }

    fn login_picker_confirm(&mut self) {
        let Some(m) = &mut self.login else { return };
        if m.stage != LoginStage::Provider {
            return;
        }
        let picks = m.filtered();
        if let Some(p) = picks.get(m.sel) {
            m.provider_id = p.id.to_string();
            m.error = None;
            m.buf.clear();
            if p.browser_auth {
                m.can_import = crate::oauth::import_existing_session(p.id)
                    .ok()
                    .flatten()
                    .is_some();
                m.method_sel = 0;
                m.stage = LoginStage::Method;
            } else {
                m.stage = LoginStage::Key;
            }
        }
    }

    fn method_option_count(m: &LoginModal) -> usize {
        if m.can_import {
            3
        } else {
            2
        }
    }

    fn on_login_method_key(&mut self, key: event::KeyEvent, _ctrl: bool) {
        let Some(m) = &mut self.login else { return };
        let n = Self::method_option_count(m);
        match key.code {
            KeyCode::Esc => {
                // Failover capture: cancel credential step, stay in manage mode.
                if m.fallback_key {
                    m.fallback_key = false;
                }
                m.stage = LoginStage::Provider;
                m.error = None;
            }
            KeyCode::Up => {
                if m.method_sel > 0 {
                    m.method_sel -= 1;
                }
            }
            KeyCode::Down => {
                if m.method_sel + 1 < n {
                    m.method_sel += 1;
                }
            }
            KeyCode::Enter => self.login_method_confirm(),
            KeyCode::Char('1') => {
                m.method_sel = 0;
                self.login_method_confirm();
            }
            KeyCode::Char('2') => {
                m.method_sel = 1.min(n.saturating_sub(1));
                self.login_method_confirm();
            }
            KeyCode::Char('3') if n >= 3 => {
                m.method_sel = 2;
                self.login_method_confirm();
            }
            _ => {}
        }
    }

    fn login_method_confirm(&mut self) {
        let (provider_id, sel, can_import, is_fallback) = match &self.login {
            Some(m) => (
                m.provider_id.clone(),
                m.method_sel,
                m.can_import,
                m.fallback_key,
            ),
            None => return,
        };
        match sel {
            0 => self.start_browser_login(&provider_id),
            1 => {
                if let Some(m) = &mut self.login {
                    m.stage = LoginStage::Key;
                    m.buf.clear();
                    m.error = None;
                }
            }
            2 if can_import => {
                match crate::oauth::import_existing_session(&provider_id) {
                    Ok(Some(tokens)) => {
                        if is_fallback {
                            if let Err(e) = crate::auth::save_provider_oauth(
                                &provider_id,
                                &tokens.access_token,
                                tokens.refresh_token,
                                tokens.expires_at,
                                tokens.meta,
                            ) {
                                if let Some(m) = &mut self.login {
                                    m.error = Some(e.to_string());
                                }
                                return;
                            }
                            let name = crate::providers::by_id(&provider_id)
                                .map(|p| p.name)
                                .unwrap_or(provider_id.as_str());
                            self.finish_fallback_credential(format!(
                                "failover · {name} · browser session saved"
                            ));
                            return;
                        }
                        if let Err(e) = crate::auth::save_oauth_session(
                            &provider_id,
                            &tokens.access_token,
                            tokens.refresh_token,
                            tokens.expires_at,
                            tokens.meta,
                        ) {
                            if let Some(m) = &mut self.login {
                                m.error = Some(e.to_string());
                            }
                            return;
                        }
                        self.apply_provider_login(&provider_id, &{
                            crate::auth::resolve_api_key().unwrap_or_default()
                        }, true);
                    }
                    Ok(None) => {
                        if let Some(m) = &mut self.login {
                            m.error = Some("no existing session found".into());
                        }
                    }
                    Err(e) => {
                        if let Some(m) = &mut self.login {
                            m.error = Some(e.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn start_browser_login(&mut self, provider_id: &str) {
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = crate::oauth::CancelFlag::new();
        let cancel_bg = cancel.clone();
        let pid = provider_id.to_string();
        if let Some(m) = &mut self.login {
            m.stage = LoginStage::Browser;
            m.browser_status = "starting browser sign-in…".into();
            m.browser_url.clear();
            m.browser_user_code.clear();
            m.error = None;
            m.oauth_rx = Some(rx);
            m.oauth_cancel = Some(cancel);
        }
        std::thread::spawn(move || {
            crate::oauth::login_browser(&pid, tx, cancel_bg);
        });
    }

    fn on_login_browser_key(&mut self, key: event::KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.cancel_oauth();
                if let Some(m) = &mut self.login {
                    m.stage = LoginStage::Method;
                    m.error = None;
                    m.browser_status.clear();
                }
            }
            KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.cancel_oauth();
                if let Some(m) = &mut self.login {
                    m.stage = LoginStage::Method;
                }
            }
            _ => {}
        }
    }

    /// Drain OAuth progress while the browser stage is open (called from main loop).
    pub fn poll_oauth_login(&mut self) {
        let Some(stage) = self.login.as_ref().map(|m| m.stage) else {
            return;
        };
        if stage != LoginStage::Browser {
            return;
        }
        let mut events = Vec::new();
        if let Some(m) = &self.login {
            if let Some(rx) = &m.oauth_rx {
                while let Ok(ev) = rx.try_recv() {
                    events.push(ev);
                }
            }
        }
        for ev in events {
            match ev {
                crate::oauth::BrowserLoginProgress::Status(s) => {
                    if let Some(m) = &mut self.login {
                        m.browser_status = s;
                    }
                }
                crate::oauth::BrowserLoginProgress::DeviceCode {
                    verification_url,
                    user_code,
                } => {
                    if let Some(m) = &mut self.login {
                        m.browser_url = verification_url;
                        m.browser_user_code = user_code;
                        m.browser_status = "open the URL and enter the code".into();
                    }
                }
                crate::oauth::BrowserLoginProgress::OpenUrl(url) => {
                    if let Some(m) = &mut self.login {
                        m.browser_url = url;
                        m.browser_status = "complete sign-in in your browser…".into();
                    }
                }
                crate::oauth::BrowserLoginProgress::Done(tokens) => {
                    let (provider_id, is_fallback) = self
                        .login
                        .as_ref()
                        .map(|m| (m.provider_id.clone(), m.fallback_key))
                        .unwrap_or_default();
                    let access = tokens.access_token.clone();
                    if let Some(m) = &mut self.login {
                        m.oauth_rx = None;
                        m.oauth_cancel = None;
                    }
                    if is_fallback {
                        // Failover-only: stash OAuth for this provider, stay on
                        // the manage picker — do not switch active login.
                        if let Err(e) = crate::auth::save_provider_oauth(
                            &provider_id,
                            &tokens.access_token,
                            tokens.refresh_token,
                            tokens.expires_at,
                            tokens.meta,
                        ) {
                            if let Some(m) = &mut self.login {
                                m.error = Some(e.to_string());
                                m.stage = LoginStage::Method;
                            }
                            continue;
                        }
                        let name = crate::providers::by_id(&provider_id)
                            .map(|p| p.name)
                            .unwrap_or(provider_id.as_str());
                        self.finish_fallback_credential(format!(
                            "failover · {name} · browser session saved"
                        ));
                    } else {
                        // Active login: save_oauth_session dual-writes the
                        // per-provider store for later failover use.
                        if let Err(e) = crate::auth::save_oauth_session(
                            &provider_id,
                            &tokens.access_token,
                            tokens.refresh_token.clone(),
                            tokens.expires_at,
                            tokens.meta.clone(),
                        ) {
                            if let Some(m) = &mut self.login {
                                m.error = Some(e.to_string());
                                m.stage = LoginStage::Method;
                            }
                            continue;
                        }
                        self.apply_provider_login(&provider_id, &access, true);
                    }
                }
                crate::oauth::BrowserLoginProgress::Failed(err) => {
                    if let Some(m) = &mut self.login {
                        m.error = Some(err);
                        m.browser_status = "sign-in failed".into();
                        m.oauth_rx = None;
                        m.oauth_cancel = None;
                        // Stay on browser stage so user can read the error, or back to method.
                        m.stage = LoginStage::Method;
                    }
                }
            }
        }
    }

    /// Mouse while the provider picker is open — same contract as `on_picker_mouse`:
    /// one-entry wheel, click row to select, second click / Enter confirms.
    fn on_login_mouse(&mut self, m: event::MouseEvent) {
        self.mouse_col = m.column;
        self.mouse_row = m.row;
        let stage = self.login.as_ref().map(|l| l.stage);
        // Only the provider list is mouse-driven for now.
        if stage != Some(LoginStage::Provider) {
            return;
        }
        match m.kind {
            MouseEventKind::ScrollUp => {
                if let Some(l) = &mut self.login {
                    l.wheel_step(-1);
                }
            }
            MouseEventKind::ScrollDown => {
                if let Some(l) = &mut self.login {
                    l.wheel_step(1);
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                let Some(l) = &self.login else { return };
                let hit = l.hit.clone();
                let col = m.column;
                let row = m.row;
                if rect_contains(hit.close, col, row) {
                    self.login = None;
                    return;
                }
                for (i, r) in &hit.rows {
                    if rect_contains(*r, col, row) {
                        let same = self.login.as_ref().map(|l| l.sel == *i).unwrap_or(false);
                        if let Some(l) = &mut self.login {
                            l.set_idx(*i);
                        }
                        if same {
                            self.login_picker_confirm();
                        }
                        return;
                    }
                }
                if !rect_contains(hit.frame, col, row) {
                    self.login = None;
                }
            }
            _ => {}
        }
    }

    fn on_login_picker_key(&mut self, key: event::KeyEvent, ctrl: bool) {
        let manage = self.login.as_ref().map(|m| m.manage_failover).unwrap_or(false);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let Some(m) = &mut self.login else { return };
        match key.code {
            KeyCode::Esc => self.login = None,
            // One entry per key — same `step` path the wheel uses (sessions picker).
            // Arrows only: j/k stay available for type-to-filter.
            KeyCode::Up => m.step(-1),
            KeyCode::Down => m.step(1),
            KeyCode::PageUp => {
                let n = m.vis_page.max(1) as i32;
                for _ in 0..n {
                    m.step(-1);
                }
            }
            KeyCode::PageDown => {
                let n = m.vis_page.max(1) as i32;
                for _ in 0..n {
                    m.step(1);
                }
            }
            KeyCode::Home => {
                m.sel = 0;
                m.scroll = 0;
            }
            KeyCode::End => {
                let count = m.count();
                if count > 0 {
                    m.sel = count - 1;
                    m.clamp_scroll();
                }
            }
            KeyCode::Enter => {
                if manage {
                    self.toggle_fallback_selected();
                } else {
                    self.login_picker_confirm();
                }
            }
            // Space / Tab toggle the selected provider in the failover chain.
            KeyCode::Char(' ') | KeyCode::Tab => self.toggle_fallback_selected(),
            // Alt+P cycles the asserted privacy tier of the selected provider
            // (Standard → ZDR → TEE → Local → …), saved as an override. Alt, not
            // Ctrl — Ctrl+P is taken by many terminals/ADEs.
            KeyCode::Char('p') if alt => self.cycle_privacy_selected(),
            KeyCode::Backspace => {
                m.filter.pop();
                m.sel = 0;
                m.scroll = 0;
                m.clamp_scroll();
            }
            KeyCode::Char('u') if ctrl => {
                m.filter.clear();
                m.sel = 0;
                m.scroll = 0;
            }
            KeyCode::Char(c) if !ctrl && !alt && !c.is_control() => {
                m.filter.push(c);
                m.sel = 0;
                m.scroll = 0;
            }
            _ => {}
        }
    }

    fn on_login_key_entry(&mut self, key: event::KeyEvent, ctrl: bool) {
        let Some(m) = &mut self.login else { return };
        match key.code {
            // Esc backs up: key → method (if browser) → provider (failover keeps manage mode).
            KeyCode::Esc => {
                let browser = crate::providers::by_id(&m.provider_id)
                    .map(|p| p.browser_auth)
                    .unwrap_or(false);
                if browser {
                    // Keep fallback_key so Method still knows this is failover capture.
                    m.stage = LoginStage::Method;
                } else if m.fallback_key {
                    m.fallback_key = false;
                    m.stage = LoginStage::Provider;
                } else {
                    m.stage = LoginStage::Provider;
                }
                m.buf.clear();
                m.error = None;
            }
            KeyCode::Enter => self.submit_login(),
            KeyCode::Backspace => {
                m.buf.pop();
            }
            KeyCode::Char('v') if ctrl => {
                if let Some(t) = clipboard_get() {
                    m.buf.push_str(t.trim());
                }
            }
            KeyCode::Char('u') if ctrl => m.buf.clear(),
            KeyCode::Char(c) if !ctrl && !c.is_control() => m.buf.push(c),
            _ => {}
        }
    }

    fn submit_login(&mut self) {
        let (provider_id, key, is_fallback) = match &self.login {
            Some(m) => (m.provider_id.clone(), m.buf.trim().to_string(), m.fallback_key),
            None => return,
        };
        let provider = crate::providers::by_id(&provider_id)
            .copied()
            .unwrap_or(*crate::providers::default_provider());

        if key.is_empty() && !provider.key_optional {
            if let Some(m) = &mut self.login {
                m.error = Some(format!("{} needs an API key", provider.name));
            }
            return;
        }

        // Failover key: save to the per-provider store and return to the picker
        // — do NOT switch the active provider.
        if is_fallback {
            if !key.is_empty() {
                if let Err(e) = crate::auth::save_provider_key(&provider_id, &key) {
                    if let Some(m) = &mut self.login {
                        m.error = Some(e.to_string());
                    }
                    return;
                }
            }
            if let Some(m) = &mut self.login {
                m.fallback_key = false;
                m.buf.clear();
                m.error = None;
                m.stage = LoginStage::Provider;
            }
            return;
        }

        // Persist the key tagged to this provider (prevents cross-provider reuse).
        if !key.is_empty() {
            if let Err(e) = crate::auth::save_api_key_for(&key, Some(&provider_id)) {
                if let Some(m) = &mut self.login {
                    m.error = Some(e.to_string());
                }
                return;
            }
        }

        self.apply_provider_login(&provider_id, &key, false);
    }

    /// Apply provider config + hot-swap HTTP client after key or OAuth success.
    fn apply_provider_login(&mut self, provider_id: &str, key: &str, via_oauth: bool) {
        let provider = crate::providers::by_id(provider_id)
            .copied()
            .unwrap_or(*crate::providers::default_provider());

        self.cfg.provider = provider.id.to_string();
        let fixed_oauth_base = via_oauth
            .then(|| crate::providers::oauth_base_url(provider.id))
            .flatten();
        self.cfg.base_url = fixed_oauth_base
            .unwrap_or(provider.base_url)
            .to_string();
        self.cfg.model = if via_oauth && provider.id == "xai" {
            "grok-4.5".to_string()
        } else {
            provider.default_model.to_string()
        };
        // Self-hosted overrides apply only when the access token is not bound
        // to a first-party OAuth inference backend.
        if fixed_oauth_base.is_none() {
            crate::config::apply_base_url_env(&mut self.cfg);
        }
        if via_oauth {
            if let Ok(ids) = crate::api::models::fetch_model_ids(
                &self.cfg.base_url,
                key,
                Some(provider.id),
            ) {
                if !ids.iter().any(|id| id == &self.cfg.model) {
                    let usable = ids.iter().rev().find(|id| {
                        let id = id.to_ascii_lowercase();
                        !["embedding", "image", "audio", "realtime", "transcribe", "tts"]
                            .iter()
                            .any(|kind| id.contains(kind))
                    });
                    if let Some(model) = usable.or_else(|| ids.last()) {
                        self.cfg.model = model.clone();
                    }
                }
            }
        }
        let _ = crate::config::save_config(&self.cfg);
        if let Some(s) = &mut self.session {
            s.model = self.cfg.model.clone();
        }
        if let Some(u) = &mut self.usage {
            u.set_model(self.cfg.model.clone());
        }

        let bearer = if key.is_empty() {
            crate::auth::resolve_api_key_for(Some(provider_id)).unwrap_or_default()
        } else {
            key.to_string()
        };
        match crate::api::ApiClient::for_provider(&self.cfg.base_url, &bearer, provider.id)
            .map(|c| c.with_style(provider.style))
        {
            Ok(client) => {
                self.client = client;
                self.authed = !bearer.is_empty() || provider.key_optional;
                self.login = None;
                let keynote = if bearer.is_empty() {
                    "no key (local)".to_string()
                } else if via_oauth {
                    format!("browser · {}", crate::auth::key_fingerprint(&bearer))
                } else {
                    format!("key {}", crate::auth::key_fingerprint(&bearer))
                };
                self.push_note(
                    Tone::Mode,
                    format!(
                        "signed in · {} · {keynote}\n  model {}  ·  {}",
                        provider.name, self.cfg.model, self.cfg.base_url,
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
        // `##` markdown heading) is saved to ~/.nur/memory.md without a turn.
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
            // Transcript row with clickable send now / dismiss (not just a note).
            self.cells.push(Cell::Queued {
                text: text.clone(),
            });
            self.scroll_to_bottom();
            self.push_note(
                Tone::Mode,
                format!(
                    "queued · {} waiting · click send now to interject next (or after cancel)",
                    self.queue.len()
                ),
            );
            return;
        }
        self.start_turn(&text);
    }

    /// Send a queued follow-up now: pull it out of the queue, drop its card, and
    /// either start a turn (idle) or interrupt + queue front (busy) so it
    /// becomes the next message with full prior context.
    fn queue_send_now(&mut self, cell_idx: usize) {
        let text = match self.cells.get(cell_idx) {
            Some(Cell::Queued { text }) => text.clone(),
            _ => return,
        };
        // Remove this occurrence from the queue (first match).
        if let Some(i) = self.queue.iter().position(|t| t == &text) {
            self.queue.remove(i);
        }
        // Drop the Queued card from the transcript.
        if cell_idx < self.cells.len() {
            self.cells.remove(cell_idx);
        }
        if self.busy {
            // Interrupt current turn; keep this follow-up at the front so Done
            // starts it next (full session context already on disk).
            self.queue.push_front(text.clone());
            self.preserve_queue_on_interrupt = true;
            self.interrupt();
            self.push_note(
                Tone::Mode,
                "send now — cancelling current turn; this follow-up goes next with full context"
                    .into(),
            );
        } else {
            self.start_turn(&text);
        }
    }

    /// Drop a queued follow-up without sending.
    fn queue_dismiss(&mut self, cell_idx: usize) {
        let text = match self.cells.get(cell_idx) {
            Some(Cell::Queued { text }) => text.clone(),
            _ => return,
        };
        if let Some(i) = self.queue.iter().position(|t| t == &text) {
            self.queue.remove(i);
        }
        if cell_idx < self.cells.len() {
            self.cells.remove(cell_idx);
        }
        self.push_note(
            Tone::Neutral,
            format!("dismissed follow-up · {} still queued", self.queue.len()),
        );
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
        // The model sees the standing goal + any one-off `/btw` notes prepended
        // as context; the transcript above shows only the plain prompt.
        let mut effective = String::new();
        if let Some(g) = &self.session_goal {
            effective.push_str(&format!("[session goal] {g}\n\n"));
        }
        for note in self.pending_btw.drain(..) {
            effective.push_str(&format!("[note] {note}\n\n"));
        }
        effective.push_str(prompt);
        agent::spawn_turn(
            runner,
            *session,
            *usage,
            effective,
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
                self.save_session_with_ui_log();
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
                // Drop queued prompts after cancel so we don't surprise-run them —
                // unless send-now asked to preserve the queue for interjection.
                if interrupted && !self.preserve_queue_on_interrupt {
                    self.queue.clear();
                    self.cells
                        .retain(|c| !matches!(c, Cell::Queued { .. }));
                } else if let Some(next) = self.queue.pop_front() {
                    self.preserve_queue_on_interrupt = false;
                    // Drop matching Queued cards for this text.
                    let next_clone = next.clone();
                    self.cells.retain(|c| match c {
                        Cell::Queued { text } => text != &next_clone,
                        _ => true,
                    });
                    self.submit_text(&next);
                } else {
                    self.preserve_queue_on_interrupt = false;
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
    /// - Chevron (left ~3 cols on a header): expand/collapse.
    /// - Exact `"click to peek"` text span only: open stable dialogue.
    /// - `http(s)://…` text spans: open in the OS default browser.
    /// - Closing the dialogue is **never** done here — only Esc / outside / ✕.
    fn click_transcript(&mut self, col: u16, row: u16) {
        let body = self.transcript_body;
        if body.width == 0 || body.height == 0 {
            return;
        }
        if col < body.x || col >= body.right() || row < body.y || row >= body.bottom() {
            return;
        }
        // While a stable peek is open, transcript clicks do nothing (outside
        // handling already ran in on_mouse and may have closed the box).
        if self.peek_open.is_some() {
            return;
        }
        let local_y = row.saturating_sub(body.y) as usize;
        // body.x already includes the 1-col left margin from draw_transcript.
        let local_x = col.saturating_sub(body.x) as usize;
        let line_idx = self.transcript_top as usize + local_y;

        // Queued follow-up: send now / dismiss (both may sit on the same line).
        if let Some(actions) = self.hit_queue_actions.get(line_idx) {
            for (cell_idx, lo, hi, action) in actions {
                if local_x >= *lo && local_x < *hi {
                    match action {
                        0 => self.queue_send_now(*cell_idx),
                        1 => self.queue_dismiss(*cell_idx),
                        _ => {}
                    }
                    return;
                }
            }
        }

        // Prefer opening URLs over expand/peek when the pointer is on a link.
        if let Some(spans) = self.hit_urls.get(line_idx) {
            for (lo, hi, url) in spans {
                if local_x >= *lo && local_x < *hi {
                    match crate::open_uri::open(url) {
                        Ok(()) => {
                            self.push_note(
                                Tone::Neutral,
                                format!("opened link · {}", truncate_url_note(url)),
                            );
                        }
                        Err(e) => {
                            self.push_note(
                                Tone::Session,
                                format!("could not open link: {e}\n  {url}"),
                            );
                        }
                    }
                    return;
                }
            }
        }

        let header = self.hit_headers.get(line_idx).copied().flatten();
        let chevron = local_x <= 3;
        // Clicking the literal "▸ expands" / "▾ collapse" text also toggles.
        let expand_phrase = self
            .hit_expand_phrase
            .get(line_idx)
            .copied()
            .flatten()
            .and_then(|(cell, lo, hi)| {
                if local_x >= lo && local_x < hi {
                    Some(cell)
                } else {
                    None
                }
            });
        let click_to_peek = self
            .hit_click_to_peek
            .get(line_idx)
            .copied()
            .flatten()
            .and_then(|(cell, lo, hi)| {
                if local_x >= lo && local_x < hi {
                    Some(cell)
                } else {
                    None
                }
            });

        // Expand-phrase wins over peek when both appear on the same header line.
        if let Some(i) = expand_phrase {
            self.toggle_cell_expand(i);
            return;
        }

        match resolve_transcript_click(chevron, header, click_to_peek) {
            TranscriptClick::ToggleExpand(i) => {
                self.toggle_cell_expand(i);
            }
            TranscriptClick::OpenPeek(i) => self.open_stable_peek(i),
            TranscriptClick::None => {}
        }
    }

    // ── helpers ────────────────────────────────────────────────────────
}

fn truncate_url_note(url: &str) -> String {
    const MAX: usize = 72;
    let n = url.chars().count();
    if n <= MAX {
        url.to_string()
    } else {
        let head: String = url.chars().take(MAX - 1).collect();
        format!("{head}…")
    }
}

impl App {
    fn replay_session_history(&mut self) {
        let Some(session) = &self.session else { return };
        if !session.ui_log.is_empty() {
            let n = session.ui_log.len();
            self.cells.push(Cell::Info {
                text: format!("history · {n} cards restored"),
                tone: Tone::Session,
            });
            for item in session.ui_log.clone() {
                if let Some(c) = ui_log_item_to_cell(&item) {
                    self.cells.push(c);
                }
            }
            return;
        }
        // Retroactive: rebuild tools (+ any reasoning summary) from input_items.
        let rebuilt = rebuild_cells_from_session(session);
        if rebuilt.is_empty() {
            let msgs: Vec<(String, String)> = session
                .messages
                .iter()
                .rev()
                .take(12)
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
            return;
        }
        self.cells.push(Cell::Info {
            text: format!(
                "history · {} messages · tools restored from session",
                session.messages.len()
            ),
            tone: Tone::Session,
        });
        self.cells.extend(rebuilt);
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

/// A coalesced paste burst flushes into one chip after this quiet gap. Small
/// enough to feel instant; large enough to stitch a PTY-split paste together.
/// 120ms gives the PTY time to deliver a wall of text that was split across
/// many frames, while still feeling instant.
const PASTE_FLUSH_MS: u64 = 120;
/// Any paste arriving within this window after a chip is created appends to
/// the same chip instead of spawning a new `[pasted 1-1]` chip. This is what
/// stops the thousand-chip spam when the terminal splits a paste.
const PASTE_MERGE_MS: u64 = 800;

/// Printable text that may be part of an unbracketed paste drip.
/// **Never** Enter or Tab — those are real keys (submit / focus), not paste.
fn key_as_paste_burst_char(key: &KeyEvent) -> Option<char> {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return None;
    }
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
    {
        return None;
    }
    match key.code {
        // Never treat CR/LF Char as paste — submit is KeyCode::Enter only.
        KeyCode::Char(c) if c != '\n' && c != '\r' => Some(c),
        _ => None,
    }
}

impl App {
    /// Handle a resolved paste. This is the **single place** where chips are
    /// created, and it implements the Claude-Code-style merge window that
    /// prevents a wall of text split across many PTY frames from becoming
    /// a thousand `[pasted 1-1 lines]` chips.
    ///
    /// - Any paste arriving within PASTE_MERGE_MS of the previous paste
    ///   appends to the same chip (or, if the previous paste was raw small
    ///   text, retroactively converts the combined text into one chip).
    /// - Small pastes stay inline unless they grow past the chip threshold
    ///   during the merge window.
    fn on_paste(&mut self, s: &str) {
        let text: String = s.chars().filter(|c| *c != '\r').collect();
        if text.is_empty() {
            return;
        }
        if let Some(m) = &mut self.login {
            // Provider stage types into the filter; key / method / browser use buf.
            if m.stage == LoginStage::Provider {
                m.filter.push_str(&text);
                m.sel = 0;
                m.scroll = 0;
                m.clamp_scroll();
            } else {
                m.buf.push_str(&text);
            }
            return;
        }
        if self.model_picker.is_some() {
            if let Some(mp) = &mut self.model_picker {
                mp.filter.push_str(&text);
                mp.sel = 0;
                mp.scroll = 0;
                mp.clamp_scroll();
            }
            return;
        }
        if self.plugin_picker.is_some() {
            if let Some(pp) = &mut self.plugin_picker {
                pp.filter.push_str(&text);
                pp.sel = 0;
                pp.scroll = 0;
                pp.clamp_scroll();
            }
            return;
        }
        if self.approval.is_some() || self.picker.is_some() {
            return;
        }

        // Reverse history search owns the keyboard — a paste extends the search
        // query, it must not leak into the stashed composer buffer.
        if self.input.search_is_active() {
            for c in text.chars() {
                self.input.search_push(c);
            }
            return;
        }

        let now = Instant::now();
        let within_merge = self
            .active_paste_at
            .map(|t| t.elapsed() < std::time::Duration::from_millis(PASTE_MERGE_MS))
            .unwrap_or(false);

        // 1) Active chip exists and we're within the merge window → append.
        if within_merge {
            if let Some(active_id) = self.active_paste_id {
                if self.input.has_paste_id(active_id) {
                    // Only merge if caret is still immediately after the active chip.
                    // This prevents a later paste at a different cursor location from
                    // growing an old chip, while still handling the PTY-split case where
                    // flushes happen back-to-back with cursor after the chip.
                    let cursor = self.input.cursor_index();
                    let mut should_merge = false;
                    if cursor > 0 {
                        if let Some(id_at) = self.input.paste_id_at(cursor - 1) {
                            if id_at == active_id {
                                should_merge = true;
                            }
                        }
                    }
                    if should_merge {
                        // Deduplicate: if the chip already ends with this exact text
                        // (can happen when both Ctrl+V and bracketed paste fire for
                        // the same clipboard), don't double-append.
                        if let Some(existing) = self.input.paste_at(cursor - 1) {
                            if existing.content.ends_with(&text) || existing.content == text {
                                return;
                            }
                        }
                        if self.input.append_to_paste(active_id, &text) {
                            self.active_paste_at = Some(now);
                            self.ensure_input_caret_visible();
                            self.palette_idx = 0;
                            self.palette_scroll = 0;
                            return;
                        }
                    }
                }
                // Active id stale (chip deleted) → fall through to new chip.
                self.active_paste_id = None;
            }

            // 2) Previous paste was raw small text, still within window.
            //    We retroactively convert combined => chip if it now qualifies,
            //    or merge raw pieces together.
            if let Some(raw_start) = self.last_raw_start {
                let raw_len = self.last_raw_len;
                // Sanity: cursor should still be at end of raw, and buffer long enough.
                // If user moved cursor or edited, we don't attempt retroactive merge.
                let cur = self.input.cursor_index();
                if cur >= raw_start + raw_len && raw_len > 0 {
                    // Delete old raw range.
                    let combined = format!("{}{}", self.last_raw_text, text);
                    let should = crate::tui::input::should_chip(&combined);
                    // Remove previous raw insertion.
                    self.input.delete_range(raw_start, raw_start + raw_len);
                    if should {
                        if let Some(id) = self.input.start_paste_chip(&combined) {
                            self.active_paste_id = Some(id);
                            self.active_paste_at = Some(now);
                            self.last_raw_start = None;
                            self.last_raw_len = 0;
                            self.last_raw_text.clear();
                            self.ensure_input_caret_visible();
                            self.palette_idx = 0;
                            self.palette_scroll = 0;
                            return;
                        }
                    } else {
                        // Still small — re-insert combined as raw and keep tracking.
                        let new_start = self.input.cursor_index();
                        self.input.insert_str(&combined);
                        self.last_raw_start = Some(new_start);
                        self.last_raw_len = combined.chars().count();
                        self.last_raw_text = combined;
                        self.active_paste_at = Some(now);
                        self.ensure_input_caret_visible();
                        self.palette_idx = 0;
                        self.palette_scroll = 0;
                        return;
                    }
                } else {
                    // Cursor moved — break raw merge chain.
                    self.last_raw_start = None;
                    self.last_raw_len = 0;
                    self.last_raw_text.clear();
                }
            }
        } else {
            // Outside merge window — clear stale raw tracker.
            self.last_raw_start = None;
            self.last_raw_len = 0;
            self.last_raw_text.clear();
            if self
                .active_paste_at
                .map(|t| t.elapsed() >= std::time::Duration::from_millis(PASTE_MERGE_MS))
                .unwrap_or(false)
            {
                self.active_paste_id = None;
            }
        }

        // 3) Fresh paste (no merge).
        if crate::tui::input::should_chip(&text) {
            if let Some(id) = self.input.start_paste_chip(&text) {
                self.active_paste_id = Some(id);
                self.active_paste_at = Some(now);
                self.last_raw_start = None;
                self.last_raw_len = 0;
                self.last_raw_text.clear();
            } else {
                // Slot exhaustion — fallback to raw but don't lose content.
                let start = self.input.cursor_index();
                self.input.insert_str(&text);
                self.last_raw_start = Some(start);
                self.last_raw_len = text.chars().count();
                self.last_raw_text = text;
                self.active_paste_id = None;
                self.active_paste_at = Some(now);
            }
        } else {
            // Small inline paste.
            // If we're still within merge window and last was raw, this path
            // was already handled above. Here it's a truly fresh small paste.
            let start = self.input.cursor_index();
            self.input.insert_str(&text);
            // Track raw for potential retroactive merge if more paste arrives quickly.
            self.last_raw_start = Some(start);
            self.last_raw_len = text.chars().count();
            self.last_raw_text = text;
            self.active_paste_id = None;
            self.active_paste_at = Some(now);
        }

        self.ensure_input_caret_visible();
        self.palette_idx = 0;
        self.palette_scroll = 0;
    }

    /// Flush the coalesced paste buffer as one paste, if any.
    fn flush_paste_accum(&mut self) {
        self.paste_accum_at = None;
        if self.paste_accum.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.paste_accum);
        self.on_paste(&text);
    }

    /// Clear the paste merge session — called when the user types / moves
    /// caret / deletes, so the next paste starts a fresh chip.
    #[allow(dead_code)]
    fn clear_paste_merge_state(&mut self) {
        self.active_paste_id = None;
        // Keep active_paste_at for a short grace? No — break immediately on edit.
        self.active_paste_at = None;
        self.last_raw_start = None;
        self.last_raw_len = 0;
        self.last_raw_text.clear();
    }
}

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


fn cells_to_ui_log(cells: &[Cell]) -> Vec<crate::agent::session::UiLogItem> {
    use crate::agent::session::UiLogItem;
    let mut out = Vec::new();
    for c in cells {
        match c {
            Cell::Banner => {}
            Cell::User(text) => out.push(UiLogItem {
                kind: "user".into(),
                text: text.clone(),
                name: None,
                args: None,
                ok: None,
                ms: None,
                thought_ms: None,
                interrupted: false,
            }),
            Cell::Assistant { text, .. } => out.push(UiLogItem {
                kind: "assistant".into(),
                text: text.clone(),
                name: None,
                args: None,
                ok: None,
                ms: None,
                thought_ms: None,
                interrupted: false,
            }),
            Cell::Thinking { text, duration, .. } => out.push(UiLogItem {
                kind: "thinking".into(),
                text: text.clone(),
                name: None,
                args: None,
                ok: None,
                ms: duration.map(|d| d.as_millis() as u64),
                thought_ms: None,
                interrupted: false,
            }),
            Cell::Tool {
                name,
                args,
                result,
                ok,
                duration,
                ..
            } => out.push(UiLogItem {
                kind: "tool".into(),
                text: result.clone().unwrap_or_default(),
                name: Some(name.clone()),
                args: Some(args.clone()),
                ok: *ok,
                ms: duration.map(|d| d.as_millis() as u64),
                thought_ms: None,
                interrupted: false,
            }),
            Cell::TurnDone {
                duration,
                thought,
                interrupted,
            } => out.push(UiLogItem {
                kind: "turn_done".into(),
                text: String::new(),
                name: None,
                args: None,
                ok: None,
                ms: Some(duration.as_millis() as u64),
                thought_ms: Some(thought.as_millis() as u64),
                interrupted: *interrupted,
            }),
            Cell::Info { text, .. } => out.push(UiLogItem {
                kind: "info".into(),
                text: text.clone(),
                name: None,
                args: None,
                ok: None,
                ms: None,
                thought_ms: None,
                interrupted: false,
            }),
            // Ephemeral — only meaningful while a turn is running.
            Cell::Queued { .. } => {}
            Cell::Error(text) => out.push(UiLogItem {
                kind: "error".into(),
                text: text.clone(),
                name: None,
                args: None,
                ok: None,
                ms: None,
                thought_ms: None,
                interrupted: false,
            }),
        }
    }
    out
}

fn ui_log_item_to_cell(item: &crate::agent::session::UiLogItem) -> Option<Cell> {
    let ms = item.ms.unwrap_or(0);
    let dur = Duration::from_millis(ms);
    match item.kind.as_str() {
        "user" => Some(Cell::User(item.text.clone())),
        "assistant" => Some(Cell::Assistant {
            text: item.text.clone(),
            streaming: false,
        }),
        "thinking" => Some(Cell::Thinking {
            text: item.text.clone(),
            active: false,
            started: Instant::now(),
            duration: if ms > 0 { Some(dur) } else { None },
            expanded: false,
        }),
        "tool" => Some(Cell::Tool {
            name: item.name.clone().unwrap_or_else(|| "tool".into()),
            args: item.args.clone().unwrap_or_else(|| "{}".into()),
            result: if item.text.is_empty() {
                None
            } else {
                Some(item.text.clone())
            },
            ok: item.ok,
            started: Instant::now(),
            duration: if ms > 0 { Some(dur) } else { None },
            expanded: false,
        }),
        "turn_done" => Some(Cell::TurnDone {
            duration: dur,
            thought: Duration::from_millis(item.thought_ms.unwrap_or(0)),
            interrupted: item.interrupted,
        }),
        "info" => Some(Cell::Info {
            text: item.text.clone(),
            tone: Tone::Session,
        }),
        "error" => Some(Cell::Error(item.text.clone())),
        _ => None,
    }
}

fn rebuild_cells_from_session(session: &Session) -> Vec<Cell> {
    let mut cells = Vec::new();
    let mut pending: std::collections::HashMap<String, (String, String)> =
        std::collections::HashMap::new();
    for it in &session.input_items {
        let ty = it
            .get("type")
            .and_then(|t| t.as_str())
            .or_else(|| it.get("role").and_then(|r| r.as_str()))
            .unwrap_or("");
        match ty {
            "user" => {
                let text = extract_item_text(it);
                if !text.is_empty() {
                    cells.push(Cell::User(text));
                }
            }
            "assistant" => {
                let text = extract_item_text(it);
                if !text.is_empty() {
                    cells.push(Cell::Assistant {
                        text,
                        streaming: false,
                    });
                }
            }
            "reasoning" => {
                let text = extract_reasoning_summary(it);
                if !text.is_empty() {
                    cells.push(Cell::Thinking {
                        text,
                        active: false,
                        started: Instant::now(),
                        duration: None,
                        expanded: false,
                    });
                }
            }
            "function_call" => {
                let call_id = it
                    .get("call_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = it
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("tool")
                    .to_string();
                let args = it
                    .get("arguments")
                    .and_then(|v| v.as_str())
                    .unwrap_or("{}")
                    .to_string();
                if !call_id.is_empty() {
                    pending.insert(call_id, (name, args));
                }
            }
            "function_call_output" => {
                let call_id = it.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                let output = it
                    .get("output")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if let Some((name, args)) = pending.remove(call_id) {
                    let ok = !output.to_lowercase().contains("cancelled");
                    cells.push(Cell::Tool {
                        name,
                        args,
                        result: Some(output),
                        ok: Some(ok),
                        started: Instant::now(),
                        duration: None,
                        expanded: false,
                    });
                }
            }
            _ => {}
        }
    }
    cells
}

fn extract_item_text(it: &serde_json::Value) -> String {
    if let Some(s) = it.get("content").and_then(|c| c.as_str()) {
        return s.to_string();
    }
    if let Some(arr) = it.get("content").and_then(|c| c.as_array()) {
        let mut parts = Vec::new();
        for p in arr {
            let ty = p.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if matches!(ty, "input_text" | "output_text" | "text") {
                if let Some(txt) = p.get("text").and_then(|t| t.as_str()) {
                    parts.push(txt.to_string());
                }
            }
        }
        return parts.join("\n");
    }
    String::new()
}

fn extract_reasoning_summary(it: &serde_json::Value) -> String {
    let mut parts = Vec::new();
    if let Some(arr) = it.get("summary").and_then(|s| s.as_array()) {
        for p in arr {
            if let Some(txt) = p.get("text").and_then(|t| t.as_str()) {
                parts.push(txt.to_string());
            } else if let Some(txt) = p.as_str() {
                parts.push(txt.to_string());
            }
        }
    }
    parts.join("\n")
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
    fn scrollbar_hit_stays_on_rail_not_transcript() {
        // Pure geometry: rail at x=78..80 must not claim col 76 (transcript).
        let track = ratatui::layout::Rect {
            x: 78,
            y: 0,
            width: 2,
            height: 20,
        };
        let left = track.x; // no overhang into transcript
        assert!(!(76 >= left && 76 < track.right()), "col 76 must miss rail");
        assert!(78 >= left && 78 < track.right(), "col 78 hits rail");
        assert!(79 >= left && 79 < track.right(), "col 79 hits rail");
        // Old bug: left = track.x - 2 would steal 76–77.
        let old_left = track.x.saturating_sub(2);
        assert!(76 >= old_left && 76 < track.right());
    }

    #[test]
    fn only_click_to_peek_text_opens_dialogue() {
        // OpenPeek only when click_to_peek_cell is Some — never from bare header.
        assert_eq!(
            resolve_transcript_click(false, Some(3), None),
            TranscriptClick::None
        );
        assert_eq!(
            resolve_transcript_click(false, Some(3), Some(3)),
            TranscriptClick::OpenPeek(3)
        );
        // Chevron expands even without click-to-peek hit.
        assert_eq!(
            resolve_transcript_click(true, Some(3), None),
            TranscriptClick::ToggleExpand(3)
        );
        // Turn strip / body without ctp hitbox: no open.
        assert_eq!(
            resolve_transcript_click(false, None, None),
            TranscriptClick::None
        );
    }

    #[test]
    fn open_peek_dismisses_on_every_side_and_close() {
        use ratatui::layout::Rect;
        let box_ = Rect::new(10, 5, 30, 12);
        let close = Rect::new(box_.x + box_.width - 4, box_.y, 3, 1);
        assert!(!peek_click_dismisses(close, box_, 20, 10));
        assert!(peek_click_dismisses(close, box_, 37, 5));
        assert!(peek_click_dismisses(close, box_, 20, 20), "below must close");
        assert!(peek_click_dismisses(close, box_, 20, 2), "above must close");
        assert!(peek_click_dismisses(close, box_, 2, 10), "left must close");
        assert!(peek_click_dismisses(close, box_, 50, 10), "right must close");
    }

    #[test]
    fn ctx_action_indices_match_labels() {
        // The confirm handler switches on these indices; keep them pinned.
        assert_eq!(CTX_ACTIONS[0].1, "Fork");
        assert_eq!(CTX_ACTIONS[1].1, "Edit");
        assert_eq!(CTX_ACTIONS[2].1, "Revert");
        assert_eq!(CTX_ACTIONS[3].1, "Copy");
        assert_eq!(CTX_ACTIONS.len(), 4);
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
