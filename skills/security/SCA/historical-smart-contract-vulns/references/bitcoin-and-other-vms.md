# Bitcoin-adjacent and other VMs

Do not run the ERC-4626 playbook on Lightning or TEAL. Use this file to **not misclassify**.

## Bitcoin Script (L1)

Bitcoin Script is not a general VM. No unbounded reentrancy loops, no ERC-20 `approve`. Bugs look like:

| Class | Wrongly trusted | Review |
|-------|-----------------|--------|
| B01 path spend | Any party can satisfy the tapscript | preimage, CSV/CLTV, Musig2, miniscript policy vs actual script |
| Amount / sighash | Sighash covers the intended outputs | `SIGHASH_NONE` / `ANYONECANPAY` surprises |
| Upgrade / taproot | Hidden script paths | every leaf in the tree |

**Local test idea:** btcdeb / rust-miniscript unit test: attacker key cannot satisfy the policy. No mainnet sweep.

DMM Bitcoin 2024 ~$305M is **exchange ops** (Immunefi scoreboard). Not Script.

## Lightning

| Class | Wrongly trusted |
|-------|-----------------|
| B02 HTLC | Timeout, hash, and amount bind both channels |
| Watchtower | Counterparty cannot steal if you are offline without a watchtower (product finding, not always in-scope) |
| Routing | onion vs advertised policy |

Local test idea: lnprototest / spec test for revoked state broadcast. Do not grief live channels.

## Stacks (Clarity)

- **Analog:** Immunefi **DoS ~$76k** (Catchme). **AlexLab** 2024 ~$4.3M and 2025 ~$16.2M (rekt leaderboard) - Clarity/protocol, not Bitcoin Script.
- **Seeds:** `unwrap!`, unbounded maps, `as-contract`.
- **Local test idea:** Clarinet test for a call that exhaustes resources the miner still includes.

## BitVM / BitVM2 / Babylon

- **B04:** the challenge game must be winnable by the honest party in the documented window.
- Review the **on-chain scripts + off-chain protocol** together. A missing challenge path is the finding.
- Babylon staking/slashing: who can slash, with what proof, on which fork.

## RGB / ordinals / Taproot Assets

Indexers and "who owns this inscription" are the trust. Bugs are usually **indexer / marketplace / metaprotocol**, not Script loops. NFT marketplace playbook only if there is an EVM wrapper.

## Rootstock / Liquid / Fed sidechains

RSK is EVM: use T01-T40 plus federated bridge T20. Liquid: federation ops + Elements Script.

## Other VMs (seeds only)

| VM | Seeds | Analog / note |
|----|-------|----------------|
| Near Wasm | NEP-141, `predecessor_account_id` | Aurora O01 $6M family |
| ink! / Substrate | `#[ink(message)]`, weights, XCM, precompiles | Moonbeam $1M truncation; Interlay $200k; Astar Zellic |
| Cardano Plutus | datum, redeemer, `checkPhase1` | double-satisfaction |
| Michelson | entrypoint, tickets, `FAILWITH` | |
| TON FunC | `op::`, bounce, `seqno`, `check_signature` | TAC 2026 TON/EVM bridge |
| Tron TVM | similar to EVM; energy; `delegatecall` diffs | Coinsbuy 2026 mixed |
| Hedera HTS | token vs contract id | |
| Algorand TEAL | `RekeyTo`, `CloseRemainderTo`, inner tx | |
| XRPL Hooks | issuer, amendment | |
| ICP canisters | inter-canister call, cycles | |
| FVM | actor id vs Ethereum address | |
| Fuel Sway | UTXO predicates | |
| VeChain | VTHO | **$50k** accrual bypass |
| Oasys | EVM | $200k |
| Story | EVM-adj | $100k |
| Hyperliquid | custom L1 | confirm before Solidity grep |

## Client bugs (not contract)

Polygon consensus $75k, Sei $2M, Sui shutdown $50k, Movement split, Acala halt, Firedancer comps. Skill: `auditing-blockchain-clients`.

## Next

`reviewing-bitcoin-adjacent` for B01-B04. For EVM-on-X (RSK, Tron, VeChain, Oasys, Story) use EVM playbooks **plus** this chain's extra row.
