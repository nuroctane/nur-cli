---
name: identifying-agentic-attackers
description: "Turn deception-program tripwires into identified attackers and lawful outcomes: tripwire event schemas and telemetry, attribution and evidence-pack standards (zachxbt-class dossier discipline, exchange/FBI/SEAL handoff), and the graduated response ladder from observe to freeze-to-handoff. Use when a honeypot, canary, or bait fires, or to design the response side of a deception program. Lawful handoff only - no hack-back."
---

# Identifying agentic attackers (attribution + handoff)

The response half of the deception family. Lures produce signals; this skill turns signals into cases: schema, correlation, evidence discipline, lawful handoff, and the graduated response ladder.

Pairing: lures live in `deploying-contract-honeypots` + `poisoning-agent-recon-surfaces`; theory in `agentic-defense-game-theory`.

## Choose your file

| You want | Load |
|----------|------|
| Event schema + telemetry plumbing for tripwires | `references/tripwire-telemetry.md` |
| From signals to identity: attribution, evidence packs, handoff targets | `references/attribution-evidence-handoff.md` |
| What to do, in what order, when something fires | `references/response-ladder.md` |

## Standing rules

1. **Attribution is probabilistic.** Wallets are not people; rented infrastructure is not the operator. Every confidence claim states its basis. Public accusations follow the dossier discipline or stay private.
2. **Handoff beats hack-back.** Every escalation path that exists is lawful (exchanges, issuers, law enforcement, SEAL 911, bounty trackers). Anything else is out of bounds (`agentic-defense-game-theory` `references/lawful-deception-bounds.md`).
3. **The ladder is pre-committed.** Decisions about pausing/sweeping/disclosing are made now, written down, and executed mechanically - not negotiated mid-incident (commitment-device logic).
4. **Ops-layer touches are emergencies; decoy-layer touches are leads.** Bybit-class events begin as odd probes. Never let tripwire routine dull ops-layer urgency.
