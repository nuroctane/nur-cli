# Attribution, evidence packs, and lawful handoff

From "address 0xABC touched my decoy" to a case an exchange, issuer, or agency will act on.

## Attribution ladder (confidence grows with corroboration)

1. **Wallet-level facts (machine-verifiable)**: tx hashes, first-touch on watermarked bait, funding chains on public ledgers, timing correlations across your tripwire events. Strength: certain about the WALLET, nothing more.
2. **Infrastructure clustering (analytical)**: shared RPC/user-agent/tooling fingerprints across decoy touches; gas/bribe patterns; cross-chain calldata-shape reuse (the same exploit script on six chains = one operator, Balancer-lesson inverted). Strength: high for OPERATOR-clustering, still not identity.
3. **Behavioral/ops-layer indicators (investigative)**: self-reported model/harness strings, TTP overlap with known campaigns (Lazarus-pattern markers per `investigator-ops.md`), language/timing patterns. Strength: expert-domain; this is zachxbt/agency territory - contribute evidence, defer conclusions.
4. **Identity (only via lawful channels)**: exchange KYC, agency action, credible researcher publication. You do not conclude identity from chain data alone. Ever.

State confidence in exactly these tiers in every artifact you produce.

## Evidence pack standard (what handoff targets actually accept)

Per incident, assemble:

1. **Summary page**: what happened, confidence tiers, requested action, your contact + counsel.
2. **Tripwire dossier**: manifest row for the fired lure, full TripwireEvents (append-only store export), calldata pre-images, marker manifest excerpt (your channels stay redacted; the fired marker is shown).
3. **Chain evidence**: tx links across every chain touched, funding-graph export (label nodes as facts vs inferences), relevant explorer archives (self-archived - explorers change).
4. **Offchain evidence**: decoy-page fetch logs, canary DNS hits, self-report payloads, hashes of every artifact with its manifest row.
5. **Impact statement**: what the attacker attempted against whom; funds at risk; why the requested action matters (freeze/blacklist/watch).
6. **Legal framing page**: prepared with counsel - jurisdiction, requested legal route, your standing.

Format discipline: PDF + machine-readable JSON annex; hash the pack; note the hash in your public deterrence signal if you issue one.

## Handoff targets and what they want

| Target | They want | Trigger |
|--------|-----------|---------|
| SEAL 911 (Security Alliance) | Fast coordination, member firm weight | live incident or high-confidence prep |
| Exchanges (abuse/compliance desks) | Evidence pack + freeze request per their format | attacker funds reachable on-ramp |
| Stablecoin issuers (CCTP/USDT freeze paths) | addresses + evidence; they act on THEIR policy | OPS15 interplay; speed matters (Drift CCTP lesson: hours) |
| Law enforcement (IC3 in the US; national cyber units) | full pack, chain-of-custody notes | theft occurred or credible threat |
| Public researchers (zachxbt-class) | raw leads they can independently verify | you want speed + public pressure, accept loss of control |
| Immunefi/bug platforms | only if the touch was actually a whitehat in scope | screen first - decoys occasionally catch GOOD actors |

The last row is not a joke: sometimes your decoy catches a whitehat doing edgy recon or a security scanner. The response ladder's observe-first posture exists for exactly this - escalate only on separation score.

## Public deterrence signals (the D lever)

When and how to go public about a fired tripwire:

- Do: "Our deception program recorded an unauthorized probe targeting [fictional/decoy surface]; evidence shared with [target type]; the wallet is watchlisted." Costly-signal value: proves capability, raises perceived p(identification) for every future attacker.
- Don't: reveal channel maps, decoy counts, or the fiction details (teaches the next attacker); don't name-and-shame below tier-3 confidence; don't signal so often that it reads as marketing (one per campaign).
- The benchmark-solutions thread itself is the template: playful, evidence-flavored, zero case-detail leakage.

## What this file explicitly rules out

- Naming humans without tier-3 evidence (and even then, usually let agencies/researchers carry public attribution).
- Pressuring handoff targets with threats or PR games - evidence quality is your only lever.
- Any "we'll get your money back ourselves" moves: funds recovery is a lawful-channel outcome, not a defender operation.
