# Taxonomy (70+ classes + OPS)

Map a finding to **one id**. Then load the playbook. Paid analogs: `hunting-x-linked-bounties`. Loss-side: `incident-timeline.md`. Ingest T-ids: `ingest-class-map.md`.

Tests stay local. Grep seeds are review seeds, not exploit recipes.

## EVM / shared DeFi

| ID | Class | Wrongly trusted | Grep / review seeds | Playbook | Analog |
|----|-------|-----------------|---------------------|----------|--------|
| T01 | Accounting desync | Shares, indexes, reserves match balances | `totalAssets`, `totalSupply`, `index`, `rewardPer` | `reviewing-erc4626-and-vaults` | Belt 2021 bounty; many vaults |
| T02 | Access control | `msg.sender` is the privileged party | `onlyOwner`, `tx.origin`, `initialize`, `setWhitelist` | `reviewing-access-control-and-auth` | Alchemix, Sense, Enzyme $400k |
| T03 | Incomplete path | Happy path is the only path | early `return`, commented `require`, TODO | `triaging-and-deduping-findings` | contest staple |
| T04 | Off-by-one / rounding | Rounding favors the protocol | `mulDiv`, `/ `, `+ 1`, tick bounds | `writing-foundry-invariant-handlers` | Balancer $1M; DFX; Vesu (Cairo) |
| T05 | Oracle / spot | AMM spot or stale feed is fair value | `getReserves`, `slot0`, `latestRoundData` | `reviewing-oracles-and-pricing` | Harvest, Mango, Enzyme oracle |
| T06 | ERC-4626 inflation | First share or donation sets price | `convertToShares`, virtual offset | `reviewing-erc4626-and-vaults` | still a contest winner |
| T07 | Reentrancy (value) | External call cannot re-enter | `safeTransfer`, `call{value`, missing `nonReentrant` | `reviewing-reentrancy-and-callbacks` | DAO 2016; ERC-777; Curve Vyper 2023 |
| T08 | Read-only reentrancy | View price during a callback is safe | `get_virtual_price`, Curve LP | `reviewing-reentrancy-and-callbacks` | many C4 Curve-adjacent |
| T09 | Flash-loan composition | Same-tx price/share is honest | anything that reads then pays | `reviewing-oracles-and-pricing` | Fei, Euler, Cream |
| T10 | Signature replay | `ecrecover` + nonce + chainId | `permit`, `EIP712`, `ecrecover` | `reviewing-signatures-permit-and-eip712` | 2026 DeFiHackLabs `ecrecover(0)` |
| T11 | `ecrecover == 0` | Zero address is not a valid signer | `ecrecover` without zero check | `reviewing-signatures-permit-and-eip712` | classic + still shipped |
| T12 | Uninitialized proxy | Impl cannot be taken over | `initialize`, `upgradeTo`, beacon | `reviewing-upgradeable-proxies` | **Wormhole $10M**; Harvest; Teller |
| T13 | Storage clash / UUPS | Layout and `_authorizeUpgrade` hold | ERC-1967 slots, `proxiableUUID` | `reviewing-upgradeable-proxies` | Optimism known-issue notes 2026 |
| T14 | Liquidation / bad debt | Bonus + oracle cannot mint bad debt | `liquidate`, `closeFactor`, `seize` | `reviewing-lending-and-liquidations` | Compound-forks, Radiant, Sonne |
| T15 | AMM / CL math | Tick, empty pool, fee-on-transfer | `uniswapV3SwapCallback`, `slot0` | `reviewing-amm-and-cl-pools` | **Raydium ticks $505k**; Kyber CL |
| T16 | Callback spoof | Callback `msg.sender` is the pool | `uniswapV2Call`, v3/v4 hooks | `reviewing-amm-and-cl-pools` | contest staple |
| T17 | Bridge message replay | Message id is unique across forks | `lzReceive`, nonce, nullifier | `reviewing-bridges-and-messaging` | Nomad, 2026 chain-nonce gaps |
| T18 | Bridge decoder / amount | Decode cannot inflate amount | `abi.encodePacked`, leftover calldata | `reviewing-bridges-and-messaging` | Qubit, many lock/mint |
| T19 | Messenger auth | Only the real endpoint can mint | `xDomainMessageSender`, trusted remote | `reviewing-bridges-and-messaging` | **Scroll spoof $1M**; Across |
| T20 | Single verifier / DVN | One signer is "decentralized" | LayerZero DVN set, guardian set | `reviewing-bridges-and-messaging` | **KelpDAO 2026 ~$292M** |
| T21 | Governance / timelock | Delay and proposer set cannot be skipped | `queue`, `execute`, `updateDelay` | `reviewing-governance-and-timelocks` | OZ Timelock reentrancy bounty |
| T22 | Flashloan governance | Votes cannot be rented for one block | `getVotes`, snapshot vs current | `reviewing-governance-and-timelocks` | Beanstalk 2022 exploit |
| T23 | Token weirdness | `transfer` moves `amount` | fee-on-transfer, rebase, missing return | `reviewing-token-standard-pitfalls` | 2026 FOT reserve desync |
| T24 | Missing return ERC-20 | `USDT`-like tokens revert or lie | `safeTransfer`, raw `transfer` | `reviewing-token-standard-pitfalls` | USDT on many forks |
| T25 | MEV / slippage | User bound the trade | `minOut`, `deadline`, `amountOutMinimum` | `reviewing-mev-ordering-and-slippage` | sandwich, Dutch skip |
| T26 | L2 sequencer | Feed is live while sequencer is down | `sequencerUptimeFeed`, grace | `reviewing-l2-sequencer-and-finality` | Chainlink L2 docs |
| T27 | L1-L2 alias | `msg.sender` on L2 is the L1 sender | aliasing, `AddressAliasHelper` | `reviewing-l2-sequencer-and-finality` | OP Stack |
| T28 | Retryable / inbox | Retryable cannot be griefed or replayed | Arbitrum inbox, `redeem` | `reviewing-l2-sequencer-and-finality` | **Arbitrum inbox ~400 ETH** |
| T29 | Custom gas token | Accounting uses the right denom | gas token decimals, portal | `reviewing-l2-sequencer-and-finality` | OP custom gas |
| T30 | Forced inclusion | Sequencer cannot censor forever | inbox delay, escape hatch | `reviewing-l2-sequencer-and-finality` | Aztec 2026 escape hatch PoCs |
| T31 | zk proof verification | Verifier rejects forged proofs | `verify`, `proof_id`, public inputs | `reviewing-l2-sequencer-and-finality` | **zkSync Era $50k**; Lite $200k |
| T32 | NFT marketplace | Listing sig and callback are bound | `onERC721Received`, royalty | `reviewing-nft-and-marketplace` | BendDAO flash claim |
| T33 | Cross-function composer | Two "safe" fns in one tx | any pair of state changers | `reviewing-cross-function-and-composer` | 2025 logic-heavy losses |
| T34 | Arbitrary call / helper | Helper cannot `transferFrom` victims | `delegatecall`, `functionCall`, Zapper | `reviewing-access-control-and-auth` | Zapper, xDai Stake, SharedStake |
| T35 | Vyper storage / reentrancy | Compiler version is in the trusted set | `vyper` 0.2.15-0.3.0 | `reviewing-reentrancy-and-callbacks` | Curve 2023 reentrancy |
| T36 | Donation / first depositor | Empty vault share price is defined | `totalSupply == 0`, dead shares | `reviewing-erc4626-and-vaults` | contest + Immunefi |
| T37 | Unchecked call / silent fail | Failed call reverts | `call` without success check | `reviewing-access-control-and-auth` | SWC-104 |
| T38 | `abi.encodePacked` collision | Packed hashes cannot collide | `keccak256(abi.encodePacked` | `reviewing-signatures-permit-and-eip712` | SWC-133 |
| T39 | ERC-4337 / paymaster | Validation phase cannot be griefed or drained | `validatePaymasterUserOp`, session key | `reviewing-signatures-permit-and-eip712` | 2026 DeFiHackLabs 4337 |
| T40 | EIP-7702 delegation | Authority cannot be stolen via self-delegate | `7702`, `delegate` | `reviewing-access-control-and-auth` | 2026 claim-drain titles |

## Solana / SVM

| ID | Class | Wrongly trusted | Seeds | Playbook | Analog |
|----|-------|-----------------|-------|----------|--------|
| S01 | Missing signer | Account is the claimed authority | `Signer`, `is_signer` | `reviewing-solana-programs` | Neodyme class 1 |
| S02 | Account confusion | Type + owner + expected address | `owner`, discriminator | `reviewing-solana-programs` | Cashio-class |
| S03 | Arbitrary CPI | Callee program id is hardcoded | `invoke`, `CpiContext` | `reviewing-solana-programs` | classic drain |
| S04 | PDA non-canonical bump | Bump is the canonical one | `find_program_address`, stored bump | `reviewing-solana-programs` | OtterSec |
| S05 | Sysvar spoof | Sysvar account is the real sysvar | `instructions` sysvar, `load_instruction_at` | `reviewing-solana-programs` | **Wormhole Solana 2022 $326M** |
| S06 | Close / reinit | Closed account cannot be revived hostile | `CLOSE_ACCOUNT`, zero data | `reviewing-solana-programs` | rent / reinit |
| S07 | Token-2022 transfer hook | Hook cannot rewrite amounts hostile | transfer hook program | `reviewing-solana-programs` | Token-2022 |
| S08 | CPI signer propagation | PDA signer seeds are exact | `invoke_signed` seeds | `reviewing-solana-programs` | |
| S09 | Tick / CL math (SVM) | Tick array matches the pool | Raydium / Orca tick | `reviewing-solana-programs` | **Raydium $505k** |
| S10 | Oracle + governance ops | Social + oracle is in the threat model | Drift / Pyth / admin keys | `reviewing-solana-programs` | **Drift 2026 ~$285M** (ops+oracle, not "missing signer") |

## Move / Cosmos / Cairo

| ID | Class | Wrongly trusted | Seeds | Playbook | Analog |
|----|-------|-----------------|-------|----------|--------|
| M01 | Wrong global address | `borrow_global` address is the intended one | Aptos `borrow_global` | `reviewing-move-modules` | |
| M02 | Shared object ACL | Shared object has a runtime ACL (it does **not**) | Sui shared, `public` | `reviewing-move-modules` | Sui design footgun |
| M03 | Ability leak | `key`/`store`/`drop` match the asset | `has key`, `store` | `reviewing-move-modules` | |
| M04 | Capability leak | Admin cap cannot leave the module | `Cap`, `witness` | `reviewing-move-modules` | |
| M05 | PTB composition | One tx of many calls is still safe | Sui PTB | `reviewing-move-modules` | |
| M06 | Validator crash / halt | Pathological tx cannot halt the network | DoS entrypoints | `reviewing-move-modules` | **Sui shutdown $50k** (F4lt); Movement chain-split |
| M07 | Integer overflow / shift-then-mul | Checked math actually checks after shifts | `<<`, `*`, integer-mate | `reviewing-move-modules` | **Cetus 2025 ~$223M** (Sui) |
| C01 | ICS-20 mint conservation | Voucher mint <= escrow | ICS-20, denom trace | `reviewing-cosmos-and-ibc` | BNB bridge 2022 class |
| C02 | ICS-23 proof forge | IAVL proof verifier is patched | Dragonberry, ICS-23 | `reviewing-cosmos-and-ibc` | Dragonberry |
| C03 | ibc-hooks reentrancy | Timeout / ack cannot re-enter mint | `ibc-hooks`, CosmWasm submsg | `reviewing-cosmos-and-ibc` | ASA-2024-007 |
| C04 | Authz / Elderflower | Grant cannot over-authorize | `x/authz` | `reviewing-cosmos-and-ibc` | Elderflower |
| C05 | AnteHandler / fee | Fee payer cannot steal block fees | Ante, fee market | `reviewing-cosmos-and-ibc` | Cronos fee theft $40k |
| K01 | Cairo rounding | `u256` div rounds the documented way | `u256`, `div` | `reviewing-cairo-and-starknet` | **Vesu rounding** |
| K02 | Cairo overflow / felt | Felt vs u256 mix is explicit | `felt252` | `reviewing-cairo-and-starknet` | |

## Bitcoin-adjacent and other VMs

| ID | Class | Wrongly trusted | Playbook | Analog |
|----|-------|-----------------|----------|--------|
| B01 | Script path spend | Tapscript / preimage is the intended path | `reviewing-bitcoin-adjacent` | |
| B02 | Lightning HTLC | Timeout and amount bind both sides | `reviewing-bitcoin-adjacent` | |
| B03 | Stacks Clarity DoS | Pathological Clarity cannot halt | `reviewing-bitcoin-adjacent` | **Stacks Immunefi ~$76k** |
| B04 | BitVM / Babylon | Challenge game actually challenges | `reviewing-bitcoin-adjacent` | research surface |
| O01 | Near NEP-141 mapping | Worthless token cannot mint on Aurora | `bitcoin-and-other-vms.md` | Aurora sanitization |
| O02 | ink! / Frontier EVM | Precompile + `delegatecall` is gated | `bitcoin-and-other-vms.md` | **Moonbeam/Polkadot $1M+** pwning.eth |
| O03 | TON / FunC | Message bounce and opcode are authenticated | `bitcoin-and-other-vms.md` | TAC 2026 bridge |
| O04 | Client / consensus | Node accepts only valid blocks | `auditing-blockchain-clients` | Polygon consensus $75k; Sei $2M |

## Ops / phishing / keys (not Solidity)

Full catalog: `investigator-ops.md`. Playbook: `reviewing-frontend-and-ops-surfaces`.

| ID | Class | Wrongly trusted | Playbook | Analog |
|----|-------|-----------------|----------|--------|
| OPS01 | Safe / multisig UI lie | Signers see the calldata they sign | `reviewing-frontend-and-ops-surfaces` | **Bybit 2025**; WazirX Liminal display |
| OPS02 | Drainer-as-a-service | Connect+sign is a mint | `reviewing-frontend-and-ops-surfaces` | Inferno / Pink / Angel |
| OPS03 | Permit / Permit2 phishing | Typed-data matches the UI | `reviewing-frontend-and-ops-surfaces` | tayvano_ campaign family |
| OPS04 | WalletConnect session | WC origin is the real dapp | `reviewing-frontend-and-ops-surfaces` | fake connect pages |
| OPS06 | Support impersonation + RDP | Google/exchange "support" is support | ops / wallet UX | Genesis creditor 2024 |
| OPS08 | Password-manager keys | LastPass is an HSM | secrets hygiene | Chris Larsen XRP |
| OPS10 | Exchange / bridge keys | n-of-m are unphished | custody, not AMM | Ronin, Harmony, DMM, Phemex, BingX |
| OPS11 | Rug / shill / insider dump | KOL is not a conservation law | tokenomics | BitBoy, Ansem, NFT rugs |
| OPS13 | NFT phishing / setApprovalForAll | Animate-my-NFT site is the market | `reviewing-frontend-and-ops-surfaces` | BAYC 2021 |
| OPS15 | Issuer freeze / CCTP lag | USDC blacklist saves you in minutes | IR policy | Circle Files 2026; Drift CCTP |
| OPS16 | Custodian contractor insider | Vendor is the principal | vendor ACL | USMS 2026 |
| OPS17 | Signer-machine malware | Hardware wallet confirms host display | airgap runbook | Radiant II 2024 |
| OPS18 | Wallet-frontend JS supply-chain | Safe Web JS is the signed binary | `reviewing-frontend-and-ops-surfaces` | Bybit Sygnia |

More ids (OPS05 poison, OPS07 fake Zoom, OPS09 blind sign, OPS12 SIM, OPS14 leftover approve, OPS19 adapter, OPS20 dust): `investigator-ops.md`.

## How to use in a review

1. Label the protocol (vault / AMM / lending / bridge / L2 / SVM / Move / IBC).
2. Walk **only the ids in that row group**.
3. For each id: grep, then a failing local test, or write "not present".
4. File one finding per broken invariant. Do not dump the table into the report.
