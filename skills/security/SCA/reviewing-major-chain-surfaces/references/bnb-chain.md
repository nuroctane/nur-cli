# BNB Chain and opBNB

Still a top loss venue by incident count (small/mid protocol hacks) even as headline TVL moved to Ethereum rollups. Low fees + history of permissive forks = densest forking debt anywhere.

## Stack & finality

- PoSA validator set (short list, opsec-critical - the 2023 bridge epoch-crossing episode showed what happens when keys cluster). Fast finality now measured in ~1.9s-block / sub-second-finality territory after Lorentz/Maxwell-era hardforks `[verify current block time]`.
- opBNB: OP Stack L2 on top, same Superchain-family caveats (`op-mainnet-superchain.md`) applied to its operator set.

## Execution surface

- EVM close to Ethereum spec at matching fork levels; gas token BNB (18 decimals, not ether-priced - any hard-coded ETH price assumption in pricing/admin-cost logic is wrong here).
- Aggressive state pruning culture -> archive-free default RPCs complicate long-window fork tests; use archive endpoints or pin blocks fast.
- Memepool behavior + sandwich economics still very active despite severity-driven MEV changes elsewhere.

## Bridge/config lessons rooted here

This chain produced the canonical trusted-root failures:

- **BNB Bridge 2022 (~$570M forged mint)**: IAVL proof verification accepted forged leaf - T18/T02 textbook. Grep any in-scope "verify inclusion" logic against this shape.
- Crosschain bridge vault epochs (Oct 2023, mostly-recovered in-chain halt): validator-set key clustering across hub security.
- Countless Multichain/Synapse-style wrapped-asset flows routed via BNB legs degraded quietly when MPC operators vanished (2023).

## Oracle notes

Chainlink coverage exists but many BSC-native venues rely on PancakeSwap TWAPs built over concentrated pools of varying depth - the thin-pool-manipulation playground. Liquidity-aware TWAP windows or nothing (T05/T09).

## Host incidents worth grepping

Uranium/Spartan/Cream-BSC era flashloan drains (2021), plus constant small-mid forks: every "fork of a fork with changed constant" is where rounding/token-weirdness classes hide (T04/T15). qBridge/Klaytn-class token-bound config bugs recur across 2024-2026.

## Test notes

Foundry forks fine; check `--fork-block-number` recency because RPC providers prune aggressively. Keep one test matrix covering both BNB mainnet + opBNB if deployed to both (shared-bytecode multi-chain blast radius lesson).
