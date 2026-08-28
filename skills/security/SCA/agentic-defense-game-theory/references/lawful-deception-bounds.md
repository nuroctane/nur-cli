# Lawful bounds of deception and sabotage-by-misdirection

"Sabotage the attackers" decomposes into lawful moves and unlawful ones. This file is the boundary. It is not legal advice; get counsel for any program touching cross-border attribution or fund freezes.

## Lawful (defender-owned surfaces, attacker self-selection)

1. **Deceive about your own protocol.** Decoy functions/contracts you deploy, decoy docs on your domain, bait wallets you own, watermarked files you "leak" on your own infra or repos you control.
2. **Observe and record.** Telemetry from tripwires on your surfaces: caller addresses, calldata, funding trails, self-reported identifiers (model/harness strings an agent volunteers).
3. **Impose cost passively.** Make bait deep enough to waste attacker time/compute; make simulations diverge from live; add verification friction to decoy content. Burning an attacker's API budget by making them crawl a decoy maze is cost imposition, not attack.
4. **Corrupt their planning data.** False-but-plausible layouts, ABIs, runbooks, addresses. They self-selected by trusting attacker-mode recon.
5. **Protocol self-defense.** Pause, guardian sweeps of YOUR protocol, blacklists inside YOUR protocol, fee/burn mechanics your users agreed to in ToS and code. Document all of it in your security model.
6. **Handoff.** Full evidence packs to law enforcement (FBI IC3 for US victims, national equivalents), exchanges via their abuse desks, SEAL 911 coordination, zachxbt-style public researchers, Immunefi/tracker escalations.

## Unlawful / do-not-do (even against confirmed attackers)

1. **Hack-back**: accessing attacker infrastructure (their bots, servers, API keys) without legal authority. Computer-misuse laws do not have a "they started it" carve-out.
2. **Taking attacker funds or private theft-flavored "irony"**: you may joke about "resend payment"; you may not build a mechanism that steals. Drainer-aesthetics = deterrent copy only. (Confiscation happens via lawful freeze paths - issuer blacklists, protocol guards, court orders.)
3. **Deploying malware/web-exploits at attacker-controlled clients**, even "defensive" ones.
4. **Spoofing third parties**: never impersonate another protocol, an exchange, or a person. Deceive only as yourself or your own decoys.
5. **Entrapment-adjacent overreach with innocents**: a decoy must not be reachable by a plausible legitimate action. If a keeper, indexer, or ordinary user could trip it, redesign (see screening below).
6. **Defaming misidentified parties**: attribution is probabilistic; publish allegations only with evidence strength stated, or hand off privately first. Wrong-address accusations have real victims.
7. **Deceiving your own users/auditors**: the deception set must be disclosed (at least to your auditors, security council, and in your security model doc) even while hidden from attackers. Covert-from-insider deception is how insiders abuse decoy powers later.

## Screening rule (innocent-safety invariant)

For every lure, write the legitimate actor who could most plausibly touch it, and make the lure reject-or-divert them:

| Lure | Plausible innocent | Invariant |
|------|--------------------|-----------|
| Decoy admin function | Multisig ops mistake | Decoy lives in a separate decoy contract, never in prod logic; calling it records + reverts politely |
| Bait wallet keys in fake .env | Random security scanner | Wallet holds nothing; first movement is logged, not drained "back" |
| "Leaked runbook" doc | Human whitehat doing recon in good faith | Doc ends with a lawful-use notice + real contact; biting it is not punished, just logged |
| Fork-divergent view function | Legit keeper bot | Divergence only in paths requiring attacker-grade sequences (value extraction shapes), never in read paths keepers depend on |
| Decoy "profit pool" | MEV searcher doing honest arb | Pool math never actually pays anyone; searcher wastes gas, nobody loses funds |

## Disclosure duty (short form)

Security model gains one section: "This protocol deploys deception surfaces against unauthorized agentic activity: tripwires, decoys, watermarked bait. Legitimate users are unaffected; auditors hold the full decoy manifest." That paragraph does double duty: insider-abuse prevention and a deterrence costly-signal you can cite when it fires publicly.

## Evidence hygiene

Tripwire logs are future exhibits: timestamped, hashed, append-only where possible, minimal PII. Chain of custody starts at the tripwire event schema - define it before deploying lures (`identifying-agentic-attackers` `references/tripwire-telemetry.md`).

## Precedents to know

- Cyber-deception is an established discipline: MITRE Engage catalogs adversary-engagement approaches for defenders `[verify current edition]`; canary-token practice (Thinkst-class) is mainstream and lawful when on your own assets.
- Safe Harbor frameworks (see `battlechain-safe-harbor-whitehat`) exist to protect *defenders* acting within scope during live incidents - read its bounds before any mid-incident countermove.
- The benchmark-solutions-style agent honeypot (2026) was run publicly by defenders and discussed openly - the norm is emerging that deceiving agents on your own surfaces is accepted; doing so while impersonating others is not.
