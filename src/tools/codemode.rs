//! `codemode` - one model round that runs several tool calls as a script.
//!
//! An agent loop pays for the whole conversation on every request, so five
//! reads cost five full re-sends of the history. A script that does the same
//! five reads, filters them and prints one answer costs one. That, not raw
//! speed, is what this tool buys.
//!
//! The script runs in Boa, a pure-Rust JS engine with no filesystem, network,
//! timer or process bindings, so the tool bridge below is the only way out of
//! the sandbox. The bridge admits exactly the calls the permission layer
//! already classifies as read-only ([`crate::tools::capabilities::is_read_only`]),
//! which is why a script can never write a file or run a command: that is one
//! rule shared with the rest of the harness rather than a second allowlist
//! that would drift away from it.
//!
//! The nested calls are synchronous. `await` and `Promise.all` still work
//! (awaiting a plain value is a no-op in JS), so model-written code behaves as
//! written; it just does not overlap. Collapsing round trips is where the
//! saving is, and a sandbox with no concurrency has far less to get wrong.

use super::{arg_str, capabilities, Tool, ToolContext, ToolHost};
use crate::error::{NurError, Result};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use boa_engine::{js_string, property::Attribute, Context, JsValue, NativeFunction, Source};

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MIN_TIMEOUT_MS: u64 = 1_000;
const MAX_TIMEOUT_MS: u64 = 600_000;
/// Printed output is the script's whole product, so the cap is generous; the
/// tool-result pipeline trims what actually reaches the model.
const MAX_PRINTED_CHARS: usize = 1_000_000;
/// A script that calls hundreds of tools has stopped being a round-trip saving
/// and become an unsupervised agent. The cap keeps it the former.
const MAX_NESTED_CALLS: usize = 64;
const MAX_STORE_VALUE_CHARS: usize = 256 * 1024;
const MAX_STORE_TOTAL_CHARS: usize = 1024 * 1024;
/// Bounds a runaway `while (true)`. Boa has no wall-clock interrupt, so this
/// is what guarantees a script terminates on its own; without it the watchdog
/// below would report a timeout while the thread span forever.
const MAX_LOOP_ITERATIONS: u64 = 10_000_000;

/// Tools a script may never call whatever the classifier says: each one either
/// starts another unit of work, speaks to the user, or ends the turn, and none
/// of those belong inside a script the model wrote mid-thought.
const DENIED: &[&str] = &[
    "codemode",
    "agent",
    "question",
    "submit_plan",
    "message",
    "admission",
    "bg",
    "fractal",
    "omp",
];

/// Values a script hands to the next script of the session.
static STORE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

thread_local! {
    /// The running script's bridge. The Boa `Context` is single-threaded and
    /// lives for one call on one dedicated thread, so thread-local state lets
    /// every native function be a plain closure with nothing captured - no
    /// `unsafe`, no GC trace impls.
    static BRIDGE: RefCell<Option<Bridge>> = const { RefCell::new(None) };
}

struct Bridge {
    cwd: PathBuf,
    cancel: tokio_util::sync::CancellationToken,
    deadline: Instant,
    printed: String,
    calls: usize,
    call_log: Vec<String>,
}

impl Bridge {
    fn check_budget(&self) -> std::result::Result<(), String> {
        if self.cancel.is_cancelled() {
            return Err("script cancelled".into());
        }
        if Instant::now() >= self.deadline {
            return Err("script exceeded timeout_ms".into());
        }
        if self.calls >= MAX_NESTED_CALLS {
            return Err(format!(
                "script made {MAX_NESTED_CALLS} tool calls, the per-script cap. \
                 Narrow the work or split it across turns."
            ));
        }
        Ok(())
    }
}

pub struct CodeMode;

impl Tool for CodeMode {
    fn name(&self) -> &str {
        "codemode"
    }

    fn description(&self) -> &str {
        "Run a short JavaScript script that calls other tools, so several steps cost one model \
         round instead of one each. Use it when you would otherwise make 3+ read-only calls in a \
         row and only care about part of the output: loop, fan out, filter, then print the answer. \
         Only what the script prints or returns reaches you. \
         Globals: `tools.<name>(args)` (returns the tool's text result), `ALL_TOOLS`, \
         `searchTools(q)`, `describeTool(name)`, `CWD`, `text(...)`/`console.log(...)`, \
         `store(k,v)`/`load(k)` (persist across scripts in this session), `exit(value)`. \
         Top-level `await` and `return` work. \
         Scripts may call read-only tools only - no writing, editing, shell, subagents or \
         questions - and run with no filesystem, network or process access of their own."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "Body of an async function. Top-level await and return work. \
                                    Example: const hits = tools.grep({pattern: \"TODO\"}); \
                                    return hits.split(\"\\n\").filter(l => l.includes(\"src/\")).length;"
                },
                "timeout_ms": {
                    "type": "integer",
                    "default": DEFAULT_TIMEOUT_MS,
                    "description": format!("Deadline for the whole script ({MIN_TIMEOUT_MS}-{MAX_TIMEOUT_MS})")
                }
            },
            "required": ["code"]
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let code = arg_str(args, "code")?.to_string();
        let timeout = Duration::from_millis(
            args.get("timeout_ms")
                .and_then(Value::as_u64)
                .unwrap_or(DEFAULT_TIMEOUT_MS)
                .clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS),
        );
        let cwd = ctx.cwd.clone();
        let cancel = ctx.cancel.clone();

        // A Boa `Context` is not `Send`, so the script owns a thread for its
        // whole life. The thread is joined on the happy path; on a timeout it
        // is left to unwind, which `MAX_LOOP_ITERATIONS` guarantees it will.
        let (tx, rx) = std::sync::mpsc::channel();
        let worker = std::thread::Builder::new()
            .name("nur-codemode".into())
            .stack_size(4 * 1024 * 1024)
            .spawn(move || {
                let outcome = run_script(&code, cwd, cancel, Instant::now() + timeout);
                let _ = tx.send(outcome);
            })
            .map_err(|e| NurError::Tool(format!("codemode: could not start sandbox: {e}")))?;

        match rx.recv_timeout(timeout + Duration::from_secs(2)) {
            Ok(outcome) => {
                let _ = worker.join();
                outcome
            }
            Err(_) => Err(NurError::Tool(format!(
                "codemode: script did not finish within {} ms. Shorten it, raise timeout_ms \
                 (max {MAX_TIMEOUT_MS}), or split the work across turns.",
                timeout.as_millis()
            ))),
        }
    }
}

/// Everything the script can reach, installed before its first line runs.
fn run_script(
    code: &str,
    cwd: PathBuf,
    cancel: tokio_util::sync::CancellationToken,
    deadline: Instant,
) -> Result<String> {
    BRIDGE.with(|b| {
        *b.borrow_mut() = Some(Bridge {
            cwd,
            cancel,
            deadline,
            printed: String::new(),
            calls: 0,
            call_log: Vec::new(),
        });
    });
    let finished = run_in_context(code);
    let bridge = BRIDGE
        .with(|b| b.borrow_mut().take())
        .unwrap_or_else(|| Bridge {
            cwd: PathBuf::new(),
            cancel: tokio_util::sync::CancellationToken::new(),
            deadline,
            printed: String::new(),
            calls: 0,
            call_log: Vec::new(),
        });
    Ok(render(finished, &bridge))
}

fn run_in_context(code: &str) -> std::result::Result<Option<String>, String> {
    let mut context = Context::default();
    let limits = context.runtime_limits_mut();
    limits.set_loop_iteration_limit(MAX_LOOP_ITERATIONS);
    limits.set_recursion_limit(400);

    install_bridge(&mut context)?;
    context
        .eval(Source::from_bytes(PRELUDE))
        .map_err(|e| format!("codemode prelude failed: {e}"))?;

    // The user's body becomes an async function so top-level `await` and
    // `return` behave the way the schema promises.
    let program = format!("__nur_run(async () => {{\n{code}\n}});");
    let promise = context
        .eval(Source::from_bytes(program.as_bytes()))
        .map_err(|e| js_error(&e.to_string()))?;
    context
        .run_jobs()
        .map_err(|e| js_error(&e.to_string()))
        .and(Ok(()))?;
    settled(&promise, &mut context)
}

/// Read the async function's promise once the job queue has drained. A promise
/// still pending here means the script awaited something nothing will resolve.
fn settled(
    promise: &JsValue,
    context: &mut Context,
) -> std::result::Result<Option<String>, String> {
    use boa_engine::builtins::promise::PromiseState;
    use boa_engine::object::builtins::JsPromise;

    let object = promise
        .as_object()
        .ok_or_else(|| "codemode: script did not return a promise".to_string())?;
    let promise = JsPromise::from_object(object.clone()).map_err(|e| format!("codemode: {e}"))?;
    match promise.state() {
        PromiseState::Fulfilled(value) => {
            if value.is_undefined() || value.is_null() {
                return Ok(None);
            }
            Ok(Some(stringify(&value, context)))
        }
        PromiseState::Rejected(err) => Err(js_error(&stringify(&err, context))),
        PromiseState::Pending => Err(
            "codemode: the script is still awaiting something that never resolves \
             (the sandbox has no timers or network of its own)"
                .into(),
        ),
    }
}

/// JSON where the value has a JSON form, the engine's own string otherwise -
/// a returned `Error` or class instance should still say something useful.
fn stringify(value: &JsValue, context: &mut Context) -> String {
    if let Ok(Some(json)) = value.to_json(context) {
        return match json {
            Value::String(s) => s,
            other => serde_json::to_string_pretty(&other).unwrap_or_else(|_| other.to_string()),
        };
    }
    value
        .to_string(context)
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_else(|_| "[unprintable value]".into())
}

fn js_error(message: &str) -> String {
    format!("codemode: script failed: {message}")
}

/// What the model gets back: what the script printed, what it returned, and a
/// one-line account of the calls it made, so a wrong answer is debuggable
/// without re-running it.
fn render(finished: std::result::Result<Option<String>, String>, bridge: &Bridge) -> String {
    let mut out = String::new();
    if !bridge.printed.is_empty() {
        out.push_str(bridge.printed.trim_end_matches('\n'));
        out.push('\n');
    }
    match finished {
        Ok(Some(value)) => {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str("[returned]\n");
            out.push_str(&value);
            out.push('\n');
        }
        Ok(None) => {
            if out.is_empty() {
                out.push_str("(script produced no output; print with text(…) or return a value)\n");
            }
        }
        Err(message) => {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&message);
            out.push('\n');
        }
    }
    if !bridge.call_log.is_empty() {
        out.push_str(&format!(
            "\n[{} tool call(s): {}]\n",
            bridge.calls,
            bridge.call_log.join(", ")
        ));
    }
    out
}

/// Native functions the prelude wraps. Everything crosses as a string: tool
/// results are text, so a JSON bridge would only add a failure mode.
fn install_bridge(context: &mut Context) -> std::result::Result<(), String> {
    let mut register =
        |name: &str, arity: usize, f: NativeFunction| -> std::result::Result<(), String> {
            context
                .register_global_callable(js_string!(name), arity, f)
                .map_err(|e| format!("codemode: could not install {name}: {e}"))
        };

    register(
        "__nur_call",
        2,
        NativeFunction::from_copy_closure(|_this, args, context| {
            let name = string_arg(args.first(), context);
            let payload = string_arg(args.get(1), context);
            match call_tool(&name, &payload) {
                Ok(result) => Ok(JsValue::from(js_string!(result.as_str()))),
                Err(message) => Err(boa_engine::JsNativeError::typ()
                    .with_message(message)
                    .into()),
            }
        }),
    )?;

    register(
        "__nur_tools",
        0,
        NativeFunction::from_copy_closure(|_this, _args, _context| {
            Ok(JsValue::from(js_string!(callable_tools_json().as_str())))
        }),
    )?;

    register(
        "__nur_describe",
        1,
        NativeFunction::from_copy_closure(|_this, args, context| {
            let name = string_arg(args.first(), context);
            Ok(JsValue::from(js_string!(describe_tool(&name).as_str())))
        }),
    )?;

    register(
        "__nur_print",
        1,
        NativeFunction::from_copy_closure(|_this, args, context| {
            let line = string_arg(args.first(), context);
            BRIDGE.with(|b| {
                if let Some(bridge) = b.borrow_mut().as_mut() {
                    if bridge.printed.len() + line.len() <= MAX_PRINTED_CHARS {
                        bridge.printed.push_str(&line);
                        bridge.printed.push('\n');
                    }
                }
            });
            Ok(JsValue::undefined())
        }),
    )?;

    register(
        "__nur_store",
        2,
        NativeFunction::from_copy_closure(|_this, args, context| {
            let key = string_arg(args.first(), context);
            let value = string_arg(args.get(1), context);
            match store_put(&key, value) {
                Ok(()) => Ok(JsValue::undefined()),
                Err(message) => Err(boa_engine::JsNativeError::typ()
                    .with_message(message)
                    .into()),
            }
        }),
    )?;

    register(
        "__nur_load",
        1,
        NativeFunction::from_copy_closure(|_this, args, context| {
            let key = string_arg(args.first(), context);
            Ok(match store_get(&key) {
                Some(value) => JsValue::from(js_string!(value.as_str())),
                None => JsValue::undefined(),
            })
        }),
    )?;

    let cwd = BRIDGE.with(|b| {
        b.borrow()
            .as_ref()
            .map(|bridge| bridge.cwd.display().to_string())
            .unwrap_or_default()
    });
    context
        .register_global_property(
            js_string!("CWD"),
            js_string!(cwd.as_str()),
            Attribute::all(),
        )
        .map_err(|e| format!("codemode: could not install CWD: {e}"))?;
    Ok(())
}

fn string_arg(value: Option<&JsValue>, context: &mut Context) -> String {
    value
        .and_then(|v| v.to_string(context).ok())
        .map(|s| s.to_std_string_escaped())
        .unwrap_or_default()
}

/// One nested call: same classification, same workspace, same cancellation as
/// a call the model makes directly.
fn call_tool(name: &str, args_json: &str) -> std::result::Result<String, String> {
    let (cwd, cancel) = BRIDGE.with(|b| {
        let borrowed = b.borrow();
        let bridge = borrowed.as_ref().ok_or("codemode: no sandbox")?;
        bridge.check_budget()?;
        Ok::<_, String>((bridge.cwd.clone(), bridge.cancel.clone()))
    })?;

    let args: Value = serde_json::from_str(args_json).unwrap_or_else(|_| serde_json::json!({}));
    script_may_call(name, &args)?;

    BRIDGE.with(|b| {
        if let Some(bridge) = b.borrow_mut().as_mut() {
            bridge.calls += 1;
            bridge.call_log.push(name.to_string());
        }
    });

    let host = ToolHost::default();
    let ctx = ToolContext { cwd, cancel };
    host.dispatch(name, args_json, &ctx)
        .map_err(|e| format!("{name}: {e}"))
}

/// The sandbox's whole policy, in one place.
fn script_may_call(name: &str, args: &Value) -> std::result::Result<(), String> {
    if DENIED.contains(&name) {
        return Err(format!(
            "{name} cannot be called from a script: it starts new work, speaks to the user or \
             ends the turn. Call it directly instead."
        ));
    }
    if !capabilities::is_read_only(name, args) {
        return Err(format!(
            "{name} with these arguments is not a read-only call, and a script may only read. \
             Call it directly so it goes through approval."
        ));
    }
    Ok(())
}

/// The roster a script sees: every tool with at least one read-only call.
fn callable_tools_json() -> String {
    let defs = ToolHost::default().tool_defs();
    let listed: Vec<Value> = defs
        .into_iter()
        .filter(|def| !DENIED.contains(&def.name.as_str()))
        .filter(|def| probe_read_only(&def.name))
        .map(|def| {
            let summary = def
                .description
                .unwrap_or_default()
                .split(". ")
                .next()
                .unwrap_or_default()
                .to_string();
            serde_json::json!({ "name": def.name, "description": summary })
        })
        .collect();
    serde_json::to_string(&listed).unwrap_or_else(|_| "[]".into())
}

/// Does this tool have *any* read-only call? Action-shaped tools reject an
/// empty argument object (fail-closed), so they are probed with the action
/// names their read-only arms actually use.
fn probe_read_only(name: &str) -> bool {
    const PROBES: &[&str] = &[
        "list",
        "status",
        "read",
        "search",
        "query",
        "get",
        "peek",
        "slice",
        "inventory",
        "tools",
        "recall",
        "explain",
        "report",
    ];
    if capabilities::is_read_only(name, &serde_json::json!({})) {
        return true;
    }
    PROBES
        .iter()
        .any(|action| capabilities::is_read_only(name, &serde_json::json!({ "action": action })))
}

fn describe_tool(name: &str) -> String {
    let found = ToolHost::default()
        .tool_defs()
        .into_iter()
        .find(|def| def.name == name);
    match found {
        Some(def) if !DENIED.contains(&name) && probe_read_only(name) => {
            serde_json::to_string_pretty(&serde_json::json!({
                "name": def.name,
                "description": def.description,
                "parameters": def.parameters,
            }))
            .unwrap_or_else(|_| "{}".into())
        }
        Some(_) => serde_json::json!({
            "error": format!("{name} is not callable from a script")
        })
        .to_string(),
        None => serde_json::json!({ "error": format!("no tool named {name}") }).to_string(),
    }
}

fn store_key(key: &str) -> String {
    let session = std::env::var("NUR_SESSION_ID").unwrap_or_else(|_| "default".into());
    format!("{session}\u{0}{key}")
}

fn store_put(key: &str, value: String) -> std::result::Result<(), String> {
    if value.len() > MAX_STORE_VALUE_CHARS {
        return Err(format!(
            "store value is {} chars, over the {MAX_STORE_VALUE_CHARS} limit",
            value.len()
        ));
    }
    let namespaced = store_key(key);
    let mut guard = STORE.lock().map_err(|_| "store is poisoned".to_string())?;
    let map = guard.get_or_insert_with(HashMap::new);
    let replacing = map.get(&namespaced).map(String::len).unwrap_or(0);
    let total: usize = map.values().map(String::len).sum();
    if total - replacing + value.len() > MAX_STORE_TOTAL_CHARS {
        return Err(format!(
            "the session store is full ({MAX_STORE_TOTAL_CHARS} chars); drop a key first"
        ));
    }
    map.insert(namespaced, value);
    Ok(())
}

fn store_get(key: &str) -> Option<String> {
    STORE.lock().ok()?.as_ref()?.get(&store_key(key)).cloned()
}

/// The JS half of the bridge. Everything the schema advertises is defined
/// here in terms of the `__nur_*` natives, so the Rust side only ever moves
/// strings.
const PRELUDE: &[u8] = br#"
"use strict";
globalThis.ALL_TOOLS = JSON.parse(__nur_tools());
globalThis.tools = {};
for (const entry of ALL_TOOLS) {
  globalThis.tools[entry.name] = (args) => __nur_call(entry.name, JSON.stringify(args ?? {}));
}
globalThis.searchTools = (query) => {
  const q = String(query ?? "").toLowerCase();
  return ALL_TOOLS.filter((t) =>
    t.name.toLowerCase().includes(q) || t.description.toLowerCase().includes(q));
};
globalThis.describeTool = (name) => JSON.parse(__nur_describe(String(name)));
globalThis.text = (...parts) => __nur_print(parts.map((p) =>
  typeof p === "string" ? p : JSON.stringify(p)).join(" "));
globalThis.console = { log: globalThis.text, error: globalThis.text, warn: globalThis.text };
globalThis.store = (key, value) => __nur_store(String(key),
  typeof value === "string" ? value : JSON.stringify(value));
globalThis.load = (key) => {
  const raw = __nur_load(String(key));
  if (raw === undefined) return undefined;
  try { return JSON.parse(raw); } catch (e) { return raw; }
};
globalThis.exit = (value) => {
  const stop = new Error("exit");
  stop.__nurExit = true;
  stop.value = value;
  throw stop;
};
globalThis.__nur_run = async (body) => {
  try {
    return await body();
  } catch (e) {
    if (e && e.__nurExit) return e.value;
    throw e;
  }
};
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch workspace with one file to read, so a script has something
    /// real to do and a real place to try to write.
    fn workspace(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nur-codemode-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("workspace");
        std::fs::write(dir.join("note.txt"), "alpha\nbeta\n").expect("seed file");
        dir
    }

    fn run(cwd: &std::path::Path, code: &str) -> String {
        let ctx = ToolContext::default_for_test(cwd);
        CodeMode
            .execute(&serde_json::json!({ "code": code }), &ctx)
            .unwrap_or_else(|e| format!("TOOL ERROR: {e}"))
    }

    /// The headline case: several reads and a filter, one model round.
    #[test]
    fn a_script_reads_through_the_bridge_and_returns_its_own_answer() {
        let dir = workspace("read");
        let out = run(
            &dir,
            r#"
            const body = tools.read_file({path: "note.txt"});
            const lines = body.split("\n").filter(l => l.includes("beta"));
            text("matched " + lines.length);
            return lines.length;
            "#,
        );
        assert!(out.contains("matched 1"), "{out}");
        assert!(out.contains("[returned]"), "{out}");
        assert!(
            out.contains("read_file"),
            "the call log names what ran: {out}"
        );
    }

    /// The security boundary. A script that asks to write must be refused, and
    /// nothing may appear on disk - a refusal that still wrote would be worse
    /// than no check at all.
    #[test]
    fn a_script_cannot_write_edit_or_run_commands() {
        let dir = workspace("write");
        for code in [
            r#"return tools.write_file({path: "pwned.txt", contents: "x"});"#,
            r#"return tools.edit_file({path: "note.txt", old_string: "alpha", new_string: "x"});"#,
            r#"return tools.bash({command: "echo pwned > pwned.txt"});"#,
        ] {
            let out = run(&dir, code);
            assert!(
                out.contains("script failed") || out.contains("not a function"),
                "a write-shaped call must fail: {out}"
            );
            assert!(!out.contains("[returned]"), "it must not succeed: {out}");
        }
        assert!(
            !dir.join("pwned.txt").exists(),
            "the sandbox leaked a write"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("note.txt")).unwrap(),
            "alpha\nbeta\n",
            "the sandbox leaked an edit"
        );
    }

    /// Spawn-shaped and turn-ending tools are denied by name even though some
    /// of them classify as read-only.
    #[test]
    fn denied_tools_are_refused_by_name() {
        for name in DENIED {
            assert!(
                script_may_call(name, &serde_json::json!({})).is_err(),
                "{name} must be unreachable from a script"
            );
        }
        // Being denied is not the same as being unknown: `submit_plan` is
        // read-only, and the deny list is what stops it.
        assert!(capabilities::is_read_only(
            "submit_plan",
            &serde_json::json!({})
        ));
    }

    /// Action-shaped tools must be judged per call, not per name: `context`
    /// reads are fine, `context register` writes the store.
    #[test]
    fn a_mutating_action_is_refused_on_an_otherwise_readable_tool() {
        assert!(script_may_call("context", &serde_json::json!({"action": "list"})).is_ok());
        for action in ["register", "delete"] {
            assert!(
                script_may_call("context", &serde_json::json!({ "action": action })).is_err(),
                "context {action} writes and must be refused"
            );
        }
    }

    /// The roster the script sees must match what it may actually call, or the
    /// model writes code against tools that always throw.
    #[test]
    fn the_advertised_roster_is_exactly_what_a_script_may_call() {
        let listed: Vec<String> = serde_json::from_str::<Value>(&callable_tools_json())
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        assert!(listed.iter().any(|n| n == "read_file"));
        assert!(listed.iter().any(|n| n == "grep"));
        for denied in DENIED {
            assert!(
                !listed.iter().any(|n| n == denied),
                "{denied} is advertised"
            );
        }
        for writer in [
            "write_file",
            "edit_file",
            "multi_edit",
            "apply_patch",
            "bash",
        ] {
            assert!(
                !listed.iter().any(|n| n == writer),
                "{writer} is advertised to scripts"
            );
        }
    }

    /// The sandbox has no host bindings of its own: everything a normal JS
    /// runtime would use to reach the machine must simply not exist.
    #[test]
    fn the_sandbox_has_no_filesystem_network_or_process() {
        let dir = workspace("bare");
        let out = run(
            &dir,
            r#"return ["require","process","fetch","XMLHttpRequest","setTimeout","Deno","import"]
                 .filter(n => typeof globalThis[n] !== "undefined");"#,
        );
        assert!(out.contains("[returned]"), "{out}");
        assert!(
            out.contains("[]"),
            "a host binding leaked into the sandbox: {out}"
        );
    }

    #[test]
    fn exit_returns_early_and_a_throw_is_reported() {
        let dir = workspace("control");
        let out = run(&dir, r#"exit("stopped"); text("unreachable");"#);
        assert!(out.contains("stopped"), "{out}");
        assert!(!out.contains("unreachable"), "{out}");

        let out = run(&dir, r#"throw new Error("boom");"#);
        assert!(out.contains("script failed"), "{out}");
        assert!(out.contains("boom"), "{out}");
    }

    /// A script that never ends must not hang the turn. The loop-iteration
    /// limit is what makes that true without a wall-clock interrupt.
    #[test]
    fn an_endless_loop_is_stopped_rather_than_hanging() {
        let dir = workspace("spin");
        let started = Instant::now();
        let out = run(&dir, "while (true) {}");
        assert!(out.contains("script failed"), "{out}");
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "the iteration limit did not bite"
        );
    }

    #[test]
    fn stored_values_survive_into_the_next_script() {
        let dir = workspace("store");
        let out = run(&dir, r#"store("found", {count: 3}); return "saved";"#);
        assert!(out.contains("saved"), "{out}");
        let out = run(&dir, r#"return load("found").count;"#);
        assert!(out.contains('3'), "{out}");
    }
}
