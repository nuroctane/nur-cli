# Plays per non-EVM attack vector (S, M, C, K, B, O)

Solana/SVM, Move, Cosmos/IBC, Cairo, Bitcoin-adjacent, and other-VM deception plays. Class ids per taxonomy. Non-EVM decoys lean harder on account/object validation bait and log-based tripwires since program logs and events are the telemetry channel.

## Solana / SVM (S01-S10)

| ID | Agent hunts | Decoy play | Tripwire signal | Effect | Exemplar |
|----|-------------|-----------|-----------------|--------|----------|
| S01 | Missing signer checks | Decoy program with a decoy "config update" instruction accepting any writer (records) | Instruction to decoy config without signer | I | Neodyme class 1 |
| S02 | Account confusion (type/owner) | Decoy program whose vault account validation is bait (holds nothing) | Deserialization of foreign account into decoy vault | I,S | Cashio |
| S03 | Arbitrary CPI | Decoy program exposing `invoke` with attacker-chosen callee (records callee) | CPI into recorder from decoy | I | classic drain class |
| S04 | Non-canonical PDA bump | Decoy PDA-derived authority accepted at wrong bump (bait) | Off-bump PDA signature attempt | I | OtterSec writeups |
| S05 | Sysvar spoof | Decoy program reading "instructions sysvar" loosely | Spoofed sysvar account passed to decoy | I,P | Wormhole $326M |
| S06 | Close/reinit revival | Decoy closable account that re-inits into a recorder state | Reinit of closed decoy account | I | rent/reinit family |
| S07 | Token-2022 transfer hooks | Decoy hook that flags hostile rewrites of amounts | Hook invoked with amount-rewriting pattern | I,P | Token-2022 rollout |
| S08 | CPI signer seeds | Decoy PDA-signing instruction with loose seed check | Seed-guessing CPIs to decoy | I | |
| S09 | Tick/CL math (Raydium/Orca forks) | Decoy CL pool program with tick-array bait | Tick-manipulation-shaped swaps on decoy | I,P | Raydium $505k |
| S10 | Oracle+governance ops fusion | Decoy oracle consumer accepting stale Pyth submissions (bait); also see ops plays | Stale publish_time on decoy consumer | I,C | Drift 2026 class |

## Move (Sui/Aptos) M01-M07

| ID | Agent hunts | Decoy play | Tripwire signal | Effect | Exemplar |
|----|-------------|-----------|-----------------|--------|----------|
| M01 | Wrong `borrow_global` address | Decoy module with global-resource bait on a well-known address | Resource access naming decoy address | I | |
| M02 | Shared-object ACL assumption | Decoy shared object "with ACL" (documents say so; runtime has none, records writers) | Unexpected writer to decoy shared object | I,P | Sui footgun |
| M03 | Ability mismatch | Decoy asset wrapper with loose `key/store` bounds | Wrapped-asset mint via decoy | I | |
| M04 | Admin Cap escape | Decoy module that mints a "transferable admin cap" (worthless, recorded) | Cap transfer events on decoy | I,S | |
| M05 | PTB composition tricks | Decoy multi-entry module where a recorded pair triggers | PTB hitting both decoy entries | I,P | |
| M06 | Validator-halting entrypoints | Decoy entry with pathological-input bait (holds no consensus role) | Pathological call patterns to decoy | I,C | Sui shutdown $50k |
| M07 | Overflow after shifts | Decoy math module with shift-then-mul bait (checked for real, records attempts) | Overflow-shaped inputs to decoy | I,P | Cetus ~$223M |

## Cosmos / IBC C01-C05

| ID | Agent hunts | Decoy play | Tripwire signal | Effect | Exemplar |
|----|-------------|-----------|-----------------|--------|----------|
| C01 | ICS-20 escrow over-mint | Decoy transfer module with denom-trace bait | Over-mint attempt on decoy channel | I | BNB-bridge class |
| C02 | ICS-23 proof forgery | Decoy light-client consumer with unpatched-shaped proof path (records) | Forged-proof submission to decoy | I,P | Dragonberry |
| C03 | ibc-hooks reentrancy | Decoy ibc-hooks receiver whose ack/timeout path is bait | Reentry-shaped ack on decoy | I | ASA-2024-007 |
| C04 | authz over-grant | Decoy grant-exec entry with loose authorization (records) | Over-broad exec through decoy | I | Elderflower |
| C05 | AnteHandler/fee theft | Decoy fee-module hook (testnet-adjacent deployments only) | Fee-stealing tx shapes at decoy | I | Cronos $40k |

## Cairo / Starknet K01-K02

| ID | Agent hunts | Decoy play | Tripwire signal | Effect | Exemplar |
|----|-------------|-----------|-----------------|--------|----------|
| K01 | u256 division rounding drift | Decoy AMM contract with rounding-direction bait | Rounding-exploit calls on decoy | I,P | Vesu |
| K02 | felt252/u256 confusion | Decoy math entry accepting mixed-width inputs (records) | Width-mismatch probes | I | |

## Bitcoin-adjacent and other VMs B01-B04, O01-O04

| ID | Agent hunts | Decoy play | Tripwire signal | Effect | Exemplar |
|----|-------------|-----------|-----------------|--------|----------|
| B01 | Tapscript spend-path confusion | Decoy descriptor/policy file "leaked" (watermarked) naming bait paths | Spend attempting bait path from decoy watch wallet | I | |
| B02 | Lightning HTLC bugs | Decoy invoice/hold flows with bait preimage handling | Malformed HTLC probes to decoy node | I | |
| B03 | Clarity DoS entrypoints | Decoy Clarity contract with pathological-input bait | Pathological calls to decoy | I,C | Stacks ~$76k |
| B04 | BitVM/Babylon challenge games | Decoy challenge contract with bait assertion path | Fake-challenge submissions | I | research surface |
| O01 | NEP-141 mapping (Aurora-class) | Decoy bridge token with mapping bait | Over-mint via decoy mapping | I | Aurora sanitization |
| O02 | ink!/Frontier precompile+delegatecall | Decoy precompile-gate module with loose gate (records) | delegatecall through decoy gate | I,S | Moonbeam-class $1M+ |
| O03 | TON message bounce auth | Decoy jetton wallet accepting unauthenticated bounces (records via log) | Bounce-replay to decoy wallet | I,P | TAC 2026 |
| O04 | Client/consensus bugs | Not decoyable safely; route to `auditing-blockchain-clients` + bug bounty instead | n/a | - | Sei $2M class |

## Cross-VM notes

- Telemetry channels differ: EVM events; Solana program logs + events; Move events; Cosmos SDK events; TON log messages. Normalize all of them into the same tripwire schema so the response ladder stays single (`identifying-agentic-attackers`).
- Decoy deployment cost/benefit is best on chains where agent recon density is high and deploy cost is trivial (EVM L2s, Solana). On Cosmos appchains, prefer doc-layer poison over contract decoys (governance powers make careless decoys risky).
