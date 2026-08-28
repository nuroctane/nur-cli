# zkEVM rollups: Linea, Scroll, zkSync Era

Validity-proof rollups. The prover is the sequencer's conscience; the contract surfaces auditors touch most are verifier entry points and escape hatches.

## Common zkEVM audit deltas (vs optimistic rollups)

- **Finality story differs**: soft-confirmation ordering risk first, then batch finality via proof verification on L1. Time-lock/withdrawal logic must distinguish L2-inclusion vs L1-verified; both windows differ per chain.
- Prover-gas mismatch: circuits simulate EVM imperfectly. Historical exploit-shaped gaps live where circuit fee tables or precompile re-implementations diverged from real EVM behavior - assume divergence until chain docs say otherwise; verify against each chain's differences doc.
- Upgrade key = the real god-mode: most zkEVM chains shipped with upgradeable verifier/router sets controlled by security councils + timelocks. Record whose signatures change the verifier, because "valid proof" is only as meaningful as who can swap what a proof proves.
- Escape-hatch code is high-value small-surface logic: forced-exit fallbacks after sequencer failure have their own rare-path bugs. Aztec-class `proof_id` / escape-hatch bugs surfaced in 2026 DeFiHackLabs reproductions - grep that class on any ZK system with emergency exits.

## Per-chain notes

### Linea

- Veloċore exploit (July 2023): operator-key compromise minted bridge tokens; recovered via operator goodwill partially. Standing lesson: sequencer/proposer keys ARE bridge keys here.
- Native token rata + ETH withdrawal flows reopened progressively through mid-2025; proving-system decentralization advanced late 2025 `[verify stage]`.
- Gas model has Linea-specific surcharges (variable gas overhead), breaking fee-assumption contracts ported blind.

### Scroll

- April 2025 whitehat save: message spoofing in cross-domain messaging path caught via Immunefi ($1M payout) - a bridge-auth bug, not circuit math. See `hunting-x-linked-bounties` row. Grep scroll-messenger consumers for domain/auth assumption copies.
- Sequencer-centralized; upgradeable core with council `[verify current]`.

### zkSync Era

- March 2024: attacker exploited a checkorder/validation gap in the `bridgehub`/da path to mint ~$196M of bridged ETH; largest ZK-chain loss; whitehat-disclosure dynamics followed (accounts frozen partially returned).
- Elastic Chain era migrations changed router semantics across 2025 `[verify current topology]` - reviews should pin which epoch the deployed addresses belong to.
- Fee model rounding quirks (L1 component amortization) historically produced dust-drain findings in aggregators.

## Grep seeds for all three

`verifyProof`, `processBatch`, `executeBatches`, `_verifyBridgeTx`, `domainSeparator` inside messengers, `escapeHatch`, `priorityQueue`, upgrade entry points without timelocks.

## Test notes

Fork-test at L1 against RouterZkSync/ZkSync Diamond Proxy contracts; L2-side sim needs chain-specific Anvil forks (`era-test-node`/Linea/Scroll devnets). Replay real proof txs (calldata carving) before claiming verifier-bypass findings - circuit-level claims need formal proof, not fuzz vibes.
