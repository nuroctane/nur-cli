# Arbitrum One and Orbit chains

Arbitrum One: the largest DeFi TVL rollup and home of restaking/lending liquidity. Orbit (RaaS): the stack any team can spin into its own appchain - Robinhood Chain is the flagship 2026 example; every Orbit chain inherits the notes below plus its own operator risk.

## Stack & proofs

- Nitro-based optimistic rollup. Permissionless validation/fault proofs live on One since 2024; sequencing centralized (One) with Timeboost-style express-lane auctions as the MEV-auction design `[verify current Timeboost parameters]`.
- Block times ~250ms on One; Orbit chains can be faster (Robinhood at 100ms). Any `block.timestamp >= deadline` style logic gets coarse; anything assuming ordering across a user's actions must tolerate same-timestamp reordering.
- Stylus (WASM contracts callable by EVM contracts, same storage space) adds an entire second audit surface on chains that enabled it: Rust/WASM ABI, gas model differences, reentrancy across WASM/EVM boundaries. If in-scope code touches Stylus, treat it as two VMs sharing one state.

## Execution quirks (auditor-visible)

- **Address aliasing**: L1-to-L2 sender addresses are offset (`+0x1111...1111`) when called through the inbox. Message-auth checks against "which L1 contract sent this" that forget aliasing either break or (worse) get bodged with allowlists - grep `alias`, `l1ToL2`, `applyAlias`, hardcoded sender comparisons.
- Arbitrum gas oracle predeploy (0x4c70...) prices L1 calldata; refund/bounty patterns interacting with it can be gamed around rapid L1 gas swings.
- No L1 mempool replay semantics for L2 txs; retryable tickets have lifetime rules (redemption windows, expiry refunds) - unexpired stale tickets executing later than expected has bitten bridge routers; check ticket expiry handling.
- Precompile divergence: several precompiles behave differently under AVM gas accounting, and `blake2f`-class calls cost differently; historic Arbitrum advisories cover `EXTCODESIZE`-family quirks on old forks - run detectors rather than assume mainnet parity.

## Canonical bridge & messaging

- The canonical router/inbox/outbox set settles to Ethereum. Custom Gateways exist per token - permissioned gateways with operators who can pause is normal, but read what deposits trust (an operator-minted withdrawal path is only as sound as that operator).
- Orbit chains: everything above applies twice - the Orbit chain also trusts its parent (usually Arbitrum One) settlement. Two-hop fraud-window thinking required: message fraud window on One + challenge window to Ethereum.

## Oracles

- Chainlink coverage strong on One; data-streams pull-model feeds used by perps. Sequencer-outage gating again mandatory for uptime-feed consumers. On fresh Orbit chains, feed coverage is thin -> apps quietly use internal TWAPs of thin pools instead; hunt `getReserves`-class reads there first (T05/T09 class lives here).

## Host incidents / precedents

- Radiant Capital multiple incidents (2024): lending logic on One + $50M+ compromise via compromised signer hardware/ops (`investigator-ops.md`).
- Arbitrum DAO/governance drama (constitution, treasury debates) - governance-heavy systems on One inherit those vote-power concentration lessons; see `reviewing-governance-and-timelocks`.
- Nothing public has drained core One infra as of 2026-08; treat validator-set compromise + sequencer censorship as the standing host threats.

## Fork/test notes

- Foundry supports Arbi-forks natively for most cases (gas specifics approximate); tests depending on ArbSys/ArbGasInfo predeploys should assert against chain behavior, not Anvil defaults.
- For Orbit deployments pin each chain's RPC separately; shared-bytecode protocols should keep one test matrix parameterized over all deployment targets (the Balancer-Nov-2025 multi-chain lesson).
