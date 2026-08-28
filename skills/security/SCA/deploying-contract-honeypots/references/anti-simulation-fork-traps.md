# Anti-simulation and fork traps (make their sims lie)

The simulation stage is the agent's moment of highest confidence. Divergence between fork-behavior and live-behavior converts that confidence into repeated paid failure. All heuristics below are **environmental oddity checks** - each is individually spoofable and that is fine: the game is ambiguity, not a single magic check (`agentic-defense-game-theory` core, lever 3).

## Honest caveat first

Environment checks can misfire on legit infrastructure (keepers simming, RPC quirks, new client releases). Every check ships with a mode: `OBSERVE` (emit OddityObserved, behave normally) for the first weeks; flip to `DIVERGE` only for paths matching attacker-shaped sequences, never for read paths keepers depend on. This keeps the screening invariant intact.

## Oddity checks (heuristics, with false-positive notes)

| Check | Live-mainnet expectation | Fork/devnet tells | FP risk |
|-------|--------------------------|-------------------|---------|
| `block.chainid` | exact chain id | matches if forked properly (weak alone) | low |
| `prevrandao`/`difficulty` distribution | indeterminate value, never a fixed default | some stacks default fixed (e.g., `0`) | medium |
| `blobbasefee` magnitude | market-driven, varies | frequently a constant default on devnets | medium |
| `block.coinbase` | real proposer address | defaults (zero-ish/special) on dev stacks | medium |
| `blockhash(block.number-1) == bytes32(0)` | non-zero (block exists) | zero on freshly-mined empty local chains | low-medium |
| Precompile presence/behavior (`0x0a` point evaluation, RIP-7212 `0x100` per chain, etc.) | matches chain spec | missing or default-output on generic stacks | medium - see `reviewing-major-chain-surfaces` `references/evm-fork-deltas.md` |
| `gasleft()` consistency across a measured loop | chain-calibrated ratios | dev-machines differ | high (avoid as sole check) |
| `block.timestamp` vs your own observed-last-update delta | plausible monotonic story | forks jumped forward hard | low-medium |
| Codesize of known system contracts (your L2 predeploys, WETH, entrypoints) | exact | defaults/absent on generic forks | low on your own chain |
| State you know only you wrote (unpublished canary slot values) | present | absent on generic forks (they fork public state only) | very low **strongest class** |

The last row is the crown jewel: a canary storage value that exists only in live state (never in any public archive they forked... note: they fork LIVE state too - so its power is as a *correlation* signal combined with timing, not a fork-detector; use for "did they touch the canary" rather than "is this a fork").

## Divergence patterns

1. **Sim-pay / live-revert (P, C).** Decoy pool/view math that pays the caller ONLY when an oddity check says "this is a simulation". Their fork test shows extractable value; the live tx reverts after bribe/gas. Repeat exposure poisons their value estimates (P).
2. **Sim-clean / live-tripwire (I, C).** The reverse: sims run clean, the live attacker-shaped sequence hits a tripwire that records, reverts politely, and wakes the guardian. Use when you want their real tx hash - the strongest identification artifact there is.
3. **Gas attrition (C).** Attacker-shaped sequences traverse increasingly expensive decoy paths. Every failed plan costs real money; the operator's planner eventually deprioritizes your protocol relative to softer targets - which is the point. Deterrence via pure economics.
4. **Plan-splitting (P).** Two decoy paths that contradict each other (one suggests proxy takeover, one suggests oracle spot). Time spent reconciling contradictory recon is time not spent on the real bug.

## Composition example

A lending market with spot-oracle exposure (T05) and an uninitialized-proxy surface (T12):

- Decoy impl (uninitialized, funds-free) with fake `pendingAdmin` slot -> catches takeover tooling (S, I).
- Fork-only-profitable spot-arb decoy pool -> their sim says T05 works; live reverts (P, C).
- Real fix, invisible in their recon because the decoys dominate the "weird surface" budget: sequencer-gated oracle feeds (real defense, no deception needed to also have it).

The equilibrium: attackers cannot distinguish which oddities are decoys and which are the real attack surface, so every audit-grade review they attempt must cover all of it. You have multiplied their required effort with a handful of cheap contracts.

## Ethics/legal reminders

- Divergence must never take a legit user's funds (inertness invariant).
- You are deceiving about your own protocol only.
- Log everything; the response ladder decides reaction, not the contract (`identifying-agentic-attackers`).
