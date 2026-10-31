//! `tool_search`: load the schemas of tools the `lean` profile keeps out of
//! the request until they are needed.
//!
//! Every request re-sends every tool schema. The full roster is ~15k tokens,
//! most of it ecosystem tools a given session never calls, and a model handed
//! dozens of tools uses them worse: Vesalius measured its tools going unused
//! in 22 of 24 runs at 20 tools and used in every run at 5. Claude Code keeps
//! rarely used tools deferred behind a search tool and OMP behind `xd://`
//! devices; this is the same idea.
//!
//! The model calls the real tool by its real name once it is loaded, so
//! permissions, approvals, plan mode and receipts see exactly what they see
//! today. Which tools are loaded is read back from the conversation itself
//! ([`loaded_from_history`]): no extra session state, and `/resume` restores
//! the same roster. Loaded schemas are appended after the always-on ones, so
//! a load costs one cache miss and later requests share the longer prefix.

use super::{Tool, ToolContext, ToolDef, ToolHost};
use crate::error::Result;
use serde_json::{json, Value};

pub const NAME: &str = "tool_search";

/// Marker line in a `tool_search` result naming what it loaded. The roster is
/// rebuilt from these lines, so the wording is part of the format.
const LOADED_PREFIX: &str = "Loaded: ";

/// Tools the `lean` profile offers on every request: the editing shell, the
/// session machinery the turn state refers to (todos, goal, background jobs,
/// peer messages, context store), delegation, and this loader.
pub const LEAN_ALWAYS_ON: &[&str] = &[
    "read_file",
    "list_dir",
    "write_file",
    "edit_file",
    "multi_edit",
    "apply_patch",
    "hashline_edit",
    "bash",
    "sgrep",
    "grep",
    "glob",
    "web_fetch",
    "web_search",
    "look",
    "git_status",
    "git_diff",
    "context",
    "goal",
    "message",
    "bg",
    "skill",
    "memory",
    "question",
    "secret",
    "todo_write",
    "submit_plan",
    "agent",
    "codemode",
    NAME,
];

/// Is `name` a tool the lean profile defers? (History scans call this per
/// item, so the roster's names are computed once.)
pub fn is_deferred(name: &str) -> bool {
    static DEFERRED: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    DEFERRED
        .get_or_init(|| deferred_defs().into_iter().map(|d| d.name).collect())
        .iter()
        .any(|n| n == name)
}

/// The deferred tools, in roster order.
pub fn deferred_defs() -> Vec<ToolDef> {
    ToolHost::default()
        .tool_defs()
        .into_iter()
        .filter(|d| d.name != NAME && !LEAN_ALWAYS_ON.contains(&d.name.as_str()))
        .collect()
}

/// One line per deferred tool for the system prompt: name and the first
/// sentence of its description, so the model knows what exists.
pub fn deferred_catalog() -> String {
    static CATALOG: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CATALOG
        .get_or_init(|| {
            deferred_defs()
                .iter()
                .map(|d| format!("- {}: {}", d.name, first_sentence(d.description.as_deref())))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .clone()
}

fn first_sentence(description: Option<&str>) -> String {
    let text = description.unwrap_or_default().trim();
    let line = text.lines().next().unwrap_or_default();
    let cut = line
        .find(". ")
        .map(|i| &line[..=i])
        .unwrap_or(line)
        .trim_end_matches('.');
    if cut.chars().count() > 110 {
        let short: String = cut.chars().take(107).collect();
        format!("{}...", short.trim_end())
    } else {
        cut.to_string()
    }
}

/// Deferred tools this conversation has loaded: every deferred tool named on
/// a `Loaded:` line of a `tool_search` result, and every deferred tool the
/// model has already called (a resumed session keeps what it used). In first
/// appearance order, so the roster only ever grows at its end.
pub fn loaded_from_history(items: &[Value]) -> Vec<String> {
    let search_calls: std::collections::HashSet<&str> = items
        .iter()
        .filter(|item| item_type(item) == Some("function_call"))
        .filter(|item| item.get("name").and_then(Value::as_str) == Some(NAME))
        .filter_map(|item| item.get("call_id").and_then(Value::as_str))
        .collect();
    let mut loaded: Vec<String> = Vec::new();
    let mut add = |name: &str| {
        if is_deferred(name) && !loaded.iter().any(|n| n == name) {
            loaded.push(name.to_string());
        }
    };
    for item in items {
        match item_type(item) {
            Some("function_call") => {
                if let Some(name) = item.get("name").and_then(Value::as_str) {
                    add(name);
                }
            }
            Some("function_call_output") => {
                let from_search = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| search_calls.contains(id));
                if !from_search {
                    continue;
                }
                let output = output_text(item);
                for line in output.lines() {
                    if let Some(names) = line.trim().strip_prefix(LOADED_PREFIX) {
                        for name in names.split(',') {
                            add(name.trim());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    loaded
}

fn item_type(item: &Value) -> Option<&str> {
    item.get("type").and_then(Value::as_str)
}

/// A function_call_output's text, whether `output` is a string or content parts.
fn output_text(item: &Value) -> String {
    match item.get("output") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Rank deferred tools against a free-text query: exact name first, then by
/// how many query words appear in the name and description.
fn search(query: &str, max: usize) -> Vec<ToolDef> {
    let query = query.to_ascii_lowercase();
    let words: Vec<&str> = query
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|w| w.len() > 1)
        .collect();
    let mut scored: Vec<(usize, ToolDef)> = deferred_defs()
        .into_iter()
        .filter_map(|def| {
            let name = def.name.to_ascii_lowercase();
            let text = format!(
                "{name} {}",
                def.description
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
            );
            let mut score = words.iter().filter(|w| text.contains(**w)).count();
            if words.iter().any(|w| *w == name) {
                score += 100;
            }
            (score > 0).then_some((score, def))
        })
        .collect();
    scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
    scored.into_iter().take(max).map(|(_, def)| def).collect()
}

pub struct ToolSearch;

impl Tool for ToolSearch {
    fn name(&self) -> &str {
        NAME
    }

    fn description(&self) -> &str {
        "Load tools that are not offered yet. The system prompt lists them by name under \
         'Loaded on demand'. Pass `names` for tools you already know, or `query` describing \
         the job (e.g. \"hand-drawn diagram\", \"browser screenshot\"). Their full schemas \
         come back and, from the next request on, you can call them by name like any other tool."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "names": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Exact tool names to load."
                },
                "query": {
                    "type": "string",
                    "description": "What you need done, when you do not know the tool's name."
                },
                "max_results": {
                    "type": "integer",
                    "description": "Most tools a query loads (default 3, at most 8)."
                }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let mut found: Vec<ToolDef> = Vec::new();
        let mut unknown: Vec<String> = Vec::new();
        if let Some(names) = args.get("names").and_then(Value::as_array) {
            let deferred = deferred_defs();
            for name in names.iter().filter_map(Value::as_str) {
                match deferred.iter().find(|d| d.name == name) {
                    Some(def) => found.push(def.clone()),
                    None if LEAN_ALWAYS_ON.contains(&name) => {}
                    None => unknown.push(name.to_string()),
                }
            }
        }
        if let Some(query) = args.get("query").and_then(Value::as_str) {
            let max = args
                .get("max_results")
                .and_then(Value::as_u64)
                .unwrap_or(3)
                .clamp(1, 8) as usize;
            for def in search(query, max) {
                if !found.iter().any(|d| d.name == def.name) {
                    found.push(def);
                }
            }
        }
        if found.is_empty() {
            let mut out = String::from("No deferred tool matched.");
            if !unknown.is_empty() {
                out.push_str(&format!(" Unknown: {}.", unknown.join(", ")));
            }
            out.push_str(" Available on demand:\n");
            out.push_str(&deferred_catalog());
            return Ok(out);
        }
        let names: Vec<&str> = found.iter().map(|d| d.name.as_str()).collect();
        let mut out = format!(
            "{LOADED_PREFIX}{}\nCall them by name from the next step on.\n",
            names.join(", ")
        );
        if !unknown.is_empty() {
            out.push_str(&format!("Unknown: {}\n", unknown.join(", ")));
        }
        for def in &found {
            out.push_str(&format!(
                "\n## {}\n{}\nParameters: {}\n",
                def.name,
                def.description.as_deref().unwrap_or_default(),
                def.parameters.clone().unwrap_or_else(|| json!({}))
            ));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(id: &str, name: &str) -> Value {
        json!({"type": "function_call", "call_id": id, "name": name, "arguments": "{}"})
    }

    fn output(id: &str, text: &str) -> Value {
        json!({"type": "function_call_output", "call_id": id, "output": text})
    }

    /// The roster is rebuilt from the conversation, so a load must survive a
    /// resume, a `Loaded:` line from any other tool must not count, and an
    /// always-on or unknown name must never be appended.
    #[test]
    fn loaded_tools_come_from_search_results_and_calls_only() {
        let items = vec![
            call("1", NAME),
            output("1", "Loaded: excalidraw, tldraw\nCall them by name"),
            call("2", "bash"),
            output("2", "Loaded: graphjin"),
            call("3", "excalidraw"),
            call("4", "graphify"),
            call("5", NAME),
            output("5", "Loaded: read_file, no_such_tool"),
        ];
        assert_eq!(
            loaded_from_history(&items),
            vec!["excalidraw", "tldraw", "graphify"]
        );
        assert!(loaded_from_history(&[]).is_empty());
    }

    /// Searching by name or by job finds the deferred tool, and the result
    /// carries the line the roster is rebuilt from.
    #[test]
    fn search_by_name_or_job_loads_the_schema() {
        let ctx = ToolContext::default_for_test(std::path::Path::new("."));
        let by_name = ToolSearch
            .execute(&json!({"names": ["excalidraw"]}), &ctx)
            .unwrap();
        assert!(by_name.starts_with("Loaded: excalidraw\n"), "{by_name}");
        assert!(by_name.contains("Parameters: {"));

        let by_job = ToolSearch
            .execute(&json!({"query": "hand-drawn diagram"}), &ctx)
            .unwrap();
        let loaded = by_job.lines().next().unwrap();
        assert!(loaded.contains("excalidraw"), "{by_job}");

        let nothing = ToolSearch
            .execute(&json!({"names": ["no_such_tool"]}), &ctx)
            .unwrap();
        assert!(nothing.starts_with("No deferred tool matched."));
        assert!(!nothing.contains(LOADED_PREFIX));
    }

    /// Every always-on name is a real tool, so the lean surface cannot
    /// silently lose one when the roster changes.
    #[test]
    fn always_on_names_exist_in_the_roster() {
        let names: Vec<String> = ToolHost::default()
            .tool_defs()
            .into_iter()
            .map(|d| d.name)
            .collect();
        for name in LEAN_ALWAYS_ON {
            if *name == "codemode" && !cfg!(feature = "codemode") {
                continue;
            }
            assert!(names.iter().any(|n| n == name), "{name} is not a tool");
        }
        assert!(!deferred_defs().is_empty());
    }
}
