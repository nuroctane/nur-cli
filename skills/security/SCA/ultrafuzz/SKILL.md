---
name: ultrafuzz
description: "Agentic smart-contract fuzzing and threat-hunting orchestrator (Monad's Ultrafuzz, MIT, github.com/monad-developers/ultrafuzz). Coordinates parallel fuzzing strategies, property-guided investigation, specialist agents, and OWASP SCVP dedupe into one campaign. Use for deep/long-running audits of Foundry projects (Solidity, some Vyper) where property-guided review beats ad-hoc test writing. Authorized targets only."
---

# Ultrafuzz - agentic smart-contract fuzzing orchestrator

Monad's open-source (MIT) agent orchestrator that plans, coordinates and
reviews fuzzing campaigns: threat-model goals -> property extraction ->
parallel strategies -> specialist agents -> review/dedupe -> report.

Upstream: `https://github.com/monad-developers/ultrafuzz` (MIT).
Requires the Foundry toolchain (`forge`, `cast`, `anvil`) on PATH.

**Authorized targets only**: your own repo, a contest repo, or an in-scope
engagement. Every PoC is a local test you own - never live systems.

## When to reach for this

- User asks for a deep fuzzing campaign / threat hunt at scale, not just a
  static pass (`static-analysis-slither-aderyn-semgrep-wake` covers static).
- The protocol has value-critical invariants (vaults, accounting, access
  control, liquidations) worth hammering.
- Skip it for tiny repos or UI-only code - a manual pass is cheaper.

## v0.1.0 learnings (use these, they are the whole point)

1. **Property-guided direct investigation first** (the NoFuzz lesson): derive
   properties from the threat model and investigate them directly. Forcing
   the model to author Foundry fuzz tests before understanding the protocol
   produced worse recall than reading with a property checklist.
2. **Reserve stateful fuzzing for what needs it**: sequence- and time-
   dependent bugs (multi-step accounting drift, liquidation ladders, epoch
   logic). Everything else is usually found faster by direct investigation.
3. **Property specification quality dominates**: a vague property wastes a
   campaign. State the invariant, its scope, and its violation concretely
   before any campaign (`discovering-protocol-properties` pairs well here).
4. **Breadth drives recall**: run many strategies in parallel; dedupe later.
5. **Map every finding to the OWASP SCVP taxonomy** so results are comparable
   and deduplicable across runs and across `reviewing-*` playbook findings.

## Workflow

1. **Setup** (ask before installing anything):
   - `git clone https://github.com/monad-developers/ultrafuzz` and build per
     its README; verify the Foundry toolchain (`forge --version`).
   - Confirm the target repo, scope, and that tests run locally.
2. **Zero-config first**: Ultrafuzz ships audit profiles - start with the
   profile that fits the repo instead of hand-tuning. Show the available
   profiles and recommend one (target size, EVM version, test coverage).
3. **Threat model -> goals -> properties**: write the goals down before
   launching. For each value-critical subsystem, one concrete property with
   scope and violation condition. This is the step that decides campaign
   quality - spend prompts here, not on test authoring.
4. **Launch the campaign** with the property set; let the orchestrator fan
   out strategies (20+). Run in the background and poll - do not block on a
   single long-running command.
5. **Monitor via CLI, not logs**: check campaign status through its CLI;
   **resume after node or process failure** instead of restarting from zero.
6. **Review + dedupe**: merge orchestrator findings with static-analysis and
   manual-review findings; dedupe by OWASP SCVP class + root cause; drop
   duplicates before reporting.
7. **Report**: findings table (severity, SCVP class, root cause, PoC test,
   suggested fix). File externally via `contest-and-bounty-reporting` when
   in scope.

## Pairing with this pack

- Static pass first: `static-analysis-slither-aderyn-semgrep-wake`.
- Invariant handlers by hand: `writing-foundry-invariant-handlers`;
  Echidna/Medusa specifics: `fuzzing-with-echidna-and-medusa`.
- Property discovery method: `discovering-protocol-properties`.
- Reference-model diffing for value-critical cores:
  `auditing-against-reference-models`.
- Dedupe/triage: `triaging-and-deduping-findings`.

## Guardrails

- Local Foundry tests only. No broadcasts, no live targets, no funded
  wallets. Out of scope = stop and say so.
- A campaign is expensive: confirm cost/appetite with the user before
  launching, and report interim findings while it runs rather than going
  silent.
