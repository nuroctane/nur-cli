# Plays per chain (tripwire mechanics everywhere)

Chain adaptations for the deception layer. Class plays come from `plays-evm.md` / `plays-non-evm.md` / `plays-ops-surfaces.md`; this file is what CHANGES per host chain. Chain facts per `reviewing-major-chain-surfaces` dossiers (refreshed 2026-08; `[verify]` items there apply here too).

## Ethereum mainnet

- Highest agent recon density; every play applies at full strength.
- Tripwire economics favor blob-era low-cost decoy deployments; keep decoy funding dust-level.
- Public-mempool probes are visible to your watchers in real time - wire mempool monitoring to the response ladder (see probe before inclusion, decide whether to let it land on a decoy).
- L1 fork-divergence checks (`anti-simulation-fork-traps.md`) work well; canary-state correlation is strongest here because archives are universal.

## Base

- Sub-second blocks + heavy agent ecosystem (consumer apps): decoys get probed often; telemetry volume is the design constraint (sample, don't stream everything).
- Sequencer outages (Jun 2026 precedent): tripwire watchers must tolerate downtime windows; do not interpret silence during an outage as attack.
- Gas is cheap: attackers can probe broadly - make decoy depth (the maze) carry the cost burden, not individual revert prices.

## Arbitrum One + Orbit chains

- AnyBTC (25/75) deny + retryable mechanics: decoy ticket stores (T28 play) fit the native inbox shapes; alias-confusion decoys (T27) are high-yield given how common the alias bug is.
- Orbit chains (incl. Robinhood Chain): single-operator sequencer means YOUR OWN ops can be a tripwire - sequencer-level flags on decoy-address activity are available to you on your own chain deployments.
- RWA/tokenized-stock rails (Robinhood): deception plays must respect compliance hooks - a decoy policy-engine override request is a beautiful tripwire (only attackers ask for it), but coordinate with compliance counsel before baiting anything that looks like a securities control.

## OP Mainnet + Superchain (Unichain, Ink, World Chain, Soneium)

- Shared stack = shared decoy toolkit; parameterize per chain id. Alias play (T27) identical mechanics.
- Superchain interop (rolling 2025-2026 `[verify]`): cross-chain message decoys (T17/T19 plays) should include interop-path shapes once live.
- Same as Base: cheap probes; depth-based cost imposition.

## Hyperliquid (HyperCore + HyperEVM)

- HyperEVM decoys: the Core<->EVM write-window gating is itself a natural fork/live divergence surface - decoy vaults whose "profitable" path requires a write-window assumption that fails live (P,C).
- HyperCore behavior cannot be decoyed (closed engine): deception lives at the EVM layer and the API/docs layer. A decoy "vault strategy doc" naming a bait HyperCore address works as recon poison.
- JELLY-lesson application: watch for agents probing YOUR socialization/ADL parameters via thin decoy markets; thin decoy perp-ish pools are excellent screens (S) - nobody legit trades them.

## Solana

- Program logs are your tripwire channel; log budgets are real (compute units) - keep decoy logging tight.
- Feature-gate awareness: a decoy program relying on a soon-to-deactivate feature dies silently; audit feature schedule before deploying decoys.
- Account-model decoys (S02/S06 plays) are cheap and effective; ATA-confusion bait (decoy ATAs that only record) catches drainer bots mass-scanning token accounts.
- Drainer-bait dynamics: bait wallets + canary NFTs work identically (SPL flavor); watch memo/transfer-hook abuse shapes on Token-2022 decoys (S07).

## BNB Chain + opBNB

- Historically the densest small-mid attacker pool: decoys yield high signal volume; expect more noise, tune screening thresholds.
- Fork-of-a-fork ecosystem: publish decoy "yield forks" of known-vulnerable lineages (decoy T14/T15 shapes) - agents sweeping forks for the parent's unpatched bug self-identify in hours.
- Fast finality + low gas: sam-first (observe) mode matters; responses to live-attack signatures can be genuinely fast here.

## Linea / Scroll / zkSync Era

- Verifier/escape-hatch decoys (T30/T31 plays) fit the native surfaces; keep decoy verifiers strictly separate from real ones (one address-character off is enough for confusion risk - name-label them clearly in your internal manifest).
- Prover-side deception is out (not defender-ownable); docs-layer poison for circuit docs is the channel instead.
- L1-side decoy router contracts (bridge-lookalikes) catch agents reconning the L1 entry points.

## Sonic / Berachain / Monad / Plasma

- Sonic: gateway dual-path (secure vs classic) makes a natural decoy duality - a decoy path that records.
- Berachain: PoL reward-vault shapes are novel bait surfaces (decoy gauge with bait emission math); the Nov 2025 halt precedent means attackers expect chain-level intervention - deterrence messaging (D) lands harder here.
- Monad: parallel-execution timing assumptions are the decoy gold - decoy paths that only "work" under serial-execution assumptions (P); their sim environments may diverge from production scheduling.
- Plasma: zero-gas USDT rails mean attacker probing is FREE - cost imposition must come from depth/verification friction, not gas; rate-limits and decoy mazes do the work.

## Sui / Aptos

- Object-model decoys: decoy shared objects with "ACL" docs (M02 play); capability-bait modules (M04).
- Events are first-class: tripwire events integrate cleanly.
- Smaller agent populations than EVM/Solana today - fewer, higher-quality decoys beat a broad field.

## TON

- Message-race decoys (O03 play): decoy jetton wallets that record bounce-replays; log-message tripwires.
- Async execution means multi-step decoys can span transactions - a decoy "vault" whose step-2 only arms after step-1 probe catches two-stage attack flows as separate identified events.

## TRON

- Energy/bandwidth delegation economics: decoys that force attacker energy expenditure are nearly free for you; resource-griefing cost plays (C) are the strongest lever on this chain.
- USDT-dominated flows: bait wallets denominated in USDT (TRC-20) fit attacker expectations.

## Cosmos SDK / IBC

- Governance-reachable state makes careless contract decoys risky - prefer doc-layer poison + IBC-packet-shape tripwires (C01-C03 plays via event telemetry).
- Decoy channels/clients on low-value paths only; never near real client trust roots.

## Bitcoin-adjacent

- Limited scripting = deception lives at the wallet/descriptor/docs layer: decoy descriptors (B01 play), decoy Lightning service endpoints, canary UTXOs.
- Watch-only bait wallets with canary spends work perfectly (chain is fully public anyway; the signal is first-touch attribution).

## Cross-chain portfolio rules

1. Deploy the same decoy CLASS across all your deployment chains (T-class parity), with per-chain WATERMARKS - a multi-chain attacker sweep lights up per-channel and per-chain at once.
2. Balancer-Nov-2025 lesson inverted: your decoys should assume the attacker works N-chains simultaneously; watch for identical calldata shapes appearing on sibling chains within minutes.
3. Per-chain telemetry funnels into ONE tripwire schema and ONE response ladder (`identifying-agentic-attackers`).
