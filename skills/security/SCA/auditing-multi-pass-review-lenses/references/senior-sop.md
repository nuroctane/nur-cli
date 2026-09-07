# Senior SOP (working order for the whole review)

The operating procedure the lenses assume. One full read, continuous annotation, no re-audit loops.

## 1. Read the protocol before reading the code

Whitepaper/docs/design docs first: stated invariants, trust assumptions, economic model. Anything the docs promise that the code must uphold is now an audit obligation - tag each `(per spec)` in your notes so spec-claims stay separable from code-verified facts.

## 2. Map the surface once

- Entry points classified: permissionless / role-gated / admin-only. Caveat: a function without a modifier but WITH an internal `msg.sender` check is role-gated - do not let grep-only classification lie to you.
- Value-holding state, external calls, storage deltas per entry point.
- Roles: who holds what; which powers are instant vs timelocked; which critical functions are unpausable (fold those into the role-compromise surface).

## 3. Fixed traversal order per function family

Single code-path pass in a stable order - constructor -> setters -> swap -> mint -> burn -> liquidate (adapt to protocol type; the point is CONSISTENCY so comparisons across findings are fair). Per-path verdict vocabulary: BLOCKS / ALLOWS / IRRELEVANT / UNCERTAIN, with UNCERTAIN = ALLOWS.

## 4. Continuous mental tools

Feynman plain-English narration, Socratic why-drilling, Inversion attacker moves - run during reading, not as a separate pass (markers `[F] [S] [I]` in notes).

## 5. Check both name variants

When tracing a function, check both `functionName` and `_functionName` (underscore-prefixed internal variants) before declaring a path guarded or unguarded.

## 6. Spec/behavioral threat extras

- Temporal threats: what changes between txs vs within one tx.
- Composability threats: what other protocols do with YOUR outputs (prices, rates, receipts).
- Protocol-type profiling: lending vs AMM vs vault vs bridge each shifts which lens leads (route deep-dives via the matching `reviewing-*` playbook).

## 7. Signal counting (scope honesty)

Before starting, count and record: nSLOC, test files/functions, fuzz/invariant/formal-verification signals (Foundry invariant, Echidna, Medusa, Certora, Halmos, fork tests). This is scope truth for the report and feeds `auditing-git-history-and-specs` if you run the history pass.

## 8. Git-history pass (optional but cheap)

If the repo has real history, run the forensics pass from `auditing-git-history-and-specs` - fix-candidate commits and late-change churn routinely point straight at the softest code.

## Verification honesty

Every "missing check" claim requires grep-confirmed write sites (all of them, not one). If confirmation is impossible, write "could not confirm" - a demoted lead with honest epistemics beats a confident fiction.
