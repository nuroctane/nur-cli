//! Real-Chrome perception & control — agent-browser-cli
//! <https://github.com/sleepinginsummer/agent-browser-cli>
//!
//! Unlike `web_fetch` (text-only, no cookies), this drives the user's *actual*
//! Chrome session through a MV3 extension bridge, so login state is preserved:
//! scan tabs, snapshot pages into `@e` element refs, click/fill/type, run JS,
//! capture screenshots (feed them to `look` for vision), and record
//! network/console activity. Requires the CLI (auto-provisioned via npm) plus
//! the `tmwd_cdp_bridge` Chrome extension loaded once by the user.
//!
//! Cookie reading is deliberately NOT exposed — session secrets stay out of
//! the model's context.

use super::{arg_str, Tool, ToolContext};
use crate::ecosystem;
use crate::error::{NurError, Result};
use serde_json::Value;

/// One element from a page snapshot: its `@e` ref and how the page describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotElement {
    /// The ref the CLI accepts (`@e7`).
    pub target: String,
    /// Role + name + state, as the snapshot reported them.
    pub description: String,
}

/// Operations the picker may choose. Mirrors the dynamic action space jev-ultrafast
/// uses: the model picks the operation, and only `type_text` requires the session
/// model to generate anything.
pub const PICK_OPERATIONS: &[&str] = &[
    "click",
    "type_text",
    "select",
    "scroll_up",
    "scroll_down",
    "wait",
    "done",
    "blocked",
];

/// Extract the indexed element table from a snapshot.
///
/// The format is the CLI's, not ours, and it has changed before, so this reads
/// both a JSON envelope (walking for objects/strings that mention an `@eN` ref)
/// and plain text lines. Anything it cannot recognise is skipped rather than
/// guessed at - the caller refuses to pick when nothing was parsed.
pub fn parse_snapshot_elements(snapshot: &str) -> Vec<SnapshotElement> {
    let mut out: Vec<SnapshotElement> = Vec::new();
    // A free function, not a closure: the JSON walker and the text pass both need
    // to append to `out`, and a closure holding `&mut out` blocks reading it.
    fn push(out: &mut Vec<SnapshotElement>, target: String, description: String) {
        if out.iter().any(|e| e.target == target) {
            return;
        }
        let description = description.trim().to_string();
        out.push(SnapshotElement {
            target,
            description: if description.is_empty() {
                "(no description)".to_string()
            } else {
                description
            },
        });
    }

    // 1. JSON: any object that carries an @e ref plus a label-ish field.
    if let Ok(value) = serde_json::from_str::<Value>(snapshot) {
        fn walk(v: &Value, push: &mut impl FnMut(String, String)) {
            match v {
                Value::Object(map) => {
                    let ref_key = ["target", "ref", "element", "id", "selector"]
                        .iter()
                        .find_map(|k| map.get(*k).and_then(Value::as_str))
                        .filter(|s| looks_like_ref(s));
                    if let Some(target) = ref_key {
                        let mut parts: Vec<String> = Vec::new();
                        for key in [
                            "role",
                            "tag",
                            "name",
                            "label",
                            "text",
                            "value",
                            "placeholder",
                        ] {
                            if let Some(text) = map.get(key).and_then(Value::as_str) {
                                if !text.trim().is_empty() {
                                    parts.push(format!("{key}={text}"));
                                }
                            }
                        }
                        push(target.to_string(), parts.join(" "));
                    }
                    for child in map.values() {
                        walk(child, push);
                    }
                }
                Value::Array(items) => {
                    for item in items {
                        walk(item, push);
                    }
                }
                Value::String(text) => {
                    for element in parse_snapshot_lines(text) {
                        push(element.target, element.description);
                    }
                }
                _ => {}
            }
        }
        walk(&value, &mut |target, description| {
            push(&mut out, target, description)
        });
        if !out.is_empty() {
            return out;
        }
    }

    // 2. Plain text.
    for element in parse_snapshot_lines(snapshot) {
        push(&mut out, element.target, element.description);
    }
    out
}

fn looks_like_ref(text: &str) -> bool {
    let t = text.trim();
    let rest = match t.strip_prefix('@') {
        Some(r) => r,
        // Without the `@`, only a plain element ref (`e7`) is unambiguous. A
        // generic key such as `id`/`selector` holding a word ("submit",
        // "header") is a selector or a name, not a ref, and offering it produced
        // `action=click target=submit`, which the CLI rejects.
        None => return is_plain_element_ref(t),
    };
    !rest.is_empty()
        && rest
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// `e7` / `E12`: the element ref without its `@` prefix.
fn is_plain_element_ref(t: &str) -> bool {
    let mut chars = t.chars();
    matches!(chars.next(), Some('e') | Some('E')) && {
        let digits: String = chars.collect();
        !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
    }
}

/// Text lines of the form `@e7 role name=... ` (or `- @e7 button "Sign in"`).
fn parse_snapshot_lines(text: &str) -> Vec<SnapshotElement> {
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Some(at) = trimmed.find('@') else {
            continue;
        };
        let after = &trimmed[at + 1..];
        let ref_len = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .count();
        if ref_len == 0 {
            continue;
        }
        let target = format!("@{}", &after[..ref_len]);
        // Skip prose that merely mentions a ref mid-sentence.
        let prefix = trimmed[..at].trim();
        if !prefix.is_empty() && !prefix.chars().all(|c| "-*•>| ".contains(c)) {
            continue;
        }
        let description = trimmed[at + 1 + ref_len..]
            .trim_start_matches(|c: char| c == ':' || c == '-' || c == ' ')
            .trim()
            .to_string();
        out.push(SnapshotElement {
            target,
            description,
        });
    }
    out
}

/// Ask Jev which operation to perform and on which element.
///
/// The shape is jev-ultrafast's: read the page into an *indexed* element table,
/// then make one request that picks the operation and the target together -
/// speculative fan-out, because only one of the two target questions can be used
/// once the operation is known. Nothing here invents a ref: the answer is mapped
/// back onto the parsed table, and a table with fewer than two elements is
/// reported instead of judged.
fn pick_action(bin: &str, args: &Value, ctx: &crate::tools::ToolContext) -> Result<String> {
    use crate::typesafe::{harness, questions};

    let goal = arg_str(args, "goal")
        .map_err(|_| NurError::Tool("pick requires goal=\"what you are trying to do\"".into()))?;
    let mut argv: Vec<String> = vec!["snapshot".into()];
    if let Ok(tab) = arg_str(args, "tab") {
        if !tab.trim().is_empty() {
            argv.push("--tab".into());
            argv.push(tab);
        }
    }
    let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
    let snapshot = ecosystem::run_capture(bin, &refs, Some(&ctx.cwd), 60_000)
        .map_err(|e| NurError::Tool(format!("snapshot failed: {e}")))?;

    let limit = args
        .get("max_candidates")
        .and_then(Value::as_u64)
        .unwrap_or(40)
        .clamp(2, 200) as usize;
    let mut elements = parse_snapshot_elements(&snapshot);
    let total = elements.len();
    elements.truncate(limit);
    if elements.len() < 2 {
        return Ok(format!(
            "browser · pick: the snapshot yielded {} indexed element(s), so there is nothing to \
             choose between. A pick from fewer than two candidates would be a guess.\n\
             Run `snapshot` yourself to see what the page reported, then use click/fill with an \
             explicit ref.\n\n{}",
            total,
            crate::typesafe::harness::preview(&snapshot, 1_500)
        ));
    }

    let cfg = crate::config::load_config()
        .map(|c| c.typesafe)
        .unwrap_or_default();
    let table = elements
        .iter()
        .map(|e| format!("{} {}", e.target, e.description))
        .collect::<Vec<_>>()
        .join(
            "
",
        );
    let state = serde_json::json!({
        "goal": goal,
        "page_elements": table,
        "note": "each line is '<ref> <description>'; the ref is what the harness can act on",
    });

    // Both questions ride one request; only the target head matching the chosen
    // operation can execute.
    let ops: Vec<String> = PICK_OPERATIONS.iter().map(|s| s.to_string()).collect();
    let targets: Vec<String> = elements.iter().map(|e| e.target.clone()).collect();
    let op_labels: Vec<questions::ChoiceOption> = PICK_OPERATIONS
        .iter()
        .map(|op| {
            questions::ChoiceOption::described(
                *op,
                match *op {
                    "click" => "press a button, link, or control",
                    "type_text" => "type into a text field (the session model writes the text)",
                    "select" => "choose an option in a dropdown",
                    "scroll_up" | "scroll_down" => "reveal more of the page",
                    "wait" => "the page is loading; wait before acting",
                    "done" => "the goal is already satisfied on this page",
                    _ => "blocked: the page offers no way to proceed",
                },
            )
        })
        .collect();
    let target_options: Vec<questions::ChoiceOption> = elements
        .iter()
        .map(|e| questions::ChoiceOption::new(e.target.clone()))
        .collect();

    let Some(client) = harness::ready(&cfg) else {
        return Ok(format!(
            "browser · pick: no TypeSafe key or local engine, so no judgment is available. The \
             indexed table is below - choose yourself, then click/fill with the ref.\n\n{table}"
        ));
    };
    let batch = harness::ask_batched(
        &client,
        &state,
        vec![
            (
                "operation".to_string(),
                questions::Question::choice(
                    "Which operation should be performed next to make progress on the goal?",
                    op_labels,
                ),
            ),
            (
                "target".to_string(),
                questions::Question::choice(
                    "If a click, type_text or select is the right operation, which element should \
                     it apply to? Prefer the element that most directly advances the goal.",
                    target_options,
                ),
            ),
        ],
    );
    let Some(batch) = batch else {
        return Ok(format!(
            "browser · pick: the judgment request failed; the indexed table is below.\n\n{table}"
        ));
    };

    let t = crate::typesafe::policy::Thresholds::from_config(&cfg);
    let op_answer = batch.get("operation");
    let target_answer = batch.get("target");
    let operation = op_answer
        .and_then(|a| a.resolve_verbatim(&ops))
        .cloned()
        .unwrap_or_default();
    let target = target_answer
        .and_then(|a| a.resolve_verbatim(&targets))
        .cloned()
        .unwrap_or_default();
    let op_band = op_answer
        .and_then(|a| a.confidence())
        .map(|c| t.band(Some(c)));
    let target_band = target_answer
        .and_then(|a| a.confidence())
        .map(|c| t.band(Some(c)));

    let mut out = vec![format!(
        "browser · pick ({} candidate(s) offered, {} judged in one request)",
        elements.len(),
        batch.answers.len()
    )];
    out.push(format!(
        "operation: {} ({})",
        if operation.is_empty() {
            "?"
        } else {
            &operation
        },
        op_band.map(|b| b.as_str()).unwrap_or("no answer")
    ));
    if matches!(operation.as_str(), "click" | "type_text" | "select") {
        out.push(format!(
            "target: {} ({}){}",
            if target.is_empty() { "?" } else { &target },
            target_band.map(|b| b.as_str()).unwrap_or("no answer"),
            if let Some(el) = elements.iter().find(|e| e.target == target) {
                format!(" · {}", el.description)
            } else {
                String::new()
            }
        ));
        if operation == "type_text" {
            out.push(
                "next: write the text yourself (the session model owns text generation), then \
                 call action=fill target=<ref> text=<your text>"
                    .into(),
            );
        } else if operation == "click" {
            out.push(format!("next: call action=click target={target}"));
        } else {
            out.push(format!(
                "next: call action=fill target={target} text=<the option to select>"
            ));
        }
    } else if operation == "done" {
        out.push("next: verify the outcome independently before reporting success".into());
    } else if operation == "wait" {
        out.push("next: take another snapshot after the page settles".into());
    } else if operation == "blocked" {
        out.push(
            "next: the page offers no path for this goal - say so rather than forcing an action"
                .into(),
        );
    } else if operation.starts_with("scroll") {
        out.push(format!(
            "next: call action=exec js=\"window.scrollBy(0, {})\"",
            if operation == "scroll_up" { -600 } else { 600 }
        ));
    }
    if op_band == Some(crate::typesafe::policy::GateAction::Escalate) {
        out.push(
            "note: the operation choice is in the escalate band - treat it as unknown and look \
             again rather than acting on it"
                .into(),
        );
    }
    out.push(String::new());
    out.push(table);
    Ok(out.join("\n"))
}

pub struct BrowserTool;

const BIN: &str = "agent-browser-cli";

/// Pure perception is free; anything that changes tabs/pages needs approval.
/// Screenshots write an image file (like `extract_frames`) so they are not
/// read-only, but plan mode allows them explicitly (see loop.rs).
pub fn is_read_only_action(args: &str) -> bool {
    let action = serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "tabs".into());
    matches!(
        action.as_str(),
        // `pick` reads the page and asks for a judgment; it performs nothing, so
        // it belongs with perception.
        "tabs"
            | "scan"
            | "snapshot"
            | "pick"
            | "tabtree"
            | "status"
            | "console"
            | "network"
            | "doctor"
    )
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    #[test]
    fn parses_the_documented_ref_lines() {
        let text = "@e1 button \"Sign in\"
@e2 textbox placeholder=Email
@e3 link \"Need help?\"
";
        let els = parse_snapshot_elements(text);
        assert_eq!(els.len(), 3);
        assert_eq!(els[0].target, "@e1");
        assert!(els[0].description.contains("Sign in"), "{:?}", els[0]);
        assert_eq!(els[1].target, "@e2");
    }

    #[test]
    fn parses_bulleted_and_indented_lines() {
        let text = "- @e7 button Continue
  * @e8 combobox Where to?
    > @e9 textbox
";
        let els = parse_snapshot_elements(text);
        assert_eq!(
            els.iter().map(|e| e.target.as_str()).collect::<Vec<_>>(),
            vec!["@e7", "@e8", "@e9"]
        );
    }

    #[test]
    fn parses_a_json_envelope_with_objects() {
        let json = r#"{"ok":true,"result":{"elements":[
            {"ref":"@e4","role":"button","name":"Continue"},
            {"ref":"@e5","tag":"input","placeholder":"Email","value":""}
        ]}}"#;
        let els = parse_snapshot_elements(&json);
        assert_eq!(els.len(), 2);
        assert!(els[0].description.contains("role=button"), "{:?}", els[0]);
        assert!(
            els[1].description.contains("placeholder=Email"),
            "{:?}",
            els[1]
        );
    }

    #[test]
    fn parses_a_json_envelope_wrapping_a_text_snapshot() {
        // A JSON string whose content is a real newline-separated snapshot (the
        // escapes here are JSON escapes, which is what a CLI envelope carries).
        // The JSON escape is assembled at runtime: source-level escaping fooled
        // this test twice, and what matters is the JSON the bridge actually sees.
        let nl_escape = "\\n";
        let json = format!("{{\"ok\":true,\"result\":\"@e1 button{nl_escape}@e2 textbox\"}}");
        let els = parse_snapshot_elements(&json);
        assert_eq!(
            els.iter().map(|e| e.target.as_str()).collect::<Vec<_>>(),
            vec!["@e1", "@e2"],
            "raw: {json} parsed: {els:?}"
        );
    }

    #[test]
    fn ignores_prose_and_missing_refs() {
        // A ref mentioned mid-sentence is not an element line.
        let text = "Click the @e3 button to continue.
no refs here
";
        assert!(
            parse_snapshot_elements(text).is_empty(),
            "{:?}",
            parse_snapshot_elements(text)
        );
        assert!(parse_snapshot_elements("").is_empty());
        assert!(parse_snapshot_elements("{\"ok\":true}").is_empty());
    }

    #[test]
    fn dedupes_and_caps_descriptions() {
        let text = "@e1 button
@e1 button again
@e1
";
        let els = parse_snapshot_elements(text);
        assert_eq!(els.len(), 1);
        // A ref with no description is still a usable candidate.
        assert_eq!(
            parse_snapshot_elements(
                "@e2
"
            )[0]
            .description,
            "(no description)"
        );
    }

    #[test]
    fn ref_shape_is_enforced_in_json() {
        // A "ref" that is a CSS selector is not an @e ref and must not be offered.
        let json = r##"{"elements":[{"ref":"#submit","role":"button"}]}"##;
        assert!(parse_snapshot_elements(json).is_empty());
    }

    #[test]
    fn a_plain_id_is_not_an_element_ref() {
        // Element objects also carry `id`/`selector` fields. Those hold names and
        // selectors, and offering one as a target produced a click the CLI
        // rejects (`target=submit`), so only `@eN` - or a bare `eN` - is a ref.
        for json in [
            r#"{"elements":[{"id":"submit","role":"button"}]}"#,
            r#"{"elements":[{"ref":"header","tag":"div"}]}"#,
            r#"{"elements":[{"selector":"login-form","role":"form"}]}"#,
        ] {
            assert!(
                parse_snapshot_elements(json).is_empty(),
                "not a ref: {json}"
            );
        }
        // The ref without its `@` prefix is still recognised, and an `@`-prefixed
        // ref keeps working whatever the field is called.
        let els = parse_snapshot_elements(r#"{"elements":[{"ref":"e7","role":"button"}]}"#);
        assert_eq!(els.len(), 1);
        assert_eq!(els[0].target, "e7");
        let els = parse_snapshot_elements(r#"{"elements":[{"id":"@e12","role":"link"}]}"#);
        assert_eq!(els.len(), 1);
        assert_eq!(els[0].target, "@e12");
        // ... but a bare word that merely starts with `e` is not one.
        assert!(parse_snapshot_elements(r#"{"elements":[{"id":"email","role":"textbox"}]}"#).is_empty());
    }

    #[test]
    fn every_pick_operation_has_a_description() {
        // The operation question offers these labels verbatim; an empty list or a
        // duplicate would make the answer unresolvable.
        assert!(PICK_OPERATIONS.len() >= 5);
        let mut sorted = PICK_OPERATIONS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), PICK_OPERATIONS.len());
        assert!(PICK_OPERATIONS.contains(&"click"));
        assert!(PICK_OPERATIONS.contains(&"done"));
    }

    #[test]
    fn pick_is_perception_not_control() {
        assert!(is_read_only_action(r#"{"action":"pick","goal":"sign in"}"#));
        assert!(!is_read_only_action(r#"{"action":"click","target":"@e1"}"#));
        assert!(!is_read_only_action(
            r#"{"action":"fill","target":"@e1","text":"x"}"#
        ));
    }
}

/// Plan mode additionally allows `screenshot` — pure perception that happens
/// to write an image file, exactly like `extract_frames`.
pub fn is_plan_safe_action(args: &str) -> bool {
    if is_read_only_action(args) {
        return true;
    }
    serde_json::from_str::<Value>(args)
        .ok()
        .and_then(|v| v.get("action")?.as_str().map(|s| s == "screenshot"))
        .unwrap_or(false)
}

impl Tool for BrowserTool {
    fn name(&self) -> &str {
        "browser"
    }

    fn description(&self) -> &str {
        "Perceive and control the user's real Chrome (login state preserved) via \
         agent-browser-cli. Perception: tabs | scan | snapshot (page → @e element \
         refs) | tabtree | console | network | status | doctor. Control: open url | \
         click @e | fill @e text | send_keys | exec js | screenshot (then `look` at \
         the saved image) | close. Prefer web_fetch for plain pages; use this when \
         the page needs a session, interaction, or visual confirmation. Needs the \
         Chrome extension loaded once (chrome://extensions → load unpacked)."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["tabs", "scan", "snapshot", "pick", "tabtree", "status", "doctor",
                             "console", "network",
                             "open", "close", "click", "fill", "send_keys", "exec",
                             "screenshot"],
                    "default": "tabs"
                },
                "url": {"type": "string", "description": "For open: target URL"},
                "target": {"type": "string", "description": "For click/fill/send_keys: @e element ref from snapshot"},
                "goal": {"type": "string", "description": "For pick: what you are trying to do on the page; Jev chooses the operation and the element ref for it"},
                "max_candidates": {"type": "integer", "description": "For pick: most elements to offer Jev (default 40)"},
                "text": {"type": "string", "description": "For fill: text to enter"},
                "keys": {"type": "string", "description": "For send_keys: key sequence, e.g. Enter"},
                "js": {"type": "string", "description": "For exec: JavaScript to run in the page"},
                "tab": {"type": "string", "description": "Optional tab id (from tabs/tabtree)"},
                "full_page": {"type": "boolean", "description": "For screenshot: capture the full page"}
            }
        })
    }

    fn execute(&self, args: &Value, ctx: &ToolContext) -> Result<String> {
        let bin = ecosystem::find_bin(BIN).ok_or_else(|| {
            NurError::Tool(
                "agent-browser-cli not found. Meta auto-installs it — or run \
                 `nur browser setup` to stage the extension and finish one-time \
                 setup for your default browser."
                    .into(),
            )
        })?;

        let action = arg_str(args, "action").unwrap_or_else(|_| "tabs".into());
        if action == "pick" {
            return pick_action(&bin, args, ctx);
        }
        let mut argv: Vec<String> = match action.as_str() {
            "tabs" => vec!["tabs".into()],
            "scan" => vec!["scan".into()],
            "snapshot" => vec!["snapshot".into()],
            "tabtree" => vec!["tabtree".into()],
            "status" => {
                // Fold in the local setup state (default browser + extension
                // staging) so the model can self-diagnose a disconnected bridge.
                let setup = crate::ecosystem::browser_setup::setup_summary();
                let live = ecosystem::run_capture(&bin, &["status"], Some(&ctx.cwd), 30_000)
                    .unwrap_or_else(|e| format!("(bridge status unavailable: {e})"));
                return Ok(format!("{setup}\n\nbridge:\n{live}"));
            }
            "doctor" => vec!["doctor".into()],
            "console" => vec!["console".into(), "list".into()],
            "network" => vec!["network".into(), "list".into()],
            "open" => vec!["open".into(), arg_str(args, "url")?],
            "close" => vec!["close".into()],
            "click" => vec!["click".into(), arg_str(args, "target")?],
            "fill" => vec![
                "fill".into(),
                arg_str(args, "target")?,
                arg_str(args, "text")?,
            ],
            "send_keys" => {
                let mut a = vec!["send-keys".into(), arg_str(args, "keys")?];
                if let Ok(t) = arg_str(args, "target") {
                    a.push("--target".into());
                    a.push(t);
                }
                a
            }
            "exec" => vec!["exec".into(), arg_str(args, "js")?],
            "screenshot" => {
                let mut a = vec!["screenshot".into()];
                if args
                    .get("full_page")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
                {
                    a.push("--full-page".into());
                }
                a
            }
            other => {
                return Err(NurError::Tool(format!("unknown browser action '{other}'")));
            }
        };
        if let Ok(tab) = arg_str(args, "tab") {
            if !tab.trim().is_empty() {
                argv.push("--tab".into());
                argv.push(tab);
            }
        }

        let refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        ecosystem::run_capture(&bin, &refs, Some(&ctx.cwd), 120_000).map_err(NurError::Tool)
    }
}
