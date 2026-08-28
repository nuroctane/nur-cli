# Poison and noise ops (corrupt the planner)

Beyond single lures: systematic degradation of the attacker's planning data. Objective codes per core legend (P dominant, C secondary).

## Poison types, by what they corrupt

### 1. Address/layout corruption (P)

- Decoy impl storage layouts that differ from real ones (published nowhere, discoverable only by probing decoys).
- Decoy governance/timelock addresses that mirror real naming conventions.
- Bait token addresses for "the new vault token" that is actually a tripwire contract.
- Rule: fiction must be orthogonal to fact - never false statements about REAL contracts (auditors/users rely on those). Fictional contracts are fair game.

### 2. Profit-estimate corruption (P, C)

- Decoy pools whose sim-value diverges from live (see `deploying-contract-honeypots` `references/anti-simulation-fork-traps.md`).
- Bait TVL/liquidity impressions via decoy subgraphs.
- Effect compounds: after N fake payoffs, the operator's prior for "this protocol is exploitable for $X" is untrustworthy in BOTH directions - they must re-verify everything at full cost. That is planner-level denial.

### 3. Tooling friction (C)

- Decoy artifacts that require extra processing steps to use (encrypted "key backups" with findable-but-slow derivation paths; docs that require assembling facts from three pages).
- Rate-limit + proof-of-work gates on decoy content (cheap for you, meters their crawlers).
- Do NOT serve content that harms consumer tooling (a legit analyst's crawler should time out, not crash).

### 4. Contradiction seeding (P)

- Two decoy surfaces implying different root causes for the same (fictional) weakness. Attacker planning must reconcile; time spent is your win.
- Keep contradictions BELOW human-obvious level: the tripwire is that only systematic cross-checking (agent behavior) hits the conflict.

### 5. Memory-loop poisoning (P, long horizon)

- Agents persist findings across sessions (operator databases, vector stores, notes). Poisoned artifacts that get stored become recurring costs: every future run re-derives the false map until the operator purges it.
- Design poison to be STORE-WORTHY: high-detail, well-structured, internally consistent - exactly what a good agent saves. You are writing for their database, not their eyes.

## Noise budget (avoid self-poisoning)

Deception has defender-side costs: telemetry volume, incident triage, community confusion, and "boy who cried honeypot" reputational risk if overdone. Budget rules:

1. Total live artifacts: 5-15. Sparse and plausible beats comprehensive and detectable.
2. Every artifact needs an owner and an expiry. Sunsets are mandatory.
3. One public "deterrence signal" per fired campaign at most (see `identifying-agentic-attackers`); constant honeypot-bragging collapses the ambiguity asset.
4. Quarterly review against attacker-tooling drift (`defi-security-trends-standards` tooling file).

## Measurement

| Metric | Meaning |
|--------|---------|
| First-touch rate per channel | Is the bait where they actually search? |
| Depth-completion rate | Do agents go past page view into action? (benchmark-solutions saw submission actions - the good signal) |
| Time-to-abandon | Longer = better cost imposition |
| False-trip rate (innocents) | Must stay ~0; >0 = redesign for screening |
| Attacker adaptation markers | Known-fake lists, changed recon patterns -> refresh |

## Worked mini-plan (example: mid-size lending protocol)

Exposure classes: T05 oracle spot (real fix shipped quietly), T12 proxy hygiene (real fix shipped), T20 messaging config (real: multi-DVN).

Deception overlay:
1. Decoy empty vault (T06/T36 bait) labeled "LegacyVault v1 [DEPRECATED]" (I).
2. Fake runbook `/ops/runbooks/timelock-rotation.md` with canary-fetch step + bait timelock address (I,P).
3. Decoy oracle-consumer contract without uptime gate, unlisted (T26 bait) (I).
4. Bait wallet keys in a "leaked" gist (I,C).
5. All watermarked per channel; watchers wired; response ladder rehearsed.

Total defender cost: days of work + dust funds. Attacker cost if they bite: full exploit pipelines built against fiction, plus identification. That asymmetry is the whole business case.
