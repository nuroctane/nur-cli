# Finding standard and four-gate judging

The quality bar: a FINDING is a concrete, unguarded, exploitable path. Anything less is a LEAD - and "default to LEAD over dropping" (honest calibration beats inflated reports).

## Every FINDING carries five fields

1. **file, function** - exact location.
2. **root cause** - the one-sentence code-level defect.
3. **minimal fix** - the smallest change eliminating the defect.
4. **proof** - concrete numbers, an execution trace, or quoted code. No proof = LEAD, no exceptions.
5. *(seam lenses only)* **seam** - which lens combination produces the bug.

## The four gates (in fixed order; a failed gate ends evaluation)

### Gate 1 - Attack execution

Read every guard, check, modifier, and constraint on the claimed path. A guard that interrupts before harm = REJECTED (or DEMOTE if a code smell remains) - quote the exact line. Speculative interruptions ("the caller would notice") do NOT count as guards.

### Gate 2 - Reachability

Can the vulnerable state exist in a live deployment? Structurally impossible (protected by an invariant) = REJECTED. Requires privileged action outside normal operation = DEMOTE. Normal usage or common token behavior = clears.

### Gate 3 - Trigger

Can an unprivileged actor execute it? Trusted-roles-only = DEMOTE. Unprivileged and profitable = clears.

**Admin-action special rule**: admin harm requires the admin to act maliciously against documented intent = REJECT outright, not even a LEAD. Clears ONLY if the finding names a concrete unprivileged amplifier:

- **race** - admin changes a value mid-flow; a user exploits the window.
- **retroactive sweep** - admin update rewrites an already-credited pending value.
- **asymmetric formula** - admin output chains into a formula users profit from.
- **access gap** - missing/tautological auth or missing init guard.

### Gate 4 - Impact

Self-harm only = REJECTED. Dust-level without compounding = DEMOTE. Material loss to an identifiable victim = CONFIRMED.

## Confidence scoring

Start 100; deductions: partial attack path -20; bounded non-compounding impact -15; requires specific-but-achievable state -10. >= 80 earns a fix recommendation; below 80, description only.

## Verdict vocabulary

CONFIRMED (finding) | DEMOTE (back to lead) | REJECTED | LEAD (title + smell + what remains unverified; no fix, no score).

## Lead promotions

- **Cross-contract echo**: root cause confirmed in one contract promotes the identical pattern wherever it appears.
- **Convergence**: two or more independent passes flag the same area and the lead was demoted (not rejected) -> promote at confidence 75. Convergence never overrides an actual code path that interrupts the attack.
- **Partial-path completion**: path reachable and unguarded, trace incomplete -> finding at confidence 75, description only.

## Chains

If output of A feeds precondition of B and combined impact exceeds either alone, emit one `Chain: [A] + [B]` finding at the lower confidence. Expect 0-2 per audit; more means you are double-counting.

## Finding stance

You are not defending the code. Judge on what the code ALLOWS, not on deployer intent or how it might be used responsibly. Trust your discomfort when a path "looks" fine.

## Safe patterns (never flag)

- `unchecked` blocks in Solidity 0.8+ (verify the reasoning is documented in the audit trail).
- Explicit narrowing casts in 0.8+ (they revert on overflow).
- MINIMUM_LIQUIDITY burn on first deposit.
- SafeERC20 usage.
- `nonReentrant` (only cross-contract reentrancy angles remain flaggable).
- Two-step admin transfer.
- Protocol-favoring rounding - unless compounding or rounding-to-zero is involved.

## Do-not-report list

Linter/compiler issues; gas micro-optimizations; naming/NatSpec; admin privileges by design; missing events; centralization without an exploit path; implausible preconditions. Exception: fee-on-transfer, rebasing, and blacklisting tokens ARE plausible for contracts accepting arbitrary tokens - check before dismissing token-behavior findings.
