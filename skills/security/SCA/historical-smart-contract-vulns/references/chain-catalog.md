# Chain catalog (L1, L2, VM)

Pick the chain, then open the VM file. Unique classes are **additions** to the EVM DeFi taxonomy, not replacements. If the chain is EVM-equivalent, still walk T26-T31.

This list is for **review routing**. It is not a TVL ranking.

## Ethereum and L2s (EVM + extras)

| Chain | VM / stack | Extra classes | Notes |
|-------|------------|---------------|-------|
| Ethereum | EVM | T01-T40 | Canonical. MEV, 4337, 7702 |
| OP Mainnet | OP Stack | T26-T30 | ETH duplication analog (saurik). Aliasing |
| Base | OP Stack | T26-T30 | same as OP |
| Unichain / World Chain / other Superchain | OP Stack | T26-T30 | custom gas token on some |
| Blast | OP-adj / yield | T26, ops | native yield assumptions |
| Mode / Mantle / Zora / Fraxtal | OP or OP-like | T26-T30 | confirm stack this session |
| Arbitrum One / Nova | Nitro | T28 retryable, T26 | inbox bounty (riptide) |
| Stylus (Arb) | WASM + EVM | client + EVM | rust programs: treat like a new VM |
| Scroll | zkEVM | T19, T31 | **$1M message spoof** |
| Linea | zkEVM | T31 | |
| Polygon zkEVM / Cardona | zkEVM | T31 | Verichains / iczc reports |
| zkSync Era | zkEVM | T31 | ChainLight soundness $50k |
| zkSync Lite | custom zk | T31 | proof verification $200k |
| Polygon PoS | EVM + Heimdall | O04 consensus, Plasma exits | $2.2M MRC20, $2M double-spend, $75k consensus |
| Gnosis / xDai | EVM | T34 | Stake arbitrary call bounty |
| BNB Chain | EVM | T18, forks | 2022 bridge; Belt bounty |
| opBNB | OP on BNB | T26 | |
| Avalanche C-Chain | EVM | T14 | Platypus analog |
| Fuji / other AVAX L1s | EVM or custom | check precompiles | |
| Fantom / Sonic | EVM | T15 | Balancer 2025 included Sonic |
| Moonbeam / Moonriver | Frontier EVM | O02 | $1M truncation |
| Astar | Frontier + zk | O02 | Zellic $50k; truncation family |
| Acala | Frontier | O02, halt | $70k block production |
| Celo | EVM | token dual | |
| Aurora | EVM on Near | O01 | **$6M** infinite spend |
| Evmos / EVMOS-adj | Cosmos EVM | C05, docs | $150k docs bounty |
| Cronos | Cosmos EVM | C05 | $40k fee theft |
| Kava EVM | Cosmos EVM | C* | |
| Sei (EVM era) | Twin / EVM | O04 | $2M+ module bounties |
| Metis / Boba | OVM-legacy / OP | T26 | |
| Immutable zkEVM | zkEVM | T31 | $1M program ceiling cited |
| Taiko | based rollup | T30 | based sequencing assumptions |
| Aztec | zk private | T30/T31 | 2026 DeFiHackLabs escape hatch / `proof_id` |
| Loopring / dYdX (hist.) | zk | T31 | |
| Starknet | Cairo | K01-K02 | Vesu rounding |
| Fuel | Sway | bitcoin-and-other-vms | UTXO-ish |
| Polygon Miden | Miden VM | client | |
| Eclipse | SVM on ETH DA | S01-S10 | Solana classes, ETH DA |

## Solana and SVM

| Chain | VM | Extra | Notes |
|-------|-----|-------|-------|
| Solana | SVM | S01-S10 | Wormhole 2022, Mango, Cashio, Raydium $505k, Drift 2026 ops |
| Sonic / other SVM L2s | SVM | S01-S10 | |
| Kamino (program) | SVM | | $1.5M bounty ceiling |

## Move

| Chain | Model | Extra | Notes |
|-------|-------|-------|-------|
| Aptos | account / `borrow_global` | M01, M03, M04 | |
| Sui | object + PTB | M02, M05, M06 | $50k shutdown bounty |
| Movement | Move + DA | M06 | chain-split bounty ~$6.7k |

## Cosmos / IBC / CosmWasm

| Chain | Extra | Notes |
|-------|-------|-------|
| Cosmos Hub | C01-C05 | ICS-23 Dragonberry |
| Osmosis | C03 ibc-hooks | ASA-2024-007 timeout reentrancy / ICS-20 |
| Injective | C* + orderbook module | $50k 2026; $500k program |
| Sei | O04 + CosmWasm | $2M+ |
| Axelar | C01 halt | $50k Marco Nunes |
| dYdX chain | CosmWasm/orderbook | |
| Neutron / Nolus / many appchains | CosmWasm | submessage reentrancy |
| Celestia | DA, not DeFi VM | light client / blob - client skill |
| Evmos / Cronos / Canto / Kava | EVM + IBC | both catalogs |

## Bitcoin-adjacent

| Surface | Extra | Notes |
|---------|-------|-------|
| Bitcoin L1 Script | B01 | constrained; no EVM reentrancy-by-loop |
| Lightning | B02 | HTLC |
| Stacks | B03 Clarity | Immunefi DoS ~$76k; **AlexLab 2024 ~$4.3M and 2025 ~$16.2M** (rekt) |
| BitVM / BitVM2 | B04 | challenge game |
| Babylon | B04 | staking / slashing scripts |
| RGB / Taproot Assets / ordinals | inscription indexers | indexer trust, not Script loops |
| Rootstock (RSK) | EVM on BTC merge | EVM catalog |
| Liquid | Elements | federated ops |

## Other VMs (do not pretend they are Solidity)

| Chain | VM | Unique review seeds | Analog |
|-------|-----|---------------------|--------|
| Near | Wasm + NEP | NEP-141 mapping | Aurora O01 |
| Polkadot / Kusama | Substrate + ink! | weights, XCM, Frontier | Moonbeam O02 |
| Interlay | Substrate | | pwning.eth $200k |
| Cardano | Plutus | datum / redeemer, double sat | |
| Tezos | Michelson | entrypoint, tickets | |
| TON | TVM / FunC | bounce, opcode, seqno | TAC 2026 |
| Tron | TVM (EVM-like) | fee, `delegatecall` diffs | |
| Hedera | HTS + EVM | token service vs ERC | |
| Algorand | TEAL / AVM | rekey, close remainder | |
| XRPL | Hooks / AMMs | amendment, issuer | |
| ICP | Motoko / Rust canisters | cycle drain, inter-canister | |
| Filecoin FVM | Wasm EVM-adj | actor IDs | |
| VeChain | EVM-like | VTHO | $50k accrual bypass |
| Oasys | EVM | | $200k merkle_bonsai |
| Story | EVM-adj | | $100k WhiteHatMage |
| Q Blockchain | | | $50k Blockian |
| Hyperliquid (L1) | custom | orderbook + bridge | confirm VM before using EVM playbooks |
| Aptos/Sui already above | | | |

## Client / DA / shared sequencing

If the bug is in **geth, reth, Firedancer, rippled, nimbus, op-node, nitro**, use `auditing-blockchain-clients` (or DarkNavy `/client-auditor`). Polygon consensus, Sei, Sui shutdown, Movement split, Acala halt sit on this border.

## Routing cheat sheet

```
EVM DeFi          -> taxonomy T01-T25 + protocol playbook
EVM L2            -> plus T26-T31 + reviewing-l2-sequencer-and-finality
SVM               -> solana.md + reviewing-solana-programs
Move              -> move-cosmos-cairo.md + reviewing-move-modules
IBC / CosmWasm    -> move-cosmos-cairo.md + reviewing-cosmos-and-ibc
Cairo             -> move-cosmos-cairo.md + reviewing-cairo-and-starknet
Bitcoin family    -> bitcoin-and-other-vms.md + reviewing-bitcoin-adjacent
Unknown           -> this file, then official docs, then stop guessing
```
