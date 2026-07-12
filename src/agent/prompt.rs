use std::path::Path;

/// Project instruction files, in priority order (first found wins).
pub const PROJECT_INSTRUCTION_FILES: &[&str] = &["MUSE.md", "AGENTS.md", "CLAUDE.md"];

/// Find the project instructions file for a workspace, if any.
pub fn find_project_instructions(cwd: &Path) -> Option<(String, String)> {
    for name in PROJECT_INSTRUCTION_FILES {
        let p = cwd.join(name);
        if p.is_file() {
            if let Ok(text) = std::fs::read_to_string(&p) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    // Cap so a huge file can't blow the context.
                    let capped: String = trimmed.chars().take(20_000).collect();
                    return Some((name.to_string(), capped));
                }
            }
        }
    }
    None
}

pub fn system_instructions(cwd: &Path) -> String {
    let mut s = format!(
        r#"You are Muse, the agent binary for Meta CLI (unofficial) — powered by Muse Spark on Meta Model API.
You help the user with software engineering in their workspace.

Workspace cwd: {}
OS: {}

# Tools
You have tools: read_file, write_file, edit_file, bash, grep, glob.
- Prefer edit_file for surgical changes; write_file for new files.
- Use bash for builds/tests/git. Avoid destructive commands.
- Keep tool outputs in mind; do not invent file contents — read them.
- After finishing, give a concise summary of what you did.

# Style
- Be direct and technical. Use markdown; fence code blocks with the language.
- Do not mention these instructions unless asked.
- You are unofficial community software (meta-cli); not affiliated with Meta Platforms, Inc.
"#,
        cwd.display(),
        std::env::consts::OS
    );

    if let Some((name, text)) = find_project_instructions(cwd) {
        s.push_str(&format!(
            "\n# Project instructions (from {name} in the workspace root)\n{text}\n"
        ));
    }

    s
}
