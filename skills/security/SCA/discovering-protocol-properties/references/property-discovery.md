# Property discovery (classes, sources, bars, patterns)

## The three property classes

| Class | Definition | Example |
|-------|-----------|---------|
| Global | Holds across the whole system, checked independent of any single call | `totalShares == sum(userShares)`; solvency: assets >= liabilities |
| Function-specific | Tied to one handler call | `deposit` never decreases `totalAssets`; `redeem` respects allowance |
| Delta-based | Before/after comparison via snapshots | User balance decreased by exactly the deposited amount; fee accrual delta == expected |

Naming rule: all property functions carry a `property_` prefix and live in one `Properties.sol` - grep-able, countable, reviewable.

## Three discovery sources (properties are derived, not brainstormed)

### 1. Source-driven reading

Read target contracts + generated handlers and note every sentence you could say starting with "should always" or "must never". Each sentence is a property draft; most drafts die, some become the campaign's core.

### 2. State-pattern scanning (ghost-variable candidates)

- **Accumulative operations**: deposits, withdrawals, mints, burns, transfers -> running totals (`totalDeposited`, `totalWithdrawn`).
- **Counter operations**: user counts, position counts, id counters -> monotonicity and upper-bound properties.
- **Extreme values**: max/min amounts observed (capture via max/min in handlers) -> "no action ever exceeded X" and boundary-replay properties.

### 3. Exposed-state enumeration (snapshot candidates)

Key balances (`balanceOf` on tokens and contracts), protocol aggregates (`totalSupply`, `totalAssets`, `totalShares`), current actor's balances. Public getter -> direct snapshot. Private state, no view -> minimal harness inheriting the target adding only the getters needed. Unobservable cumulative behavior -> ghost variables in a `Ghosts` struct (`totalDeposited`, `lastTimestamp`).

## Quality bars (what keeps suites fast and diagnosable)

1. **No speculative ghosts**: a ghost variable exists only if at least one property needs it.
2. **Harness restraint**: build an inheriting harness only when strictly necessary - prefer public interface access.
3. **Snapshot discipline**: snapshot only where a delta property exists; snapshots cost gas and slow every run.
4. **Revert-safety**: snapshot reads must never revert; wrap fallible calls in try/catch.
5. **Success-gating**: ghost updates go AFTER the external call so they record only successful effects; in clamped-forwarding handlers, updates live in the unclamped version (all paths flow through it).
6. **Diagnosability**: assertion helpers emit descriptive failure events before `assert(false)` - a property failure you cannot interpret is wasted coverage.

## Canonical patterns

### Delta pattern (order matters: snapshot -> call -> snapshot -> ghost -> property)

```solidity
function vault_deposit(uint256 assets, address to) public asActor {
    snapshotBefore();
    vault.deposit(assets, to);
    snapshotAfter();
    ghosts.totalDeposited += assets;
    property_depositIncreasesShares(assets);
}
```

### Cumulative-conservation pattern

Ghost totals accumulate in handlers; a global invariant compares ghost sums against protocol state: `ghosts.totalDeposited - ghosts.totalWithdrawn == vault.totalAssets()` (modulo fees - encode the fee model explicitly or exclude it loudly).

### Structural pattern

One `State` struct + a private snapshot function capturing actor and protocol fields symmetrically before/after - one shape, reused by every delta property, keeps comparisons honest.

## Handoff

Properties + ghosts + snapshots -> `writing-foundry-invariant-handlers` (wire-up) or `fuzzing-with-echidna-and-medusa` (campaign). Record per-property: source perspective, first-failure seed, and whether failure was a real bug or a modeling gap.
