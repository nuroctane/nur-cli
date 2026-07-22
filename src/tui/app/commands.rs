//! Slash-command handlers (`/help`, `/model`, `/cd`, `/graphify`, …).
//!
//! Split out of `app.rs` to keep the god-file in check. This is a child module
//! of `app`, so it retains access to `App`'s private fields and methods. The
//! command table itself (`COMMANDS`) and the dispatch entry point live here.

use super::{fmt_num, scan_prompt, App, Cell, TurnMode, COMMANDS};
use crate::agent::{self, AgentEvent, PermissionMode, Session};
use crate::theme::Tone;
use crate::tools::ToolHost;
use crate::usage::{TokenUsage, UsageTracker};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Launch `nur <args>` in a **new console window** so a long job (a multi-GB
/// `local up` download, a `bench run`) shows its own live progress without
/// freezing or corrupting the TUI. Non-Windows: detached background process.
fn spawn_console(exe: &std::path::Path, args: &[String]) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0000_0010); // CREATE_NEW_CONSOLE
    }
    #[cfg(not(windows))]
    {
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
    }
    cmd.spawn().map(|_| ())
}

/// Spawn a command detached (null stdio, don't wait). Used for `cua autostart
/// enable/disable`, which may pop a UAC prompt — we must not block the TUI on it.
fn spawn_detached(bin: &str, args: &[&str]) -> std::io::Result<()> {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW — don't touch nur's console
    }
    cmd.spawn().map(|_| ())
}

impl App {
    // ── slash commands ──────────────────────────────────────────────────
    pub(super) fn run_command(&mut self, raw: &str) {
        let mut parts = raw.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("").trim().to_string();

        match cmd {
            "/exit" | "/quit" => self.should_quit = true,
            "/help" | "/commands" => self.cmd_help(),
            "/clear" => {
                self.cells.retain(|c| matches!(c, Cell::Banner));
                self.scroll_from_bottom = 0;
                // Say what this did NOT do: the model still has the full
                // conversation, and the session's replay log is untouched.
                self.push_note(
                    Tone::Neutral,
                    "screen cleared - the model still has the full context, and the session                      history is intact (/compact to actually shrink context, /new for a                      fresh session)"
                        .into(),
                );
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
                self.refresh_skill_palette_cache();
                let skills = agent::skills::load_skills(&self.cwd);
                if skills.is_empty() {
                    self.push_note(
                        Tone::Skill,
                        "no skills found - add ~/.nur/skills/<name>/SKILL.md\n\
                         or ~/.agents/skills/<name>/SKILL.md  (graphify install --platform agents)\n\
                         the agent can also load them itself via the `skill` tool"
                            .into(),
                    );
                } else {
                    let mut s = String::from(
                        "skills - invoke with /name (sticky) or /name <prompt> (one-shot)\n\
                         natural-language phrases also activate many skills\n\
                         agent can load via the `skill` tool\n",
                    );
                    for sk in skills.iter().take(80) {
                        let desc: String = sk.description.chars().take(64).collect();
                        s.push_str(&format!("  /{} - {}\n", sk.name, desc));
                    }
                    if skills.len() > 80 {
                        s.push_str(&format!(
                            "  ... +{} more (type /partial-name to filter in the palette)\n",
                            skills.len() - 80
                        ));
                    }
                    if !self.sticky_skills.is_empty() {
                        s.push_str(&format!(
                            "\nsticky this session: {}\n",
                            self.sticky_skills.join(", ")
                        ));
                    }
                    self.push_note(Tone::Skill, s);
                }
            }
            "/usage" | "/cost" => self.cmd_usage(),
            "/budget" => self.cmd_budget(&arg),
            "/turns" => self.cmd_budget(&if arg.is_empty() {
                "turns".into()
            } else {
                format!("turns {arg}")
            }),
            "/poor" => self.cmd_poor(&arg),
            "/undo" => self.cmd_undo(),
            "/failover" => self.open_failover(),
            "/fusion" => self.cmd_fusion(&arg),
            "/local" => self.cmd_local(&arg),
            "/bench" => self.cmd_bench(&arg),
            "/receipt" => self.cmd_receipt(),
            "/cua" => self.cmd_cua(&arg),
            "/permissions" => self.cmd_permissions(&arg),
            "/hooks" => self.push_note(Tone::Skill, agent::hooks::HooksConfig::load().summary()),
            "/context" => self.cmd_context(),
            "/status" => self.cmd_status(),
            "/doctor" => self.cmd_doctor(),
            "/model" | "/models" => self.cmd_model(&arg),
            "/plugins" | "/plugin" => self.cmd_plugins(&arg),
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
            // Cross-agent session migration (import Claude/Codex/Cursor/Grok).
            "/takeover" | "/hijack" => self.cmd_chagent(&arg),
            "/config" => self.cmd_config(),
            "/init" => {
                self.submit_text(
                    "Analyze this codebase (structure, build/test commands, conventions, \
                     architecture) and create a NUR.md file at the workspace root that future \
                     agent sessions can use as project instructions. Keep it under 120 lines.",
                );
            }
            "/scan" => self.cmd_scan(&arg),
            "/graphify" => self.cmd_graphify(&arg),
            "/graphjin" | "/gj" => self.cmd_graphjin(&arg),
            "/plur" => self.cmd_plur(&arg),
            "/ruflo" => self.cmd_ruflo(&arg),
            "/akarso" => self.cmd_akarso(&arg),
            "/openseo" => self.cmd_openseo(),
            "/ecosystem" => {
                // Heals missing pieces (excalidraw, etc.) — same as one-shot ensure.
                self.push_note(Tone::Skill, "checking / provisioning ecosystem…".into());
                let st = crate::ecosystem::ensure_ecosystem(false);
                self.push_note(Tone::Skill, st.report());
            }
            "/login" => self.open_login(),
            "/logout" => self.cmd_logout(),
            "/goal" => self.cmd_goal(&arg),
            "/graph" => self.cmd_graph(),
            "/swarm" | "/subagents" | "/agents" => self.cmd_swarm(&arg),
            "/draw" => self.cmd_draw(&arg),
            "/steer" => self.cmd_steer(&arg),
            "/bro" => self.cmd_bro(&arg),
            "/btw" => self.cmd_btw(&arg),
            "/codesearch" | "/cs" => self.cmd_codesearch(&arg),
            "/feedback" => self.cmd_feedback(&arg),
            "/mc" | "/mcp" => self.cmd_mc(&arg),
            "/tips" => self.cmd_tips(),
            // Skeuomorphic-ui skill aliases → canonical /skeuomorphic-ui.
            "/skeuo" | "/skeu" | "/skeuomorphic" => {
                self.cmd_skill_or_unknown("/skeuomorphic-ui", &arg)
            }
            "/bug" => self.push_note(
                Tone::Neutral,
                "report an issue (unofficial community project)\n  \
                 https://github.com/nuroctane/nur-cli/issues\n  \
                 or use  /feedback <what happened>  to file one from here"
                    .into(),
            ),
            other => self.cmd_skill_or_unknown(other, &arg),
        }
        // Slash commands answer inline, and their card is appended at the end of
        // the transcript. If the user had scrolled up to re-read something, that
        // card landed below the viewport and the screen appeared not to react at
        // all - including for errors like "unknown command".
        self.scroll_to_bottom();
    }

    /// Unknown slash -> installed skill.
    ///
    /// - `/skillname` / `/skillname on|off` — sticky session mode (toggle)
    /// - `/skillname <prompt>` — one-shot turn with that skill activated
    fn cmd_skill_or_unknown(&mut self, cmd: &str, arg: &str) {
        let name = cmd.trim().trim_start_matches('/').trim();
        if name.is_empty() {
            self.push_error(format!("unknown command: {cmd} - try /help"));
            return;
        }
        let Some(sk) = agent::skills::skill_by_name(&self.cwd, name) else {
            // `/help` and the palette list skill commands that ship as plugins,
            // so a name can be advertised here and still not be installed -
            // "unknown command, try /help" then sends the user back to the list
            // that suggested it. Say which case this is.
            if COMMANDS.iter().any(|(c, _)| *c == cmd) {
                self.push_error(format!(
                    "{cmd} is a skill that isn't installed here - /plugins to install it,                      or /skills to see what is available"
                ));
            } else {
                self.push_error(format!("unknown command: {cmd} - try /help"));
            }
            return;
        };

        let arg = arg.trim();
        let force = match arg.to_ascii_lowercase().as_str() {
            "on" | "yes" | "1" | "enable" => Some(true),
            "off" | "no" | "0" | "disable" | "clear" => Some(false),
            "" => None,
            _ => None, // may be a prompt
        };
        let is_force_keyword = matches!(
            arg.to_ascii_lowercase().as_str(),
            "on" | "yes" | "1" | "enable" | "off" | "no" | "0" | "disable" | "clear" | ""
        );

        // One-shot: /skillname do the thing
        if !is_force_keyword {
            let section = agent::skills::slash_activation_section(&sk);
            let display = format!("/{} {}", sk.name, arg);
            let model_prompt = format!(
                "{section}\n# User request (via /{name})\n{arg}\n",
                name = sk.name,
                section = section,
                arg = arg
            );
            self.push_note(
                Tone::Skill,
                format!("/{} · one-shot activation for this turn", sk.name),
            );
            self.start_turn_labeled(&display, &model_prompt);
            return;
        }

        let already = self
            .sticky_skills
            .iter()
            .any(|n| n.eq_ignore_ascii_case(&sk.name));
        let enable = force.unwrap_or(!already);
        if enable {
            if !already {
                self.sticky_skills.push(sk.name.clone());
            }
            self.push_note(
                Tone::Mode,
                format!(
                    "/{} on - sticky for this session\n  {}\n  /{} off to disable · /{} <prompt> for one-shot",
                    sk.name,
                    sk.description.chars().take(160).collect::<String>(),
                    sk.name,
                    sk.name
                ),
            );
        } else if already {
            self.sticky_skills
                .retain(|n| !n.eq_ignore_ascii_case(&sk.name));
            self.push_note(
                Tone::Mode,
                format!("/{} off - skill no longer sticky", sk.name),
            );
        } else {
            self.push_note(Tone::Mode, format!("/{} already off", sk.name));
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

    /// Akarso social posting — read-only from the slash (status/list). Actual
    /// publishing goes through the agent (`akarso` tool) so it's approval-gated;
    /// `/akarso` never posts directly.
    fn cmd_akarso(&mut self, arg: &str) {
        let arg = arg.trim();
        let json = match arg {
            "" | "status" | "auth" | "check" => r#"{"action":"auth_check"}"#.to_string(),
            "accounts" | "acc" => r#"{"action":"accounts_list"}"#.to_string(),
            "health" => r#"{"action":"accounts_health"}"#.to_string(),
            "posts" | "list" | "ls" => r#"{"action":"posts_list"}"#.to_string(),
            "profiles" => r#"{"action":"profiles_list"}"#.to_string(),
            "help" | "-h" | "--help" => {
                self.push_note(
                    Tone::Skill,
                    "akarso — social posting across 14 platforms\n  \
                     /akarso            auth status\n  \
                     /akarso accounts   connected accounts\n  \
                     /akarso posts      list posts\n  \
                     to publish/schedule, just ask (e.g. \"post X to LinkedIn and X\") — \
                     the akarso tool runs it with approval\n  \
                     first run:  akarso auth login  ·  akarso accounts connect <platform>"
                        .into(),
                );
                return;
            }
            other => {
                self.push_error(format!(
                    "/akarso: unknown '{other}' — try: (blank) · accounts · posts · health · profiles · help"
                ));
                return;
            }
        };
        let host = ToolHost::default();
        let ctx = crate::tools::ToolContext {
            cwd: self.cwd.clone(),
            cancel: CancellationToken::new(),
        };
        match host.dispatch("akarso", &json, &ctx) {
            Ok(s) => self.push_note(Tone::Skill, s),
            Err(e) => self.push_error(e.to_string()),
        }
    }

    /// OpenSEO — open the dashboard + MCP docs and point at the setup. OpenSEO is
    /// an MCP server (no CLI); connect it via the `executor`/`/mcp` gateway.
    fn cmd_openseo(&mut self) {
        let _ = crate::open_uri::open("https://openseo.so/docs/mcp");
        let _ = crate::open_uri::open("https://app.openseo.so");
        self.push_note(
            Tone::Skill,
            "OpenSEO — open-source Semrush/Ahrefs alternative (SEO via MCP)\n  \
             opened dashboard + MCP docs in your browser\n  \
             1. sign up / self-host, then connect the MCP: https://openseo.so/docs/mcp\n  \
             2. add it via the executor gateway (/mcp) so its tools are callable\n  \
             3. then ask for keyword research · backlinks · rank tracking · site audit · competitor SEO\n  \
             skill: /openseo activates the workflow guidance"
                .into(),
        );
    }

    /// Run graphjin actions from the TUI without going through the model.
    ///
    /// Read-only by construction: the write path (`mutate`) is deliberately not
    /// reachable from a slash command — a data mutation should go through the
    /// agent loop's approval gate, not a one-liner.
    fn cmd_graphjin(&mut self, arg: &str) {
        let arg = arg.trim();
        let mut parts = arg.splitn(2, char::is_whitespace);
        let action = parts.next().unwrap_or("").trim();
        let rest = parts.next().unwrap_or("").trim();

        let json = match action {
            "" | "status" => r#"{"action":"status"}"#.to_string(),
            "catalog" | "search" if !rest.is_empty() => {
                serde_json::json!({"action": "catalog", "search": rest}).to_string()
            }
            "schema" if !rest.is_empty() => {
                serde_json::json!({"action": "schema", "id": rest}).to_string()
            }
            "help" => serde_json::json!({"action": "help", "search": rest}).to_string(),
            "explain" if !rest.is_empty() => {
                serde_json::json!({"action": "explain", "query": rest}).to_string()
            }
            "query" | "q" if !rest.is_empty() => {
                serde_json::json!({"action": "query", "query": rest}).to_string()
            }
            "security" => serde_json::json!({"action": "security", "role": rest}).to_string(),
            "ask" if !rest.is_empty() => {
                serde_json::json!({"action": "ask", "instruction": rest}).to_string()
            }
            _ => {
                self.push_note(
                    Tone::Skill,
                    "graphjin — governed access to live data\n  \
                     /graphjin                        status (CLI · client config · server reachable?)\n  \
                     /graphjin catalog <intent>       search gj_catalog — always start here\n  \
                     /graphjin schema <id>            detail for a catalog entry\n  \
                     /graphjin help [topic]           GraphQL shape guidance from the server\n  \
                     /graphjin explain <graphql>      compile to SQL without running it\n  \
                     /graphjin query <graphql|name>   run a read query, or a saved query by name\n  \
                     /graphjin security [role]        role permission matrix\n  \
                     /graphjin ask <instruction>      server-side agent, evidence-backed\n\n\
                     writes go through the model (graphjin tool, action=mutate) so they hit \
                     the approval gate.\n\
                     install:  npm install -g graphjin\n\
                     connect:  graphjin cli setup <server-url>   (serve one: graphjin serve --path ./config)"
                        .to_string(),
                );
                return;
            }
        };

        let host = ToolHost::default();
        let ctx = crate::tools::ToolContext {
            cwd: self.cwd.clone(),
            cancel: CancellationToken::new(),
        };
        match host.dispatch("graphjin", &json, &ctx) {
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
            None => self.push_error(format!("unknown mode '{arg}' — use manual, plan, or auto")),
        }
    }

    fn cmd_logout(&mut self) {
        match crate::auth::logout(false) {
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
            (
                "↑ ↓  ·  wheel",
                "scroll transcript (wheel on input scrolls prompt)",
            ),
            ("drag in input", "select prompt  ·  large paste → chip"),
            ("drag scrollbar", "scrub history"),
            ("drag text", "select transcript + auto-copy"),
            ("click ↓ End", "jump to latest"),
            ("click card  ·  ▸", "peek  ·  expand"),
            ("right/2×-click prompt", "menu: fork · edit · revert · copy"),
            ("Ctrl+A", "select all (input, or transcript if empty)"),
            ("Ctrl+C", "copy selection  ·  else cancel / double-tap quit"),
            ("Ctrl+V  ·  Ctrl+X", "paste (chips big blobs)  ·  cut"),
            ("Ctrl+P / N", "prompt history  (also Alt+↑/↓)"),
            ("Enter  ·  Shift+Enter", "send  ·  newline"),
            ("Shift+Tab", "cycle permission mode"),
            (
                "Ctrl+R",
                "reverse-search prompt history  (type · Ctrl+R older · Esc cancel)",
            ),
            ("Esc", "close peek  →  cancel turn  →  clear input"),
            ("Ctrl+L", "clear transcript view"),
            ("y  ·  a  ·  n", "approve once  ·  always  ·  deny"),
        ];
        for (k, v) in keys {
            s.push_str(&format!("  {k:<22}  {v}\n"));
        }

        s.push_str("\nsessions browser  (/sessions · /resume)\n");
        for (k, v) in [
            ("↑ ↓  ·  wheel", "move selection"),
            ("Enter", "open session"),
            ("Tab  ·  Space", "toggle this workspace / all"),
            ("c  ·  i", "switch window: sessions ⇄ takeover"),
            ("click row", "select  ·  click again to open"),
            ("click ✕  ·  Esc", "close"),
        ] {
            s.push_str(&format!("  {k:<22}  {v}\n"));
        }

        s.push_str("\ntakeover · cross-agent import  (/takeover · /hijack)\n");
        for (k, v) in [
            ("/takeover", "import picker (foreign sessions only)"),
            ("/takeover ls [agent]", "list migratable sessions"),
            ("/takeover <agent> [ref]", "import <id|latest> and resume"),
        ] {
            s.push_str(&format!("  {k:<25}  {v}\n"));
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
        let mut usage =
            UsageTracker::new(session.id.clone(), self.cfg.model.clone(), self.cwd.clone());
        usage.set_provider(self.cfg.provider.clone());
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
        let turn_cap = if self.cfg.max_turns == 0 {
            "∞".into()
        } else {
            self.cfg.max_turns.to_string()
        };
        let rates = self
            .usage
            .as_ref()
            .map(|t| t.active_rates())
            .unwrap_or_else(|| crate::pricing::rates_for(&self.cfg.provider, &self.cfg.model));
        let cost_tag = if rates.is_estimate() {
            "list-price estimate"
        } else {
            "reported"
        };
        self.push_note(
            Tone::Usage,
            format!(
            "session usage\n  input    {} tok ({} cached)\n  output   {} tok ({} reasoning)\n  \
             total    {} tok  (cap {tok_cap})\n  cost     ~${:.4}  ({cost_tag}, cap {cost_cap})\n  \
             rates    ${:.4}/M in · ${:.4}/M out · ${:.4}/M cache-read\n  \
             source   {} · {}/{}\n  note     {}\n  \
             turns    per-prompt cap {turn_cap}\n  status   {}\n  \
             optional ceilings: /budget  ·  prompt saver: /poor\n  \
             tip  ~$ is published list price × tokens — not your invoice",
            fmt_num(u.input_tokens),
            fmt_num(u.cached_tokens),
            fmt_num(u.output_tokens),
            fmt_num(u.reasoning_tokens),
            fmt_num(u.total_tokens),
            u.estimated_cost_usd(),
            rates.input_per_mtok_usd,
            rates.output_per_mtok_usd,
            rates.cache_read_per_mtok_usd,
            rates.source,
            rates.provider_id,
            rates.model_id,
            rates.note,
            crate::config::status_path().display(),
        ),
        );
    }

    /// One-line summary of optional stop valves (all unlimited by default).
    fn budget_status_line(&self) -> String {
        let cost = self
            .cfg
            .max_session_cost_usd
            .map(|c| format!("${c:.2}"))
            .unwrap_or_else(|| "∞".into());
        let toks = self
            .cfg
            .max_session_tokens
            .map(|t| fmt_num(t))
            .unwrap_or_else(|| "∞".into());
        let turns = if self.cfg.max_turns == 0 {
            "∞".into()
        } else {
            self.cfg.max_turns.to_string()
        };
        format!("cost {cost} · tokens {toks} · turns {turns}")
    }

    fn cmd_receipt(&mut self) {
        let text = crate::agent::receipt::render(&self.session_id);
        self.push_note(Tone::Session, text);
    }

    /// `/cua [on|off|status]` — control the Cua computer-use desktop driver.
    ///
    /// - `on`  registers cua's always-on background driver (elevated, runs at
    ///   every logon) so cua can see and control your desktop without nur
    ///   launching it each time — Windows shows a UAC prompt.
    /// - `off` removes it; cua computer-use stays available on demand (nur
    ///   starts it only when a task needs the desktop). This is the default.
    /// - no arg / `status` shows whether the driver is installed and running.
    fn cmd_cua(&mut self, arg: &str) {
        let Some(bin) = crate::ecosystem::cua_driver_path() else {
            self.push_note(
                Tone::Session,
                "cua-driver isn't installed yet — it auto-provisions on install / next nur open. \
                 Run `nur ecosystem ensure` (or reopen nur) to install it, then `/cua`."
                    .into(),
            );
            return;
        };
        match arg.trim().to_lowercase().as_str() {
            "on" | "enable" | "start" => match spawn_detached(&bin, &["autostart", "enable"]) {
                Ok(()) => self.push_note(
                    Tone::Session,
                    "cua · turning ON the always-on desktop driver — approve the Windows UAC prompt.\n\
                     Once on, cua can see and control your desktop in the background (runs elevated \
                     at every logon), so computer-use tasks start instantly.\n\
                     Turn it off any time with /cua off · check /cua status."
                        .into(),
                ),
                Err(e) => self.push_note(Tone::Neutral, format!("cua enable failed: {e}")),
            },
            "off" | "disable" | "stop" => match spawn_detached(&bin, &["autostart", "disable"]) {
                Ok(()) => self.push_note(
                    Tone::Session,
                    "cua · turning OFF the always-on desktop driver. Nothing runs in the background — \
                     cua computer-use is still available on demand (nur launches it only when a task \
                     needs the desktop). Check /cua status."
                        .into(),
                ),
                Err(e) => self.push_note(Tone::Neutral, format!("cua disable failed: {e}")),
            },
            _ => {
                let ver = crate::ecosystem::cmd_version_pub(&bin, &["--version"])
                    .unwrap_or_else(|| "installed".into());
                let status = crate::ecosystem::run_capture(&bin, &["autostart", "status"], None, 4_000)
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|e| {
                        format!("(status unavailable: {})", e.chars().take(80).collect::<String>())
                    });
                self.push_note(
                    Tone::Session,
                    format!(
                        "cua-driver {ver} — computer-use desktop driver\n{status}\n\n\
                         /cua on   — always-on: cua sees/controls your desktop in the background \
                         (elevated, starts at logon; UAC prompt)\n\
                         /cua off  — on-demand only: nothing runs in the background (default)",
                    ),
                );
            }
        }
    }

    // ── /fusion — multi-model debate → one synthesized answer ────────────
    fn cmd_fusion(&mut self, arg: &str) {
        let arg = arg.trim();
        let mut it = arg.splitn(2, char::is_whitespace);
        let kw = it.next().unwrap_or("");
        let rest = it.next().unwrap_or("").trim();
        match kw.to_ascii_lowercase().as_str() {
            "" => self.fusion_status(),
            "off" | "clear" | "none" | "reset" => {
                self.cfg.fusion_panel.clear();
                let _ = crate::config::save_config(&self.cfg);
                self.push_note(Tone::Neutral, "fusion · panel cleared (off)".into());
            }
            "panel" | "set" | "add" => {
                let adding = kw.eq_ignore_ascii_case("add");
                let ids: Vec<String> = rest
                    .split(|c: char| c == ',' || c.is_whitespace())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
                if ids.is_empty() {
                    self.push_error(
                        "usage: /fusion panel <ids>   e.g.  /fusion panel openai,anthropic,groq"
                            .into(),
                    );
                    return;
                }
                let mut valid = if adding {
                    self.cfg.fusion_panel.clone()
                } else {
                    Vec::new()
                };
                let mut unknown = Vec::new();
                for id in ids {
                    if crate::providers::by_id(&id).is_some() {
                        if !valid.iter().any(|v| v == &id) {
                            valid.push(id);
                        }
                    } else {
                        unknown.push(id);
                    }
                }
                self.cfg.fusion_panel = valid;
                let _ = crate::config::save_config(&self.cfg);
                if !unknown.is_empty() {
                    self.push_error(format!(
                        "skipped unknown provider id(s): {}  (see /login for ids)",
                        unknown.join(", ")
                    ));
                }
                self.fusion_status();
            }
            // Anything else is the question to fuse.
            _ => self.start_fusion(arg),
        }
    }

    fn fusion_status(&mut self) {
        if self.cfg.fusion_panel.is_empty() {
            self.push_note(
                Tone::Neutral,
                "fusion · panel empty (off)\n  \
                 set one:  /fusion panel openai,anthropic,groq\n  \
                 then ask: /fusion <question>\n  \
                 the active model always joins the panel and writes the final answer"
                    .into(),
            );
        } else {
            self.push_note(
                Tone::Neutral,
                format!(
                    "fusion panel: {active} (active · synthesizer) + {panel}\n  \
                     ask:  /fusion <question>   ·   change:  /fusion panel <ids>   ·   off:  /fusion off",
                    active = self.cfg.provider,
                    panel = self.cfg.fusion_panel.join(", "),
                ),
            );
        }
    }

    /// Fan the question out to [active model + panel], then synthesize one answer.
    fn start_fusion(&mut self, question: &str) {
        let question = question.trim();
        if question.is_empty() {
            self.fusion_status();
            return;
        }
        if !self.authed {
            self.push_error("signed out — run /login before /fusion".into());
            return;
        }
        if self.cfg.fusion_panel.is_empty() {
            self.push_error(
                "no fusion panel set — /fusion panel openai,anthropic,groq  (then /fusion <question>)"
                    .into(),
            );
            return;
        }
        if self.busy {
            self.push_error("busy — wait for the current turn to finish".into());
            return;
        }
        let (Some(session), Some(usage)) = (self.session.take(), self.usage.take()) else {
            self.push_error("internal: session busy".into());
            return;
        };
        self.cells.push(Cell::User(format!("/fusion {question}")));
        if !self.title_from_prompt {
            self.window_base = question.to_string();
            self.title_from_prompt = true;
        }
        self.scroll_to_bottom();
        self.scrollbar_drag = false;
        self.selecting = false;
        self.select_anchor = None;
        self.mouse_left_down = false;
        super::enable_mouse();
        self.busy = true;
        self.cancelling = false;
        self.turn_kind = TurnMode::Chat;
        self.turn_started = Instant::now();
        self.thought_accum = Duration::ZERO;
        self.status = "fusion · starting…".into();
        let cancel = CancellationToken::new();
        self.cancel = Some(cancel.clone());
        crate::agent::fusion::spawn_fusion(
            self.client.clone(),
            self.cfg.provider.clone(),
            self.cfg.model.clone(),
            self.cfg.fusion_panel.clone(),
            question.to_string(),
            session,
            usage,
            self.tx.clone(),
            cancel,
        );
    }

    // ── /local — managed local models (llama.cpp + GGUF) ────────────────
    fn cmd_local(&mut self, arg: &str) {
        let mut it = arg.trim().splitn(2, char::is_whitespace);
        let sub = it.next().unwrap_or("").to_ascii_lowercase();
        let rest = it.next().unwrap_or("").trim();
        match sub.as_str() {
            "" | "status" => self.push_note(Tone::Neutral, crate::local::status_report()),
            "models" => self.push_note(Tone::Neutral, crate::local::models_report()),
            "down" | "stop" => self.push_note(Tone::Neutral, crate::local::stop_report()),
            "up" => {
                let mut args = vec!["local".to_string(), "up".to_string()];
                if !rest.is_empty() {
                    args.push(rest.to_string());
                }
                self.launch_console(args);
            }
            _ => self.push_error("usage: /local [status | models | up [tier|url] | down]".into()),
        }
    }

    // ── /bench — benchmark models on your tasks ─────────────────────────
    fn cmd_bench(&mut self, arg: &str) {
        let mut it = arg.trim().splitn(2, char::is_whitespace);
        let sub = it.next().unwrap_or("").to_ascii_lowercase();
        let rest = it.next().unwrap_or("").trim();
        match sub.as_str() {
            "" | "list" => self.push_note(Tone::Neutral, crate::bench::list_report()),
            "add" => {
                let mut p = rest.splitn(2, char::is_whitespace);
                let name = p.next().unwrap_or("").trim();
                let prompt = p.next().unwrap_or("").trim();
                if name.is_empty() || prompt.is_empty() {
                    self.push_error(
                        "usage: /bench add <name> <prompt>   (add a pass/fail gate via CLI: nur bench add <name> \"...\" --check \"cargo test\")"
                            .into(),
                    );
                } else {
                    match crate::bench::add_task(name, prompt, None) {
                        Ok(m) => self.push_note(Tone::Neutral, m),
                        Err(e) => self.push_error(format!("bench add: {e}")),
                    }
                }
            }
            "remove" | "rm" => {
                if rest.is_empty() {
                    self.push_error("usage: /bench remove <name>".into());
                } else {
                    self.push_note(Tone::Neutral, crate::bench::remove_report(rest));
                }
            }
            "run" => {
                let mut p = rest.splitn(2, char::is_whitespace);
                let name = p.next().unwrap_or("").trim();
                if name.is_empty() {
                    self.push_error("usage: /bench run <name|all> [model,model]".into());
                    return;
                }
                let mut args = vec!["bench".to_string(), "run".to_string(), name.to_string()];
                if let Some(models) = p.next().map(str::trim).filter(|s| !s.is_empty()) {
                    args.push("--models".to_string());
                    args.push(models.to_string());
                }
                self.launch_console(args);
            }
            "optimize" | "opt" | "gepa" => {
                let mut p = rest.split_whitespace();
                let name = p.next().unwrap_or("").trim();
                if name.is_empty() {
                    self.push_error(
                        "usage: /bench optimize <name|all> [gens] [pop]   — evolves the standing \
                         instruction against your recorded tasks (costs tokens)"
                            .into(),
                    );
                    return;
                }
                let mut args = vec![
                    "bench".to_string(),
                    "optimize".to_string(),
                    name.to_string(),
                ];
                if let Some(gens) = p.next() {
                    args.push("--gens".into());
                    args.push(gens.to_string());
                }
                if let Some(pop) = p.next() {
                    args.push("--pop".into());
                    args.push(pop.to_string());
                }
                self.launch_console(args);
            }
            _ => self.push_error(
                "usage: /bench [list | add <name> <prompt> | remove <name> | run <name> [models] \
                 | optimize <name|all> [gens] [pop]]"
                    .into(),
            ),
        }
    }

    /// Launch `nur <args>` in a new console window (long jobs) + note it.
    fn launch_console(&mut self, args: Vec<String>) {
        let exe = match std::env::current_exe() {
            Ok(p) => p,
            Err(e) => {
                self.push_error(format!("cannot locate the nur executable: {e}"));
                return;
            }
        };
        match spawn_console(&exe, &args) {
            Ok(()) => self.push_note(
                Tone::Neutral,
                format!(
                    "launched `nur {}` in a new window — watch progress there",
                    args.join(" ")
                ),
            ),
            Err(e) => self.push_error(format!("could not launch `nur {}`: {e}", args.join(" "))),
        }
    }

    fn cmd_undo(&mut self) {
        match crate::tools::undo::undo_last(&self.session_id) {
            Ok(msg) => {
                let left = crate::tools::undo::depth(&self.session_id);
                self.push_note(Tone::Session, format!("undo · {msg}  ({left} more)"));
            }
            Err(e) => self.push_note(Tone::Neutral, format!("undo · {e}")),
        }
    }

    /// Cost-saver for the **prompt** (not spend caps). Optional args:
    /// `status` / `show` — report poor + budget without toggling.
    fn cmd_poor(&mut self, arg: &str) {
        let a = arg.trim().to_ascii_lowercase();
        if a == "status" || a == "show" {
            let poor = if self.cfg.poor_mode { "ON" } else { "OFF" };
            self.push_note(
                Tone::Usage,
                format!(
                    "poor mode {poor}  ·  budgets {}\n  \
                     /poor           toggle prompt saver (PLUR/skills/memory off)\n  \
                     /budget …       optional spend/turn ceilings (default ∞)\n  \
                     /budget clear   unlimited everything this process",
                    self.budget_status_line()
                ),
            );
            return;
        }
        if !a.is_empty() && a != "on" && a != "off" && a != "toggle" {
            self.push_error("usage: /poor  |  /poor on|off  |  /poor status".into());
            return;
        }
        match a.as_str() {
            "on" => self.cfg.poor_mode = true,
            "off" => self.cfg.poor_mode = false,
            _ => self.cfg.poor_mode = !self.cfg.poor_mode,
        }
        if self.cfg.poor_mode {
            self.push_note(
                Tone::Usage,
                format!(
                    "poor mode ON · skipping PLUR auto-inject and long memory \
                     (tools + skill NL/slash still full). budgets {}\n  \
                     /poor again to restore · /budget cost|tokens|turns for optional ceilings \
                     (all default unlimited) · set poor_mode=true in config.toml to persist",
                    self.budget_status_line()
                ),
            );
        } else {
            self.push_note(
                Tone::Usage,
                format!(
                    "poor mode OFF · full prompt context restored · budgets {}",
                    self.budget_status_line()
                ),
            );
        }
    }

    fn cmd_permissions(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg == "reload" {
            self.permissions.reload(&self.cwd);
            self.push_note(
                Tone::Skill,
                format!("permissions reloaded\n{}", self.permissions.summary()),
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

    /// Optional session ceilings (all unlimited by default): cost, tokens, turns.
    /// Show, set, clear, or save to config.toml. `0` / `off` / `∞` clears a cap.
    fn cmd_budget(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() || arg == "show" || arg == "status" {
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
            let turns = if self.cfg.max_turns == 0 {
                "unlimited".into()
            } else {
                self.cfg.max_turns.to_string()
            };
            let used_c = self.u_session.estimated_cost_usd();
            let used_t = self.u_session.total_tokens;
            let poor = if self.cfg.poor_mode { "ON" } else { "OFF" };
            self.push_note(
                Tone::Usage,
                format!(
                    "budget (defaults unlimited)\n  \
                     cost     {cost}  · spent ~${used_c:.4} (list-price est · see /usage)\n  \
                     tokens   {toks}  · used {}\n  \
                     turns    {turns}  · agent rounds per prompt\n  \
                     poor     {poor}  · prompt saver (/poor)\n  \
                     /budget cost <usd|unlimited|0>   e.g. /budget cost 2.5\n  \
                     /budget tokens <n|unlimited|0>   e.g. /budget tokens 500000\n  \
                     /budget turns <n|unlimited|0>    e.g. /budget turns 80\n  \
                     /turns <n|unlimited|0>           short for /budget turns\n  \
                     /budget clear                    unlimited everything this process\n  \
                     /budget save                     write ceilings to config.toml",
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
                self.cfg.max_turns = 0;
                self.push_note(
                    Tone::Usage,
                    "budget cleared · cost ∞ · tokens ∞ · turns ∞ (this process)".into(),
                );
            }
            "save" => match crate::config::save_config(&self.cfg) {
                Ok(()) => self.push_note(
                    Tone::Usage,
                    format!(
                        "budget saved ({}) → {}",
                        self.budget_status_line(),
                        crate::config::config_path().display()
                    ),
                ),
                Err(e) => self.push_error(format!("could not save config: {e}")),
            },
            "cost" => {
                let Some(v) = parts.next() else {
                    self.push_error("usage: /budget cost <usd|unlimited|0|off>".into());
                    return;
                };
                if is_unlimited_token(v) {
                    self.cfg.max_session_cost_usd = None;
                    self.push_note(
                        Tone::Usage,
                        "budget cost unlimited (this process · /budget save to persist)".into(),
                    );
                    return;
                }
                match v.parse::<f64>() {
                    Ok(n) if n.is_finite() && n > 0.0 => {
                        self.cfg.max_session_cost_usd = Some(n);
                        self.push_note(
                            Tone::Usage,
                            format!(
                                "budget cost set to ${n:.4} (this process · /budget save to persist)"
                            ),
                        );
                    }
                    Ok(n) if n == 0.0 => {
                        self.cfg.max_session_cost_usd = None;
                        self.push_note(
                            Tone::Usage,
                            "budget cost unlimited (this process · /budget save to persist)".into(),
                        );
                    }
                    _ => {
                        self.push_error("cost must be a positive number, or unlimited|0|off".into())
                    }
                }
            }
            "tokens" => {
                let Some(v) = parts.next() else {
                    self.push_error("usage: /budget tokens <n|unlimited|0|off>".into());
                    return;
                };
                if is_unlimited_token(v) {
                    self.cfg.max_session_tokens = None;
                    self.push_note(
                        Tone::Usage,
                        "budget tokens unlimited (this process · /budget save to persist)".into(),
                    );
                    return;
                }
                match v.parse::<u64>() {
                    Ok(0) => {
                        self.cfg.max_session_tokens = None;
                        self.push_note(
                            Tone::Usage,
                            "budget tokens unlimited (this process · /budget save to persist)"
                                .into(),
                        );
                    }
                    Ok(n) => {
                        self.cfg.max_session_tokens = Some(n);
                        self.push_note(
                            Tone::Usage,
                            format!(
                                "budget tokens set to {} (this process · /budget save to persist)",
                                fmt_num(n)
                            ),
                        );
                    }
                    _ => self
                        .push_error("tokens must be a positive integer, or unlimited|0|off".into()),
                }
            }
            "turns" => {
                // bare `/budget turns` or `/turns` with no arg → show status
                let Some(v) = parts.next() else {
                    let turns = if self.cfg.max_turns == 0 {
                        "unlimited".into()
                    } else {
                        self.cfg.max_turns.to_string()
                    };
                    self.push_note(
                        Tone::Usage,
                        format!(
                            "turns per prompt  {turns}\n  \
                             /budget turns <n|unlimited|0>   cap agent rounds\n  \
                             /turns <n|unlimited|0>          same"
                        ),
                    );
                    return;
                };
                if is_unlimited_token(v) {
                    self.cfg.max_turns = 0;
                    self.push_note(
                        Tone::Usage,
                        "turns unlimited (this process · /budget save to persist)".into(),
                    );
                    return;
                }
                match v.parse::<u32>() {
                    Ok(0) => {
                        self.cfg.max_turns = 0;
                        self.push_note(
                            Tone::Usage,
                            "turns unlimited (this process · /budget save to persist)".into(),
                        );
                    }
                    Ok(n) if n <= 1_000_000 => {
                        self.cfg.max_turns = n;
                        self.push_note(
                            Tone::Usage,
                            format!(
                                "turns set to {n} per prompt (this process · /budget save to persist)"
                            ),
                        );
                    }
                    _ => self.push_error(
                        "turns must be a positive integer ≤ 1000000, or unlimited|0|off".into(),
                    ),
                }
            }
            _ => self.push_error(
                "usage: /budget [cost|tokens|turns] <n|unlimited|0|off> · clear · save".into(),
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
                 window   {} tok  ·  meter = last-turn tokens / configured window\n  \
                 cached   {} tok  (prompt-cache hits when the API reports them)\n  \
                 tip  window comes from models.dev when known, else config.toml context_window\n  \
                 /compact frees context when this climbs",
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
        let auth = if self.authed {
            "signed in"
        } else {
            "no key - /login"
        };
        let bro = if self.bro {
            "\n  bro      on - chill delivery"
        } else {
            ""
        };
        let sticky = if self.sticky_skills.is_empty() {
            String::new()
        } else {
            format!("\n  sticky   {}", self.sticky_skills.join(", "))
        };
        self.push_note(
            Tone::Session,
            format!(
                "status\n  version  nur v{}\n  model    {}  - effort {}\n  mode     {}  ({})\n  \
                 session  {}\n  cwd      {}\n  auth     {}\n  tokens   {} session - ctx {ctx_pct:.0}%  - ~${:.4}{bro}{sticky}",
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

    /// Inline health check — the interactive cousin of `nur doctor`.
    fn cmd_doctor(&mut self) {
        let sh = crate::tools::shell_backend();
        let auth = if self.authed {
            "signed in"
        } else {
            "no key — /login"
        };
        let mut lines = format!(
            "doctor · nur v{}\n  model    {}\n  cwd      {}\n  auth     {}\n  shell    {}\n",
            env!("CARGO_PKG_VERSION"),
            self.cfg.model,
            self.cwd.display(),
            auth,
            sh.label,
        );
        lines.push_str("\n");
        lines.push_str(&crate::ecosystem::quick_status());
        lines.push_str("\n");
        lines.push_str(&crate::plugins::quick_status());
        self.push_note(Tone::Skill, lines);
    }

    fn cmd_model(&mut self, arg: &str) {
        // No argument → open the live model chooser for the active provider.
        // `/model <id>` still switches directly (scripts / power users).
        if arg.is_empty() {
            self.open_model_picker();
            return;
        }
        self.apply_model_selection(arg);
    }

    fn cmd_plugins(&mut self, arg: &str) {
        // Bare `/plugins` → marketplace picker (same UX as provider picker).
        let arg = arg.trim();
        if arg.is_empty() {
            self.open_plugin_picker();
            return;
        }
        let mut parts = arg.split_whitespace();
        let cmd = parts.next().unwrap_or("").to_lowercase();
        let id = parts.next().unwrap_or("").to_string();
        match cmd.as_str() {
            "list" | "ls" => {
                self.push_note(Tone::Skill, crate::plugins::quick_status());
                let rows = crate::plugins::marketplace_rows();
                let mut lines = String::from("catalog:\n");
                for r in rows {
                    lines.push_str(&format!(
                        "  {:<16}  {}  {}\n",
                        r.id,
                        r.status_badge(),
                        r.name
                    ));
                }
                self.push_note(Tone::Skill, lines);
            }
            "install" | "add" => {
                if id.is_empty() {
                    self.push_error("usage: /plugins install <id>  (or /plugins to browse)".into());
                    return;
                }
                match crate::plugins::install_plugin(&id) {
                    Ok(msg) => self.push_info(msg),
                    Err(e) => self.push_error(e),
                }
            }
            "enable" => {
                if id.is_empty() {
                    self.push_error("usage: /plugins enable <id>".into());
                    return;
                }
                match crate::plugins::set_enabled(&id, true) {
                    Ok(()) => self.push_info(format!("enabled {id}")),
                    Err(e) => self.push_error(e),
                }
            }
            "disable" => {
                if id.is_empty() {
                    self.push_error("usage: /plugins disable <id>".into());
                    return;
                }
                match crate::plugins::set_enabled(&id, false) {
                    Ok(()) => self.push_info(format!("disabled {id}")),
                    Err(e) => self.push_error(e),
                }
            }
            "uninstall" | "remove" => {
                if id.is_empty() {
                    self.push_error("usage: /plugins uninstall <id>".into());
                    return;
                }
                match crate::plugins::uninstall_plugin(&id) {
                    Ok(msg) => self.push_info(msg),
                    Err(e) => self.push_error(e),
                }
            }
            other => {
                // Treat bare id as install/open hint.
                if crate::plugins::by_id(other).is_some() {
                    match crate::plugins::install_plugin(other) {
                        Ok(msg) => self.push_info(msg),
                        Err(e) => self.push_error(e),
                    }
                } else {
                    self.push_error(format!(
                        "unknown /plugins arg '{other}' — try list · install · enable · disable · uninstall"
                    ));
                }
            }
        }
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

    pub(super) fn cmd_resume(&mut self, arg: &str) {
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
                let mut tracker =
                    UsageTracker::new(loaded.id.clone(), self.cfg.model.clone(), self.cwd.clone());
                tracker.set_provider(self.cfg.provider.clone());
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
                self.replay_session_history();
            }
            Err(e) => self.push_error(format!("could not open session: {e}")),
        }
    }

    /// takeover — cross-agent session migration (chagent engine).
    ///
    /// - `/takeover`                     open the takeover window (parity with
    ///                                   `/sessions`, foreign sessions only)
    /// - `/takeover ls [agent]`          list migratable sessions
    /// - `/takeover <agent> [id|latest]` import that session and resume it
    ///
    /// Alias: `/hijack`. Press `c` in either window to switch to the other.
    pub(super) fn cmd_chagent(&mut self, arg: &str) {
        if self.busy {
            self.push_error("wait for the current turn to finish".into());
            return;
        }
        let arg = arg.trim();
        let mut it = arg.split_whitespace();
        let first = it.next().unwrap_or("");
        match first.to_ascii_lowercase().as_str() {
            "" => self.open_chagent_picker(),
            "ls" | "list" => {
                let tool = it.next().map(|s| s.to_string());
                self.chagent_list(tool);
            }
            "help" | "-h" | "--help" | "?" => self.push_note(
                Tone::Session,
                "takeover — migrate a session from another agent into nur\n  \
                 /takeover                 import picker (Claude · Codex · Cursor · Grok)\n  \
                 /takeover ls [agent]      list migratable sessions\n  \
                 /takeover <agent> [ref]   import <ref|latest> and resume it\n  \
                 alias: /hijack  ·  press c in the picker to switch to /sessions"
                    .into(),
            ),
            tool if crate::agent::chagent::is_foreign_tool(tool) => {
                let reference: String = it.collect::<Vec<_>>().join(" ");
                let reference = if reference.trim().is_empty() {
                    "latest".to_string()
                } else {
                    reference
                };
                self.chagent_migrate(tool, &reference);
            }
            other => self.push_error(format!(
                "chagent: unknown agent '{other}' — try claude · codex · cursor · grok, \
                 or /chagent (picker) / /chagent ls"
            )),
        }
    }

    /// List migratable foreign sessions as a transcript note.
    pub(super) fn chagent_list(&mut self, tool: Option<String>) {
        let cwd = self.cwd.display().to_string();
        let mut errors = Vec::new();
        let found = match tool {
            // Matches the takeover window: every workspace, not just this cwd.
            Some(t) => match crate::agent::chagent::list_foreign(&t, &cwd, 0, true) {
                Ok(v) => v,
                Err(e) => {
                    self.push_error(format!("takeover: {e}"));
                    return;
                }
            },
            None => crate::agent::chagent::list_all(&cwd, 0, true, &mut errors),
        };
        if found.is_empty() {
            let mut m = "takeover · no migratable sessions found".to_string();
            if !errors.is_empty() {
                m.push_str(&format!("\n  {}", errors.join("\n  ")));
            }
            self.push_note(Tone::Session, m);
            return;
        }
        let mut out = format!("chagent · {} migratable session(s):", found.len());
        for fs in found.iter().take(40) {
            out.push_str(&format!(
                "\n  {:<12} {}  ·  {}  ·  {}",
                crate::agent::chagent::tool_label(&fs.tool),
                fs.short_id(),
                fs.updated().format("%b %d %H:%M"),
                fs.preview(),
            ));
        }
        out.push_str("\n\nimport:  /chagent <agent> <id|latest>   (e.g.  /chagent claude latest)");
        self.push_note(Tone::Session, out);
    }

    /// Import one foreign session into a native session, then resume it.
    pub(super) fn chagent_migrate(&mut self, tool: &str, reference: &str) {
        if self.busy {
            self.push_error("wait for the current turn to finish".into());
            return;
        }
        let cwd = self.cwd.display().to_string();
        let model = self.cfg.model.clone();
        match crate::agent::chagent::migrate(tool, reference, &cwd, &model) {
            Ok(mig) => {
                let id = mig.session.id.clone();
                let label = mig.source_label.clone();
                let src_short: String = mig.source_id.chars().take(8).collect();
                let turns = mig.imported_turns;
                let src_cwd = mig.source_cwd.clone();
                let warnings = mig.warnings.clone();
                // Resume the freshly-saved native session (re-homes + replays).
                self.cmd_resume(&id);
                let mut note =
                    format!("chagent · imported from {label} ({src_short})  ·  {turns} turns");
                if let Some(oc) = src_cwd {
                    if !oc.eq_ignore_ascii_case(&cwd) {
                        note.push_str(&format!("\n  was  {oc}  ·  tools sandboxed here"));
                    }
                }
                note.push_str(
                    "\n  ⚠ transcript is inert history — verify files/branch/tests before continuing",
                );
                for w in warnings.iter().take(4) {
                    note.push_str(&format!("\n  ⚠ {w}"));
                }
                self.push_note(Tone::Session, note);
            }
            Err(e) => self.push_error(format!("chagent import failed: {e}")),
        }
    }

    fn cmd_config(&mut self) {
        let turns = if self.cfg.max_turns == 0 {
            "unlimited (0)".into()
        } else {
            self.cfg.max_turns.to_string()
        };
        self.push_info(format!(
            "config ({})\n  model           {}\n  base_url        {}\n  effort          {}\n  \
             max_turns       {}\n  stream          {}\n  context_window  {}\n  \
             budgets         {}\n\npaths\n  \
             home     {}\n  status   {}\n  usage    {}\n  sessions {}",
            crate::config::config_path().display(),
            self.cfg.model,
            self.cfg.base_url,
            self.cfg.reasoning_effort,
            turns,
            self.cfg.stream,
            fmt_num(self.cfg.context_window),
            self.budget_status_line(),
            crate::config::muse_home().display(),
            crate::config::status_path().display(),
            crate::config::usage_log_path().display(),
            crate::config::sessions_dir().display(),
        ));
    }

    /// Standing session goal, prepended to every turn as context (invisible in
    /// the transcript). `/goal` shows it; `/goal clear` removes it.
    fn cmd_goal(&mut self, arg: &str) {
        let arg = arg.trim();
        match arg {
            "" => match &self.session_goal {
                Some(g) => {
                    self.push_note(Tone::Plan, format!("goal · {g}\n  /goal clear to drop it"))
                }
                None => self.push_info(
                    "no session goal set  ·  /goal <what you're trying to achieve>".into(),
                ),
            },
            "clear" | "none" | "off" => {
                self.session_goal = None;
                self.push_info("session goal cleared".into());
            }
            _ => {
                self.session_goal = Some(arg.to_string());
                self.push_note(
                    Tone::Plan,
                    format!("goal set · {arg}\n  every turn now carries this as context"),
                );
            }
        }
    }

    /// `/draw` — manage the tldraw offline app and open/build interactive
    /// `.tldraw` boards. No arg → status; `install` → fetch the app; a file path
    /// → open it directly; anything else → seed a turn that builds a board.
    fn cmd_draw(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() {
            match crate::tools::tldraw::app_path() {
                Some(p) => self.push_note(
                    Tone::Neutral,
                    format!(
                        "tldraw offline · installed ({})\n  /draw <file.tldraw> open · \
                         /draw <idea> build a board · /draw install reinstall",
                        p.display()
                    ),
                ),
                None => self.push_note(
                    Tone::Mode,
                    "tldraw offline · not installed\n  /draw install to fetch the official app, \
                     then /draw <file.tldraw> to open"
                        .into(),
                ),
            }
            return;
        }
        if arg.eq_ignore_ascii_case("install") || arg.eq_ignore_ascii_case("setup") {
            if !self.authed {
                self.push_error("signed out — /login first".into());
                return;
            }
            if self.busy {
                self.push_error("busy — finish the current turn first".into());
                return;
            }
            self.start_turn_labeled(
                "/draw install",
                "Install the tldraw offline desktop app by calling the `tldraw` tool with \
                 action=install. Then call action=status and report whether it installed.",
            );
            return;
        }
        if arg.ends_with(".tldraw") || arg.ends_with(".tldr") {
            self.draw_open_file(arg);
            return;
        }
        if !self.authed {
            self.push_error("signed out — /login first".into());
            return;
        }
        if self.busy {
            self.push_error("busy — finish the current turn first".into());
            return;
        }
        let model_prompt = format!(
            "Design / open an interactive tldraw offline board for this request.\n{arg}\n\n\
             Use the `tldraw` tool (and shell only if you need the canvas HTTP API beyond the tool).\n\n\
             Capabilities (official offline app):\n\
             - Static boards: action=create (Desktop .tldraw, dark theme, contrast-safe shapes).\n\
             - Interactive boards with document scripts / agent-shapes: action=open path= to an existing \
             interactive file (ZIP .tldraw with script/). open AUTO-ENABLES scripts (script-workspace → applied).\n\
             - Live edits: action=api code=\"return await api.getDocs()\" etc. after open.\n\
             - Re-enable scripts: action=enable_scripts path=…\n\n\
             Rules:\n\
             1. Install if needed (action=install), then status.\n\
             2. Prefer open of a real interactive board when the user names a .tldraw path \
             (e.g. C:\\\\Users\\\\david\\\\Scripts\\\\nn-digits.tldraw).\n\
             3. For new static diagrams use create (title + shapes). NEVER write_file fake JSON.\n\
             4. Contrast-aware shapes under dark theme (blue/green/red/… with readable labels).\n\
             5. After open, confirm scripts line shows state=applied when hasScript boards.\n\
             6. Report Desktop/path + Alt+Tab for the window.",
        );
        self.start_turn_labeled(&format!("/draw {arg}"), &model_prompt);
    }

    /// Open a `.tldraw`/`.tldr` file directly in the installed app.
    /// Prefer Desktop (create output location), then cwd, then absolute.
    fn draw_open_file(&mut self, rel: &str) {
        let p = std::path::Path::new(rel);
        let abs = if p.is_absolute() {
            p.to_path_buf()
        } else {
            let desk = crate::tools::tldraw::desktop_dir().join(rel);
            if desk.is_file() {
                desk
            } else {
                let name = p
                    .file_name()
                    .map(|n| crate::tools::tldraw::desktop_dir().join(n));
                match name {
                    Some(n) if n.is_file() => n,
                    _ => self.cwd.join(rel),
                }
            }
        };
        if !abs.is_file() {
            self.push_error(format!(
                "no such file: {}\n  (new boards land on Desktop: {})",
                abs.display(),
                crate::tools::tldraw::desktop_dir().display()
            ));
            return;
        }
        match crate::tools::tldraw::launch_on_file(&abs) {
            Ok(msg) => self.push_note(Tone::Neutral, msg),
            Err(e) => self.push_error(e.to_string()),
        }
    }

    /// Inject a message into the **running** turn without cancelling it
    /// (steering). Idle → nothing to steer, so it sends normally.
    fn cmd_steer(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() {
            self.push_info(
                "steer · /steer <text> feeds a message into the running turn on its next round \
                 (no cancel). While a turn runs you can also type a follow-up and click \
                 'steer' on its queued card."
                    .into(),
            );
            return;
        }
        if !self.authed {
            self.push_error("signed out — run /login first".into());
            return;
        }
        if self.busy {
            self.steer_now(arg);
        } else {
            self.push_note(
                Tone::Mode,
                "no turn running — sending as a normal message".into(),
            );
            self.start_turn(arg);
        }
    }

    /// Map this codebase and publish a shareable scan to foglamp.dev. Seeds an
    /// agent turn with the bundled `scan` skill; an optional `/scan <focus>`
    /// centers the map on one area. The transcript shows a short label while the
    /// model receives the full instruction template.
    fn cmd_scan(&mut self, arg: &str) {
        if !self.authed {
            self.push_error("signed out — run /login before /scan".into());
            return;
        }
        if self.busy {
            self.push_error("busy — wait for the current turn to finish, then /scan".into());
            return;
        }
        // Plan mode blocks write_file — scan must write `.foglamp/scan.json`.
        // Lift to auto so the map is produced; upload still waits on user yes
        // (skill step 3). Manual would also work but multi-file explore is noisy.
        if self.permission_mode.get() == PermissionMode::Plan {
            self.set_permission_mode(PermissionMode::Auto);
            self.push_note(
                Tone::Mode,
                "scan needs writes — switched plan → auto for this map \
                 (upload still asks first)"
                    .into(),
            );
        }
        let focus = arg.trim();
        let display = if focus.is_empty() {
            "/scan · map this codebase → foglamp".to_string()
        } else {
            format!("/scan · {focus}")
        };
        // If a prior local map exists, surface the path so "where is it" is obvious.
        let local = self.cwd.join(".foglamp").join("scan.json");
        if local.is_file() {
            self.push_note(
                Tone::Skill,
                format!(
                    "existing local map · {} · re-scan will refresh it · \
                     upload only after you say yes",
                    local.display()
                ),
            );
        } else {
            self.push_note(
                Tone::Skill,
                "scanning → local `.foglamp/scan.json` first · \
                 you'll be asked before anything uploads to foglamp.dev"
                    .into(),
            );
        }
        self.start_turn_labeled(&display, &scan_prompt(focus));
    }

    /// Toggle chill mode: every turn carries the `BRO_STYLE` rider so replies
    /// come back in plain, low-jargon language. `on`/`off` force a state.
    fn cmd_bro(&mut self, arg: &str) {
        self.bro = match arg.trim().to_lowercase().as_str() {
            "on" | "yes" | "1" => true,
            "off" | "no" | "0" => false,
            _ => !self.bro,
        };
        if self.bro {
            self.push_note(
                Tone::Mode,
                "bro mode on · plain words, straight answers — same facts, chill delivery\n  \
                 /bro again (or /bro off) to switch back"
                    .into(),
            );
        } else {
            self.push_note(Tone::Mode, "bro mode off · back to normal".into());
        }
    }

    /// One-off "by the way" note folded into the *next* turn only.
    fn cmd_btw(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() {
            self.push_info("usage: /btw <a note to add to your next message>".into());
            return;
        }
        self.pending_btw.push(arg.to_string());
        self.push_note(
            Tone::Neutral,
            format!(
                "noted · will ride along with your next message ({})",
                self.pending_btw.len()
            ),
        );
    }

    /// Fast code search over the workspace (ripgrep via the `grep` tool).
    fn cmd_codesearch(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() {
            self.push_info("usage: /codesearch <regex or text>  (alias /cs)".into());
            return;
        }
        let host = ToolHost::default();
        let ctx = crate::tools::ToolContext {
            cwd: self.cwd.clone(),
            cancel: CancellationToken::new(),
        };
        let args = serde_json::json!({ "pattern": arg }).to_string();
        match host.dispatch("grep", &args, &ctx) {
            Ok(s) => {
                let body = if s.trim().is_empty() {
                    format!("no matches for `{arg}`")
                } else {
                    format!("codesearch · {arg}\n{s}")
                };
                self.push_note(Tone::Neutral, body);
            }
            Err(e) => self.push_error(e.to_string()),
        }
    }

    /// File a GitHub issue from the TUI. Uses `gh` when available (creates the
    /// issue and returns its URL); otherwise opens a prefilled new-issue page.
    fn cmd_feedback(&mut self, arg: &str) {
        let arg = arg.trim();
        if arg.is_empty() {
            self.push_info("usage: /feedback <what happened / what you'd like>".into());
            return;
        }
        const REPO: &str = "nuroctane/nur-cli";
        let title: String = arg.lines().next().unwrap_or(arg).chars().take(80).collect();
        let body = format!(
            "{arg}\n\n---\nnur v{}  ·  {}  ·  model {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            self.cfg.model,
        );
        if crate::ecosystem::find_bin("gh").is_some() {
            let out = std::process::Command::new("gh")
                .args([
                    "issue", "create", "--repo", REPO, "--title", &title, "--body", &body,
                ])
                .output();
            match out {
                Ok(o) if o.status.success() => {
                    let url = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    self.push_note(Tone::Session, format!("feedback filed · {url}"));
                    return;
                }
                Ok(o) => {
                    let err = String::from_utf8_lossy(&o.stderr);
                    // gh present but not authed/failed → fall through to browser.
                    self.push_info(format!(
                        "gh couldn't file it ({}); opening the browser…",
                        err.trim()
                    ));
                }
                Err(_) => {}
            }
        }
        // Browser fallback: prefilled new-issue page.
        let url = format!(
            "https://github.com/{REPO}/issues/new?title={}&body={}",
            urlencode(&title),
            urlencode(&body)
        );
        if open_in_browser(&url) {
            self.push_note(
                Tone::Session,
                "opened a prefilled issue in your browser".into(),
            );
        } else {
            self.push_info(format!("file it here:\n  {url}"));
        }
    }

    /// Manage MCP servers via the Executor gateway (executor.sh).
    fn cmd_mc(&mut self, arg: &str) {
        let arg = arg.trim();
        let action = if arg.is_empty() { "sources" } else { arg };
        let json = match action {
            "sources" | "list" | "ls" => r#"{"action":"sources"}"#.to_string(),
            "status" => r#"{"action":"status"}"#.to_string(),
            "search" | "find" => {
                self.push_error(
                    "usage: /mc search <query>  — use the executor tool for calls".into(),
                );
                return;
            }
            _ if action.starts_with("search ") => {
                let q = action.trim_start_matches("search ").trim();
                serde_json::json!({"action":"search","query":q}).to_string()
            }
            other => {
                self.push_error(format!(
                    "unknown /mc action '{other}' — try: sources · status · search <q>"
                ));
                return;
            }
        };
        let host = ToolHost::default();
        let ctx = crate::tools::ToolContext {
            cwd: self.cwd.clone(),
            cancel: CancellationToken::new(),
        };
        match host.dispatch("executor", &json, &ctx) {
            Ok(s) => self.push_note(
                Tone::Skill,
                format!(
                    "mcp servers (via executor gateway)\n{s}\n\n\
                     add one:  executor tool → action=call, or `executor install`\n\
                     the agent uses the `executor` tool for OpenAPI/GraphQL/MCP calls"
                ),
            ),
            Err(e) => self.push_error(format!(
                "{e}\n  MCP is provided by the Executor gateway — `nur ecosystem ensure` installs it"
            )),
        }
    }

    /// The interaction tips that used to clutter the opening banner.
    fn cmd_tips(&mut self) {
        self.push_note(
            Tone::Mode,
            "tips\n  \
             drag text            select + auto-copy\n  \
             drag the scrollbar   scrub history (right edge)\n  \
             click a card  ·  ▸   peek  ·  expand\n  \
             click http(s) link   open in your default browser\n  \
             right/2×-click prompt  fork · edit · revert · copy\n  \
             ↓ End                jump to latest\n  \
             Shift+Tab            cycle manual → plan → auto\n  \
             Ctrl+A/C/V/X         select-all · copy · paste · cut\n  \
             paste a big block    collapses into a [pasted N lines] chip\n  \
             Esc                  close peek → cancel turn → clear input\n  \
             /help                full command + key reference"
                .into(),
        );
    }
}

/// Minimal percent-encoding for GitHub issue query params (no extra dep).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Open a URL in the OS default browser (best-effort).
fn open_in_browser(url: &str) -> bool {
    #[cfg(windows)]
    let r = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(url).spawn();
    r.is_ok()
}

/// Values that mean "no cap" for `/budget cost|tokens|turns` and `/turns`.
/// Accepts `unlimited`, `0`, `off`, and common synonyms (case-insensitive).
fn is_unlimited_token(v: &str) -> bool {
    let t = v.trim().to_ascii_lowercase();
    matches!(
        t.as_str(),
        "0" | "off"
            | "none"
            | "null"
            | "nil"
            | "unlimited"
            | "unlimit"
            | "inf"
            | "infinity"
            | "infinite"
            | "∞"
            | "-"
            | "clear"
            | "reset"
    )
}

#[cfg(test)]
mod unlimited_token_tests {
    use super::is_unlimited_token;

    #[test]
    fn accepts_unlimited_and_zero() {
        for v in [
            "0",
            "unlimited",
            "UNLIMITED",
            "off",
            "Off",
            "none",
            "inf",
            "infinity",
            "∞",
            "clear",
        ] {
            assert!(is_unlimited_token(v), "should accept {v}");
        }
        assert!(!is_unlimited_token("80"));
        assert!(!is_unlimited_token("2.5"));
    }
}
