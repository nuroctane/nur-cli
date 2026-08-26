# EVM and L2 (review seeds)

Use with `taxonomy.md` T01-T40. Protocol playbooks stay the source of hunt procedure. This file is **historical pressure**: which classes paid, which lost money, what to grep on L2s.

## L1 Ethereum - extra surfaces (2024-2026)

| Surface | Taxonomy | Grep / review | Local test idea | Analog |
|---------|----------|---------------|-----------------|--------|
| ERC-4337 paymaster / session key | T39 | `validatePaymasterUserOp`, `session`, `UserOp` | validation-phase cannot drain paymaster or skip missing funds; fuzz `UserOp` gas | DeFiHackLabs 2026 titles |
| EIP-7702 | T40 | `7702`, `authorization`, `delegate` | authority cannot be permanently stolen by a crafted authorization list in a local anvil test | 2026 claim-drain titles |
| Permit2 / AllowanceHolder | T10 | `permit`, `AllowanceHolder` | nonce + spender + deadline bound; replay across forks | aggregator drains |
| Uniswap v4 hooks | T16 | `beforeSwap`, `afterSwap`, `unlock` | hook return data cannot mint extra; only pool manager calls hook | contest + Uniswap v4 $15.5M ceiling |
| ERC-20 `approve` race | T23 | `approve(0)` | | classic |
| Beacon / 7702 + proxy | T12+T40 | | do not combine blindly | |

## OP Stack (OP, Base, Superchain, many others)

| Class | Wrongly trusted | Seeds | Analog |
|-------|-----------------|-------|--------|
| T27 alias | L2 `msg.sender` is the L1 EOA | `AddressAliasHelper`, `l1Sender` | |
| T26 sequencer | Chainlink answer is live | `sequencerUptimeFeed`, `grace` | |
| T29 custom gas | Portal accounts native as ETH 18 dec | gas token decimals | |
| T30 forced inclusion | Sequencer will include your tx | deposit inbox delay | |
| ETH duplication | L2 ETH supply vs L1 lock | portal `finalize` | **saurik $2M** |
| Safe uninitialized on L2 | Same Safe as L1, empty init | Wintermute OP 2022 ~$27.6M | Initialize check on every L2 Safe deploy |
| Censorship | Proposer can hide withdrawals forever | output root, proof window | iosiro $20k Optimism |

Local test idea: mock `sequencerUptimeFeed` down for `grace + 1`; liquidation that still runs is a finding if the program promised freeze.

## Arbitrum Nitro

| Class | Seeds | Analog |
|-------|-------|--------|
| T28 retryable | `Inbox`, `retryable`, `redeem`, `submissionFee` | **riptide ~400 ETH** uninitialized inbox |
| Alias | Arb alias vs OP alias (different constant) | |
| Delayed inbox | anyone can grief gas | |

Local test idea: retryable to an uninitialized implementation cannot be taken over; `redeem` cannot mint extra ETH.

## zkEVM / validity (Scroll, Linea, Polygon zkEVM, zkSync Era, Taiko)

| Class | Wrongly trusted | Analog |
|-------|-----------------|--------|
| T31 soundness | Verifier accepts a forged public input | **ChainLight Era $50k**; Lite $200k |
| T19 message spoof | Bridge believes a fake L1 sender | **Scroll $1M** 2025 |
| T30 based vs centralized sequencer | Inclusion policy | Taiko based |
| Proof lag | Oracle used before finality | |

You will not re-prove the circuit in Foundry. Review **bindings**: every message field is in the public inputs; leftover bytes cannot change amount.

## Polygon PoS (not zkEVM)

| Class | Analog |
|-------|--------|
| MRC20 `transfer` without balance | **$2.2M** |
| Plasma / exit double-spend | **~$2M** |
| Heimdall consensus bypass | **$75k** |
| Predicate / log confusion | Asymmetric.re Polygon log confusion |

## Blast / yield-bearing native

Native yield means `balanceOf` and "ETH" diverge. Treat as T01 + T23. Do not price a vault off `address(this).balance` if yield is swept elsewhere.

## Account abstraction on L2

Same T39/T40. Paymasters on L2 inherit sequencer downtime: a paymaster that prices gas with a stalled feed is T26+T39.

## 2026 DeFiHackLabs EVM titles (pattern only)

Uninitialized proxy (Renegade), LayerZero `delegate` / `approveAndCall`, CCTP unverified attestation, fee-on-transfer reserve desync, Uniswap V3 spot used as vault NAV, flashloan meta-vault logic, abandoned Governor. Full list: `incident-corpus.md` year 2026.

## Next playbook

Default: protocol class. If the repo is a rollup / portal / messenger: `reviewing-l2-sequencer-and-finality` then `reviewing-bridges-and-messaging`.
