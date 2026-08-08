---
name: dogwood
description: AWS Dogwood runtime verification for AI agents - author .dw policies (Cedar extended with temporal logic since formerly once count_within) and evaluate them against an agent tool-call event stream. Use to govern or verify tool calls with temporal policy, validate a .dw policy, or replay a trace against a policy.
---

# Dogwood - runtime verification for AI agents

**Dogwood** (https://github.com/dogwood-policy/dogwood, Apache-2.0) is a policy
language that extends Cedar with temporal logic - `since`, `formerly`, `once`,
`count_within` - evaluated over an agent's event stream. It is used to *govern
tool calls* (e.g. "no more than N destructive calls within a window", "verify a
step followed a prerequisite").

> **Honesty note:** Dogwood ships a **reference** Rust interpreter that is
> explicitly **not production-grade enforcement**. Treat the `dogwood` tool as
> an **on-demand evaluation / guardrail** layer, not a runtime trust anchor and
> not a gate on every tool call. For real enforcement, port the policy to your
> enforcement path.

## Prerequisite

The integration shells out to the `dogwood` CLI. It is not auto-installed.

```sh
cargo install --git https://github.com/dogwood-policy/dogwood amzn-dogwood-cli
dogwood --version
```

If the binary is missing, `dogwood` reports `status` as not installed with the
install hint above. The tool degrades gracefully (no crash, no auto-install).

## Tool actions (`dogwood` tool)

| action | purpose |
|--------|---------|
| `status` / `which` | is the dogwood CLI available? path/version + install hint |
| `check-parse` (policy) | syntax-check a `.dw` policy |
| `validate` (policy, schema?, event_schema?, providers?, macros?) | validate a policy against schemas |
| `replay` (policy, trace, schema?) | replay a trace.log against a policy |
| `lower` (policy, schema, emit?) | lower Dogwood into Cedar policies/schema/JSON |
| `schema` (schema_kind, input/manifest) | validate schemas or generate an action schema from MCP |

`format` is `human` (default) or `json`.

Examples:

```
dogwood(action=status)
dogwood(action=validate, policy=policy.dw, schema=policy.cedarschema)
dogwood(action=validate, policy=policy.dw, schema=policy.cedarschema,
        event_schema=events.dwschema, providers=providers.json)
dogwood(action=replay, policy=policy.dw, schema=policy.cedarschema, trace=trace.log)
dogwood(action=check-parse, policy=policy.dw)
dogwood(action=lower, policy=policy.dw, schema=policy.cedarschema, emit=both)
dogwood(action=schema, schema_kind=mcp, manifest=tools.json)
```

## Authoring a `.dw` policy

A Dogwood policy is a Cedar policy extended with temporal operators over the
agent's event stream. A minimal policy looks like:

```
// Permit a tool call unless more than 2 destructive calls happened in the last 60s.
@deny("rate-limit destructive calls")
permit(principal, action == Action::"DeleteFile", resource)
when { count_within(0..60s, event.action == Action::"DeleteFile") < 3 };
```

Temporal operators:

- **`once(pred)`** - true if `pred` holds on some past event.
- **`formerly(pred)`** - true if `pred` held on the immediately prior event.
- **`since(pred)`** - true if `pred` has held continuously since the last time
  another predicate was true; used to constrain how long a state has persisted.
- **`count_within(range, pred)`** - counts matching events in a time window
  (e.g. `0..60s`).

An event stream records the agent's tool calls (principal, action, resource,
timestamp). Match on `event.action`, `event.principal`, `event.resource`, and
custom event fields via the event schema.

## Workflow

1. Write `policy.dw` (and schemas: `policy.cedarschema`, optionally
   `events.dwschema`).
2. `check-parse` to confirm it parses; fix syntax errors.
3. `validate` against the schema to confirm well-typed / well-formed.
4. Record a trace of a session (tool-call event log) and `replay` it against
   the policy to see which calls pass / are denied.

## What not to do

- Do **not** present Dogwood as enforced runtime policy. It is an evaluation /
  guardrail layer on demand.
- Do **not** shell out to `dogwood` via `bash` - use the `dogwood` tool so the
  install hint and status handling are consistent.
- Do **not** auto-install the CLI as part of provisioning; it stays an
  on-demand tool.
