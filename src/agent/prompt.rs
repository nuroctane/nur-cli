use super::memory::memory_prompt_excerpt;
use super::mode::PermissionMode;
use super::skills::{load_skills, skill_activation};
use crate::ecosystem;
use crate::tools::shell_backend;
use std::path::{Path, PathBuf};

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
    /// Provider aliases the user named in this turn's message (for agent.provider nudge).
    named_providers: Vec<String>,
}

impl PromptContext {
    pub fn build(cwd: &Path, is_subagent: bool, model: &str, provider: &str) -> Self {
        Self::build_with_opts(cwd, is_subagent, model, provider, false, None)
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
    ) -> Self {
        let plur = if is_subagent || poor_mode {
            String::new()
        } else {
            ecosystem::plur_inject(&format!(
                "coding agent session in {}",
                cwd.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("workspace")
            ))
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
        let (activation, activation_label) = if is_subagent {
            (String::new(), None)
        } else if let Some(text) = user_text {
            let loaded = load_skills(cwd);
            match skill_activation(text, &loaded) {
                Some(a) => (a.section, Some(a.label)),
                None => (String::new(), None),
            }
        } else {
            (String::new(), None)
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
            named_providers,
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

    /// Render with the live bits: permission mode and the todo list, both of
    /// which can change between requests within a turn.
    pub fn render(&self, mode: PermissionMode, todos_render: &str) -> String {
        let mode_block = match mode {
            PermissionMode::Plan => {
                r#"
# Permission mode: PLAN  (explore + analyze, no repo changes)
You may read, parse, and understand the workspace freely, AND run shell for
analysis and scratch/media work - reading files, grep/ripgrep, running tests or
linters to observe, ffmpeg/extract_frames to cut up a video, copying a clip to a
temp dir, one-off analysis scripts, etc. Non-mutating compute never needs
permission here.

Free: read_file, list_dir, grep, glob, web_fetch, web_search, look, extract_frames,
git_status, git_diff, skill, memory(read), todo_write, submit_plan,
graphify(query|path|explain|status|report|affected), excalidraw(status|reference),
plur(recall|status|…), ruflo(memory_search|status|…), executor(search|status),
and bash for the above.

BLOCKED in plan mode (do NOT attempt - they need manual/auto via Shift+Tab):
- Authoring code: write_file, edit_file, multi_edit, apply_patch.
- Submitting/mutating the repo via shell: git commit/push/add/reset/checkout/
  restore/stash/merge/rebase/pull, gh pr create/merge, and dependency installs
  (npm/pnpm/yarn/pip/cargo/… install/add).
- Mutating knowledge: graphify(extract|update), excalidraw(create|export),
  plur(learn|capture), ruflo(memory_store|swarm_init), executor(call|install),
  memory(append), agent.

Do your investigation, then deliver the plan via submit_plan. Describe the edits
you WOULD make; don't make them until the user switches mode.
"#
            }
            PermissionMode::Manual => {
                r#"
# Permission mode: MANUAL
Mutating tools need user approval. Prefer apply_patch/multi_edit for structured edits.
"#
            }
            PermissionMode::Auto => {
                r#"
# Permission mode: AUTO
Tools auto-approved. Prefer minimal safe diffs; avoid destructive shell.
"#
            }
        };

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
                 Use grep/glob for search, read_file/list_dir for file contents, and \
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
            return prompt;
        }

        let mut s = format!(
            r#"{role}

Workspace: {}
OS: {} · shell: {}

{mode_block}
# Tools
read_file, list_dir, write_file, edit_file, multi_edit, apply_patch, bash, grep, glob,
web_fetch, web_search, look, extract_frames, git_status, git_diff,
context (RLM store), anydoc (docs→md), goal, proposal, harness (continual refine),
connectome (agent-native memory + chronicle continuity), repl (persistent Python REPL),
graphify, excalidraw, plur, ruflo, skill, memory, todo_write, submit_plan, agent

## Tool policy - search and failure handling (critical for all backends including Meta)
- SEARCH - ripgrep only: ALWAYS use `grep` and `glob` tools for any code/content search. NEVER use bash commands like `grep`, `rg`, `ag`, `find`, `ls`, `Get-ChildItem`, etc. for searching. The `grep`/`glob` tools are ripgrep-backed, sandboxed, respect .gitignore, and are the only reliable search path. This applies to all models with no exceptions.
- FILE IO - dedicated tools only: `list_dir` for directory shape, `read_file` for contents. NEVER use bash `cat`, `type`, `ls`, `dir`, `head`, `tail` to read workspace. Cheaper, faster, and never hangs.
- GIT - use `git_status` / `git_diff` tools, not `bash git ...` - they are approval-free and structured. Reserve bash git only when the tool does not cover the needed flag.
- BASH: real shell when available (Git Bash/pwsh); output reports `shell: <backend>` + `exit_code` + stdout/stderr. Prefer non-interactive one-shot commands. Captures truncated at 80k/40k. Default timeout 60s (hard max 180s). Idle (no output ~45s) is killed. The harness refuses identical retries of a failed/timed-out command and refuses hang-prone patterns (dev servers, watch, `read -p`, `while true`).
- FAILURE RECOVERY - mandatory: If ANY tool returns error, `exit_code != 0`, timeout, idle-kill, or cancellation:
  1) STOP - read exit_code/stdout/stderr.
  2) Do NOT retry the identical failing command (harness will refuse anyway).
  3) SWITCH to the canonical tool: failed `ls` -> `list_dir`, failed `cat` -> `read_file`, failed `grep` via bash -> `grep` tool, failed `find` -> `glob`/`grep`.
  4) If a base command repeatedly fails (e.g. command not found on Windows), immediately use the dedicated tool and never hang the turn.
  5) On timeout/idle-kill, do not bump timeout_ms - change approach.
  Claude and other models often re-run the same failing script - that wastes the turn. Self-correct immediately.
- HANG PREVENTION: Never run interactive, watch, or long-lived server commands. Prefer `grep`/`list_dir`/`read_file`/`glob`.
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
- skill: action=list / action=read - load one skill by name when needed. Skills are **not**
  pre-loaded into this prompt (catalog would waste tokens). Discover with skill(list) or
  skill(read, name=…). Never load every playbook at once (e.g. cybersecurity: one by name).
- Skills activate on demand only: natural-language intent (e.g. "think like fable") or
  `/skill-name` slash. When a **SKILL ACTIVATED** block appears below, follow it for the turn.
- UI polish -> design-eng. Site clone -> clone-website-meta. Security -> cybersecurity then one playbook.
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

        // Per-turn nudge - only when the user explicitly asked to delegate to
        // these providers this turn (bare mentions are filtered out upstream).
        if !self.named_providers.is_empty() {
            s.push_str(&format!(
                "\nUser asked to delegate to: {} - pass agent.provider for each.\n",
                self.named_providers.join(", ")
            ));
        }

        s.push_str(
            r#"
# Workflow
1. Orient - git_status + targeted grep/read
2. Plan - todo_write for multi-step; submit_plan in plan mode
3. Implement - smallest correct change; verify with tests/build
4. Report - what changed, how to verify

# Style
Direct technical markdown. Fence code with languages.
"#,
        );

        if !todos_render.is_empty() && todos_render != "(no todos)" {
            s.push_str(&format!("\n# Current todos\n{todos_render}\n"));
        }

        if let Some((name, text)) = &self.project {
            s.push_str(&format!("\n# Project instructions ({name})\n{text}\n"));
        }

        // Activation first so it outranks generic workflow defaults.
        // (No full skills catalog - only the matched skill body, if any.)
        s.push_str(&self.activation);
        s.push_str(&self.memory);
        s.push_str(&self.plur);
        s.push_str(&self.optmem);

        // Prime / RLM / Continual Harness / Connectome session state (outside the chat window).
        let sid = std::env::var("NUR_SESSION_ID").unwrap_or_default();
        if !sid.is_empty() {
            s.push_str(&super::goal::prompt_block(&sid));
            s.push_str(&super::harness::prompt_block(&sid));
            let inv = super::context_store::prompt_inventory(&sid);
            if !inv.is_empty() {
                s.push('\n');
                s.push_str(&inv);
                s.push('\n');
            }
        }
        // Agent-native memory (arXiv:2606.24775) + Connectome continuity scope.
        // Project-scoped so the same agent persists across sessions on a machine.
        let cfg = crate::config::load_config().unwrap_or_default();
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
            // Central router: give the model the routing table so it stops
            // guessing which memory resident to use (m4).
            s.push_str("\n");
            s.push_str(super::memory_router::routing_guidance());
            s.push_str("\n\n");
            // Router snapshot (empty query → recency+confidence ranked). Inject
            // only when there is real content so the prompt doesn't bloat.
            let routed = super::memory_router::read(&mem_scope, "", 3, 2);
            let has_content = routed.contains("[vector] top-")
                || routed.contains("[hierarchical]")
                || routed.contains("[graph]");
            if has_content {
                s.push_str("\n# Routed memory snapshot\n");
                s.push_str(&routed);
                s.push('\n');
            }
        }
        s
    }
}

/// One-shot convenience (used outside the turn loop).
#[allow(dead_code)]
pub fn system_instructions(
    cwd: &Path,
    mode: PermissionMode,
    is_subagent: bool,
    todos_render: &str,
    model: &str,
    provider: &str,
) -> String {
    PromptContext::build(cwd, is_subagent, model, provider).render(mode, todos_render)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_prompt_matches_the_focused_non_recursive_tool_surface() {
        let prompt = PromptContext::build(Path::new("."), true, "test-model", "test-provider")
            .render(PermissionMode::Auto, "(no todos)");
        assert!(prompt.contains("read_file"));
        assert!(prompt.contains("write_file"));
        assert!(prompt.contains("Delegation tools and OMP are intentionally unavailable"));
        assert!(!prompt.contains("# Cross-provider subagents"));
        assert!(!prompt.contains("plur:"));
        assert!(!prompt.contains("ruflo:"));
    }
}
