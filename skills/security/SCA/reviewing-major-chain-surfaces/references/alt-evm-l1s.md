# Alt EVM L1s: Sonic, Berachain, Monad, Plasma (+ Polygon PoS, Avalanche C-Chain)

Independent EVM-compatible L1s shipping or resurging 2024-2026. Each changes something executional an Ethereum-tuned audit misses.

## Sonic

- Fantom successor (late 2024). ~sub-second blocks + fee monetization sharing for app deployers.
- SonicGateway: native secure bridge with fraud-window-style failover to classic gateway. Audit item: dual-path redemption logic consistency - assets canonicalized via one path while apps count both.
- Parallel-execution ambitions on roadmap `[verify current]` - same parallelism caveats as Monad below once enabled.

## Berachain

- Mainnet Feb 2025; Proof-of-Liquidity design ties validator stake to ecosystem LP positions (BGT emission direction).
- **Chain-halt precedent (Nov 3, 2025)**: Balancer V2 vault exploit hit BEX (Berachain native DEX is Balancer-tech-forked); validators halted the whole network and hard-forked to trap funds ([CoinDesk](https://www.coindesk.com/markets/2025/11/03/berachain-halts-network-to-contain-balancer-linked-exploit-to-conduct-emergency-hard-fork)). Audit doctrine: social-consensus rollback capability EXISTS and was used - state neutrality assumptions are wrong here; also any Balancer-derived math inherits the exact T04/T15 class drained on six chains that day. Grep forks of Balancer stable-pool invariant math harder.
- PoL adds reward-vault trust graphs unusual elsewhere: gauge/emission accounting bugs = value leaks even when swap math is clean.

## Monad

- Parallel-EVM mainnet Nov 24, 2025 ([Backpack/The Block coverage](https://learn.backpack.exchange/articles/monad-mainnet-launch)). Optimistic parallel execution preserving serial-equivalent results - but timing/scheduling differences still break assumptions imported from sequential chains:
  - Deterministic simulation assumptions about *when* dependencies serialize (same-block interactions behave differently under contention).
  - Reorg/pending-visibility patterns in indexers/drivers.
  - MonadBFT fast-finality (~1s slots claimed at launch `[verify]`) tightens some staleness math instead.
- Keep fuzz suites running with randomized scheduling over dependent pairs - the serialized-devnet-passes / production-races pattern is THE new-class trap here.

## Plasma

- Stablecoin-focused L1, mainnet beta Sept 25, 2025 ([The Defiant](https://thedefiant.io/news/blockchains/plasma-stablecoin-chain-mainnet-beta-tge-launch-date)); zero-fee USDT-transfer pitch, Bitcoin-anchored security narrative.
- Zero-gas rails remove spam-economics defenses: rate-limiting/state-bloat vector shifts from "costly to attacker" to policy-enforced - DoS-by-design-surface review required (`SC10:2026` OWASP).
- Bridge design + custody mix (BTC anchoring mechanics) young; treat canonical asset flows as audit-top items `[details still moving - verify docs]`.

## Polygon PoS

- Legacy high-volume venue, increasingly migrated workload to AggLayer/zkEVM successors `[verify current posture]`. Bor/Heimdall validator-set compromise remains top threat (2021-2022 events); heavy fork debt in older venues. Treat like BNB Chain density-wise.

## Avalanche C-Chain

- Subnet architecture isolates many validators; C-Chain proper vs subnets differ in fee market and validator guarantees. Historical Culinary/Zabu-class small drains plus 2023-era TRON-adjacent ops issues - mostly ops-heavy record.

## Cross-cutting checklist for this tier

1. Which signatures CAN halt or hard-fork your chain? Note in report scope assumptions.
2. Does native DEX math fork Balancer/Uni v2 lineages? Apply matching historical classes.
3. Fee-market deviations (zero gas, shared fees, surcharge models) invalidate gas-grief assumptions elsewhere standard.
4. Parallel or pipelined execution? Add randomized-order tests.
