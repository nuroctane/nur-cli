use super::memory::memory_prompt_excerpt;
use super::mode::PermissionMode;
use super::skills::{load_skills, skill_activation};
use crate::ecosystem;
use crate::tools::shell_backend;
use std::path::{Path, PathBuf};

/// Project instruction files (first found wins). NUR.md preferred; META.md/MUSE.md legacy.
pub const PROJECT_INSTRUCTION_FILES: &[&str] =
    &["NUR.md", "META.md", "MUSE.md", "AGENTS.md", "CLAUDE.md"];

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
/// or slash commands — works for every provider.
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
    /// PLUR inject block — auto-loaded so the agent remembers past corrections.
    plur: String,
    /// Natural-language skill activation for this user turn (injected body).
    activation: String,
    /// Short label for TUI status when activation fires (e.g. `fable-method`).
    activation_label: Option<String>,
}

impl PromptContext {
    pub fn build(cwd: &Path, is_subagent: bool, model: &str, provider: &str) -> Self {
        Self::build_with_opts(cwd, is_subagent, model, provider, false, None)
    }

    /// `poor_mode`: skip PLUR inject and long memory excerpts to cut background
    /// token spend (toggle via `/poor`). Does **not** disable skill activation —
    /// NL and slash skills still fire.
    ///
    /// `user_text`: current user message — used for natural-language skill
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
        let memory = if poor_mode {
            String::new()
        } else {
            memory_prompt_excerpt(3000)
        };
        // Skills: on-demand only. NL activation runs for every provider (not
        // gated by poor_mode). Subagents skip — they get a focused task prompt.
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
        Self {
            cwd: cwd.to_path_buf(),
            is_subagent,
            model: model.to_string(),
            provider: provider.to_string(),
            shell_label: shell_backend().label.clone(),
            project: find_project_instructions(cwd),
            memory,
            plur,
            activation,
            activation_label,
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
analysis and scratch/media work — reading files, grep/ripgrep, running tests or
linters to observe, ffmpeg/extract_frames to cut up a video, copying a clip to a
temp dir, one-off analysis scripts, etc. Non-mutating compute never needs
permission here.

Free: read_file, list_dir, grep, glob, web_fetch, web_search, look, extract_frames,
git_status, git_diff, skill, memory(read), todo_write, submit_plan,
graphify(query|path|explain|status|report|affected), excalidraw(status|reference),
plur(recall|status|…), ruflo(memory_search|status|…), executor(search|status),
and bash for the above.

BLOCKED in plan mode (do NOT attempt — they need manual/auto via Shift+Tab):
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
If asked your name or who you are: say you are **Nur** (NurCLI). Do **not** call yourself Meta, Muse, or Claude unless the user is asking about a different product. The backend provider/model above is how requests are routed — not your product name.",
                self.provider, self.model
            )
        };

        let mut s = format!(
            r#"{role}

Workspace: {}
OS: {} · shell: {}

{mode_block}
# Tools
read_file, list_dir, write_file, edit_file, multi_edit, apply_patch, bash, grep, glob,
web_fetch, web_search, look, extract_frames, git_status, git_diff, graphify, excalidraw,
plur, ruflo, skill, memory, todo_write, submit_plan, agent

## Tool policy — search and failure handling (critical for all backends including Meta)
- SEARCH — ripgrep only: ALWAYS use `grep` and `glob` tools for any code/content search. NEVER use bash commands like `grep`, `rg`, `ag`, `find`, `ls`, `Get-ChildItem`, etc. for searching. The `grep`/`glob` tools are ripgrep-backed, sandboxed, respect .gitignore, and are the only reliable search path. This applies to ALL models including Meta Llama / Muse Spark — no exceptions.
- FILE IO — dedicated tools only: `list_dir` for directory shape, `read_file` for contents. NEVER use bash `cat`, `type`, `ls`, `dir`, `head`, `tail` to read workspace. Cheaper, faster, and never hangs.
- GIT — use `git_status` / `git_diff` tools, not `bash git ...` — they are approval-free and structured. Reserve bash git only when the tool does not cover the needed flag.
- BASH: real shell when available (Git Bash/pwsh); output reports `shell: <backend>` + `exit_code` + stdout/stderr. Prefer non-interactive commands. Captures are truncated at 80k/40k.
- FAILURE RECOVERY — mandatory: If ANY tool returns error, `exit_code != 0`, timeout, or cancellation:
  1) STOP — read exit_code/stdout/stderr.
  2) Do NOT retry the identical failing command more than once.
  3) SWITCH to the canonical tool: failed `ls` -> `list_dir`, failed `cat` -> `read_file`, failed `grep` via bash -> `grep` tool, failed `find` -> `glob`/`grep`.
  4) If a base command repeatedly fails (e.g. command not found on Windows), immediately use the dedicated tool and never hang the turn.
  5) If you hit a timeout, the process tree was killed — try a narrower path or a different tool, not the same command with longer timeout.
  Meta models were observed to hang when a base command failed (no self-correction) — you MUST self-correct now.
- HANG PREVENTION: Never run interactive or watch-mode commands. Set narrow paths. If a tool times out after 120s, it is killed — explain the failure and try `grep`/`list_dir`/`read_file` instead.
- Paths are sandboxed to the workspace — never scan drive roots (`/`, `C:\`, `~`).
- web_search -> find docs/errors; web_fetch -> read a result url (text only — not video)
- look: attach image(s) or a short video for **vision**. Prefer look over guessing from filenames.
- extract_frames: sparse keyframes via ffmpeg (default ~1fps, max ~8). Writes `.nur/frames/…`
  and auto-queues look. Use for design-from-video — never frame-by-frame every pixel.
- Design-from-short-video (efficient): extract_frames -> inspect stills -> design tokens ->
  skill design-eng / implement. User paths to .png/.mp4 in the prompt auto-attach when present.
- graphify: code knowledge graph (graphify-out/). Prefer query/path/explain over broad grep when
  the graph exists. extract defaults to code-only AST (local, free).
- excalidraw: hand-drawn diagrams. create writes `.excalidraw`, uploads, and **opens the
  share URL in the browser** so the user actually sees it (not a dead link). Prefer over
  mermaid when they want a real diagram. skill(action=read, name=excalidraw) for templates.
- plur: shared engram memory (~/.plur/). learn corrections/preferences; inject/recall across
  sessions. Auto-injected at session start. Never store secrets.
- ruflo: vector memory + swarm harness. Global DB at ~/.nur/ruflo/. Prefer plur for preferences,
  ruflo for pattern/embedding memory, graphify for code structure.
- executor: MCP gateway (executor.sh) for external OpenAI/GraphQL/MCP integrations — not for
  local repo edits. action=sources|search|call.
- skill: action=list / action=read — load one skill by name when needed. Skills are **not**
  pre-loaded into this prompt (catalog would waste tokens). Discover with skill(list) or
  skill(read, name=…). Never load every playbook at once (e.g. cybersecurity: one by name).
- Skills activate on demand only: natural-language intent (e.g. "think like fable") or
  `/skill-name` slash. When a **SKILL ACTIVATED** block appears below, follow it for the turn.
- UI polish -> design-eng. Site clone -> clone-website-meta. Security -> cybersecurity then one playbook.
- agent: spawn explore (read-only) or general subagent for parallel research
- todo_write: maintain a live task list for multi-step work (always keep one in_progress)
- submit_plan: formal plan artifact in plan mode
- memory: local markdown journal ~/.nur/memory.md (never store secrets) — complementary to plur
- Prefer edit_file / multi_edit / apply_patch over full rewrites


# Workflow
1. Orient — git_status + targeted grep/read
2. Plan — todo_write for multi-step; submit_plan in plan mode
3. Implement — smallest correct change; verify with tests/build
4. Report — what changed, how to verify

# Style
Direct technical markdown. Fence code with languages.
"#,
            self.cwd.display(),
            std::env::consts::OS,
            self.shell_label,
        );

        if !todos_render.is_empty() && todos_render != "(no todos)" {
            s.push_str(&format!("\n# Current todos\n{todos_render}\n"));
        }

        if let Some((name, text)) = &self.project {
            s.push_str(&format!("\n# Project instructions ({name})\n{text}\n"));
        }

        // Activation first so it outranks generic workflow defaults.
        // (No full skills catalog — only the matched skill body, if any.)
        s.push_str(&self.activation);
        s.push_str(&self.memory);
        s.push_str(&self.plur);
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
