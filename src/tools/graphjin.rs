//! GraphJin — governed access to live data. Wraps the `graphjin` CLI
//! (npm: `graphjin`; also Homebrew/Scoop/deb/rpm/Docker).
//!
//! GraphJin compiles GraphQL to optimised SQL across 12+ engines (Postgres,
//! MySQL, MongoDB, SQLite, Oracle, MSSQL, Snowflake, BigQuery, …) and serves it
//! behind roles, row-level security, and allow-lists. Alongside application
//! data it exposes `gj_*` system roots: `gj_catalog` (discovery), `gj_code`
//! (the repo as queryable tables), `gj_security`, `gj_config`, `gj_runtime`.
//!
//! **Shape of the integration.** `graphjin cli …` is an MCP/JSON-RPC *client*
//! that talks to a running GraphJin server; it is configured once with
//! `graphjin cli setup <url>` and stores the server + token in
//! `~/.config/graphjin/client.json`. So this tool needs no `--path` and holds
//! no credentials — it drives the CLI the operator already pointed at their
//! server. Ergonomic subcommands (`explain`, `query exec`, `audit`, `health`)
//! are used where they exist; discovery and the server-side agent go through
//! GraphJin's tool-name parity surface, `graphjin cli <tool> --args '{…}'`,
//! which is name-stable against `MCPAllToolNames`.
//!
//! **Why a first-class tool rather than a shell-out.** GraphJin's contract is
//! catalog-first and enforced server-side in Go: discovery must precede action,
//! an `answered` result is downgraded to `blocked` when a step was skipped, and
//! model-claimed actions never count — only real tool results do. That is only
//! worth anything if the caller surfaces it, so [`annotate_agent_result`] makes
//! a `blocked` envelope read as the failure it is instead of a plausible
//! answer.
//!
//! Complement, not replacement, for `graphify`: graphify *traverses* a local
//! code graph offline (path / affected); `gj_code` *joins* code to live data,
//! config, and security posture. See `docs/integrations-ax-graphjin.md`.

use super::{arg_str, Tool, ToolContext};
use crate::ecosystem;
use crate::error::{MuseError, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

pub struct GraphJin;

/// Actions that cannot change data.
///
/// Note the honest limit: nur classifies, GraphJin enforces. The server's own
/// `agent.read_only` kill-switch and per-role RLS are what actually stop a
/// write; this gate is nur's permission layer, not a security boundary.
pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "status".into());
    matches!(
        action.as_str(),
        "status" | "catalog" | "schema" | "help" | "explain" | "query" | "security" | "ask"
    )
}

/// Does this look like a GraphQL document rather than a saved-query name?
///
/// `query exec` takes a document; `query run` takes a name. A document always
/// has a selection set, and a saved-query name never contains braces.
pub fn looks_like_graphql(q: &str) -> bool {
    q.contains('{')
}

impl Tool for GraphJin {
    fn name(&self) -> &str {
        "graphjin"
    }

    fn description(&self) -> &str {
        "GraphJin — query live databases through one governed GraphQL→SQL surface \
         (Postgres/MySQL/Mongo/SQLite/Snowflake/BigQuery/…), plus the `gj_*` system roots: \
         gj_catalog (discovery), gj_code (the repo as queryable tables, joinable with live \
         data), gj_security, gj_config, gj_runtime. \
         DISCOVERY COMES FIRST — the server rejects action taken without it: \
         action=catalog: search gj_catalog for tables/queries matching an intent; \
         action=schema: detail for a catalog id; \
         action=help: GraphQL shape/usage guidance from the server; \
         action=explain: compile a query to SQL WITHOUT running it; \
         action=query: run a read query — a GraphQL document, or a saved query by name; \
         action=security: role permission matrix (who can read/write what); \
         action=ask: hand ONE instruction to GraphJin's server-side agent and get a typed, \
         evidence-backed answer; \
         action=mutate: a write — gated, and blocked in plan mode; \
         action=status (default): CLI, server, and database reachable? \
         A result whose status is `blocked` is a FAILURE, not an answer — it means required \
         discovery was skipped; go back to action=catalog. \
         Needs `graphjin` on PATH (npm i -g graphjin) and a one-time \
         `graphjin cli setup <server-url>`. \
         For offline code-structure questions with no database, prefer `graphify`."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["status", "catalog", "schema", "help", "explain", "query", "security", "ask", "mutate"],
                    "default": "status"
                },
                "query": {
                    "type": "string",
                    "description": "GraphQL document for explain/query/mutate — or, for query, the name of a saved query (no braces)"
                },
                "variables": {
                    "type": "object",
                    "description": "GraphQL variables for explain/query/mutate"
                },
                "search": {
                    "type": "string",
                    "description": "Intent to search gj_catalog for (action=catalog)"
                },
                "id": {
                    "type": "string",
                    "description": "Catalog entry id for action=schema"
                },
                "instruction": {
                    "type": "string",
                    "description": "One natural-language instruction for action=ask"
                },
                "role": {
                    "type": "string",
                    "description": "Role to run/compile as; row-level security applies (e.g. anon, user). For action=security, the role to audit (omit for all)."
                }
            }
        })
    }

    fn execute(&self, args: &Value, _ctx: &ToolContext) -> Result<String> {
        let action = arg_str(args, "action").unwrap_or_else(|_| "status".into());
        let role = arg_str(args, "role").ok().filter(|r| !r.is_empty());
        let vars = variables_json(args);

        match action.as_str() {
            "status" => status(),
            "catalog" => {
                let search = arg_str(args, "search")?;
                call_tool(
                    "query_catalog",
                    &serde_json::json!({ "search": search }),
                    60_000,
                )
            }
            "schema" => {
                let id = arg_str(args, "id")?;
                call_tool("query_catalog", &serde_json::json!({ "ids": [id] }), 60_000)
            }
            "help" => {
                let topic = arg_str(args, "search").unwrap_or_default();
                let payload = if topic.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::json!({ "query": topic })
                };
                call_tool("graphql_help", &payload, 60_000)
            }
            "explain" => {
                let q = arg_str(args, "query")?;
                let mut argv = vec!["cli".to_string(), "explain".to_string(), q];
                push_opt(&mut argv, "--role", role.as_deref());
                push_opt(&mut argv, "--vars", vars.as_deref());
                run(&argv, 60_000)
            }
            "query" | "mutate" => {
                let q = arg_str(args, "query")?;
                // `exec` for a document, `run` for a saved query by name.
                let mut argv = if looks_like_graphql(&q) {
                    vec!["cli".into(), "query".into(), "exec".into(), q]
                } else {
                    vec!["cli".into(), "query".into(), "run".into(), q]
                };
                push_opt(&mut argv, "--vars", vars.as_deref());
                run(&argv, 120_000)
            }
            "security" => {
                let mut argv = vec!["cli".to_string(), "audit".to_string()];
                if let Some(r) = role {
                    argv.push(r);
                }
                run(&argv, 60_000)
            }
            "ask" => {
                let instruction = arg_str(args, "instruction")?;
                let out = call_tool(
                    "ask_graphjin_agent",
                    &serde_json::json!({ "instruction": instruction }),
                    600_000,
                )?;
                Ok(annotate_agent_result(&out))
            }
            other => Err(MuseError::Tool(format!(
                "unknown graphjin action '{other}' — use \
                 status|catalog|schema|help|explain|query|security|ask|mutate"
            ))),
        }
    }
}

fn push_opt(argv: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(v) = value {
        argv.push(flag.to_string());
        argv.push(v.to_string());
    }
}

fn variables_json(args: &Value) -> Option<String> {
    args.get("variables")
        .filter(|v| !v.is_null())
        .map(|v| v.to_string())
}

/// GraphJin answers with a typed envelope; `blocked` means its guards refused
/// because discovery evidence was missing. A model reads a bare JSON blob as
/// success, so say it plainly and keep the envelope intact underneath.
pub fn annotate_agent_result(out: &str) -> String {
    let status = serde_json::from_str::<Value>(out)
        .ok()
        .and_then(|v| find_status(&v));
    match status.as_deref() {
        Some("blocked") => format!(
            "GraphJin BLOCKED this run — required discovery evidence was missing, so any \
             answer below is NOT trustworthy. Search gj_catalog for the entities involved \
             (action=catalog), inspect them (action=schema), then ask again.\n\n{out}"
        ),
        Some("refused") => format!(
            "GraphJin REFUSED this instruction (policy or role). Do not retry by rephrasing — \
             check action=security for the applicable rule.\n\n{out}"
        ),
        _ => out.to_string(),
    }
}

/// The envelope may arrive bare or wrapped in an MCP content payload, so look
/// for `status` at the top level and one level down.
fn find_status(v: &Value) -> Option<String> {
    if let Some(s) = v.get("status").and_then(|s| s.as_str()) {
        return Some(s.to_string());
    }
    for key in ["result", "data", "content"] {
        if let Some(inner) = v.get(key) {
            if let Some(s) = inner.get("status").and_then(|s| s.as_str()) {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn find_bin() -> Option<String> {
    ecosystem::find_bin("graphjin")
}

/// Where `graphjin cli setup` records the server URL + token.
fn client_config() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    Some(home.join(".config").join("graphjin").join("client.json"))
}

const INSTALL_HINT: &str = "graphjin CLI not found on PATH. Install with:\n  \
     npm install -g graphjin\n\
     (or: brew install dosco/graphjin/graphjin · scoop install graphjin · docker pull dosco/graphjin)\n\
     Then point it at a running server once:  graphjin cli setup <server-url>";

const SETUP_HINT: &str = "no GraphJin server configured — run once:\n  \
     graphjin cli setup <server-url>      e.g. http://localhost:8080\n\
     start one with:  graphjin serve --path ./config";

fn status() -> Result<String> {
    let mut s = String::new();
    let bin = match find_bin() {
        Some(bin) => {
            s.push_str(&format!("graphjin CLI: {bin}\n"));
            if let Some(v) = ecosystem::cmd_version_pub(&bin, &["version"]) {
                s.push_str(&format!("version: {v}\n"));
            }
            bin
        }
        None => {
            s.push_str("graphjin CLI: NOT FOUND\n");
            s.push_str(INSTALL_HINT);
            s.push('\n');
            return Ok(s);
        }
    };
    let _ = bin;

    match client_config().filter(|p| p.is_file()) {
        Some(p) => s.push_str(&format!("client config: {}\n", p.display())),
        None => {
            s.push_str("client config: NOT FOUND\n");
            s.push_str(SETUP_HINT);
            s.push('\n');
            return Ok(s);
        }
    }

    // Database reachability is the real readiness signal — a configured client
    // pointing at a server with no database behind it is not usable.
    match run(&["cli".into(), "health".into()], 30_000) {
        Ok(out) => {
            let head: String = out.lines().take(12).collect::<Vec<_>>().join("\n");
            s.push_str(&format!("server: reachable\n{head}\n"));
        }
        Err(e) => s.push_str(&format!("server: unreachable — {e}\n")),
    }
    Ok(s)
}

/// Call an MCP tool by its exact name through GraphJin's parity surface.
fn call_tool(tool: &str, args: &Value, timeout_ms: u64) -> Result<String> {
    run(
        &[
            "cli".to_string(),
            tool.to_string(),
            "--args".to_string(),
            args.to_string(),
        ],
        timeout_ms,
    )
}

fn run(args: &[String], timeout_ms: u64) -> Result<String> {
    let bin = find_bin().ok_or_else(|| MuseError::Tool(INSTALL_HINT.into()))?;
    let mut argv: Vec<&str> = args.iter().map(String::as_str).collect();
    // Structured output — the default, but pinned so a config change cannot
    // silently turn results into a human table the model has to parse.
    argv.extend_from_slice(&["--format", "json"]);
    ecosystem::run_capture(&bin, &argv, None::<&Path>, timeout_ms).map_err(|e| {
        if e.contains("cli setup") || e.contains("no GraphJin server") {
            MuseError::Tool(format!("{e}\n\n{SETUP_HINT}"))
        } else {
            MuseError::Tool(format!("graphjin {}: {e}", args.join(" ")))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_mutate_is_write_class() {
        for action in [
            "status", "catalog", "schema", "help", "explain", "query", "security", "ask",
        ] {
            let args = serde_json::json!({ "action": action }).to_string();
            assert!(is_read_only_action(&args), "{action} must be read-only");
        }
        assert!(
            !is_read_only_action(r#"{"action":"mutate"}"#),
            "mutate must be gated"
        );
        // Fail-closed on an action we don't know.
        assert!(!is_read_only_action(r#"{"action":"drop_everything"}"#));
        // Missing action defaults to status.
        assert!(is_read_only_action("{}"));
    }

    #[test]
    fn a_blocked_agent_answer_is_labelled_as_a_failure() {
        let blocked = r#"{"status":"blocked","answer":"There are 42 orders.","evidence":[]}"#;
        let out = annotate_agent_result(blocked);
        assert!(out.contains("BLOCKED"), "must not read as a plain answer");
        assert!(out.contains("NOT trustworthy"));
        assert!(out.contains(blocked), "the raw envelope must survive");

        assert!(annotate_agent_result(r#"{"status":"refused"}"#).contains("REFUSED"));
        // Wrapped in an MCP content envelope, the status must still be found.
        let wrapped = r#"{"result":{"status":"blocked","answer":"x"}}"#;
        assert!(annotate_agent_result(wrapped).contains("BLOCKED"));

        // A real answer passes through untouched.
        let ok = r#"{"status":"answered","answer":"42","evidence":["gj_catalog:orders"]}"#;
        assert_eq!(annotate_agent_result(ok), ok);
        // Non-JSON output is never mangled.
        assert_eq!(annotate_agent_result("plain text"), "plain text");
    }

    /// `query exec` takes a document, `query run` takes a name — picking the
    /// wrong one is a silent "saved query not found".
    #[test]
    fn documents_and_saved_query_names_are_told_apart() {
        assert!(looks_like_graphql("{ users { id email } }"));
        assert!(looks_like_graphql(
            "query Q($id: ID!) { user(id: $id) { id } }"
        ));
        assert!(!looks_like_graphql("orders_by_region"));
        assert!(!looks_like_graphql("getUser"));
    }

    #[test]
    fn optional_flags_are_omitted_rather_than_passed_empty() {
        let mut argv = vec!["cli".to_string(), "explain".to_string()];
        push_opt(&mut argv, "--role", None);
        assert_eq!(argv.len(), 2, "a missing role must not add a bare flag");
        push_opt(&mut argv, "--role", Some("anon"));
        assert_eq!(argv[2..], ["--role".to_string(), "anon".to_string()]);
    }

    #[test]
    fn variables_are_forwarded_as_json_only_when_present() {
        assert_eq!(variables_json(&serde_json::json!({})), None);
        assert_eq!(
            variables_json(&serde_json::json!({"variables": null})),
            None
        );
        assert_eq!(
            variables_json(&serde_json::json!({"variables": {"id": 7}})).as_deref(),
            Some(r#"{"id":7}"#)
        );
    }

    /// The parity surface is name-stable against the server's tool list; these
    /// are the exact names `MCPAllToolNames` guarantees.
    #[test]
    fn parity_tool_calls_use_exact_server_tool_names() {
        for tool in ["query_catalog", "graphql_help", "ask_graphjin_agent"] {
            let err = call_tool(tool, &serde_json::json!({}), 1_000);
            // With no binary installed this is the install hint; with one, it is
            // a setup/connection error. Either way it must never be a panic and
            // must stay attributable to graphjin.
            if let Err(e) = err {
                let msg = e.to_string();
                assert!(
                    msg.contains("graphjin"),
                    "error for {tool} should name the tool: {msg}"
                );
            }
        }
    }

    #[test]
    fn a_missing_binary_explains_how_to_install_it() {
        assert!(INSTALL_HINT.contains("npm install -g graphjin"));
        assert!(INSTALL_HINT.contains("graphjin cli setup"));
        assert!(SETUP_HINT.contains("graphjin serve"));
    }
}
