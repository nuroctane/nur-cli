---
name: agentic-defense-game-theory
description: "Router for deception-based active defense against AI-agent attackers of DeFi protocols: game theory of honeypots, the agentic attack loop and where to intercept it, lawful bounds of sabotage-by-misdirection, and pointers to the play libraries (contract honeypots, recon-surface poisoning, attacker identification). Use for 'how do I defend my protocol against rogue agents', deception program design, or tripwire strategy. Load one reference slice. Lawful/defender-owned surfaces only."
---

# Agentic defense game theory (deception router)

Attackers now run agent loops: tasking, recon, planning, simulation, execution, laundering. Each stage consumes observations. Deception is the discipline of **supplying the observations** - making attacker effort unprofitable, identifying whoever bites, and handing the evidence to people who can act lawfully.

This is the theory + routing layer. The concrete plays live in three companion skills:

| Need | Skill |
|------|-------|
| Onchain deception primitives + per-attack-vector plays | `deploying-contract-honeypots` |
| Poisoned docs / decoy repos / watermarked leak channels | `poisoning-agent-recon-surfaces` |
| Tripwire telemetry, attribution, lawful handoff | `identifying-agentic-attackers` |

Historical class mapping (`T01`..`OPS20`) comes from `historical-smart-contract-vulns` `references/taxonomy.md` - the play libraries reference those ids directly.

## Lawful gate (read before any play)

Defender-owned surfaces only. You deceive about YOUR protocol, YOUR decoy contracts, YOUR docs, YOUR bait wallets. You never: deploy malware at an attacker, break into their infrastructure, steal their funds, or spoof a third party. Read `references/lawful-deception-bounds.md` first. Deceiving your own users or auditors is out of scope - that is insider abuse, not defense.

## How to use

1. State the objective: deter, delay, detect, identify, or exhaust. Most programs want **identify + impose cost**; pure deterrence talk without tripwires is cheap-talk collapse (see core).
2. Map the attacker loop for YOUR protocol: `references/attacker-loop-map.md` - including where agents land when they recon you (verified source, docs, explorer labels, governance forum, fork tooling).
3. Pick plays from the companion skills keyed to your exposure classes. Every play row carries an effect code: **C** cost imposition, **I** identification, **D** deterrence signaling, **P** planner poisoning, **S** type separation.
4. Wire tripwires to the response ladder in `identifying-agentic-attackers` before going live. A lure nobody watches is just free recon for the attacker.
5. Drill your own red team against the deception set - `researcher-gym-and-curriculum` + the SCA playbooks double as the attacker model.

## The one insight worth internalizing

The public 2026 benchmark-solutions honeypot experiment worked because it **did not advertise itself**: solution pages were indexed where agents search, the "solution" was never marked as bait, and the retrieval step made attackers self-identify (model id + harness in the query, submission actions logged). Agents did not know they were in a case. Case ids never leaked. The defender simply *was where the agent lands, offering exactly what it wants, instrumented end to end*. Every play in this family is that pattern re-instantiated onchain and offchain.

## Fat references (read as needed)

| File | Contents |
|------|----------|
| `references/game-theory-core.md` | Signaling vs screening, commitment devices, attacker EV math, repeated games, POMDP poisoning |
| `references/attacker-loop-map.md` | Stage-by-stage agent loop, defender levers, case study dissection |
| `references/lawful-deception-bounds.md` | What you may and may not do; collateral-damage rules; disclosure duty |
