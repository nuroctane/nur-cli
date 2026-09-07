# Invariant synthesis (taxonomy + catalog)

Turn code structure into a written invariant catalog before hunting violations. An invariant with every write site enumerated is simultaneously a specification and a bug detector - "the gap is both an invariant and a potential bug."

## Synthesis taxonomy

| Type | How to derive | Watch for |
|------|---------------|-----------|
| Conservation | Delta-pair scan: find paired state changes (mint/burn, deposit/withdraw, lock/release) and assert the pair sums to zero | **Negative conservation**: expected flows that produce ZERO storage delta - where did the value go? |
| Guard-lifting to global | Take a function-local check ("amount > 0") and lift it to a global property ("every state-changing path enforces amount > 0"); verify ALL write sites | Any write site missing the guard = finding candidate |
| Ratio ordering | Share/price/rate relations that must be monotone (never decreased, bounded above/below) | Directions flip on edge states (empty vault, zero supply) |
| State machine vs togglable flag | Distinguish real state machines (ordered transitions, terminal states) from booleans that claim to be one | Flags pretending to be machines: re-init, pause->unpause state loss |
| Temporal / check-then-update | Order of checks, effects, interactions within and across txs | Update-after-external-call windows; cross-tx staleness |
| Cross-contract assumptions | What your code assumes about tokens/oracles/routers that the other side does not enforce | Fee-on-transfer, blacklists, callback reentry, decimals |
| Economic | Derived from incentives: who profits from breaking what, slippage bounds, fee bounds | Cases where breaking the invariant is MORE profitable than honoring it |

## Entry-point classification (feeds the catalog)

Permissionless / role-gated / admin-only - with the standing caveat that an unmodified function with an internal `msg.sender` check IS role-gated. Unclassifiable = treat as permissionless (attacker's assumption).

## Catalog format (keep it during the whole engagement)

```
## I-3 Vault shares never decrease except via withdraw/redeem
- source: conservation, deposit/withdraw pairing
- write sites: Vault.deposit, Vault.withdraw, Vault.redeem, Migration.execute (3/4 guarded)
- gaps: Migration.execute burns without user signature -> finding candidate
- status: confirmed | refuted | unverified
```

`G-N` = global (system-wide), `I-N` = single-contract, `X-N` = cross-contract, `E-N` = economic. Every catalog entry lists ALL write sites with confirmed/unconfirmed marks - grep-verified, not recalled.

## Bridge to testing

Each catalog entry is a test target: property tests via `discovering-protocol-properties`, handler invariants via `writing-foundry-invariant-handlers`, formal targets via `formal-verification-halmos-certora-kontrol`. An entry nobody tests is a lead against the protocol, not against you - note it in the report's coverage section.
