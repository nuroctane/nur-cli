# Solana notes (ops/config delta)

Deep program-level review belongs to `reviewing-solana-programs`. This file is the chain-ops/config delta auditors need in addition, refreshed 2026-08.

## Network character

- ~400ms slots, optimistic confirmation ~sub-second; halts happen (historically several per year; verify current outage record - liveness is a real prod concern, so time-based locks/"epoch expiry" logic in protocols must tolerate multi-hour pauses).
- Firedancer/RollingFire client diversity progressing through 2025 `[verify current stage]`; single-client incidents are now historic rather than current.
- Feature gates activate/deactivate via SIMD stake-weighted votes; **deactivation of old features has broken programs holding stale expectations**. Before auditing a program, check enabled/deactivated features for the target cluster/epoch - e.g., loader v3/v2 nuances and older syscall behaviors shifting under pinned dependencies.

## Program/runtime deltas that trip audits

- CPI account limit raised (128 -> 256+, later changes) - programs written under the old ceiling may have overflow-safe patterns now unnecessary, or conversely rely on depth limits that changed. Verify against current runtime limits `evm-fork-deltas.md` equivalent here = feature-gate changelog.
- Compute budget defaults + requested heap frame instructions: programs under-budgeted for new defaults behave differently than mainnet-simulated runs from stale devnets.
- Token-22 (Token Extensions): transfer hooks and confidential transfers change replay/reentrancy-equivalent reasoning ("reentrancy" in Solana = CPI callback reentry, same idea). Hook-enabled mints multiply attack surfaces per policy pointer.
- Verified-build reputation systems improved over 2025-2026 (SOURCE file verification, otter/verified-program tooling) - still confirm build reproducibility before trusting off-chain IDLs claimed to match deployed programs.

## Oracle stack differences vs EVM

- Pyth pull-model dominates Solana pricing: consuming apps submit recent price updates (anyone can) - staleness gating checks HER submitted update's publish_time, exploitable if app accepts arbitrary-age submissions under some path (repeat T05 class on Solana). Switchboard pull likewise. Grep `publish_time`, staleness epsilon params first.

## Host incidents (post-2024)

- Mango Markets governance/oracle market-thin governance attack (2022, S10 class) remains the canonical Solana loss anatomy.
- Cashio fake-collateral account (2022, S02): account-validation omissions - the Anchor constraint-discipline lesson driving today's checklists.
- Drift Protocol incident (April 2026): ~$285M loss mixing protocol/ops + oracle factors, plus Circle CCTP mint/unfreeze interplay (~$232M unfrozen ~6h later) - see `investigator-ops.md`; repeatedly miscited as "forgot Signer" which is wrong-doctrine poison.
- Step Finance treasury/phishing-class loss Q1 2026 ~$27M `[writeup variance exists]` - ops family again.

## Test/tooling notes

- `anchor test`/`bankrun` (solana-bankrun, Surfpool-class local clusters) beat full devnet replay for unit work; for incident reconstruction use real RPC snapshots + `program log` capture.
- Emit logs assertions (msg! pattern audits) and event-schema drift matter more on Solana tooling than Solidity reviewers expect - include instruction-data discriminant validation in every diff review (unchecked deserialization = classic custom-program vulnerability class).
