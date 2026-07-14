use super::memory::memory_prompt_excerpt;
use super::mode::PermissionMode;
use super::skills::{load_skills, skills_prompt_section};
use crate::ecosystem;
use crate::tools::shell_backend;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

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

struct SkillCacheEntry {
    loaded_at: Instant,
    rendered: String,
}

static SKILL_CACHE: OnceLock<Mutex<HashMap<String, SkillCacheEntry>>> = OnceLock::new();
fn skill_cache() -> &'static Mutex<HashMap<String, SkillCacheEntry>> {
    SKILL_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn cached_skills(cwd: &Path) -> String {
    let key = cwd.to_string_lossy().to_string();
    let ttl = Duration::from_secs(30);
    // Fast path: cache hit within TTL
    if let Ok(cache) = skill_cache().lock() {
        if let Some(entry) = cache.get(&key) {
            if entry.loaded_at.elapsed() < ttl {
                return entry.rendered.clone();
            }
        }
    }
    // Miss or expired: reload
    let skills = load_skills(cwd);
    let rendered = skills_prompt_section(&skills);
    if let Ok(mut cache) = skill_cache().lock() {
        cache.insert(
            key,
            SkillCacheEntry {
                loaded_at: Instant::now(),
                rendered: rendered.clone(),
            },
        );
    }
    rendered
}

/// The parts of the system prompt that come off disk (project instructions,
/// memory, skills, shell probe). Built **once per user turn** — rebuilding it
/// per model request re-read every SKILL.md and project instruction file on every API call.
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
    skills: String,
    /// PLUR inject block — auto-loaded so the agent remembers past corrections.
    plur: String,
}

impl PromptContext {
    pub fn build(cwd: &Path, is_subagent: bool, model: &str, provider: &str) -> Self {
        Self::build_with_opts(cwd, is_subagent, model, provider, false)
    }

    /// `poor_mode`: skip PLUR inject, skills catalog, and long memory excerpts
    /// to cut background token spend (toggle via `/poor`).
    pub fn build_with_opts(
        cwd: &Path,
        is_subagent: bool,
        model: &str,
        provider: &str,
        poor_mode: bool,
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
        let skills = if poor_mode {
            String::new()
        } else {
            cached_skills(cwd)
        };
        Self {
            cwd: cwd.to_path_buf(),
            is_subagent,
            model: model.to_string(),
            provider: provider.to_string(),
            shell_label: shell_backend().label.clone(),
            project: find_project_instructions(cwd),
            memory,
            skills,
            plur,
        }
    }

    /// Render with the live bits: permission mode and the todo list, both of
    /// which can change between requests within a turn.
    pub fn render(&self, mode: PermissionMode, todos_render: &str) -> String {
        let mode_block = match mode {
            PermissionMode::Plan => r#"
# Permission mode: PLAN  (explore + analyze, no repo changes)
You may read, parse, and understand the workspace freely, AND run shell for
analysis and scratch/media work — reading files, grep/ripgrep, running tests or
linters to observe, ffmpeg/extract_frames to cut up a video, copying a clip to a
temp dir, one-off analysis scripts, etc. Non-mutating compute never needs
permission here.

Free: read_file, list_dir, grep, glob, web_fetch, web_search, look, extract_frames,
git_status, git_diff, skill, memory(read), todo_write, submit_plan,
graphify(query|path|explain|status|report|affected), plur(recall|status|…),
ruflo(memory_search|status|…), executor(search|status), and bash for the above.

BLOCKED in plan mode (do NOT attempt — they need manual/auto via Shift+Tab):
- Authoring code: write_file, edit_file, multi_edit, apply_patch.
- Submitting/mutating the repo via shell: git commit/push/add/reset/checkout/
  restore/stash/merge/rebase/pull, gh pr create/merge, and dependency installs
  (npm/pnpm/yarn/pip/cargo/… install/add).
- Mutating knowledge: graphify(extract|update), plur(learn|capture),
  ruflo(memory_store|swarm_init), executor(call|install), memory(append), agent.

Do your investigation, then deliver the plan via submit_plan. Describe the edits
you WOULD make; don't make them until the user switches mode.
"#,
            PermissionMode::Manual => r#"
# Permission mode: MANUAL
Mutating tools need user approval. Prefer apply_patch/multi_edit for structured edits.
"#,
            PermissionMode::Auto => r#"
# Permission mode: AUTO
Tools auto-approved. Prefer minimal safe diffs; avoid destructive shell.
"#,
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
web_fetch, web_search, look, extract_frames, git_status, git_diff, graphify, plur, ruflo,
skill, memory, todo_write, submit_plan, agent

## Tool policy
- grep/glob: ripgrep-backed; pass narrow paths — never scan drive roots
- list_dir for directory shape; read_file for contents — cheaper than shell ls/cat
- Paths are sandboxed to the workspace
- bash: real shell when available (Git Bash/pwsh); output header labels the backend
- git_status/git_diff (diff|staged|log|show): approval-free repo inspection — prefer over bash git
- web_search → find docs/errors; web_fetch → read a result url (text only — not video)
- look: attach image(s) or a short video for **vision**. Prefer look over guessing from filenames.
- extract_frames: sparse keyframes via ffmpeg (default ~1fps, max ~8). Writes `.nur/frames/…`
  and auto-queues look. Use for design-from-video — never frame-by-frame every pixel.
- Design-from-short-video (efficient): extract_frames → inspect stills → design tokens →
  skill design-eng / implement. User paths to .png/.mp4 in the prompt auto-attach when present.
- graphify: code knowledge graph (graphify-out/). Prefer query/path/explain over broad grep when
  the graph exists. extract defaults to code-only AST (local, free).
- plur: shared engram memory (~/.plur/). learn corrections/preferences; inject/recall across
  sessions. Auto-injected at session start. Never store secrets.
- ruflo: vector memory + swarm harness. Global DB at ~/.nur/ruflo/. Prefer plur for preferences,
  ruflo for pattern/embedding memory, graphify for code structure.
- executor: MCP gateway (executor.sh) for external OpenAPI/GraphQL/MCP integrations — not for
  local repo edits. action=sources|search|call.
- skill: action=list / action=read — packs pre-installed: design-eng (Emil), clone-website-meta,
  cybersecurity (817 playbooks — load one by name, never all), context-pruning (DCP patterns),
  opencode-awesome catalog, executor-gateway, akm-manager, plur, ruflo, graphify.
- UI work → skill design-eng / emil-design-eng. Site clone → clone-website-meta. Security →
  cybersecurity router then skill(read, name=<specific>). Long context → /compact + context-pruning.
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

        s.push_str(&self.memory);
        s.push_str(&self.plur);
        s.push_str(&self.skills);
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
