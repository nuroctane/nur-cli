# Discovery perspectives (rotate when output dries up)

Sequential lenses for finding properties you are not seeing. One at a time, in this session. Each has a signature question and typical property families it yields.

| # | Perspective | Signature question | Property families it yields |
|---|-------------|--------------------|-----------------------------|
| 1 | Conservation auditor | What enters, what leaves, and does the difference reconcile? | Solvency, supply conservation, escrow balance == obligation, negative-conservation traps (flows that should move state but do not) |
| 2 | Adversarial profit maximizer | Playing as an attacker inside the rules, what sequence extracts value? | No-single-tx-profit bounds, sandwich-resistance bounds, no-risk-free-arb between paired functions, fee floor/ceiling holds under any sequence |
| 3 | Roundtrip rounding analyst | What happens when operations reverse (deposit+withdraw, mint+burn, swap back)? | Roundtrip never creates value; roundtrip loss is bounded; zero-amount roundtrips are no-ops; share math symmetric |
| 4 | State-transition mapper | Draw the real state machine; which transitions are missing or illegal-but-possible? | Illegal-transition revert properties, terminal-state permanence, no-state-loss-on-abort, phase-boundary invariants |
| 5 | Protocol-type specialist | What does EVERY protocol of this type guarantee? | Type canon: lending (no bad debt absent oracle failure), vaults (share price monotonicity), AMMs (constant product within fee bounds), staking (reward debt non-negative) |
| 6 | Synthesizer | Merge the property drafts; kill duplicates, tautologies, and untestable wishes | Deduped, prioritized suite ordered by blast radius |

## Usage rules

1. Run 1-2-3 first on almost any protocol; they are the highest-yield by far.
2. Perspective 5 is a seed list: type-canon properties are known-good but generic - the money is in protocol-specific overrides of the canon.
3. Every property gets its perspective recorded. When a property fails later, the perspective tells you whether failure smells like a bug (conservation, profit) or a modeling gap (state mapper).
4. A perspective that yields nothing is a finding about YOUR understanding - note it and revisit after the lens passes in `auditing-multi-pass-review-lenses`.

## Anti-goals

- Do not generate hundreds of trivial asserts - each weak property dilutes failure triage and slows runs (every property executes every sequence).
- Do not encode deployment-configuration constants as eternal truths unless the suite is scoped to that deployment.
- Do not let two properties encode contradictory models silently - contradictory failures mean your MODEL is wrong, which is itself a discovery worth writing down.
