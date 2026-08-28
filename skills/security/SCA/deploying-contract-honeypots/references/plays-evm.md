# Plays per EVM attack vector (T01-T40)

Deception plays keyed to the pack taxonomy (`historical-smart-contract-vulns` `references/taxonomy.md`). Columns: what the agent hunts at recon, the decoy play, the tripwire signal, effect codes (C/I/D/P/S), exemplar analog the class is known by. Deploy per `decoy-primitives.md` invariants - decoys are inert, separate from prod, and disclosed to your own auditors.

## Accounting, access, math, oracles (T01-T11)

| ID | Agent hunts | Decoy play | Tripwire signal | Effect | Exemplar |
|----|-------------|-----------|-----------------|--------|----------|
| T01 | Share/index desync: `totalAssets` vs balances, reward indexes | Decoy vault with index variables readable but never consistent under write pressure; attacker "desyncs" a decoy | Decoy vault `Touched` event with caller | I,C | Belt bounty; many vaults |
| T02 | `onlyOwner`/`initialize` surface, role docs | Decoy role-registry with an "owner" that is a canary; role-grant calls recorded | Role-grant to decoy registry | I | Alchemix, Enzyme $400k |
| T03 | Incomplete paths: early returns, TODO-shaped gaps | Ship a decoy "partial migration" contract whose unfinished function is the bait | Call to decoy's unfinished fn | I,P | contest staple |
| T04 | Rounding direction bugs in mulDiv/tick bounds | Decoy pool with rounding documented as favoring users (bait) and instrumented math | Rounding-exploit-shaped calldata on decoy | I,C | Balancer $1M-class |
| T05 | Spot/stale oracle reads (`slot0`, `latestRoundData`) | Fork-profitable decoy spot pool (divergence pattern 1); sim pays, live reverts | Live revert on decoy pool + calldata hash | P,C | Harvest, Mango |
| T06 | ERC-4626 inflation on empty vaults | Decoy empty vault with virtual-offset missing (bait), fully instrumented | First deposit-after-donation pattern on decoy | I,P | recurring contest winner |
| T07 | Reentrancy via external calls | Decoy contract with a callback that re-enters a recorder (not a value contract) | Reentrant arrival at recorder | I | The DAO; Curve/Vyper |
| T08 | Read-only reentrancy on price views | Decoy `getRate` view wired to a callback-observable flag | `getRate` called during a callback window | I,P | many C4 findings |
| T09 | Flash-loan composition profit | Decoy flash-lender whose repayment accounting is bait | Loan opened+closed on decoy with exploit-shaped path | C | Fei/Euler-era |
| T10 | Signature replay (missing nonce/chainId) | Decoy claim contract accepting "signed" withdraws with replayable domain | Two identical sigs to decoy | I | 2026 ecrecover(0) titles |
| T11 | `ecrecover == 0` accepts junk | Decoy permit-style entry with zero-sig accepted (bait) | Zero-input sig on decoy | I,S | classic, still ships |

## Proxies, lending, AMM (T12-T18, T23-T24, T36)

| ID | Agent hunts | Decoy play | Tripwire signal | Effect | Exemplar |
|----|-------------|-----------|-----------------|--------|----------|
| T12 | Uninitialized proxy/impl takeover | Decoy UUPS impl (uninitialized, funds-free) whose `initialize` grants a worthless "admin role" and records caller | `initialize` on decoy proxy | I,S | Wormhole $10M class |
| T13 | Storage clash in upgrades | Decoy impl with wrong layout mapping attacker writes onto tripwire slots | Write lands in canary slot region | I,P | 2026 OP-stack notes |
| T14 | Liquidation/bad-debt math | Decoy market with bait bonus params and no real collateral | Liquidation-shaped calls on decoy market | I,C | Radiant/Sonne |
| T15 | AMM/CL tick math | Decoy CL pool with tick boundary bait | Empty-pool/tick-bound probes | I,P | Raydium $505k; Kyber |
| T16 | Callback sender spoof | Decoy pool-lookalike that accepts swap callbacks from anyone (records) | Callback to decoy from non-pool | I,S | contest staple |
| T17 | Bridge message replay | Decoy message app on a test-message bridge path with non-unique ids (bait) | Duplicate message id replay attempt | I | Nomad; 2026 nonce gaps |
| T18 | Bridge decoder inflation | Decoy lockbox whose decode accepts packed leftovers (bait) | Overlong-calldata mint attempt on decoy | I,P | Qubit-class |
| T23 | Fee-on-transfer/rebase desync | Decoy pair seeded with a FOT-looking token | FOT-shaped deposit on decoy | I | 2026 FOT desync rows |
| T24 | Non-standard ERC-20 returns | Decoy router path "supporting" USDT-like tokens loosely | Failed-return transfer attempts on decoy | I | USDT-on-forks |
| T36 | First-depositor share price | Same as T06 decoy; distinct trigger = pure donation | Donation-then-tiny-deposit on decoy | I,P | Immunefi paying class |

## Signatures, composers, governance (T19-T22, T33, T37-T40)

| ID | Agent hunts | Decoy play | Tripwire signal | Effect | Exemplar |
|----|-------------|-----------|-----------------|--------|----------|
| T19 | Messenger auth gap (mint from "trusted remote") | Decoy messenger endpoint with documented-but-never-real trusted remote | Cross-domain call naming decoy remote | I | Scroll spoof $1M |
| T20 | Single-DVN/verifier quorum in YOUR messaging config | Publish a decoy "verifier set" doc describing a 1-of-1 config that does not exist; instrument the endpoint it names | Endpoint named in decoy doc receiving proofs | I,P,D | KelpDAO ~$292M |
| T21 | Timelock delay skip | Decoy timelock with `updateDelay` bait | Delay-update calls on decoy timelock | I | OZ bounty |
| T22 | Flashloan governance | Decoy governor with snapshot-vs-current confusion (bait) | Same-block vote+execute on decoy | I,C | Beanstalk |
| T33 | Cross-function composition | Decoy contract with two individually-safe state changers whose pair is recorded | Pair-sequence detection on decoy | I,P | 2025 logic losses |
| T37 | Unchecked call silent-fail | Decoy helper ignoring call results (bait) | Helper-path call on decoy | I | SWC-104 |
| T38 | `encodePacked` collisions | Decoy sig-boundary contract using packed hashes | Collision-shaped inputs to decoy | I | SWC-133 |
| T39 | 4337 paymaster grief/drain | Decoy paymaster with validation-phase bait; sponsor policy that records | UserOp naming decoy paymaster | I | 2026 4337 titles |
| T40 | EIP-7702 delegation abuse | Decoy delegator contract "leaked" via watermarked doc; authorization tuples to it recorded | Authorization naming decoy delegate | I,D | 2026 claim-drain titles |

## Bridges/L2/zk/NFT (T25-T32, T34-T35)

| ID | Agent hunts | Decoy play | Tripwire signal | Effect | Exemplar |
|----|-------------|-----------|-----------------|--------|----------|
| T25 | Missing slippage bounds | Decoy aggregator router without minOut (bait) | Sandwich-shaped swaps on decoy | I | sandwich family |
| T26 | Sequencer-uptime blind spot | Decoy feed consumer without uptime gate (bait); REAL one stays gated | Price-consume during outage window on decoy | I,P | Chainlink L2 class |
| T27 | L1->L2 alias confusion | Decoy inbox-lookalike with alias-broken auth | Alias-mismatched sender to decoy | I,S | OP Stack |
| T28 | Retryable grief/replay | Decoy Arb-inbox-lookalike ticket store | Stale-ticket redemption on decoy | I | Arb inbox ~400 ETH |
| T29 | Custom gas token denominations | Decoy portal with gas-denom bait | Wrong-denom portal calls | I | OP custom-gas |
| T30 | Forced-inclusion/escape hatch | Decoy escape-hatch module (this class produced real 2026 PoCs - decoy must be clearly separate) | Hatch-path calls on decoy | I | Aztec 2026 PoCs |
| T31 | zk proof verification gaps | Decoy verifier with `proof_id` handling bait | Forged-proof probes on decoy | I,P | zkSync Era/Lite |
| T32 | NFT listing sig binding | Decoy marketplace fill path with loose listing sig | Replay-shaped fills on decoy | I | BendDAO-class |
| T34 | Arbitrary-call helpers | Decoy zapper with `functionCall` passthrough (bait) | `transferFrom`-shaped helper calls | I | Zapper/xDai-class |
| T35 | Vyper 0.2.15-0.3.0 reentrancy | Decoy Vyper module compiled from the cursed band, isolated, funds-free | External-call reentry into decoy | I | Curve 2023 |

## Usage rules

1. One row in play = one decoy contract + one event schema + one response-ladder entry. No shotgun seeding.
2. Rotate: a decoy touched once has taught you what you needed; retire or re-bait it.
3. When a tripwire fires, the play's job is done at identification - everything after belongs to `identifying-agentic-attackers`.
