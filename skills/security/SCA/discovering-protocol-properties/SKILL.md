---
name: discovering-protocol-properties
description: "How to discover fuzz/invariant properties for a protocol: the three property classes (global, function-specific, delta-based), source-driven discovery (accumulative ops, counters, extreme values, exposed-state enumeration), protocol-type adaptation, property quality bars (no speculative ghosts, snapshot discipline, success-gating, diagnosability), and discovery perspectives (adversarial profit maximizer, conservation auditor, rounding analyst, state-transition mapper). Complements the mechanics skills. Use when building a fuzzing/invariant campaign from scratch. Load one reference slice."
---

# Discovering protocol properties

The hardest part of fuzzing is not running the fuzzer - it is knowing what to assert. This skill is the discovery methodology: where properties come from, how they are shaped, and the quality bars that keep a suite fast and diagnosable. Mechanics live elsewhere: handlers in `writing-foundry-invariant-handlers`, fuzzer config in `fuzzing-with-echidna-and-medusa`. Distilled from fizz-class property-generation methodology (pashov/skills) and aviggiano's testing-goals docs; runs single-session.

## Choose your file

| You want | Load |
|----------|------|
| Property classes, discovery sources, quality bars, patterns | `references/property-discovery.md` |
| Discovery perspectives to rotate through when output dries up | `references/discovery-perspectives.md` |

## Single-session discipline

Discovery is reading + writing Solidity in this session - no per-perspective subagents, no parallel property generation. Rotate perspectives sequentially; when a perspective stops yielding, move to the next and come back only with new code knowledge.

## Pipeline

1. Skim the invariant catalog if `auditing-git-history-and-specs` ran (`G/I/X/E` entries are property seeds).
2. Run `references/property-discovery.md`: classify state changes, enumerate snapshot candidates, draft properties per pattern.
3. Stuck? Rotate `references/discovery-perspectives.md`, one at a time.
4. Hand properties to the mechanics skills; every property gets a name, an owner perspective, and a failure-diagnosis note.

## Quality bar in one line

If a property failure would not make you stop and investigate, it is not a property - it is a tautology burning fuzz budget.
