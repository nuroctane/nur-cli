# SWC map (frozen 2020) plus living replacements

[SWC Registry](https://swcregistry.io/) is useful as a **shared ID language**. It has not been maintained since 2020. Always cross-check Solodit, EthTrust, and SCSVS.

Living replacements:

- [EEA EthTrust Security Levels](https://entethalliance.org/specs/ethtrust-sl/) (Solidity-specific, maintained)
- [SCSVS](https://github.com/ComposableSecurity/SCSVS) (process + testing standard)
- [Solodit](https://solodit.xyz/) (real contest findings, current)

| ID | Title | What to look for | Nur playbook |
|----|-------|------------------|--------------|
| SWC-100 | Function default visibility | Missing `external`/`public`/`internal`/`private` on pre-0.5; today: accidental `public` | `reviewing-access-control-and-auth` |
| SWC-101 | Integer overflow/underflow | `unchecked`, pre-0.8, mulDiv order | `writing-foundry-invariant-handlers` |
| SWC-102 | Outdated compiler | Ancient solc, known optimizer bugs | Foundry gate |
| SWC-103 | Floating pragma | `^0.8.0` vs pinned `0.8.26` | Foundry gate |
| SWC-104 | Unchecked call return | `call`/`send`/`transfer` without check; non-standard ERC-20 | `reviewing-token-standard-pitfalls` |
| SWC-105 | Unprotected ether withdrawal | missing access control on `withdraw`/`sweep` | `reviewing-access-control-and-auth` |
| SWC-106 | Unprotected SELFDESTRUCT | `selfdestruct` on impl/proxy | `reviewing-upgradeable-proxies` |
| SWC-107 | Reentrancy | CEI broken, ERC-777/721 hooks, read-only | `reviewing-reentrancy-and-callbacks` |
| SWC-108 | State var default visibility | unlabeled storage | Foundry gate |
| SWC-109 | Uninitialized storage pointer | struct in storage without init (old solc) | Foundry gate |
| SWC-110 | Assert violation | `assert` for input checks (should be `require`) | Foundry gate |
| SWC-111 | Deprecated functions | `suicide`, `sha3`, `callcode`, `throw` | Foundry gate |
| SWC-112 | Delegatecall to untrusted | user-supplied impl | `reviewing-upgradeable-proxies` |
| SWC-113 | DoS with failed call | push payments, unbounded refunds | `reviewing-reentrancy-and-callbacks` |
| SWC-114 | Transaction order dependence | approve race, auction sniping | `reviewing-mev-ordering-and-slippage` |
| SWC-115 | tx.origin auth | phishable | `reviewing-access-control-and-auth` |
| SWC-116 | Block values as time | `block.timestamp` for randomness or long delays | `reviewing-mev-ordering-and-slippage` |
| SWC-117 | Signature malleability | ECDSA s-value, compact sigs | `reviewing-signatures-permit-and-eip712` |
| SWC-118 | Incorrect constructor name | pre-0.4.22 name-as-constructor | Foundry gate |
| SWC-119 | Shadowing state variables | child shadows parent storage | `reviewing-upgradeable-proxies` |
| SWC-120 | Weak randomness | `blockhash`/`prevrandao` for payouts | `reviewing-oracles-and-pricing` |
| SWC-121 | Signature replay | missing nonce/chainid/deadline | `reviewing-signatures-permit-and-eip712` |
| SWC-122 | Lack of signature verification | ecrecover == address(0) | `reviewing-signatures-permit-and-eip712` |
| SWC-123 | Requirement violation | `require` on invariant instead of input | Foundry gate |
| SWC-124 | Write to arbitrary storage | assembly sstore, array underflow | `reviewing-upgradeable-proxies` |
| SWC-125 | Incorrect inheritance order | C3 linearization / storage clash | `reviewing-upgradeable-proxies` |
| SWC-126 | Insufficient gas griefing | relayer gas stipend | `reviewing-mev-ordering-and-slippage` |
| SWC-127 | Arbitrary jump with function type | user-supplied function pointer | Foundry gate |
| SWC-128 | DoS with block gas limit | unbounded loops over users | `reviewing-reentrancy-and-callbacks` |
| SWC-129 | Typographical error | `=+` vs `+=` | Foundry gate |
| SWC-130 | Unexpected ether balance | `this.balance == X` assumptions | `reviewing-erc4626-and-vaults` |
| SWC-131 | Unused variables | dead state | Foundry gate |
| SWC-132 | Unexpected ether | `address(this).balance` used as accounting | `reviewing-erc4626-and-vaults` |
| SWC-133 | Hash collisions (abi.encodePacked) | dynamic types packed together | `reviewing-signatures-permit-and-eip712` |
| SWC-134 | Hardcoded gas | `call.gas(2300)` vs stipend changes | `reviewing-reentrancy-and-callbacks` |
| SWC-135 | Code with no effects | unused return | Foundry gate |
| SWC-136 | Unencrypted private data on-chain | "secret" in storage | `onchain-read-recon` |

Economic classes SWC barely covers (use Solodit + Nur playbooks): oracle manipulation, ERC-4626 inflation, read-only reentrancy, sequencer downtime, cross-chain message replay, governance delay bypass, fee-on-transfer, donation attacks.
