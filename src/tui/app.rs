//! Interactive TUI: streaming transcript, slash-command palette, tool
//! approval modals, and a persistent usage statusline (bottom-left).

use crate::agent::{self, AgentEvent, AgentRunner, ApprovalDecision, Session};
use crate::api::MetaClient;
use crate::config::Config;
use crate::error::Result;
use crate::tui::input::InputState;
use crate::usage::{TokenUsage, UsageTracker};
use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEventKind, KeyModifiers,
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
    ("/usage", "token usage + cost for this session"),
    ("/cost", "alias for /usage"),
    ("/model", "show or switch model"),
    ("/effort", "reasoning effort: minimal|low|medium|high|xhigh"),
    ("/sessions", "list recent sessions"),
    ("/resume", "resume a session by id prefix"),
    ("/init", "generate a MUSE.md project guide"),
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
    Info(String),
    Error(String),
}

#[derive(PartialEq)]
enum TurnMode {
    Chat,
    Compact,
}

pub struct ApprovalState {
    pub name: String,
    pub args: String,
    pub respond: Option<oneshot::Sender<ApprovalDecision>>,
}

pub struct App {
    pub client: MetaClient,
    pub cfg: Config,
    pub cwd: PathBuf,
    pub auto_approve: bool,
    pub approved_tools: Arc<Mutex<HashSet<String>>>,

    pub cells: Vec<Cell>,
    tool_cells: HashMap<u64, usize>,
    pub scroll_from_bottom: u16,

    pub input: InputState,
    pub queue: VecDeque<String>,

    pub busy: bool,
    mode: TurnMode,
    pub turn_started: Instant,
    pub status: String,
    pub spinner_epoch: Instant,

    pub session: Option<Box<Session>>,
    pub usage: Option<Box<UsageTracker>>,
    pub session_id: String,
    pub u_session: TokenUsage,
    pub u_last: TokenUsage,

    pub approval: Option<ApprovalState>,
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
        let _ = stdout().execute(DisableBracketedPaste);
        let _ = stdout().execute(LeaveAlternateScreen);
    }
}

pub async fn run_tui(
    client: MetaClient,
    cfg: Config,
    cwd: PathBuf,
    auto_approve: bool,
    session: Session,
    usage: UsageTracker,
    initial_prompt: Option<String>,
) -> Result<()> {
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableBracketedPaste)?;
    // Hardware cursor hidden — we paint a Meta blue block caret ourselves.
    stdout().execute(Hide)?;
    let _guard = TermGuard;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let (tx, rx) = mpsc::unbounded_channel();
    let u_session = usage.session_usage().clone();
    let session_id = session.id.clone();

    let mut app = App {
        client,
        cfg,
        cwd,
        auto_approve,
        approved_tools: Arc::new(Mutex::new(HashSet::new())),
        cells: vec![Cell::Banner],
        tool_cells: HashMap::new(),
        scroll_from_bottom: 0,
        input: InputState::new(),
        queue: VecDeque::new(),
        busy: false,
        mode: TurnMode::Chat,
        turn_started: Instant::now(),
        status: "idle".into(),
        spinner_epoch: Instant::now(),
        session: Some(Box::new(session)),
        usage: Some(Box::new(usage)),
        session_id,
        u_session,
        u_last: TokenUsage::default(),
        approval: None,
        palette_idx: 0,
        quit_armed: None,
        tx,
        rx,
        cancel: None,
        should_quit: false,
    };

    app.replay_session_tail(8);

    if let Some(p) = initial_prompt {
        if !p.trim().is_empty() {
            app.submit_text(&p);
        }
    }

    loop {
        // Drain agent events first so the frame is fresh.
        while let Ok(ev) = app.rx.try_recv() {
            app.on_agent_event(ev);
        }

        terminal.draw(|f| super::ui::draw(f, &mut app))?;

        if app.should_quit {
            break;
        }

        // ~30fps so Meta spinners / caret blink stay smooth without burning CPU.
        if event::poll(Duration::from_millis(32))? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
                Event::Paste(s) => {
                    if app.approval.is_none() {
                        app.input.insert_str(&s);
                    }
                }
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

    // ── keys ───────────────────────────────────────────────────────────
    fn on_key(&mut self, key: event::KeyEvent) {
        // Approval modal swallows all keys.
        if self.approval.is_some() {
            self.on_approval_key(key.code);
            return;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        match key.code {
            KeyCode::Char('c') if ctrl => {
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
            KeyCode::Up => {
                if self.palette_visible() {
                    self.palette_idx = self.palette_idx.saturating_sub(1);
                } else if self.input.on_first_line() {
                    self.input.history_prev();
                } else {
                    self.input.move_up_line();
                }
            }
            KeyCode::Down => {
                if self.palette_visible() {
                    let n = self.palette_matches().len();
                    if self.palette_idx + 1 < n {
                        self.palette_idx += 1;
                    }
                } else if self.input.on_last_line() {
                    self.input.history_next();
                } else {
                    self.input.move_down_line();
                }
            }
            KeyCode::Left if ctrl => self.input.word_left(),
            KeyCode::Right if ctrl => self.input.word_right(),
            KeyCode::Left => self.input.move_left(),
            KeyCode::Right => self.input.move_right(),
            KeyCode::Home => self.input.move_line_home(),
            KeyCode::End => self.input.move_line_end(),
            KeyCode::Backspace => self.input.backspace(),
            KeyCode::Delete => self.input.delete(),
            KeyCode::PageUp => self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(10),
            KeyCode::PageDown => {
                self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(10)
            }
            KeyCode::Char('l') if ctrl => {
                self.cells.retain(|c| matches!(c, Cell::Banner));
                self.scroll_from_bottom = 0;
            }
            KeyCode::Char('a') if ctrl => self.input.move_line_home(),
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
        self.busy = true;
        self.mode = TurnMode::Chat;
        self.turn_started = Instant::now();
        self.status = "thinking".into();
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
        AgentRunner {
            client: self.client.clone(),
            config: self.cfg.clone(),
            cwd: self.cwd.clone(),
            auto_approve: self.auto_approve,
            verbose: false,
            approved_tools: self.approved_tools.clone(),
        }
    }

    fn interrupt(&mut self) {
        if let Some(c) = &self.cancel {
            c.cancel();
        }
        // If the loop is blocked on an approval, deny it so it can unwind.
        if let Some(mut a) = self.approval.take() {
            if let Some(respond) = a.respond.take() {
                let _ = respond.send(ApprovalDecision::Deny);
            }
        }
        self.status = "interrupting…".into();
    }

    // ── agent events ───────────────────────────────────────────────────
    fn on_agent_event(&mut self, ev: AgentEvent) {
        match ev {
            AgentEvent::Status(s) => self.status = s,
            AgentEvent::ReasoningDelta(d) => {
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
            AgentEvent::Done {
                session,
                usage,
                result,
                interrupted,
            } => {
                self.finish_thinking();
                self.finish_streaming();
                self.u_session = usage.session_usage().clone();
                self.session_id = session.id.clone();
                self.session = Some(session);
                self.usage = Some(usage);
                self.busy = false;
                self.cancel = None;
                self.status = "idle".into();
                match (&self.mode, result, interrupted) {
                    (_, _, true) => self.push_info("interrupted".into()),
                    (TurnMode::Compact, Ok(summary), _) => {
                        self.push_info(format!(
                            "context compacted — summary:\n{summary}"
                        ));
                    }
                    (TurnMode::Compact, Err(e), _) => {
                        self.push_error(format!("compaction failed: {e}"))
                    }
                    (TurnMode::Chat, Err(e), _) => self.push_error(e),
                    (TurnMode::Chat, Ok(_), _) => {}
                }
                self.mode = TurnMode::Chat;
                if let Some(next) = self.queue.pop_front() {
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
            "/usage" | "/cost" => self.cmd_usage(),
            "/model" => self.cmd_model(&arg),
            "/effort" => self.cmd_effort(&arg),
            "/sessions" => self.cmd_sessions(),
            "/resume" => self.cmd_resume(&arg),
            "/config" => self.cmd_config(),
            "/init" => {
                self.submit_text(
                    "Analyze this codebase (structure, build/test commands, conventions, \
                     architecture) and create a MUSE.md file at the workspace root that future \
                     agent sessions can use as project instructions. Keep it under 120 lines.",
                );
            }
            other => self.push_error(format!("unknown command: {other} — try /help")),
        }
    }

    fn cmd_help(&mut self) {
        let mut s = String::from("commands\n");
        for (name, desc) in COMMANDS {
            s.push_str(&format!("  {name:<10} {desc}\n"));
        }
        s.push_str(
            "\nkeys\n  Enter send · \\+Enter or Ctrl+J newline · ↑/↓ history\n  \
             Esc interrupt/clear · Ctrl+C twice quit · PgUp/PgDn scroll · Ctrl+L clear\n  \
             tool approvals: y allow once · a always allow · n deny",
        );
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
        self.mode = TurnMode::Compact;
        self.turn_started = Instant::now();
        self.status = "compacting".into();
        let runner = self.make_runner();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let mut session = *session;
            let mut usage = *usage;
            let res = agent::compact_session(&runner, &mut session, &mut usage).await;
            let _ = tx.send(AgentEvent::Done {
                session: Box::new(session),
                usage: Box::new(usage),
                result: res.map_err(|e| e.to_string()),
                interrupted: false,
            });
        });
    }

    fn cmd_usage(&mut self) {
        let u = &self.u_session;
        self.push_info(format!(
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
                self.push_info(format!(
                    "resumed session {}",
                    &self.session_id[..8.min(self.session_id.len())]
                ));
                self.replay_session_tail(8);
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
        self.cells
            .push(Cell::Info(format!("─ history ({} messages) ─", session.messages.len())));
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
        self.cells.push(Cell::Info(s));
    }

    fn push_error(&mut self, s: String) {
        self.cells.push(Cell::Error(s));
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
