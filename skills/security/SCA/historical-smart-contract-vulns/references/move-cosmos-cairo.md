# Move, Cosmos/IBC, Cairo

Three VMs. Load **one** playbook. Do not apply Solidity reentrancy recipes blindly: Move aborts, CosmWasm uses submessages, Cairo is felt/`u256`.

## Move (Aptos, Sui, Movement)

### Mental model

| | Aptos | Sui |
|---|-------|-----|
| Storage | account resources, `borrow_global` | objects, IDs |
| Auth | signer + resource | owner / shared / frozen |
| Composition | scripts | Programmable Transaction Blocks (PTB) |
| Footgun | wrong address in `borrow_global` | **shared objects have no runtime ACL** |

### M01 Wrong global / wrong object

- **Wrongly trusted:** the resource at `addr` is the protocol's.
- **Seeds:** `borrow_global`, `borrow_global_mut`, `object::id`.
- **Local test idea:** Move unit test calling with a second account that published a lookalike resource.

### M02 Shared object ACL (Sui)

- **Wrongly trusted:** "it is a shared object, so only the module can mutate it." The runtime does **not** ACL shared objects. The module must check sender/capability on every entry.
- **Seeds:** `share_object`, `public entry`, `public`.
- **Local test idea:** PTB: random address calls the shared object's `public` entry. Must abort without a cap.

### M03 Ability leak

- **Seeds:** `has key`, `store`, `drop`, `copy`.
- **Local test idea:** a value with `store` that should be soulbound can be wrapped into an attacker object.

### M04 Capability leak

- **Seeds:** `AdminCap`, `TreasuryCap`, `witness`.
- **Local test idea:** cap cannot be transferred unless documented; `drop` on a cap is a finding if it bricks mint.

### M05 PTB composition

- **Wrongly trusted:** each entry is safe in isolation.
- **Local test idea:** one PTB that deposits, borrows, and liquidates. Ghost conservation across the block.

### M06 Halt / crash / split

- **Analogs:** **Sui temporary shutdown $50k** (F4lt, Immunefi); **Movement Labs chain-split ~$6.7k**.
- **Local test idea:** pathological payloads (huge vectors, nested objects). Client-level: `auditing-blockchain-clients`.

### M07 Overflow / shift-then-mul

- **Analog:** **Cetus 2025 ~$223M** (Sui). Checked math that shifts then multiplies can skip the overflow check.
- **Local test idea:** extreme liquidity add/remove; assert abort or 1:1 conservation. Do not paste the public exploit onto a funded host.

## Cosmos SDK, IBC, CosmWasm

### C01 ICS-20 conservation

- **Wrongly trusted:** voucher mint on dest equals escrow on source (plus fees).
- **Seeds:** `MsgTransfer`, denom trace, `ibc/`, escrow module account.
- **Local test idea:** simapp test: mint without escrow increment fails. Analog family: BNB 2022 mint-via-proof.

### C02 ICS-23 / IAVL (Dragonberry)

- **Wrongly trusted:** unpatched ICS-23 verifies IAVL proofs.
- **Seeds:** `ics23`, IAVL, light client.
- **Review:** go.mod versions of `ibc-go`, `ics23`. After BNB 2022 the ecosystem patched **Dragonberry**. Missing patch on a fork is a finding.
- **Do not** write a proof-forger. Cite the advisory and the module version.

### C03 ibc-hooks / CosmWasm reentrancy

- **Wrongly trusted:** packet timeout / ack cannot re-enter mint.
- **Seeds:** `ibc-hooks`, `Sudo`, `submessage`, `reply`.
- **Analog:** **ASA-2024-007** (ibc-hooks timeout reentrancy / infinite ICS-20 mint; Osmosis TVL cited; patched privately first).
- **Local test idea:** CosmWasm: `reply` on failure path calls `Transfer` again. Must not double-mint.

### C04 Authz / Elderflower

- **Wrongly trusted:** grants are least privilege.
- **Seeds:** `x/authz`, `GenericAuthorization`, `/cosmos.bank.v1beta1.MsgSend`.
- **Local test idea:** grant for one message type cannot execute another.

### C05 AnteHandler / fees

- **Analog:** **Cronos $40k** theft of current-block fees.
- **Seeds:** Ante, fee market, `min-gas-prices`.

### Other Cosmos paid / halt

| Case | $ | Notes |
|---------|-------|
| Evmos docs-driven | $150k | read the spec |
| Sei | $2M+$75k | usmannk + Catchme |
| Axelar halt | $50k | Marco Nunes |
| Injective | $50k (2026 pointer) | f4lc0n |
| Acala (Frontier on Cosmos-adj / Polkadot) | $70k halt | Lastc0de |

## Cairo / Starknet

### K01 Rounding

- **Analog:** **Vesu** rounding convention disclosures (kankodu, Alex / vesu docs).
- **Seeds:** `u256`, `div`, `u256_div`, fee share.
- **Local test idea:** 1 wei deposit/borrow; rounding must favor the protocol as documented, and cannot brick withdraw.

### K02 felt vs u256

- **Wrongly trusted:** `felt252` arithmetic is the same as Solidity `uint256`.
- **Seeds:** `felt252`, `u256`, range checks.
- **Local test idea:** max felt + 1 path; overflow must abort.

### Other Starknet

Account abstraction is native. Session keys: T39-shaped. Bridges to ETH: T17-T19 on both sides.

| Case | Class | Stated loss | Source |
|------|-------|-------------|--------|
| zkLend 2025 | K02 / market | ~$9.6M | [rekt](https://rekt.news/leaderboard) |
| Vesu rounding | K01 | (bounty / disclosure) | vesu docs |

## Next

- Move -> `reviewing-move-modules`
- IBC -> `reviewing-cosmos-and-ibc`
- Cairo -> `reviewing-cairo-and-starknet`
