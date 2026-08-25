---
name: fuzzing-with-echidna-and-medusa
description: "Property-based fuzzing of Solidity with Echidna (Trail of Bits) and Medusa (Crytic) as a complement to Foundry invariants. Use when forge invariant handlers are not enough or the repo already has Echidna properties."
---

# Fuzzing with Echidna and Medusa

Foundry invariants are the default in this pack (`writing-foundry-invariant-handlers`). Use Echidna/Medusa when:

- the repo already has `echidna.yaml` / Medusa config
- you need coverage-guided or assertion-mode fuzz beyond `forge test`
- you are following Trail of Bits [building-secure-contracts](https://github.com/crytic/building-secure-contracts) property templates

In-scope, local, no broadcast. Same conservation laws as the Foundry handler skill.

## Tools

- Echidna: `pip install echidna` or the Crytic binary. Properties are `echidna_*` functions that return `bool` (or assertion mode).
- Medusa: Crytic's Go fuzzer, often faster corpus/coverage. YAML config, assertion or property mode.
- Both read compilation via crytic-compile (Foundry, Hardhat, solc).

## Steps

### 1. State the properties

Reuse the English laws from the handler skill. Prefix:

```solidity
function echidna_solvency() public view returns (bool) {
    return address(vault).balance >= vault.totalDeposits();
}
```

Or assertion mode (`assert` inside the property) if the config says so.

### 2. Constrain like a handler

Unconstrained Echidna will hammer `withdraw` until everything reverts. Use:

- `crytic_constructor` / deploy wrapper that sets up actors
- input bounding inside the property contract
- `filterFunctions` / Medusa `fuzz.testing.onlyFunctions` so you do not fuzz `upgradeTo`

### 3. Run

```bash
echidna . --contract VaultEchidna --config echidna.yaml
medusa fuzz --config medusa.json
```

Start with a short timeout, then lengthen on the hottest properties (solvency, share conservation).

### 4. Turn crashes into Foundry tests

When Echidna prints a shrinking sequence, copy it into `test/Replay_*.t.sol`. Researchers file Foundry tests; corpus traces are not a report.

## Expected output

Config + property contract, a run summary (passes / counterexamples), and any counterexample replayed as a Foundry unit test. Next: `contest-and-bounty-reporting` if a property fails on in-scope code.
