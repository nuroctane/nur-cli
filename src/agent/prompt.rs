use super::memory::memory_prompt_excerpt;
use super::mode::PermissionMode;
use super::skills::{load_skills, skill_activation};
use crate::ecosystem;
use crate::tools::shell_backend;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Attribution for the outgoing prompt. Counts are deliberately local estimates
/// (roughly four characters per token) so the budget guard still has a safe
/// answer when a provider omits usage metadata.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptAttribution {
    pub system_tokens: u64,
    pub tool_schema_tokens: u64,
    pub dialogue_tokens: u64,
    pub tool_output_tokens: u64,
    pub stable_prefix_fingerprint: String,
}

impl PromptAttribution {
    pub fn estimated_input_tokens(&self) -> u64 {
        self.system_tokens
            .saturating_add(self.tool_schema_tokens)
            .saturating_add(self.dialogue_tokens)
            .saturating_add(self.tool_output_tokens)
    }
}

fn estimate_tokens(text: &str) -> u64 {
    // Four characters/token is intentionally slightly conservative for JSON,
    // paths, and source snippets, which dominate agent prompts.
    ((text.chars().count() as u64).saturating_add(3)) / 4
}

/// Chars a provider effectively bills for one inline image: vision models
/// tokenize by tiles (~1k tokens for a screenshot), not by the base64 text.
/// Counting the data URL as text made a single 380 KB screenshot look like
/// ~95k tokens and triggered compaction before the first request.
pub const IMAGE_CHARS_ALLOWANCE: u64 = 4_096;

/// Token estimate for one input item with inline media priced as media.
pub fn estimate_item_tokens(item: &Value) -> u64 {
    fn chars(value: &Value, key: Option<&str>) -> u64 {
        match value {
            Value::Null => 4,
            Value::Bool(_) => 5,
            Value::Number(n) => n.to_string().len() as u64,
            Value::String(text)
                if matches!(key, Some("image_url" | "image" | "data"))
                    && text.starts_with("data:") =>
            {
                IMAGE_CHARS_ALLOWANCE.min(text.len() as u64)
            }
            Value::String(text) => text.chars().count() as u64 + 2,
            Value::Array(values) => values.iter().map(|v| chars(v, None) + 1).sum::<u64>() + 2,
            Value::Object(map) => {
                map.iter()
                    .map(|(k, v)| k.len() as u64 + 3 + chars(v, Some(k)) + 1)
                    .sum::<u64>()
                    + 2
            }
        }
    }
    chars(item, None).saturating_add(3) / 4
}

fn stable_fingerprint(parts: &[&str]) -> String {
    // FNV-1a is deterministic across processes and is only a telemetry label,
    // not a security primitive. Do not expose prompt material in status text.
    let mut h: u64 = 0xcbf29ce484222325;
    for part in parts {
        for b in part.as_bytes() {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    format!("{h:016x}")
}

/// Attribute a rendered request without retaining or displaying its contents.
/// Tool outputs are split from ordinary dialogue to make repeated schemas and
/// large observations visible in request telemetry.
pub fn attribute_request(
    instructions: &str,
    tools: &[crate::api::types::ToolDef],
    input_items: &[Value],
) -> PromptAttribution {
    let tool_json = serde_json::to_string(tools).unwrap_or_default();
    let mut out = PromptAttribution {
        system_tokens: estimate_tokens(instructions),
        tool_schema_tokens: estimate_tokens(&tool_json),
        stable_prefix_fingerprint: stable_fingerprint(&[instructions, &tool_json]),
        ..Default::default()
    };
    for item in input_items {
        let tokens = estimate_item_tokens(item);
        let is_tool_output = item
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind == "function_call_output");
        if is_tool_output {
            out.tool_output_tokens = out.tool_output_tokens.saturating_add(tokens);
        } else {
            out.dialogue_tokens = out.dialogue_tokens.saturating_add(tokens);
        }
    }
    out
}

/// Project instruction files (first found wins). NUR.md is preferred.
pub const PROJECT_INSTRUCTION_FILES: &[&str] = &["NUR.md", "AGENTS.md", "CLAUDE.md"];

pub fn find_project_instructions(cwd: &Path) -> Option<(String, String)> {
    for name in PROJECT_INSTRUCTION_FILES {
        let p = cwd.join(name);
        if p.is_file() {
            if let Ok(text) = std::fs::read_to_string(&p) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    let capped: String = trimmed.chars().take(20_000).collect();
                    return Some((name.to_string(), capped));
                }
            }
        }
    }
    None
}

/// The parts of the system prompt that come off disk (project instructions,
/// memory, shell probe) plus **on-demand** skill activation.
///
/// Skills are **not** catalogued into every prompt (that burned tokens on
/// large installs). They activate only via natural-language intent matching
/// or slash commands - works for every provider.
///
/// Built **once per user turn** so disk is not re-read on every model round.
pub struct PromptContext {
    cwd: PathBuf,
    is_subagent: bool,
    /// Active model id (wire format).
    model: String,
    /// Human provider name (e.g. "xAI Grok", "Meta Model API").
    provider: String,
    shell_label: String,
    project: Option<(String, String)>,
    memory: String,
    /// PLUR inject block - auto-loaded so the agent remembers past corrections.
    plur: String,
    /// OptMem wake + rules (upstream ~/.optmem); skipped for subagents / poor_mode / disabled.
    optmem: String,
    /// Natural-language skill activation for this user turn (injected body).
    activation: String,
    /// Short label for TUI status when activation fires (e.g. `fable-method`).
    activation_label: Option<String>,
    /// Name of the skill that activated this turn, for the skill layer's own
    /// usage check (no separate reviewer - the skill machinery owns it).
    activation_skill: Option<String>,
    /// The skill's own checkable requirements, extracted by code.
    activation_requirements: Vec<String>,
    /// Path of the activated skill file (diagnostics + status).
    activation_path: Option<String>,
    /// Provider aliases the user named in this turn's message (for agent.provider nudge).
    named_providers: Vec<String>,
    /// Effort `ultracode`: orchestrate work that splits across parallel subagents.
    ultracode: bool,
}

/// Effort `ultracode` for the root agent. The strongest effort rung already
/// rides the request; this adds the orchestration half.
const ULTRACODE_BLOCK: &str = r#"
# Effort: ULTRACODE
The user chose maximum effort with multi-agent orchestration. When the work splits:
- Plan it with todo_write first: independent pieces versus steps that must stay in order.
- Fan out: issue several `agent` calls in one turn (they run concurrently): explore for
  reading and research, general for scoped changes, each with a complete, self-contained prompt.
- Integrate: read every report, reconcile conflicts, and verify the combined result yourself
  (build, tests) before calling it done.
Small or single-step requests still get a direct answer; do not fan out work that does not split.
"#;

/// Start the prompt sources that launch a runtime (PLUR's Node CLI, OptMem's
/// Python wake) as a session opens, so its first turn waits on neither, or
/// only on whichever is still running. Later turns read their caches.
pub fn prefetch_slow_sources(cwd: &Path, cfg: &crate::config::Config) {
    if cfg.poor_mode {
        return;
    }
    ecosystem::plur_prefetch(cwd);
    if cfg.optmem.enabled {
        crate::optmem::prefetch_wake();
    }
}

impl PromptContext {
    /// Build the complete prompt on Tokio's blocking pool, for every provider.
    /// Skill activation can perform blocking Jev HTTP calls, and the other
    /// prompt sources can read disk or run subprocesses. None belongs on an
    /// async worker. Keep all inputs owned until the build has finished.
    ///
    /// A failed worker is an explicit error, not an empty prompt or a retry on
    /// the async worker: either would hide missing skills or block the runtime.
    pub async fn build_with_opts_async(
        cwd: &Path,
        is_subagent: bool,
        model: &str,
        provider: &str,
        poor_mode: bool,
        user_text: Option<&str>,
        effort: &str,
    ) -> Result<Self, tokio::task::JoinError> {
        let cwd = cwd.to_path_buf();
        let model = model.to_owned();
        let provider = provider.to_owned();
        let user_text = user_text.map(str::to_owned);
        let effort = effort.to_owned();
        Self::build_off_thread(move || {
            Self::build_with_opts(
                &cwd,
                is_subagent,
                &model,
                &provider,
                poor_mode,
                user_text.as_deref(),
                &effort,
            )
        })
        .await
    }

    // Shared dispatch boundary so tests can exercise slow/panicking sources
    // without depending on credentials, network, or installed skills.
    async fn build_off_thread(
        build: impl FnOnce() -> Self + Send + 'static,
    ) -> Result<Self, tokio::task::JoinError> {
        tokio::task::spawn_blocking(build).await
    }

    /// `poor_mode`: skip PLUR inject and long memory excerpts to cut background
    /// token spend (toggle via `/poor`). Does **not** disable skill activation -
    /// NL and slash skills still fire.
    ///
    /// `user_text`: current user message - used for natural-language skill
    /// activation (e.g. "think like fable" → inject fable-method body).
    pub fn build_with_opts(
        cwd: &Path,
        is_subagent: bool,
        model: &str,
        provider: &str,
        poor_mode: bool,
        user_text: Option<&str>,
        effort: &str,
    ) -> Self {
        // Prefetched per workspace and refreshed between turns, so the PLUR CLI
        // never runs in front of a model request (only a first turn may wait).
        let plur = if is_subagent || poor_mode {
            String::new()
        } else {
            ecosystem::plur_context(cwd)
                .map(|s| format!("\n# PLUR shared memory (auto-injected)\n{s}\n"))
                .unwrap_or_default()
        };
        let cfg = crate::config::load_config().unwrap_or_default();
        let optmem = crate::optmem::prompt_block(cfg.optmem.enabled, is_subagent, poor_mode);
        let memory = if poor_mode {
            String::new()
        } else {
            memory_prompt_excerpt(3000)
        };
        // Skills: on-demand only. NL activation runs for every provider (not
        // gated by poor_mode). Subagents skip - they get a focused task prompt.
        let (
            activation,
            activation_label,
            activation_skill,
            activation_requirements,
            activation_path,
        ) = if is_subagent {
            (String::new(), None, None, Vec::new(), None)
        } else if let Some(text) = user_text {
            let loaded = load_skills(cwd);
            match skill_activation(text, &loaded) {
                Some(a) => (
                    a.section,
                    Some(a.label),
                    Some(a.name),
                    a.requirements,
                    Some(a.path),
                ),
                None => (String::new(), None, None, Vec::new(), None),
            }
        } else {
            (String::new(), None, None, Vec::new(), None)
        };
        // Cross-provider nudge: only when the user *explicitly asks* to delegate
        // work to another backend this turn (e.g. "spawn a grok subagent",
        // "have claude review"). A bare mention ("the claude error") must never
        // nudge a fan-out - that wasted tokens and derailed turns.
        let named_providers = if is_subagent {
            Vec::new()
        } else {
            user_text
                .map(crate::providers::delegated_providers_in_text)
                .unwrap_or_default()
        };
        Self {
            cwd: cwd.to_path_buf(),
            is_subagent,
            model: model.to_string(),
            provider: provider.to_string(),
            shell_label: shell_backend().label.clone(),
            project: find_project_instructions(cwd),
            memory,
            plur,
            optmem,
            activation,
            activation_label,
            activation_skill,
            activation_requirements,
            activation_path,
            named_providers,
            // Subagents cannot delegate, so orchestration is the root's job.
            ultracode: !is_subagent && crate::providers::canonical_effort(effort) == "ultracode",
        }
    }

    /// True when natural-language skill activation fired this turn.
    pub fn has_skill_activation(&self) -> bool {
        !self.activation.is_empty()
    }

    /// Short label for status UI (`fable-method`, `tdd`, …).
    pub fn skill_activation_label(&self) -> Option<&str> {
        self.activation_label.as_deref()
    }

    /// The activated skill's name, its own stated requirements and its path, for
    /// the skill layer's usage check.
    pub fn active_skill(&self) -> Option<(&str, &[String], &str)> {
        self.activation_skill.as_deref().map(|n| {
            (
                n,
                self.activation_requirements.as_slice(),
                self.activation_path.as_deref().unwrap_or(""),
            )
        })
    }

    /// The skill body that natural-language activation selected this turn, as
    /// a conversation item. It is appended to the session once per turn (right
    /// after the user's message) instead of being rendered into the system
    /// prompt: there it would change the cached prefix on every turn and be
    /// re-read at full price on every round of the turn it belongs to.
    pub fn activation_item(&self) -> Option<Value> {
        if self.activation.is_empty() {
            return None;
        }
        Some(crate::api::types::user_text_item(self.activation.trim()))
    }

    /// Render the request prompt: a byte-stable system prompt plus the state
    /// that changes between requests (permission mode, todo list, routed
    /// memory and session inventories).
    ///
    /// Every provider nur speaks to caches by prefix (OpenAI/Gemini/DeepSeek/
    /// vLLM automatically, Anthropic at explicit breakpoints), and the system
    /// prompt is the first thing in that prefix. One changed byte in it
    /// re-reads system + tools + the whole history at full price, so nothing
    /// that can change inside a session is allowed in `system`. The live state
    /// rides as the final input item instead, where it costs its own few
    /// hundred tokens and nothing else.
    pub fn render(&self, mode: PermissionMode, todos_render: &str) -> RenderedPrompt {
        // Product identity is always Nur. Backend provider/model are facts only.
        let role = if self.is_subagent {
            format!(
                "You are a focused NurCLI SUBAGENT (backend: {} · model id: {}). Complete the delegated task and return a concise report. Do not ask the user questions.",
                self.provider, self.model
            )
        } else {
            format!(
                "You are **Nur**, the coding agent for **NurCLI** (the user's personal CLI).\n\
Backend this session: **{}** · model id: `{}`.\n\
If asked your name or who you are: say you are **Nur** (NurCLI). The backend provider/model above is how requests are routed - not your product name.",
                self.provider, self.model
            )
        };

        if self.is_subagent {
            let permission = match mode {
                PermissionMode::Plan => {
                    "PLAN: inspect and analyze only. Do not write files or mutate the repository."
                }
                PermissionMode::Manual => {
                    "MANUAL: mutating tools require approval relayed through the parent."
                }
                PermissionMode::Auto => {
                    "AUTO: make only the scoped changes required by the delegated task."
                }
            };
            let mut prompt = format!(
                "{role}\n\nWorkspace: {}\nOS: {} · shell: {}\nPermission mode: {permission}\n\n\
                 # Focused tools\n{}\n\n\
                 Use sgrep (plain-English, semantic) first for code search, grep for an exact \
                 identifier or regex, glob for file names, read_file/list_dir for file contents, and \
                 git_status/git_diff for repository state. Prefer dedicated edit tools over \
                 shell rewrites. Never run interactive/watch commands. Keep paths inside the \
                 workspace. Read tool errors before changing approach.\n\n\
                 Delegation tools and OMP are intentionally unavailable in child runs. Do not \
                 substitute another backend when a tool or provider fails. Return a clear \
                 failure with any useful partial findings so the parent can retry or recover.\n\n\
                 Finish with a concise report of findings, files touched, and checks run.\n",
                self.cwd.display(),
                std::env::consts::OS,
                self.shell_label,
                crate::tools::SUBAGENT_TOOL_NAMES.join(", "),
            );
            if let Some((name, text)) = &self.project {
                prompt.push_str(&format!("\n# Project instructions ({name})\n{text}\n"));
            }
            return RenderedPrompt {
                system: prompt,
                turn_state: String::new(),
            };
        }

        let mut s = format!(
            r#"{role}

Workspace: {}
OS: {} · shell: {}

# Permission modes
The active mode is stated in the turn-state item at the end of the conversation
and the user can switch it (Shift+Tab) at any time; always act on the latest.

PLAN (explore + analyze, no repo changes): read, parse, and understand the
workspace freely, AND run shell for analysis and scratch/media work - reading
files, grep/ripgrep, running tests or linters to observe, ffmpeg/extract_frames
to cut up a video, copying a clip to a temp dir, one-off analysis scripts, etc.
Non-mutating compute never needs permission here.
Free: read_file, list_dir, sgrep, grep, glob, web_fetch, web_search, look, extract_frames,
git_status, git_diff, skill, memory(read), todo_write, submit_plan, codemode,
graphify(query|path|explain|status|report|affected), excalidraw(status|reference),
plur(recall|status|…), ruflo(memory_search|status|…), executor(search|status),
enclave(status|tools), and bash for the above.
BLOCKED in plan mode (do NOT attempt - they need manual/auto via Shift+Tab):
- Authoring code: write_file, edit_file, multi_edit, apply_patch.
- Submitting/mutating the repo via shell: git commit/push/add/reset/checkout/
  restore/stash/merge/rebase/pull, gh pr create/merge, and dependency installs
  (npm/pnpm/yarn/pip/cargo/… install/add).
- Mutating knowledge: graphify(extract|update), excalidraw(create|export),
  plur(learn|capture), ruflo(memory_store|swarm_init), executor(call|install),
  enclave(call), memory(append), agent.
Do your investigation, then deliver the plan via submit_plan. Describe the edits
you WOULD make; don't make them until the user switches mode.

MANUAL: mutating tools need user approval. Prefer apply_patch/multi_edit for
structured edits.

AUTO: tools auto-approved. Prefer minimal safe diffs; avoid destructive shell.

# Tools
read_file, list_dir, write_file, edit_file, multi_edit, apply_patch, bash, sgrep, grep, glob,
web_fetch, web_search, look, extract_frames, git_status, git_diff,
context (RLM store), anydoc (docs→md), goal, proposal, harness (continual refine),
connectome (agent-native memory + chronicle continuity), repl (persistent Python REPL),
graphify, excalidraw, plur, ruflo, skill, memory, todo_write, submit_plan, agent,
codemode (JS script that calls read-only tools in one round)

The full tool surface is offered on every request so the schemas stay cached
across the session. Never invent a tool call whose schema is not available.

## Turn economics
Every turn re-sends the whole conversation, so a turn costs far more than a
tool call. Go wide rather than deep: when the next calls do not depend on each
other's output - reading several files, running the build and the tests,
probing two hypotheses - issue them together in one turn instead of one per
turn. You never have to babysit slow work: a long `bash` command is handed to a
background job and its result arrives on its own, so keep doing useful work
instead of polling it. When the next step is 3+ read-only calls whose outputs
feed each other and you only need part of the result - read these files and
count the matches, grep then open each hit, walk a directory tree - write one
`codemode` script instead: it calls `tools.<name>(args)` in a loop and only
what it prints comes back. Mutating work stays in ordinary tool calls.

## Tool policy - search and failure handling (critical for all backends including Meta)
- SEARCH - `sgrep` first, then `grep`/`glob`; never the shell. `sgrep` is semantic search (Context.dev sgrep, local, ripgrep-backed): describe what the code does in plain English - "where are retries with backoff", "code that validates the session token" - and it returns the few ranked chunks that matter, exact identifier hits first. Lead with it whenever you know the behavior but not the name, when orienting in unfamiliar code, or when a grep would return more than a screen; pass `pattern` to have it rerank a regex's matches by intent instead. Use `grep` for an exact identifier, string or regex you already know, and `glob` for file names. NEVER use bash commands like `grep`, `rg`, `ag`, `find`, `ls`, `Get-ChildItem`, etc. for searching: the search tools are sandboxed, respect .gitignore, and are the only reliable search path. When sgrep is not installed it says so and answers with grep, so leading with it is never a wasted call. This applies to all models with no exceptions.
- FILE IO - dedicated tools only: `list_dir` for directory shape, `read_file` for contents. NEVER use bash `cat`, `type`, `ls`, `dir`, `head`, `tail` to read workspace. Cheaper, faster, and never hangs.
- GIT - use `git_status` / `git_diff` tools, not `bash git ...` - they are approval-free and structured. Reserve bash git only when the tool does not cover the needed flag.
- BASH: real shell when available (Git Bash/pwsh). The result is the stdout alone; `Stderr:` and `Exit code: N` sections appear only when non-empty / non-zero (no `Exit code` line means 0), `(no output)` when nothing was printed. Long output keeps head and tail around a `...N bytes truncated; complete output in <path>...` marker (`max_output_length` sets the cap; `read_file` the path when the middle matters). Prefer non-interactive one-shot commands. Default timeout 60s (hard max 180s); a command still producing output at the deadline is handed to a background job whose result arrives in a later turn, a silent one is killed. The harness refuses identical retries of a failed/timed-out command and refuses hang-prone patterns (dev servers, watch, `read -p`, `while true`).
- FAILURE RECOVERY - mandatory: If ANY tool returns error, `exit_code != 0`, timeout, idle-kill, or cancellation:
  1) STOP - read exit_code/stdout/stderr.
  2) Do NOT retry the identical failing command (harness will refuse anyway).
  3) SWITCH to the canonical tool: failed `ls` -> `list_dir`, failed `cat` -> `read_file`, failed `grep` via bash -> `sgrep`/`grep` tool, failed `find` -> `glob`/`grep`.
  4) If a base command repeatedly fails (e.g. command not found on Windows), immediately use the dedicated tool and never hang the turn.
  5) On timeout/idle-kill, do not bump timeout_ms - change approach.
  Claude and other models often re-run the same failing script - that wastes the turn. Self-correct immediately.
- HANG PREVENTION: Never run interactive, watch, or long-lived server commands. Prefer `sgrep`/`grep`/`list_dir`/`read_file`/`glob`.
- Paths are sandboxed to the workspace - never scan drive roots (`/`, `C:\`, `~`).
- web_search -> find docs/errors; web_fetch -> read a result url (text only - not video)
- look: attach image(s) or a short video for **vision**. Prefer look over guessing from filenames.
- extract_frames: sparse keyframes via ffmpeg (default ~1fps, max ~8). Writes `.nur/frames/…`
  and auto-queues look. Use for design-from-video - never frame-by-frame every pixel.
- Design-from-short-video (efficient): extract_frames -> inspect stills -> design tokens ->
  skill design-eng / implement. User paths to .png/.mp4 in the prompt auto-attach when present.
- graphify: code knowledge graph (graphify-out/). Prefer query/path/explain over broad grep when
  the graph exists. extract defaults to code-only AST (local, free).
- excalidraw: hand-drawn diagrams. create writes `.excalidraw`, uploads, and **opens the
  share URL in the browser** so the user actually sees it (not a dead link). Prefer over
  mermaid when they want a real diagram. skill(action=read, name=excalidraw) for templates.
  NEVER OS-open a local `.excalidraw` (Start-Process / open_path / xdg-open) — that causes
  Windows "Open with". Browser share URL only. Prefer docs/ or .nur/diagrams/ (never Desktop).
  Supports elements_path= and from_mermaid=. For offline desktop boards use tldraw /draw.
- plur: shared engram memory (~/.plur/). learn corrections/preferences; inject/recall across
  sessions. Auto-injected at session start. Never store secrets.
- optmem: permanent OptMem under ~/.optmem (wake/note/nap/recall). Auto-wake for root agents;
  subagents must not run memo. Prefer for lasting decisions; plur for style prefs.
- headroom: context compression - large tool results are auto-compressed when enabled
  (default on; disable via [headroom] enabled=false). Tool: status|doctor|compress.
- ruflo: vector memory + swarm harness. Global DB at ~/.nur/ruflo/. Prefer plur for preferences,
  ruflo for pattern/embedding memory, graphify for code structure.
        - **egaki** - image/video/speech gen (`egaki` CLI). Prefer `egaki login --provider chatgpt` or `xai-oauth`; BYOK via google/openai/fal. Then image|video|speech → `.nur/media/` → look.
  Writes .nur/media/ - then look. Prefer over guessing pixels.
- fractal: hierarchical agent loops in git worktrees (Unix). Unattended nodes bypass
  approvals - confirm with the user before node start. Factory overnight prefers fractal.
- penecho: 20k canvas beyond chat (ink/MathJax/plots). `launch` auto-installs via npm,
  writes ~/.penecho/config.env from nur auth (or codex/claude/kimi CLI), opens browser to
  http://127.0.0.1:3888. Also stop|restart|inject|export_png. Never ask the user to diagnose.
- bg: background jobs so long work does not block the agent. action=run|list|status|result|cancel.
  Status chip in the TUI footer. /bg slash. Diagram tools accept background=true on install/launch.
- diagram skill: router - architecture→excalidraw (browser share only), offline board→tldraw,
  ink/math→penecho. /diagram <idea>.
- executor: MCP gateway (executor.sh) for external OpenAPI/GraphQL/MCP integrations - not for
  local repo edits. action=sources|search|call.
- enclave: Enclave (enclave.ai) security agents over native MCP - pentests, code security review,
  findings, CVE monitoring. action=tools first (names + schemas), then call tool=<name> arguments={{...}}.
  Calls start real work on real systems: target only what the user named or Enclave already has in
  scope. No key -> tell the user `nur auth login --provider enclave`.
- skill: action=list / action=read - load one skill by name when needed. Skills are **not**
  pre-loaded into this prompt (catalog would waste tokens). Discover with skill(list) or
  skill(read, name=…). Never load every playbook at once (e.g. cybersecurity: one by name).
- Skills activate on demand only: natural-language intent (e.g. "think like fable") or
  `/skill-name` slash. When a **SKILL ACTIVATED** item appears in the conversation, follow it for the turn.
- UI polish -> design-eng. Site clone -> clone-website-meta. Security -> cybersecurity then one playbook. Smart-contract / DeFi whitehat -> sc-research then one playbook. Paid Immunefi/X bounty -> hunting-x-linked-bounties then the Playbook column. Historical bounty/hack class -> historical-smart-contract-vulns then one reference slice. Bybit/drainer/phishing/zachxbt -> investigator-ops slice or reviewing-frontend-and-ops-surfaces.
- agent: spawn explore (read-only) or general subagent - see **# Cross-provider subagents** below when the user names another provider
- todo_write: maintain a live task list for multi-step work (always keep one in_progress)
- submit_plan: formal plan artifact in plan mode
- memory: local markdown journal ~/.nur/memory.md (never store secrets) - complementary to plur
- Prefer edit_file / multi_edit / apply_patch over full rewrites

# Cross-provider subagents
Only when the user **explicitly asks you to run work on another backend** - e.g. "spawn a
grok subagent", "have claude review this", "ask gemini to research", "run this on chatgpt",
"fan out to claude and grok" - deploy via the `agent` tool with the structured **`provider`**
field set. A **bare mention** of a model or provider ("the claude error", "5.6 sol is slow",
"like grok does") is **not** a delegation request - just do the work yourself on this session's
backend; do not spawn a subagent. When you do delegate, do **not** claim you "switched models"
in prose - only `agent(provider=…)` runs elsewhere. Omit `provider` (and `model`) when
inheriting this session's backend.

Concrete shapes (mirror these; aliases are fine - nur resolves them):

- Claude / Anthropic:
  agent({{"provider":"claude","subagent_type":"general","description":"claude review","prompt":"Review auth for race conditions"}})
- Grok / xAI:
  agent({{"provider":"grok","subagent_type":"general","description":"grok audit","prompt":"Audit failover paths"}})
- Gemini / Google API:
  agent({{"provider":"gemini","subagent_type":"explore","description":"gemini research","prompt":"Map the graphify module"}})
- Antigravity (own OAuth - not the same as gemini/google):
  agent({{"provider":"antigravity","subagent_type":"general","description":"agy implement","prompt":"Implement the missing test"}})

Alias → catalog id (pass either): claude/sonnet/opus/haiku → anthropic; grok → xai;
gemini → google; chatgpt/gpt → openai; antigravity/agy stays **antigravity** (never collapse to google).

**Missing credentials:** nur opens `/login` pre-selected to that provider and **blocks** the spawn.
There is **no silent fallback** to the parent provider. Do not re-run the same task on the parent
and pretend it succeeded. After the user finishes `/login`, nur injects a mandatory re-deploy with
the exact `agent(...)` call - follow that instruction immediately.

**Failure isolation:** a failed `agent` result stays failed even if it contains partial output.
Do not call `omp`, switch to the parent backend, or choose another provider as a substitute unless
the user explicitly requested that route. Retry the same named provider after authentication or
report the failure clearly. OMP is an explicit specialized delegation tool, never an automatic
recovery path for broken subagent orchestration.

Fan-out: one `agent` call per target provider; set `provider` on every call the user named.
"#,
            self.cwd.display(),
            std::env::consts::OS,
            self.shell_label,
        );

        s.push_str(
            r#"
# Workflow
1. Orient - git_status + sgrep for intent, grep for names, then read
2. Plan - todo_write for multi-step; submit_plan in plan mode
3. Implement - smallest correct change; verify with tests/build
4. Report - what changed, how to verify

# Tool-call rationale
Before the first tool call for a task, give at most one short sentence explaining
the next action. Do not repeat a stock preamble before later calls. Keep tool-call
commentary concise; the tool result is the evidence.

# Style
Direct technical markdown. Fence code with languages.

# Turn state
The last item of the conversation, headed `[nur turn state]`, is written by the
harness, not the user: the active permission mode, the current todo list, and
session inventories (goal, harness, context variables, routed memory). It
reflects the moment the request was sent; follow it over anything earlier.
"#,
        );

        if let Some((name, text)) = &self.project {
            s.push_str(&format!("\n# Project instructions ({name})\n{text}\n"));
        }

        // Memory excerpts and the PLUR/OptMem blocks are read once per user
        // turn and change only when something was learned, so they stay in the
        // cached prefix. (The activated skill body is a conversation item: see
        // `activation_item`.)
        s.push_str(&self.memory);
        s.push_str(&self.plur);
        s.push_str(&self.optmem);

        let cfg = crate::config::load_config().unwrap_or_default();
        if cfg.native_memory {
            // Central router: give the model the routing table so it stops
            // guessing which memory resident to use (m4).
            s.push('\n');
            s.push_str(super::memory_router::routing_guidance());
            s.push('\n');
        }

        // ── Turn state: everything that can change between requests ──
        let mut t = format!("{TURN_STATE_MARKER}\n");
        let mode_line = match mode {
            PermissionMode::Plan => {
                "Permission mode: PLAN - explore and analyze only; no repo changes (see # Permission modes)."
            }
            PermissionMode::Manual => {
                "Permission mode: MANUAL - mutating tools need user approval."
            }
            PermissionMode::Auto => "Permission mode: AUTO - tools auto-approved.",
        };
        t.push_str(mode_line);
        t.push('\n');
        // Plan mode blocks `agent`, so there is nothing to fan out.
        if self.ultracode && !matches!(mode, PermissionMode::Plan) {
            t.push_str(ULTRACODE_BLOCK);
        }
        // Per-turn nudge - only when the user explicitly asked to delegate to
        // these providers this turn (bare mentions are filtered out upstream).
        if !self.named_providers.is_empty() {
            t.push_str(&format!(
                "\nUser asked to delegate to: {} - pass agent.provider for each.\n",
                self.named_providers.join(", ")
            ));
        }
        if !todos_render.is_empty() && todos_render != "(no todos)" {
            t.push_str(&format!("\n# Current todos\n{todos_render}\n"));
        }

        // Prime / RLM / Continual Harness / Connectome session state (outside the chat window).
        let sid = std::env::var("NUR_SESSION_ID").unwrap_or_default();
        if !sid.is_empty() {
            t.push_str(&super::goal::prompt_block(&sid));
            t.push_str(&super::harness::prompt_block(&sid));
            let inv = super::context_store::prompt_inventory(&sid);
            if !inv.is_empty() {
                t.push('\n');
                t.push_str(&inv);
                t.push('\n');
            }
        }
        // Agent-native memory (arXiv:2606.24775) + Connectome continuity scope.
        // Project-scoped so the same agent persists across sessions on a machine.
        if cfg.native_memory {
            let mem_scope = {
                let proj = self
                    .cwd
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("workspace");
                if sid.is_empty() {
                    format!("{proj}:global")
                } else {
                    format!("{proj}:{sid}")
                }
            };
            // Router snapshot (empty query → recency+confidence ranked). Inject
            // only when there is real content so the prompt doesn't bloat.
            let routed = super::memory_router::read(&mem_scope, "", 3, 2);
            let has_content = routed.contains("[vector] top-")
                || routed.contains("[hierarchical]")
                || routed.contains("[graph]");
            if has_content {
                t.push_str("\n# Routed memory snapshot\n");
                t.push_str(&routed);
                t.push('\n');
            }
        }
        RenderedPrompt {
            system: s,
            turn_state: t,
        }
    }
}

/// First line of the turn-state item. Wire shapers key on it to keep cache
/// breakpoints off the one block that is different on every request.
pub const TURN_STATE_MARKER: &str = "[nur turn state]";

/// A rendered request prompt. `system` is identical across every request of a
/// session until something durable changes (project instructions, memory,
/// model); `turn_state` is sent as the final input item and never persisted.
pub struct RenderedPrompt {
    pub system: String,
    pub turn_state: String,
}

impl RenderedPrompt {
    /// The turn-state item to append after the session history, or `None` for
    /// prompts (subagents) that have no live state.
    pub fn turn_state_item(&self) -> Option<Value> {
        if self.turn_state.is_empty() {
            None
        } else {
            Some(crate::api::types::user_text_item(&self.turn_state))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_attribution_separates_tool_output_and_fingerprints_stable_prefix() {
        let tools = vec![crate::api::types::ToolDef {
            type_: "function".into(),
            name: "read_file".into(),
            description: Some("Read a file".into()),
            parameters: Some(serde_json::json!({"type":"object"})),
        }];
        let items = vec![
            serde_json::json!({"role":"user", "content":"inspect this"}),
            serde_json::json!({"type":"function_call_output", "call_id":"x", "output":"a long result"}),
        ];
        let a = attribute_request("stable system", &tools, &items);
        let b = attribute_request("stable system", &tools, &items);
        assert!(a.system_tokens > 0 && a.tool_schema_tokens > 0);
        assert!(a.dialogue_tokens > 0 && a.tool_output_tokens > 0);
        assert_eq!(a.stable_prefix_fingerprint, b.stable_prefix_fingerprint);
    }

    #[test]
    fn child_prompt_matches_the_focused_non_recursive_tool_surface() {
        let rendered = PromptContext::build_with_opts(
            Path::new("."),
            true,
            "test-model",
            "test-provider",
            false,
            None,
            "high",
        )
        .render(PermissionMode::Auto, "(no todos)");
        let prompt = rendered.system;
        assert!(rendered.turn_state.is_empty());
        assert!(prompt.contains("read_file"));
        assert!(prompt.contains("write_file"));
        assert!(prompt.contains("Delegation tools and OMP are intentionally unavailable"));
        assert!(!prompt.contains("# Cross-provider subagents"));
        assert!(!prompt.contains("plur:"));
        assert!(!prompt.contains("ruflo:"));
    }

    /// The system prompt is the head of every provider's cache prefix. Mode
    /// switches and todo updates happen several times a turn; if either moved
    /// a byte of `system`, system + tools + the whole history would be re-read
    /// at full price on the next request.
    #[test]
    fn root_system_prompt_is_identical_across_mode_and_todo_changes() {
        let ctx = PromptContext::build_with_opts(
            Path::new("."),
            false,
            "test-model",
            "test-provider",
            true,
            None,
            "high",
        );
        let auto = ctx.render(PermissionMode::Auto, "(no todos)");
        let plan = ctx.render(PermissionMode::Plan, "- [ ] write the test\n- [x] read it");
        assert_eq!(auto.system, plan.system);
        assert!(auto.turn_state.starts_with(TURN_STATE_MARKER));
        assert!(auto.turn_state.contains("AUTO"));
        assert!(plan.turn_state.contains("PLAN"));
        assert!(plan.turn_state.contains("write the test"));
        assert!(!auto.turn_state.contains("write the test"));
        let item = plan
            .turn_state_item()
            .expect("root prompts carry turn state");
        assert_eq!(item["role"], "user");
    }
}
