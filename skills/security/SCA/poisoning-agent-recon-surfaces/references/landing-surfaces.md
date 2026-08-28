# Landing surfaces (be where they land)

Each surface: what agents do there, the decoy shape, and the innocent-safety note. Deploy the shapes that match YOUR recon profile (inventory method: `agentic-defense-game-theory` `references/attacker-loop-map.md`).

## 1. Search-indexed docs (the benchmark-solutions surface)

- **Decoy shape**: "internal" pages on your docs domain with plausible paths (`/ops/runbooks/timelock-rotation.md`, `/admin/legacy-note`, `/infra/rpc-endpoints-prod`). Indexed (robots allows, sitemap includes), never linked from public nav.
- Content: consistent fictional ops detail - fake timelock addresses (your decoy contracts), fake rotation schedules, a "verify access" step that phones home (a canary URL fetch).
- Innocent safety: human visitors get the same content plus a lawful-use footer with your security contact; nothing executes; nothing is real infrastructure.
- Effect: I (fetch telemetry + self-reported access step), P (fake addresses dominate their plan).

## 2. robots.txt and sitemap lures

- `Disallow: /admin-console/` for a path that does not exist except as a canary page. Agents that treat robots as a treasure map fetch it; fetch = logged, watermarked per channel.
- Keep it to 1-2 entries; robots-lures are the most publicly-discussed trick and pattern-matched agents may flag them - ambiguity budget matters (`game-theory-core.md` lever 3).

## 3. Verified source comments (explorer layer)

- Comments in your VERIFIED source are read by agents constantly. A neutral comment ("legacy migration notes moved to internal wiki") plus a plausible path is a quiet pointer into surface 1.
- Never put bait INSIDE prod logic comments; pointers only. Auditors review prod source - keep it clean and honest there.

## 4. Decoy repos and leaked-file lures

- A repo on your org (or a decoy org) that looks like an abandoned internal tool: README, half-real code, and the prize - a committed `.env` / `secrets.example` / pem with watermarked fake keys (`deploying-contract-honeypots` `references/bait-funds-and-canaries.md` bait wallets).
- Alternative distribution: gist-class pastes "leaked" in places agents index for `<protocol> private key`, `<protocol> seed phrase`.
- Innocent safety: keys correspond to bait wallets with nothing in them; repo has a security.txt + lawful-use notice.

## 5. Explorer labels and decoy contracts

- Name-label your decoy contracts plausibly ("ProtocolX: LegacyVault [DEPRECATED]") via explorer label/verify metadata you control. Agents weight labels heavily.
- Pair with plays-evm decoys: the label is the pointer, the contract is the tripwire.

## 6. Decoy infrastructure endpoints

- Decoy RPC endpoints (your own domain) that behave like slightly-misconfigured nodes for known attacker query shapes; log everything (OPS19 play).
- Decoy subgraph/graphql with bait entity shapes (fake treasury accounts with bait balances).
- Decoy admin panels (air-gapped data, watch-listed logins) on lookalike subdomains you own - OPS01/OPS06 plays.

## 7. Governance/forum-layer poison

- A forum post that looks like an internal test "ignore - staging proposal" with bait execution details. Agents scraping governance for executable proposals find a recorded decoy.
- Keep this rare: governance-layer poison can confuse your own community; get comms sign-off, label subtly for humans (the post footer can be honest without signaling attackers: "internal test artifact - do not execute").

## 8. Third-party aggregator surfaces

- You do not control them; you CAN feed them: publish (via your own channels) the same watermarked decoy metadata aggregators scrape (labels, tags, docs mirrors). Expect drift; watermark everything so downstream copies still identify their origin artifact.

## Anti-patterns

- Poison that impersonates someone else (unlawful; also destroys trust if discovered).
- Bait paths linked from your main nav (poisons innocents; fails screening).
- Static poison left for years (attacker communities archive and share "known fake" lists - refresh).
- Poison contradicted by your own honest docs in ways a human would flag as deliberate (keep the fiction orthogonal to real facts, not contradictory: never state false things about real contracts).
