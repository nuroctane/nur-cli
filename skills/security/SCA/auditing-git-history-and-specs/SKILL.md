---
name: auditing-git-history-and-specs
description: "Audit surfaces beyond source code: git/repo history forensics (security-fix-shaped commits, late-change churn, dangerous-area hotspots, forked dependency lineage, dev-pattern risk), invariant synthesis from code structure (conservation, guard-lifting, ratio ordering, temporal ordering, cross-contract assumptions, economic invariants), and spec-vs-code auditing of whitepapers and design docs. Distilled from x-ray-class methodology, single-session by design. Use for pre-audit recon, upgrade risk review, or invariant catalog building. Load one reference slice."
---

# Auditing git history and specs

Two high-yield surfaces most reviews skip: the repository's own history and the written spec. History shows where the team has been bleeding; specs show what the code quietly stopped promising. Methodology distilled from x-ray-class audit tooling (pashov/skills), orchestration removed - everything runs in this session with git + grep.

## Choose your file

| You want | Load |
|----------|------|
| Git forensics: fix-shaped commits, churn, forked-dep lineage | `references/git-forensics-classes.md` |
| Invariant synthesis taxonomy + catalog format | `references/invariant-synthesis.md` |
| Spec/doc vs code obligations, threat profiling | `references/spec-vs-code-audit.md` |

## Where this sits in the pack

- Feeds `auditing-multi-pass-review-lenses` (invariant catalog = lens 5 ammunition; fix candidates = priority map).
- Feeds `auditing-foundry-smart-contract-security` (test-gap signals from history).
- Upgrade/redeployment reviews on any chain: pair with `reviewing-major-chain-surfaces` chain file + `reviewing-upgradeable-proxies`.

## Single-session discipline

All seven git analyses are `git log`/`git diff` invocations you run and interpret yourself - sequential, no workers, no per-file subagents. On monorepos, scope to the in-scope paths first (`git log -- <paths>`), or history noise will eat the session.

## Standing output

Three artifacts, sized to stay readable: a verdict report (<500 lines), an invariant catalog (`G-N` global guards, `I-N` single-contract, `X-N` cross-contract, `E-N` economic), and an entry-point/flow map. Keep them next to the codebase during the engagement.
