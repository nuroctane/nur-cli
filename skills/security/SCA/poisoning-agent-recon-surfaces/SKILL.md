---
name: poisoning-agent-recon-surfaces
description: "Offchain deception for AI-agent attackers of DeFi protocols: landing-surface design (decoy docs, bait search results, fake internal runbooks, robots.txt lures, decoy repos and RPCs), per-channel watermarking so exploits reveal their source, and planner-poisoning ops (misleading layouts, ABIs, addresses). Pairs with deploying-contract-honeypots. Lawful/defender-owned surfaces only. Load one reference slice."
---

# Poisoning agent recon surfaces (offchain deception)

Agents attack what they can read. Their recon layer - search, crawlers, verified source, docs, repos, explorer labels - is defender-ownable real estate. This skill plants watermarked misdirection there so the planning stage of the attack loop runs on corrupted inputs.

Pairing: onchain primitives in `deploying-contract-honeypots`; attribution in `identifying-agentic-attackers`; lawful gate in `agentic-defense-game-theory` `references/lawful-deception-bounds.md`.

## Choose your file

| You want | Load |
|----------|------|
| Design landing surfaces agents actually hit | `references/landing-surfaces.md` |
| Watermark everything so leaks self-identify | `references/watermarking-channel-ids.md` |
| Corrupt planning data; run noise ops on a budget | `references/poison-noise-ops.md` |

## The pattern (from the public 2026 agent-honeypot experiment)

Figure out what the agent will search. Be exactly there, with exactly the artifact it wants, instrumented end to end. Never label the bait. Never leak your case correlation. Serve identification questions opportunistically (self-reported model/harness in the benchmark-solutions case; "verify access" steps in runbook decoys). The agent should complete its mission - against your fiction.

## Quality bars

1. **Plausibility first.** A decoy runbook with wrong-but-consistent details beats an obvious fake. Agents cross-check; inconsistencies should be rare and internal (details that only conflict under deep analysis - that conflict IS the tripwire).
2. **One-way information.** Every poison surface must be inert when consumed by a legitimate party: a human whitehat reading a decoy doc wastes a minute and finds a lawful-use notice; an attacker acting on it finds bait.
3. **Counted and versioned.** Every artifact has an id, a channel, a watermark, a deploy date, and a retirement plan. Unmanaged poison eventually poisons your own incident response.
4. **Real docs stay excellent.** Poison overlays your public security posture; it does not replace honest docs, bounties, or audits.
