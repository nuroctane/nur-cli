---
name: security-audit
description: Security guidance and vulnerability review for codebases, APIs, services, CLI tools, libraries, and daemons. Use when asked security questions, for focused reviews, vulnerability research, security audits, or pen tests. Run the complete workflow only for explicit codebase audit or pen-test requests, full/comprehensive/end-to-end reviews, or requested report artifacts.
---

# Security audit

Turn an audit request into a coverage-led hunt with adversarial
validation, not a checklist walk. This is a workflow layer on top of
evidence producers, not a scanner itself: run `secscan` first for scanner
output, then run this workflow over the code. A scanner finding is not a
confirmed vulnerability, and a defense-in-depth gap is not a vulnerability.

## The six phases

1. **Reconnaissance.** Map architecture, trust boundaries, input surfaces,
   and prior evidence. Write two artifacts before hunting:
   `architecture.md` (components, boundaries, where untrusted input enters,
   what each component trusts) and `coverage-ledger.json` (the units of
   code to be hunted, each with an owner and a status). No hunting before
   the ledger exists, or coverage is whatever the hunter happened to see.
2. **Coverage-led hunting.** Assign hunters from ledger units, one unit per
   hunter, using the `agent` tool with `explore` for research-style hunting
   and `general` for broad investigation. Each hunter works in its own
   worktree (`agent.worktree`) and writes only to its own scratch path.
   The root agent owns the shared run files and is the only writer of
   `findings.json` and `coverage-ledger.json`.
3. **Candidate validation.** Every unique candidate goes to a fresh
   verifier that tries to disprove it. The agent that checks a finding is
   never the agent that found it. A verifier that only re-reads the
   hunter's notes is not a verifier: it must reproduce the boundary
   failure from the source, or name the exact unresolved fact.
4. **Structured output.** Record every candidate in `findings.json` with
   one of three verdicts, and validate it with
   `scripts/validate_findings.py`:
   - `confirmed`: a complete source trace and a bounded observed result.
     Severity requires impact: likelihood x impact, not deviation from a
     checklist.
   - `needs_validation`: an exact unresolved fact, and no severity. A
     candidate stays here when the sandbox cannot be proven, when target
     code cannot be executed safely, or when the trace is incomplete.
   - `rejected`: a disproved candidate, with the disproof recorded so the
     next hunter does not re-find it.
5. **Independent record verification.** Fresh agents verify the final
   source claims in the report. Material replacements get another verifier.
   A claim that changed during verification is re-verified, not carried.
6. **Target-neutral reporting.** Derive `REPORT.md`, `FINDINGS-DETAIL.md`,
   and `NEEDS-VALIDATION.md` from the verified records and the ledger.
   The report describes what was found and what was not covered; it does
   not claim coverage the ledger does not show.

## Attack classes to hunt, by ledger unit

- Core: authentication, authorization, input validation, output encoding,
  secrets handling, cryptography, session management.
- Wildcard: any boundary where untrusted input crosses into a trusted
  component, including CLI args, files, environment, network, and model
  output treated as data.
- Memory safety and binary: unsafe code, unchecked lengths, integer
  handling, deserialization of untrusted bytes.
- AI and LLM: prompt injection, agent/tool misuse, output handling where
  model output is executed, parsed, or trusted.
- Web, protocol, and auth: protocol state machines, RPC boundaries,
  token handling, redirect and origin checks.
- Client side, supply chain and release: dependency trust, build and
  release integrity, update channels.
- CI and supply chain: unpinned third-party Actions (pin to a full
  commit SHA; tags and branches are mutable and repointable, see the
  `workflows` check in `secscan`), `pull_request_target` pwn requests,
  over-broad `permissions:`, mutable transitive dependencies. An
  unpinned Action is a candidate, not a confirmed vulnerability:
  severity still needs impact.
- Cloud and deployment, resource exhaustion, data isolation and lifecycle,
  desktop/mobile and local IPC.

## Rules

- Only confirm established boundary failures. A suspicious pattern with
  no trace to a boundary is `needs_validation`, not `confirmed`.
- Severity requires impact. Do not assign severity to an unconfirmed
  candidate.
- Sandbox: nur-cli's bash OS sandbox (network off by default, workspace
  path sandbox) is a partial fit for target-controlled builds, not a full
  descriptor-based promotion protocol. Without proven sandbox controls,
  the workflow keeps the lead as `needs_validation` instead of executing
  target code.
- Credentials go through the `secret` tool and `$NAME` only, never
  through findings, reports, or tool results. A finding that needs a
  credential to reproduce is recorded as `needs_validation` with the
  secret name and purpose, not with a value.
- Change nothing until the user picks findings. An audit reports; a fix
  is a separate request.
