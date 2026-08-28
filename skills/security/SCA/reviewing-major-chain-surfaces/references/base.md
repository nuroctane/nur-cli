# Base

Coinbase's OP Stack rollup, settled to Ethereum. Highest-throughput consumer L2 by tx count through 2025-2026; the default home for consumer apps, agentic payments experiments, and big retail launches.

## Stack & proofs

- OP Stack rollup; fault proofs live since late 2024 (Stage 1 territory). Sequencer is centralized (single Base-operated sequencer) with the Superchain roadmap for shared/decentralized sequencing `[verify current status]`.
- Sub-second blocks (rollup moved to fast block cadence; apps using `block.timestamp` granularity assumptions at seconds-scale need re-checking) `[verify current ms-level parameters]`.
- Real availability risk: sequencer outages happen. Official postmortem exists for the June 25-26, 2026 block-production outages ([blog.base.dev](https://blog.base.dev/postmortem-june-25th-block-production-outage)). Design your protocol for "sequencer down for hours" not minutes: staleness guards, deadline enforcement, no auto-execution that assumes liveness.

## Execution quirks (OP Stack)

- Same EVM spec as Ethereum mainnet at equal fork levels, plus Superchain hardfork timing (Ecotone/Canyon/Fjord/Granite/Holocene/Isthmus-class upgrades) changes gas metering details between deployments of different vintage - grep for assumptions tied to specific gas costs (`calldata cost`, memory expansion) when porting.
- No mempool worth trusting: public + private flow mixed; naive first-seen-ordering strategies get front-run routinely even at 2s blocks, let alone sub-second ones.
- L1 data fees priced into execution; big calldata saves matter less post-blobs but still show up in user costs, so aggregators batching micro-payments can trip the EIP-7623 calldata-floor interaction (`evm-fork-deltas.md`).

## Canonical bridge & messaging

- Standard OP messenger + portal; withdrawals took ~7 days historically, fault-proof era shortened game windows but honest-exit path still assumes someone challenges - do not build UX on "withdrawals are quick".
- CCTP V2 native routes for USDC are the fast-lane norm on Base now; if your protocol moves USDC cross-chain, attestation-window logic (soft-finality wait times) is the #1 config audit item after KelpDAO's single-DVN lesson (April 2026).

## Oracles

- Chainlink feeds present for majors; Pyth pulls increasingly used for perp-style apps. On ANY OP Stack chain, a consumer reading prices must gate on the **sequencer uptime feed** (grace window after restart) - un-gated readers liquidate users during outages; that has been a repeatable paid finding class across OP stack deployments.

## Host incidents to grep for

- BALD (2023): LP pull rug on Base launch-week pool - owner-trust lesson, T01 class.
- BetterBank reward minting exploit (Aug 2025, ~$5M): distribution/reward accounting letting attacker mint rewards far above real activity - grep reward accounting invariants on Base-native DeFi.
- Balancer V2 vault class-drain hit Base alongside five other chains Nov 2025 - same bytecode deployed N-chains means one math bug is N exploits; check whether in-scope code forks Balancer weight-pool math (constant-invariant rounding assumptions).
- Base itself has never had its core (sequencer/bridge contracts) drained publicly as of 2026-08; still model sequencer compromise as the top host threat (Stage 1 assumption).

## Fork/test notes

- `--fork-url` against any public Base RPC works with Foundry; pin block numbers because garbage-collection pruning shortens recent-state windows on some providers.
- Local Anvil stacks emulating OP fork rules are imperfect around predeploys/gas oracle - tests relying on OP-specific predeploy state (WETH9, GasPriceOracle) should read defaults rather than fork them blindly.
