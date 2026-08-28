# Ethereum mainnet (baseline)

The reference EVM. Every other dossier here is a diff against this one.

## Stack & finality

- Pure L1, no sequencer, no bridge trust beyond deposit/withdraw semantics.
- Finality via Casper FFG; epochs finalize after two checkpoints. Reorgs past ~2 epochs are effectively unheard of in practice, but never treat 1-2 slot reorgs as impossible (MEV-boost relay partitions have produced them).
- ~12s slots; blobs for DA since Dencun (EIP-4844); Fusaka-era blob pricing parameters below.

## Execution surface auditors must know

- Full opcode set including all Cancun additions (mcopy, transient storage tload/tstore, MCOPY gas) and Prague additions on top.
- Precompiles at L1 are the superset most chains approximate: modexp, bn254 pairs, blake2f, POINT_EVALUATION (0x0a, KZG), BLS12-381 curve ops (0x0b-0x0d, from EIP-2537). `evm.codes` is the authority; test precompile-dependent contracts on the actual target chain because Orbit/zkEVM/others diverge (see each dossier).
- No tx-size cap historically -> since Fusaka EIP-7934 there is both an RLP block-size limit (~10 MiB with beacon margin) and a per-tx ceiling of 16,777,216 gas (2^24). Contracts doing giant batches or onchain migrations should no longer assume "one huge tx always fits".
- MEV: private orderflow (Flashbots Protect-class) exists but inclusion is never guaranteed; anything safety-critical cannot rely on private ordering.
- Account abstraction on EOAs changed character after Pectra: any account can now carry code (EIP-7702). See `evm-fork-deltas.md` - "is this address an EOA?" is no longer a safe assumption anywhere, including mainnet.

## Oracles

- Chainlink automation + feeds have deepest coverage here; TWAP-viable pools exist for majors. Staleness/duration conventions differ per feed; check `updatedAt` vs your protocol's own max-staleness parameter every time.

## Canonical infra to trust carefully

- The beacon deposit contract is the only sacred singleton; everything else (rETH, stETH, LBTC class wrappers, restaking receipts) has upgradeability and operator risk - review those like any proxy system.

## Host incidents worth grepping for

- The DAO (T07 textbook reentrancy, 2016), Parity multisig `selfdestruct` (T12, 2017).
- Euler donate-and-self-liquidate (T14, 2023, funds returned) and Curve/Vyper reentrancy-via-compiler (T35, 2023).
- Balancer V2 composable-stables rounding/math drain landed on Ethereum too (Nov 2025, ~$128M across six chains simultaneously - multi-chain same-bytecode blast radius).
- Term Finance governance takeover Aug 24 2026 (~$8.5M, minutes): governance tokens taken over then treasury drained before guardrails could act - grep `vote()`, `execute()`, quorum math, and any voting-power snapshot you can buy in one block.

## Fork-test notes

- `forge test --fork-url <eth-rpc>` works directly; pin a specific block for reproducibility.
- Stateful fuzzing over forks is slow on full history - use narrowed `vm.roll` / prank patterns around the contract under test instead of replaying epochs.
