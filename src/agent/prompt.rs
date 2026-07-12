use super::memory::memory_prompt_excerpt;
use super::mode::PermissionMode;
use super::skills::{load_skills, skills_prompt_section};
use crate::tools::shell_backend;
use std::path::{Path, PathBuf};

pub const PROJECT_INSTRUCTION_FILES: &[&str] = &["MUSE.md", "AGENTS.md", "CLAUDE.md"];

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
/// memory, skills, shell probe). Built **once per user turn** — rebuilding it
/// per model request re-read every SKILL.md and MUSE.md on every API call.
pub struct PromptContext {
    cwd: PathBuf,
    is_subagent: bool,
    shell_label: String,
    project: Option<(String, String)>,
    memory: String,
    skills: String,
}

impl PromptContext {
    pub fn build(cwd: &Path, is_subagent: bool) -> Self {
        Self {
            cwd: cwd.to_path_buf(),
            is_subagent,
            shell_label: shell_backend().label.clone(),
            project: find_project_instructions(cwd),
            memory: memory_prompt_excerpt(3000),
            skills: skills_prompt_section(&load_skills(cwd)),
        }
    }

    /// Render with the live bits: permission mode and the todo list, both of
    /// which can change between requests within a turn.
    pub fn render(&self, mode: PermissionMode, todos_render: &str) -> String {
        let mode_block = match mode {
            PermissionMode::Plan => r#"
# Permission mode: PLAN
Research/design only. Tools: read_file, list_dir, grep, glob, web_fetch, web_search,
git_status, git_diff, skill, memory(read), todo_write, submit_plan.
No write_file/edit_file/multi_edit/apply_patch/bash/agent. Deliver plans via submit_plan.
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

        let role = if self.is_subagent {
            "You are a focused SUBAGENT. Complete the delegated task and return a concise report. Do not ask the user questions."
        } else {
            "You are Muse — a Claude-class coding agent for Meta CLI (unofficial), powered by Muse Spark on Meta Model API."
        };

        let mut s = format!(
            r#"{role}

Workspace: {}
OS: {} · shell: {}

{mode_block}
# Tools
read_file, list_dir, write_file, edit_file, multi_edit, apply_patch, bash, grep, glob,
web_fetch, web_search, git_status, git_diff, skill, memory, todo_write, submit_plan, agent

## Tool policy
- grep/glob: ripgrep-backed; pass narrow paths — never scan drive roots
- list_dir for directory shape; read_file for contents — cheaper than shell ls/cat
- Paths are sandboxed to the workspace
- bash: real shell when available (Git Bash/pwsh); output header labels the backend
- git_status/git_diff (diff|staged|log|show): approval-free repo inspection — prefer over bash git
- web_search → find docs/errors; web_fetch → read a result url
- skill: action=list shows installed skill packs; action=read loads full instructions —
  check before starting a task a skill covers
- agent: spawn explore (read-only) or general subagent for parallel research
- todo_write: maintain a live task list for multi-step work (always keep one in_progress)
- submit_plan: formal plan artifact in plan mode
- memory: durable notes in ~/.muse/memory.md (never store secrets)
- Prefer edit_file / multi_edit / apply_patch over full rewrites

# Workflow (Claude-class)
1. Orient — git_status + targeted grep/read
2. Plan — todo_write for multi-step; submit_plan in plan mode
3. Implement — smallest correct change; verify with tests/build
4. Report — what changed, how to verify

# Style
Direct technical markdown. Fence code with languages.
Unofficial community software — not Meta Platforms, Inc.
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
) -> String {
    PromptContext::build(cwd, is_subagent).render(mode, todos_render)
}
