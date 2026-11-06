//! Compaction and Jev context handling for the turn loop.
use super::*;
use crate::agent::{chronicle, context_store, native_memory};

/// Resolve OMP-style remote compact URL. Opt-in only:
/// - `compaction.remote_endpoint` in config.toml (implies want remote), or
/// - `compaction.remote_enabled = true` plus endpoint / `NUR_COMPACT_REMOTE_ENDPOINT`, or
/// - `NUR_COMPACT_REMOTE=1` plus `NUR_COMPACT_REMOTE_ENDPOINT`.
pub(super) fn remote_compact_endpoint(cfg: &Config) -> Option<String> {
    let cfg_ep = cfg.compaction.remote_endpoint.trim();
    let env_ep = std::env::var("NUR_COMPACT_REMOTE_ENDPOINT")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let env_force = std::env::var("NUR_COMPACT_REMOTE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if !cfg_ep.is_empty() {
        // OMP: setting remoteEndpoint is enough.
        return Some(cfg_ep.to_string());
    }
    if cfg.compaction.remote_enabled || env_force {
        return env_ep;
    }
    None
}

/// OMP `compaction.remoteEndpoint` protocol: POST `{systemPrompt,prompt}` → `{summary}`.
pub(super) struct RemoteCompactSummary {
    summary: String,
    endpoint_origin: String,
    input_tokens_estimate: u64,
    output_tokens: u64,
    usage_reported: bool,
}

pub(super) async fn try_remote_compact_summary(
    runner: &AgentRunner,
    system_prompt: &str,
    items: &[Value],
) -> Option<RemoteCompactSummary> {
    let endpoint = remote_compact_endpoint(&runner.config)?;

    // Preserve both the original goal and the newest in-flight work. The old
    // `take(80)` retained only the beginning of long sessions, and appended the
    // summary instruction a second time even though it is already in `items`.
    let serialized = bounded_remote_compact_transcript(items, 120_000);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .ok()?;
    let openai_compatible = endpoint
        .split('?')
        .next()
        .unwrap_or(&endpoint)
        .trim_end_matches('/')
        .ends_with("/chat/completions");
    let body = if openai_compatible {
        serde_json::json!({
            "model": runner.config.model,
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": serialized},
            ],
            "stream": false,
        })
    } else {
        serde_json::json!({
            "systemPrompt": system_prompt,
            "prompt": serialized,
        })
    };
    let resp = client.post(&endpoint).json(&body).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let summary = v
        .get("summary")
        .and_then(Value::as_str)
        .or_else(|| {
            v.pointer("/choices/0/message/content")
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let input_tokens_estimate = (serialized.chars().count() as u64).div_ceil(3);
    let reported_input = v
        .pointer("/usage/input_tokens")
        .and_then(Value::as_u64)
        .or_else(|| v.pointer("/usage/prompt_tokens").and_then(Value::as_u64));
    let reported_output = v
        .pointer("/usage/output_tokens")
        .and_then(Value::as_u64)
        .or_else(|| {
            v.pointer("/usage/completion_tokens")
                .and_then(Value::as_u64)
        });
    let usage_reported = reported_input.is_some() || reported_output.is_some();
    Some(RemoteCompactSummary {
        summary: summary.to_string(),
        endpoint_origin: compact_endpoint_origin(&endpoint),
        input_tokens_estimate: reported_input.unwrap_or(input_tokens_estimate),
        output_tokens: reported_output
            .unwrap_or_else(|| (summary.chars().count() as u64).div_ceil(3)),
        usage_reported,
    })
}

pub(super) fn compact_endpoint_origin(endpoint: &str) -> String {
    let without_scheme = endpoint
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(endpoint);
    without_scheme
        .split('/')
        .next()
        .unwrap_or("remote-endpoint")
        .split('?')
        .next()
        .unwrap_or("remote-endpoint")
        .to_string()
}

pub(super) fn bounded_remote_compact_transcript(items: &[Value], max_chars: usize) -> String {
    let full = items
        .iter()
        .filter_map(|item| serde_json::to_string(item).ok())
        .collect::<Vec<_>>()
        .join("\n");
    if full.chars().count() <= max_chars {
        return full;
    }

    const MARKER: &str = "\n...[middle omitted for bounded remote compaction]...\n";
    let content_budget = max_chars.saturating_sub(MARKER.chars().count());
    let head_budget = content_budget / 4;
    let tail_budget = content_budget.saturating_sub(head_budget);
    let head: String = full.chars().take(head_budget).collect();
    let tail: String = full
        .chars()
        .rev()
        .take(tail_budget)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head}{MARKER}{tail}")
}

pub async fn compact_session(
    runner: &AgentRunner,
    session: &mut Session,
    usage: &mut UsageTracker,
) -> Result<String> {
    compact_session_with_mode(runner, session, usage, CompactionMode::Manual).await
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CompactionMode {
    Manual,
    Pressure,
    Model,
}

impl CompactionMode {
    pub(super) fn jev_wins(self, replace_summary: bool, reduction: f64, minimum: f64) -> bool {
        replace_summary && (self == Self::Manual || (reduction > 0.0 && reduction >= minimum))
    }
}

pub(super) async fn compact_session_with_mode(
    runner: &AgentRunner,
    session: &mut Session,
    usage: &mut UsageTracker,
    mode: CompactionMode,
) -> Result<String> {
    let before = session.input_items.len();
    let result = compact_session_inner(runner, session, usage, mode).await;
    record_auxiliary_telemetry(&session.id);
    receipt::record(
        &session.id,
        receipt::Event::Compaction {
            trigger: match mode {
                CompactionMode::Manual => "manual",
                CompactionMode::Pressure => "context_pressure",
                CompactionMode::Model => "model",
            }
            .into(),
            strategy: match &result {
                Ok(s) if s.starts_with("[compact: jev unchanged") => "jev_unchanged",
                Ok(s) if s.starts_with("[compact: jev-scored") => "jev_pruned",
                Ok(_) => "summary",
                Err(_) => "failed",
            }
            .into(),
            detail: result
                .as_ref()
                .map(|s| compact_receipt_detail(s))
                .unwrap_or_else(|_| "compaction failed; transcript backup preserved".into()),
            items_before: before,
            items_after: session.input_items.len(),
        },
    );
    result
}

pub(super) fn compact_receipt_detail(report: &str) -> String {
    // The summary is user content. Only the final code-generated status belongs
    // in the receipt, even if the summary itself contains a compact marker.
    report
        .rfind("[compact:")
        .map(|i| report[i..].to_string())
        .unwrap_or_else(|| "compaction completed".into())
}

pub(super) async fn compact_session_inner(
    runner: &AgentRunner,
    session: &mut Session,
    usage: &mut UsageTracker,
    mode: CompactionMode,
) -> Result<String> {
    snapshot_before_compact(session);

    let early_mem_scope = {
        let proj = std::path::Path::new(&session.cwd)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace");
        format!("{proj}:{}", session.id)
    };

    // Native-memory upkeep runs on *every* compaction path, including the Jev fast
    // path below. It used to sit after that path's early return, so enabling Jev
    // quietly stopped the tier ladder from ageing (recent→l1→l2→l3) and stopped
    // consolidation for every successful fast-path compaction.
    if runner.config.native_memory {
        let _ = native_memory::consolidate_localized(&early_mem_scope, 32);
        // closes the tier-ladder gap: age-progress recent→l1 and l2→l3 so deep
        // tiers are produced automatically, not only by manual remember/consolidate.
        let _ = native_memory::promote_aged(&early_mem_scope);
        let _ = chronicle::append(
            &early_mem_scope,
            "compact",
            "compaction requested - native memory upkeep completed",
            None,
        );
    }

    // Jev-scored compaction first. This is the cheapest release valve in the
    // harness: Jev is shown the whole conversation with tool results replaced by
    // short notes, and asked two questions per tool call - does the call still
    // matter, does its result still need to be kept verbatim. Dead calls go,
    // survivors are untouched, and text is never rewritten. If it frees enough
    // context, the summarizing model call is skipped entirely: no frontier
    // tokens, no lossy summary, and the working transcript stays readable.
    let mut items = session.input_items.clone();
    let mut jev_note: Option<String> = None;
    let ts = runner.config.typesafe.clone();
    if ts.enabled && ts.compaction.enabled {
        let opts = crate::typesafe::compact::CompactOptions::from_config(&ts.compaction)
            .with_goal(typesafe_goal(session, ts.compaction.goal_prompts));
        let for_task = items.clone();
        let cfg = ts.clone();
        let outcome = crate::typesafe::context::blocking(move || {
            crate::typesafe::compact::try_compact_items(&cfg, &for_task, &opts)
        })
        .await
        .unwrap_or(Err(crate::typesafe::compact::CompactSkip::NoJudgments));
        match outcome {
            Ok(outcome) => {
                let pretty = outcome.stats.summary();
                if mode.jev_wins(
                    ts.compaction.replace_summary,
                    outcome.stats.reduction_ratio(),
                    ts.compaction.min_reduction,
                ) {
                    crate::typesafe::telemetry::record_prune(0, 0, 0, 1);
                    if outcome.items == items {
                        return Ok(format!("[compact: jev unchanged - judgments kept the transcript; no summary written · {pretty}]"));
                    }
                    // Keep reporting out of the provider transcript: a verbose
                    // synthetic banner can cost more tokens than a small prune.
                    let decisions = outcome.decision_counts();
                    let kept = outcome.items.len();
                    session.input_items = outcome.items;
                    runner.persist_session(session);
                    return Ok(format!(
                    "[compact: jev-scored, no summary · {pretty} · {decisions} · {kept} items retained · \
                     precompact bak written]"
                ));
                }
                items = outcome.items;
                jev_note = Some(format!(
                    "{pretty}; summary required by {}",
                    if ts.compaction.replace_summary {
                        "automatic context recovery threshold"
                    } else {
                        "replace_summary=false"
                    }
                ));
            }
            Err(reason) => {
                if mode == CompactionMode::Manual
                    && ts.compaction.replace_summary
                    && reason != crate::typesafe::compact::CompactSkip::Unavailable
                {
                    return Ok(format!("[compact: jev unchanged - {reason}; transcript preserved, no summary written]"));
                }
                jev_note = Some(format!("{reason}; summary fallback"));
            }
        }
    }

    // The summary is text: inline screenshots add nothing to it, cost a tile
    // budget each, and break the request on a text-only summarizer. `items` is
    // the summarizer's copy, so the live session keeps its images.
    for item in items.iter_mut() {
        strip_inline_images(item);
    }
    // Thin old tool bodies for the summarizer so we don't re-pay huge dumps.
    let thinned = thin_tool_bodies_for_compact(
        &mut items,
        runner.config.compact_tool_body_max_chars as usize,
        runner.config.compact_keep_user_turns as usize,
    );
    let user_prompt =
        "Summarize this conversation for a fresh context window. Capture: goals, decisions, \
         files touched, current state, pending next steps. Prefer decisions over raw tool dumps. \
         Dense bullets.";
    let system_prompt = "You compress agent conversations into handoff summaries. \
             Preserve goals, decisions, file paths, and next steps; drop redundant tool noise.";
    items.push(user_text_item(user_prompt));
    let compact_input_estimate = serde_json::to_string(&items)
        .map(|text| (text.chars().count() as u64).div_ceil(3))
        .unwrap_or(0)
        .saturating_add((system_prompt.chars().count() as u64).div_ceil(3));
    if let Some(block) = preflight_request_budget(
        &runner.config,
        usage,
        &runner.config.provider,
        &runner.config.model,
        compact_input_estimate,
        runner.config.request_output_reserve_tokens,
    ) {
        match block {
            // The summarizer request itself would not fit the model's
            // window. Shrink to a bounded recent working set locally and
            // summarize THAT, instead of giving up on model-assisted
            // compaction at exactly the volume where it is needed. A budget
            // cap stays a hard stop.
            PreflightBlock::ContextWindow(_reason) => {
                emergency_compact_session(runner, session);
                items = session.input_items.clone();
                thin_tool_bodies_for_compact(
                    &mut items,
                    runner.config.compact_tool_body_max_chars as usize,
                    runner.config.compact_keep_user_turns as usize,
                );
                items.push(user_text_item(user_prompt));
            }
            PreflightBlock::Budget(msg) => {
                return Err(NurError::Budget(format!("compaction {msg}")));
            }
        }
    }

    // OMP-compatible remote summarization (compaction.remoteEndpoint). Opt-in;
    // any failure falls through to the local model path below.
    let remote_summary = try_remote_compact_summary(runner, system_prompt, &items).await;
    let summary = if let Some(remote) = remote_summary {
        let remote_usage =
            TokenUsage::estimated(remote.input_tokens_estimate, remote.output_tokens);
        // The endpoint is an explicit remote boundary, not the active route.
        // Pricing is necessarily an estimate unless the endpoint reports a
        // native cost, but tokens still count toward session safeguards.
        usage.record_request_as(
            "remote-compaction",
            &remote.endpoint_origin,
            remote_usage,
            None,
            crate::usage::PURPOSE_COMPACTION_REMOTE,
        );
        session.usage.add(usage.last_usage());
        receipt::record(
            &session.id,
            receipt::Event::RemoteCompaction {
                endpoint_origin: remote.endpoint_origin.clone(),
                input_tokens_estimate: remote.input_tokens_estimate,
                output_tokens: remote.output_tokens,
                usage_reported: remote.usage_reported,
            },
        );
        remote.summary
    } else {
        let req = ResponseRequest {
            model: runner.config.model.clone(),
            input: Value::Array(items),
            instructions: Some(system_prompt.into()),
            tools: None,
            tool_choice: None,
            store: Some(false),
            include: Some(vec!["reasoning.encrypted_content".into()]),
            reasoning: Some(ReasoningConfig {
                effort: Some("low".into()),
                summary: None,
            }),
            stream: Some(false),
            parallel_tool_calls: None,
            prompt_cache_key: Some(format!("compact:{}", session.id)),
            max_output_tokens: (runner.config.request_output_reserve_tokens > 0)
                .then_some(runner.config.request_output_reserve_tokens),
        };
        let resp = runner.client.create_response(&req).await?;
        let raw = resp.accounting_usage();
        usage.record_side_request(raw, resp.id.clone(), crate::usage::PURPOSE_COMPACTION);
        session.usage.add(usage.last_usage());
        let summary = resp.output_text();
        if summary.is_empty() {
            return Err(NurError::Other("compaction produced no summary".into()));
        }
        summary
    };

    // New context: summary + last N user/assistant display messages + the tail of
    // the live working items.
    //
    // That last part is not optional. `session.messages` only gains an assistant
    // entry when the turn *returns*, so mid-run it holds the user prompt and
    // nothing else. Rebuilding from it alone handed the model a summary and the
    // original request with no trace of the work in flight — so it answered as
    // if starting fresh, produced no tool calls, and the loop treated that as a
    // completed turn. A silent stop, right at the token volume where compaction
    // first fires.
    let keep_n = runner.config.compact_keep_user_turns.max(1) as usize;
    // RLM invariant (Prime): compaction summarizes chat but context_store vars remain.
    // Connectome: live edge is compacted; deep memory is maintained *localized* (paper M4).
    let mem_scope = {
        let proj = std::path::Path::new(&session.cwd)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("workspace");
        format!("{proj}:{}", session.id)
    };
    let store_inv = context_store::prompt_inventory(&session.id);
    let mem_inv = if runner.config.native_memory {
        native_memory::prompt_block(&mem_scope, "", 1_200)
    } else {
        String::new()
    };
    let store_note = {
        let mut parts = Vec::new();
        if !store_inv.is_empty() {
            parts.push(format!(
                "[RLM context store preserved - tool `context`]\n{store_inv}"
            ));
        }
        if !mem_inv.is_empty() {
            parts.push(mem_inv);
        }
        // Running tools are preserved across the summary: the jobs keep
        // running while it is written, and their results land afterwards.
        if !runner.is_subagent {
            if let Some(running) = crate::bg_jobs::running_note(&session.id) {
                parts.push(running);
            }
            if let Some(running) = crate::monitors::running_note(&session.id) {
                parts.push(running);
            }
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", parts.join("\n"))
        }
    };
    let mut new_items = vec![user_text_item(&format!(
        "[Context compacted. Summary of the conversation so far:]\n\n{summary}{store_note}"
    ))];
    let recent = recent_dialogue_items(&session.messages, keep_n);
    let kept = recent.len();
    let tail = safe_tail_after_compact(&session.input_items, COMPACT_KEEP_WORKING_ITEMS);
    let (tail_kept, strategy) = if runner.config.kv_stable_compact {
        // KV-stable: carry the recent edge verbatim and preserve its ordering,
        // maximizing a reusable prefix at providers that cache it.
        new_items.extend(recent);
        (
            extend_unique_items(&mut new_items, tail),
            "kv-stable: recent edge carried verbatim",
        )
    } else {
        // Classic rebuild: put the in-flight work directly after the new
        // summary, then reconstruct recent dialogue. This deliberately opts
        // out of retaining the former cacheable edge while preserving valid
        // tool-call/result pairs for stateless provider loops.
        let tail_kept = extend_unique_items(&mut new_items, tail);
        extend_unique_items(&mut new_items, recent);
        (tail_kept, "classic rebuild: working tail reconstructed")
    };
    session.input_items = new_items;
    runner.persist_session(session);
    Ok(format!(
        "{summary}\n\n[compact: thinned {thinned} tool bodies · kept {kept} recent dialogue items · \
         {tail_kept} working items · precompact bak written · context_store vars preserved · \
         {strategy}{}]",
        jev_note
            .map(|n| format!(" · jev pre-pass: {n}"))
            .unwrap_or_default()
    ))
}

pub(super) fn extend_unique_items(
    items: &mut Vec<Value>,
    additions: impl IntoIterator<Item = Value>,
) -> usize {
    let before = items.len();
    for item in additions {
        if !items.contains(&item) {
            items.push(item);
        }
    }
    items.len() - before
}

/// Keep a request-valid recent context when the provider refuses both the
/// original request and the model-assisted compaction request as too large.
///
/// The full persisted transcript is copied to `*.precompact.bak` first. The
/// live request retains recent dialogue and complete tool-call/result pairs,
/// which is enough for the agent to continue or reconstruct details with tools.
pub(super) fn emergency_compact_session(runner: &AgentRunner, session: &mut Session) -> usize {
    snapshot_before_compact(session);
    let keep_turns = runner.config.compact_keep_user_turns.max(2) as usize;
    let mut new_items = vec![user_text_item(
        "[Emergency context recovery: the provider rejected the prior request as larger than its \
         context window. Older details remain in the persisted session and precompact backup. \
         Continue the current task from the recent dialogue and complete tool results below. \
         Re-read workspace files when an omitted detail is needed.]",
    )];
    new_items.extend(recent_dialogue_items(&session.messages, keep_turns));
    new_items.extend(safe_tail_after_compact(
        &session.input_items,
        EMERGENCY_KEEP_WORKING_ITEMS,
    ));
    session.input_items = new_items;
    runner.persist_session(session);
    session.input_items.len()
}

/// Fit all request components, retaining every tool name and valid schema.
/// Original large payloads stay retrievable, and call/result pairs are removed
/// together when their arguments alone cannot fit the provider window.
pub(super) fn fit_recovery_request(
    session: &mut Session,
    instructions: &mut String,
    tools: &mut [crate::api::types::ToolDef],
    window: u64,
    reserve: u64,
) -> u64 {
    fn kind(item: &Value) -> &str {
        item.get("type").and_then(Value::as_str).unwrap_or("")
    }
    fn call_id(item: &Value) -> &str {
        item.get("call_id").and_then(Value::as_str).unwrap_or("")
    }
    fn compact_schema(value: &mut Value) {
        if let Some(object) = value.as_object_mut() {
            object.remove("description");
            object.remove("examples");
            object.remove("title");
            if let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) {
                for property in properties.values_mut() {
                    compact_schema(property);
                }
            }
            for field in ["items", "additionalProperties"] {
                if let Some(value) = object.get_mut(field) {
                    compact_schema(value);
                }
            }
            for field in ["oneOf", "anyOf", "allOf"] {
                if let Some(values) = object.get_mut(field).and_then(Value::as_array_mut) {
                    for value in values {
                        compact_schema(value);
                    }
                }
            }
        }
    }
    fn excerpt(session: &str, name: &str, body: &str, limit: usize) -> String {
        let name = format!("{name}_{}", uuid::Uuid::new_v4().simple());
        let saved = context_store::register(
            session,
            &name,
            body,
            "context_recovery",
            "provider-window",
        );
        let pointer = match saved {
            Ok(variable) => format!("Full content: context tool peek/slice/search name={} (id={}).", variable.name, variable.id),
            Err(_) => "Full prior dialogue/tool results remain in the persisted session and precompact backup; original skills remain on disk.".into(),
        };
        let notice =
            format!("[Context recovery. {pointer} Retrieve omitted details before acting.]\n");
        let remaining = limit.saturating_sub(notice.len());
        let mut head_end = (remaining / 2).min(body.len());
        while !body.is_char_boundary(head_end) {
            head_end -= 1;
        }
        let mut tail_start = body.len().saturating_sub(remaining / 2);
        while !body.is_char_boundary(tail_start) {
            tail_start += 1;
        }
        let head = &body[..head_end];
        let tail = &body[tail_start..];
        format!("{notice}{head}\n{tail}")
    }
    let reserve = reserve.min(window / 8);
    for tool in tools.iter_mut() {
        if let Some(description) = &mut tool.description {
            *description = description.chars().take(240).collect();
        }
        if let Some(parameters) = &mut tool.parameters {
            compact_schema(parameters);
        }
    }
    let instruction_limit = (window.saturating_mul(4) / 3).max(512) as usize;
    if instructions.len() > instruction_limit {
        *instructions = excerpt(
            &session.id,
            "recovery_instructions",
            instructions,
            instruction_limit,
        );
    }
    let fixed = attribute_request(instructions, tools, &[])
        .estimated_input_tokens()
        .saturating_add(reserve)
        .saturating_add(128);
    let available_chars = window.saturating_sub(fixed).saturating_mul(4).max(512) as usize;
    let individual = available_chars / session.input_items.len().max(1);
    let mut omitted_calls = std::collections::HashSet::new();
    for (index, item) in session.input_items.iter_mut().enumerate() {
        // Inline images are priced as media (see `estimate_item_tokens`), so a
        // normal screenshot never counts as oversized text here.
        let measured = crate::agent::prompt::estimate_item_tokens(item).saturating_mul(4) as usize;
        if measured <= individual.max(256) {
            continue;
        }
        // An item that is still too large with its images drops them to a
        // placeholder; the base64 must never be excerpted as text.
        if strip_inline_images(item) > 0 {
            let again = crate::agent::prompt::estimate_item_tokens(item).saturating_mul(4) as usize;
            if again <= individual.max(256) {
                continue;
            }
        }
        let encoded = serde_json::to_string(item).unwrap_or_default();
        if kind(item) == "function_call" {
            omitted_calls.insert(call_id(item).to_string());
            continue;
        }
        if kind(item) == "function_call_output" {
            let body = item
                .get("output")
                .and_then(Value::as_str)
                .unwrap_or(&encoded)
                .to_string();
            item["output"] = Value::String(excerpt(
                &session.id,
                &format!("recovery_result_{index}"),
                &body,
                individual.max(256),
            ));
        } else {
            *item = user_text_item(&excerpt(
                &session.id,
                &format!("recovery_item_{index}"),
                &encoded,
                individual.max(256),
            ));
        }
    }
    session
        .input_items
        .retain(|item| !omitted_calls.contains(call_id(item)));
    // Remove older dialogue/pairs as complete groups until the envelope fits.
    while session.input_items.len() > 1
        && attribute_request(instructions, tools, &session.input_items)
            .estimated_input_tokens()
            .saturating_add(reserve)
            > window
    {
        let id = call_id(&session.input_items[0]).to_string();
        session.input_items.remove(0);
        if !id.is_empty() {
            session.input_items.retain(|item| call_id(item) != id);
        }
    }
    reserve
}

pub(super) fn snapshot_before_compact(session: &Session) {
    let path = session.path();
    if path.is_file() {
        let pre = path.with_extension("precompact.bak");
        let _ = std::fs::copy(path, pre);
    }
}

/// Working items carried across a compaction so the model can see the task it
/// was mid-way through.
pub(super) const COMPACT_KEEP_WORKING_ITEMS: usize = 12;
pub(super) const EMERGENCY_KEEP_WORKING_ITEMS: usize = 16;

/// Take the tail of the working items, keeping only *complete* tool pairs.
///
/// A `function_call_output` whose `function_call` was summarised away is a hard
/// 400 on both wire formats — OpenAI rejects a `tool` message with no preceding
/// declaration, Anthropic rejects a `tool_result` with no matching `tool_use`.
/// Dropping either half of a split pair keeps the slice valid for both.
pub(super) fn safe_tail_after_compact(items: &[Value], want: usize) -> Vec<Value> {
    let start = items.len().saturating_sub(want);
    let tail = &items[start..];
    fn kind(v: &Value) -> &str {
        v.get("type").and_then(|t| t.as_str()).unwrap_or("")
    }
    fn call_id(v: &Value) -> &str {
        v.get("call_id").and_then(|c| c.as_str()).unwrap_or("")
    }
    let calls: std::collections::HashSet<&str> = tail
        .iter()
        .filter(|v| kind(v) == "function_call")
        .map(call_id)
        .collect();
    let outputs: std::collections::HashSet<&str> = tail
        .iter()
        .filter(|v| kind(v) == "function_call_output")
        .map(call_id)
        .collect();
    tail.iter()
        .filter(|v| match kind(v) {
            "function_call" => outputs.contains(call_id(v)),
            "function_call_output" => calls.contains(call_id(v)),
            _ => true,
        })
        .cloned()
        .collect()
}

/// Truncate oversized `function_call_output` bodies outside the last `keep_user_turns`
/// user messages. Returns how many bodies were thinned.
pub(super) fn thin_tool_bodies_for_compact(
    items: &mut [Value],
    max_chars: usize,
    keep_user_turns: usize,
) -> usize {
    if max_chars == 0 {
        return 0;
    }
    let user_idxs: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, it)| it.get("role").and_then(|r| r.as_str()) == Some("user"))
        .map(|(i, _)| i)
        .collect();
    let protect_from = if user_idxs.len() > keep_user_turns.max(1) {
        user_idxs[user_idxs.len() - keep_user_turns.max(1)]
    } else {
        0
    };

    let mut n = 0usize;
    for (i, it) in items.iter_mut().enumerate() {
        if i >= protect_from {
            continue;
        }
        if it.get("type").and_then(|t| t.as_str()) != Some("function_call_output") {
            continue;
        }
        let Some(out) = it.get("output").and_then(|o| o.as_str()) else {
            continue;
        };
        if out.chars().count() <= max_chars {
            continue;
        }
        let preview: String = out.chars().take(max_chars).collect();
        let total = out.chars().count();
        if let Some(m) = it.as_object_mut() {
            m.insert(
                "output".into(),
                Value::String(format!(
                    "{preview}\n… [thinned for compact: {total} → {max_chars} chars]"
                )),
            );
        }
        n += 1;
    }
    n
}

/// One completed tool call from this response, kept for the post-execution
/// judge. Bodies stay verbatim; this is a reference, not a rewrite.
pub(crate) struct TsToolResult {
    pub tool: String,
    pub args: String,
    pub body: String,
    pub ok: bool,
}

/// The ongoing job as Jev should see it: the last `prompts` user turns and any
/// assistant reply, bounded.
pub(super) fn typesafe_goal(session: &Session, prompts: usize) -> String {
    let items = recent_dialogue_items(&session.messages, prompts);
    let mut out = String::new();
    for item in items {
        let text = item
            .get("content")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(|p| p.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        if text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&crate::typesafe::harness::judge_preview(text, 1_500));
    }
    out
}

/// Code-built facts for the gate: for each pending call, the index of an
/// identical earlier call whose result is still in context, and how many times
/// that exact call has already failed.
///
/// This is deliberately code, not a question: Jev is only asked to judge the
/// cases where a duplicate actually exists. "Did anything change since?" is a
/// fact too, so any other state-changing call (earlier in the transcript or
/// earlier in this batch) resets both facts: a read after an edit is not a
/// duplicate, and a failure before a fix is not the same failure again.
pub(super) fn typesafe_call_facts(calls: &[FunctionCallRef], items: &[Value]) -> Vec<(Option<usize>, u32)> {
    enum Outcome {
        Pending,
        Ok,
        /// In context only in part (cut down by Jev compaction).
        Partial,
        Failed,
    }
    let mut order: Vec<(String, String, String)> = Vec::new(); // call_id, name, args
    let mut outcome_of: std::collections::HashMap<String, Outcome> =
        std::collections::HashMap::new();
    for item in items {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        let call_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
        match kind {
            "function_call" => order.push((
                call_id.to_string(),
                item.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                item.get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            )),
            "function_call_output" => {
                let body = item.get("output").and_then(Value::as_str).unwrap_or("");
                let outcome = if body.trim_start().starts_with("error:") {
                    Outcome::Failed
                } else if body.contains("chars dropped by Jev-scored compaction") {
                    Outcome::Partial
                } else {
                    Outcome::Ok
                };
                outcome_of.insert(call_id.to_string(), outcome);
            }
            _ => {}
        }
    }
    calls
        .iter()
        .enumerate()
        .map(|(k, c)| {
            let want = normalize_args(&c.arguments);
            let same = |name: &str, args: &str| name == c.name && normalize_args(args) == want;
            let changes = |name: &str, args: &str| {
                !same(name, args) && !crate::tools::capabilities::is_read_only_call(name, args)
            };
            let mut dup: Option<usize> = None;
            let mut failures = 0u32;
            for (i, (id, name, args)) in order.iter().enumerate() {
                if changes(name, args) {
                    dup = None;
                    failures = 0;
                    continue;
                }
                if !same(name, args) {
                    continue;
                }
                match outcome_of.get(id).unwrap_or(&Outcome::Pending) {
                    Outcome::Failed => failures += 1,
                    Outcome::Ok => dup = Some(i),
                    Outcome::Partial | Outcome::Pending => {}
                }
            }
            if calls[..k].iter().any(|p| changes(&p.name, &p.arguments)) {
                dup = None;
                failures = 0;
            }
            (dup, failures)
        })
        .collect()
}

/// Canonical form of a tool call's arguments, for "is this the same call?".
///
/// Structural when the arguments parse as JSON (`{"a": 1}` and `{"a":1}` are the
/// same call; key order does not matter), with a whitespace collapse as the
/// fallback for tools that take raw text. Whitespace *inside* a JSON string is
/// significant and stays significant.
pub(super) fn normalize_args(args: &str) -> String {
    match serde_json::from_str::<Value>(args) {
        Ok(_) => canonical_tool_arguments(args),
        Err(_) => args.split_whitespace().collect::<Vec<_>>().join(" "),
    }
}

/// This turn's tool trace as one-line evidence: what was called, and roughly
/// what came back. Code-built, bounded, and never the whole repository.
pub(super) fn typesafe_trace_evidence(items: &[Value], max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items.iter().rev() {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "function_call_output" => {
                let body = item.get("output").and_then(Value::as_str).unwrap_or("");
                let failed = body.trim_start().starts_with("error:");
                out.push(format!(
                    "{}result: {}",
                    if failed { "FAILED " } else { "" },
                    crate::typesafe::harness::judge_preview(body, 200)
                ));
            }
            "function_call" => {
                out.push(format!(
                    "call {} {}",
                    item.get("name").and_then(Value::as_str).unwrap_or("?"),
                    crate::typesafe::harness::judge_preview(
                        item.get("arguments").and_then(Value::as_str).unwrap_or(""),
                        160
                    )
                ));
            }
            _ => {}
        }
        if out.len() >= max {
            break;
        }
    }
    out.reverse();
    out
}
