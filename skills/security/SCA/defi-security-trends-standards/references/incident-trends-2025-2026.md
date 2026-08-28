# Incident trends 2025-2026 (tracker frame)

Permanent incident rows live in `historical-smart-contract-vulns/references/incident-timeline.md`. This file is the *trend frame*: what the numbers say and where trackers disagree. Numbers below as reported at 2026-08 refresh; re-verify before citing in paid reports.

## The shape of losses

| Period | Read | Source class |
|--------|------|--------------|
| 2025 full year | DeFi protocol-bucket losses fell ~74% to ~$680M; ~89% of that was protocol-logic exploits, not key theft ([deepstrike synthesis](https://deepstrike.io/blog/defi-hacks-exploits-statistics), Immunefi scoreboard) | Immunefi/trackers |
| 2025 total-theft view | Including ops/custodial (Bybit $1.46B): ~$2.87-3.4B stolen across ~150 events; North Korea-attributed ~$2B+ | TRM 2026 report / Chainalysis |
| H1 2026 | Record incident COUNT: ~207 incidents/~$972M (some tallies run $1.1-1.32B on wider inclusion); highest-ever half-year count, dollars down per-incident vs ops era | TRM/Blockaid variants |
| Q2 2026 alone | ~99 incidents / ~$746M - bridge-config class worst category ([shattered summary](https://shattered.io/defi-exploits-q2-record-2026/)) | monthlies |
| April 2026 | Worst single month ever cited: $635M+ dominated by KelpDAO (~$292M) + Drift (~$285M) | multiple |

## Trend lines that matter for scoping

1. **Logic >> keys inside DeFi; keys >> logic outside it.** Bybit-shaped ops compromise still dominates raw dollars industry-wide while in-protocol bugs dominate DeFi-bucket stats. Reports must name which bucket they mean.
2. **Messaging-layer CONFIG is the bridge-era successor**: KelpDAO's single-DVN verifier quorum (Apr 18 2026, Lazarus-attributed per [Chainalysis](https://www.chainalysis.com/blog/kelpdao-bridge-exploit-april-2026/) / [Halborn](https://www.halborn.com/blog/post/explained-the-kelp-dao-hack-april-2026/)) made "count your verifiers" the top bridge checklist item.
3. **Same-bytecode, N-chain blast radius** (Balancer V2 Nov 3 2025 across six chains) - coverage plans must treat every deployment target as one review unit.
4. **Governance-speed exploits** recur (Term Finance ~$8.5M Aug 24 2026 takeover in minutes) - guardrail latency is now an audited property.
5. **Hardware/firmware ops layer surfaced** (Coldcard-class July 2026 event ~$110M dwarfed all DeFi exploits that month `[verify publicly confirmed detail]`) - ops-vs-code classification discipline (`investigator-ops.md`) matters more each quarter.

## Tracker disagreement discipline

Immunefi scoreboard vs TRM vs Chainalysis vs PeckShield differ by: bucket definitions (ops included?), bounty-save netting, unfreeze/recoveries, and attribution claims. House rule stays: **prefer Immunefi scoreboard + official postmortems in reports; monthlies are leads, not citations**. Mark anything secondhand `[verify]`.

## Whitehat-side barometer

- Immunefi reports platform payouts continuing to grow ($13M+ on-platform in H1 2026 scale figures circulating `[verify]`).
- Highest-value saves stay in L2/messaging/bounty-heavy tiers; see `hunting-x-linked-bounties` `paid-payouts.md` rows for current-ceiling calibration before scoping bug bounties.
