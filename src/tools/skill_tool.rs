//! Skill tool — list installed skills and load a skill's full instructions
//! on demand. Skills are never bulk-injected into the system prompt.

use super::{arg_str, Tool, ToolContext};
use crate::agent::skills::{load_skills, skills_prompt_section};
use crate::error::{NurError, Result};
use serde_json::Value;

pub struct SkillTool;

impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Agent skills (SKILL.md packs). Skills are on-demand only (not pre-loaded). \
         action=list shows installed skills; action=search finds installed ones and those \
         available in the skills market (only the user installs: suggest `nur skills add \
         <id>`); action=read loads a named skill's full instructions. Prefer when NL/slash \
         did not already activate a skill."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "search", "read"], "default": "list"},
                "name": {"type": "string", "description": "Skill name (for action=read)"},
                "query": {"type": "string", "description": "Words to match: a filter for action=list, the search for action=search, e.g. \"audit\" or \"postgres\""}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "list".into());
        let skills = load_skills(&ctx.cwd);

        match action.as_str() {
            "list" => {
                // Optional filter: the full catalog is 1500+ entries and the
                // unfiltered dump used to overflow into the context store,
                // hiding exactly the skills the caller was hunting for
                // (session 5b30168a: `list` truncated before sc-research).
                if let Ok(q) = arg_str(args, "query") {
                    let q = q.trim();
                    if !q.is_empty() {
                        let ql = q.to_ascii_lowercase();
                        let matches: Vec<&crate::agent::skills::Skill> = skills
                            .iter()
                            .filter(|s| {
                                s.name.to_ascii_lowercase().contains(&ql)
                                    || s.description.to_ascii_lowercase().contains(&ql)
                            })
                            .collect();
                        if matches.is_empty() {
                            return Ok(format!(
                                "No installed skills match '{q}'. Try a shorter substring \
                                 (e.g. query=\"audit\", query=\"fuzz\", query=\"defi\")."
                            ));
                        }
                        let mut out =
                            format!("Installed skills matching '{q}' ({}):\n", matches.len());
                        for sk in &matches {
                            out.push_str(&format!("- **{}**: {}\n", sk.name, sk.description));
                        }
                        out.push_str(
                            "\nUse skill(action=read, name=<name>) for full instructions.",
                        );
                        return Ok(out);
                    }
                }
                let mut out = skills_prompt_section(&skills);
                if !skills.is_empty() {
                    out.push_str(
                        "\nUse skill(action=read, name=<name>) for full instructions. \
                         Tip: the catalog is large - skill(action=list, query=\"<substring>\") \
                         filters by name/description. \
                         Users can also activate via /skill-name or natural-language intent.",
                    );
                }
                Ok(out)
            }
            // Installed skills plus the market index. The model can find a
            // skill to suggest but never install one: new instructions in the
            // next session must be the user's own action.
            "search" => {
                let query = arg_str(args, "query").unwrap_or_default();
                let q = query.trim().to_ascii_lowercase();
                if q.is_empty() {
                    return Err(NurError::Tool("skill search needs `query`".into()));
                }
                let installed: Vec<&crate::agent::skills::Skill> = skills
                    .iter()
                    .filter(|s| {
                        q.split_whitespace().all(|w| {
                            s.name.to_ascii_lowercase().contains(w)
                                || s.description.to_ascii_lowercase().contains(w)
                        })
                    })
                    .take(15)
                    .collect();
                let mut out = format!("Installed ({}):\n", installed.len());
                for sk in &installed {
                    out.push_str(&format!("- **{}**: {}\n", sk.name, sk.description));
                }
                match crate::skills_market::load_index() {
                    Ok(index) => {
                        let available: Vec<String> = crate::skills_market::search(&index, &q, 15)
                            .into_iter()
                            .filter(|e| {
                                !skills.iter().any(|s| s.name.eq_ignore_ascii_case(&e.name))
                            })
                            .map(|e| crate::skills_market::describe(e, false))
                            .collect();
                        out.push_str(&format!(
                            "\nAvailable in the market ({}); only the user installs these, \
                             with `/skills add <id>` - suggest one, never run it yourself:\n",
                            available.len()
                        ));
                        if let Some(note) = &index.unsigned {
                            out.push_str(&format!("({note})\n"));
                        }
                        for line in available {
                            out.push_str(&format!("- {line}\n"));
                        }
                    }
                    Err(e) => out.push_str(&format!("\nMarket index unavailable: {e}\n")),
                }
                Ok(out)
            }
            "read" => {
                let name = arg_str(args, "name")?;
                let skill = skills
                    .iter()
                    .find(|s| s.name.eq_ignore_ascii_case(&name))
                    .ok_or_else(|| {
                        NurError::Tool(format!(
                            "skill '{name}' not found — action=list to see installed skills"
                        ))
                    })?;
                read_skill_body(skill)
            }
            other => Err(NurError::Tool(format!(
                "unknown action '{other}' - use list, search or read"
            ))),
        }
    }
}

/// Load a skill's full instructions for the model. `disable-model-invocation`
/// skills are user-only: the tool refuses so the model cannot pull a body the
/// author explicitly kept out of model reach.
fn read_skill_body(skill: &crate::agent::skills::Skill) -> Result<String> {
    if skill.disable_model_invocation {
        return Err(NurError::Tool(format!(
            "skill '{}' sets disable-model-invocation - the user can run /{} explicitly",
            skill.name, skill.name
        )));
    }
    // Re-read the file so large packs aren't truncated.
    let body = std::fs::read_to_string(&skill.path)
        .map(|t| crate::agent::skills::strip_frontmatter(&t).unwrap_or(t))
        .unwrap_or_else(|_| skill.body.clone());
    let body: String = body.chars().take(80_000).collect();
    // A market-installed skill is third-party text: say whose, at what commit.
    let source = skill
        .path
        .parent()
        .and_then(crate::skills_market::provenance_line)
        .map(|line| format!("{line}\n"))
        .unwrap_or_default();
    Ok(format!(
        "# Skill: {} ({})\n{source}\n{}",
        skill.name,
        skill.path.display(),
        body
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(dir: &std::path::Path, frontmatter: &str) -> std::path::PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let path = dir.join("SKILL.md");
        std::fs::write(
            &path,
            format!("---\n{frontmatter}---\n\n# Body\nstep one\n"),
        )
        .unwrap();
        path
    }

    #[test]
    fn read_refuses_a_disable_model_invocation_skill() {
        let dir = std::env::temp_dir().join(format!("nur-skilltool-{}", std::process::id()));
        let path = write_skill(
            &dir,
            "name: checklist-helper\ndescription: keeps a checklist\ndisable-model-invocation: true\n",
        );
        let skill = crate::agent::skills::parse_skill(&path).unwrap();
        let err = read_skill_body(&skill).unwrap_err().to_string();
        assert!(
            err.contains("disable-model-invocation"),
            "expected refusal, got: {err}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn read_loads_a_normal_skill() {
        let dir = std::env::temp_dir().join(format!("nur-skilltool-ok-{}", std::process::id()));
        let path = write_skill(&dir, "name: plain-helper\ndescription: helps\n");
        let skill = crate::agent::skills::parse_skill(&path).unwrap();
        let body = read_skill_body(&skill).unwrap();
        assert!(body.contains("step one"), "{body}");
        std::fs::remove_dir_all(&dir).ok();
    }
}
