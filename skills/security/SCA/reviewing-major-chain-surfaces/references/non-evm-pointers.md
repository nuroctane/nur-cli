# Non-EVM pointers: Sui, Aptos, TON, TRON, Cosmos SDK

Quick deltas only. Deep review routes to dedicated playbooks: `reviewing-move-modules` (Sui/Aptos), `reviewing-solana-programs` (SVM), `reviewing-cosmos-and-ibc`, `reviewing-bitcoin-adjacent`.

## Sui

- Move-object model; global storage is objects, not accounts-map - audit ownership graph transitions, shared-vs-owned object races.
- Cetus overflow drain (~$223M, May 2025): integer-mate overflow in CLMM math (M07 class) - Move has checked arithmetic by default but unchecked blocks/wrapping casts still exist; grep `#[allow(unused)]`-adjacent unsafe math, matei overflow patterns in pool math.
- zkLogin identity trust differs from keypairs (OAuth attestation trees) - auth-boundary reviews include JWKS freshness.

## Aptos

- Move 2 semantics differences vs legacy Move (resource scoping changes) - version-pin review per module bytecode published era.
- Chain-split-class whitehat finding ($6.7k Movement-adjacent 2025 row in `hunting-x-linked-bounties`) shows consensus-layer surface is huntable via docs divergence.

## TON

- FunC/Fift actor model with async message passing - reentrancy analog is *message race*, no atomic cross-contract tx guaranteed unless same transaction tree; every two-step flow is a partial-state window (TAC incident 2026 ~$2.8M cross-chain path, O03).
- Jetton standards evolving (TEP-74 + extensions) - transfer_notification callbacks double-fire if dashboards misparse replayed bounces.

## TRON

- EVM-compatible but TVM energy/bandwidth metering diverges heavily; delegate-resource economics create griefing classes invisible on Ethereum (attacker pre-delegating resources to victim calls). USDT-denominated contracts dominate; fee-abstracted approvals change approve-race assumptions slightly (no mempool-equivalent public ordering guarantees identical to Ethereum).

## Cosmos SDK / IBC

- App-chain governance = instant-fork powers usually; code audits must map gov-proposal reachability to contract/module state ("can a proposal call anything?" equivalent).
- IBC packet timeout/receipt handling and relayer-paid gas pins: half-completed packet flows are the norm, not exceptional. 2025 rows: Wormhole $50k messaging crit (marcotnunes), Axelar cross-chain halt $50k - see the paid-payouts table for class mapping.
- Modules using `sudo`/custom msg routers repeatedly show weak allowlist checks vs IBCHandler origin checks (`reviewing-cosmos-and-ibc` covers the walkthrough).

## Cross-cutting note

Loss records for this tier concentrate on two shapes: object/asset validation omissions (Move/SVM families) and trusted-operator/messaging configs (TON/Cosmos bridges). Start there before micro-math sweeps.
