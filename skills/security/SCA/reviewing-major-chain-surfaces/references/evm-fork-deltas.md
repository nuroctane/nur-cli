# EVM fork deltas and post-Pectra attack surfaces

Read once per engagement. Deltas auditors actually get bitten by, Pectra (May 2025) -> Fusaka (Dec 2025) -> Glamsterdam (next), plus EIP-7702 abuse doctrine.

## Pectra (Prague/Electra, mainnet May 7 2025)

| EIP | What changed | Audit consequence |
|-----|--------------|-------------------|
| EIP-7702 | EOAs can carry delegation-designated code (`0xef0100` prefix) signed per-tx-batch by the EOA key | Biggest new surface; see below |
| EIP-7251 | Max effective validator balance 32 -> 2048 ETH | Staking wrapper accounting shifts; share/rate math off-by-scale bugs |
| EIP-7002 | Execution-layer triggerable withdrawals | Exit-path automation becomes contract-reachable; permission mapping needed |
| EIP-7691 | More blobs per block | Cheaper L2 DA; nothing directly at contract layer |
| EIP-7623 | Calldata floor cost increase | Batching/migration contracts pricing calldata can revert or overspend assumptions |

## Fusaka (Fulu/Osaka, mainnet Dec 3 2025)

- **EIP-7594 PeerDAS**: blob sampling scaling; consensus-layer only, but blob-parameter forks (BPO1 Dec 17 2025, BPO2 Jan 7 2026 - target 10 then 14) move data costs under fee-market sensitive apps ([ethereum.org roadmap](https://ethereum.org/roadmap/fusaka/)).
- **EIP-7934 RLP execution block size limit** ~10 MiB + **per-tx gas ceiling 16,777,216**: mega-tx migrations/batch systems that "always fit" may silently not anymore; check batch size bounds against both limits.
- ETHL1 gas-limit raise trajectory continued through 2026 `[verify current target]`.

## Glamsterdam (next header feature set, H1 2026+ targeting)

- Headliner: **EIP-7928 Block-Level Access Lists** enabling parallel execution groundwork; candidates around it: EIP-7732 enshrined PBS, state-access gas repricing (EIP-8037-class) `[verify what actually lands]`.
- Audit consequence pattern: whenever parallel execution of independent txs arrives L1-wide, spot-check contracts relying on within-block sequential observation of others' effects (same class as Monad notes in `alt-evm-l1s.md`).

## EIP-7702 delegated-EOA attack doctrine (post-Pectra reality)

Delegation indicator account code means ANY address can behave as a contract when its owner signs an authorization tuple. Confirmed threat shapes:

1. **Phishing-signature = root grant.** A tricked signature authorizing a malicious delegator hands total control. Inferno-drainer-class phishing adopted this fast ([GoPlus guide](https://goplussecurity.medium.com/understanding-eip-7702-phishing-attacks-a-comprehensive-guide-to-protection-strategies-for-wallets-8e8372e3d5ea); [Zealynx research](https://www.zealynx.io/research/smart-contracts/eip-7702-wallet-security)). Protocol-side: never treat "signed something" as intent-free; assume drainer UX mimics your own flows.
2. **Uninitialized security-critical storage.** Delegated implementations often skip initializer discipline EOA-flows never had - first-caller-grabs-owner slots patterns recur ([Nethermind writeup](https://www.nethermind.io/blog/eip-7702-attack-surfaces-what-developers-should-know)).
3. **Nonce binding / replay.** Authorization tuples bind to account nonce at sign-time; cross-chain misuse and stale-tuple resurrection need explicit checks in infra wallets.
4. **Delegate-contract bugs.** Public delegator implementations become protocol-relevant dependency code; diff-review them like proxy logic slots.
5. **Recovery UX breakage.** Once an EOA carries malicious code, key-only recovery assumptions die - support playbooks must route through actual chain state ([documented real case](https://ethereum.stackexchange.com/questions/172100/how-to-recover-an-eoa-from-a-malicious-eip-7702-contract)).

Primary references: [SlowMist/DHL deep-dive](https://slowmist.medium.com/in-depth-discussion-on-eip-7702-and-best-practices-968b6f57c0d5), [Halborn considerations](https://www.halborn.com/blog/post/eip-7702-security-considerations), [spec](https://eips.ethereum.org/EIPS/eip-7702).

## Precompile divergence cheat sheet

| Environment | Delta vs Ethereum |
|-------------|-------------------|
| OP Stack family | Matches L1 set at equal forks; extra predeploys exist but none change curve ops results |
| Arbitrum One/Orbit | Gas accounting over some precompiles differs under AVM; historic advisories cover edge behaviors - detector-run, don't assume |
| zkEVM trio (Linea/Scroll/zkSync) | Circuit re-implementations historically diverged on rare inputs - modexp/class edge cases worth targeted tests |
| RIP-7212 secp256r1 (0x100) | Adoption is chain-by-chain `[verify per target]` - passkey-verifying wallet contracts must feature-gate presence before calling |
| HyperEVM | Adds Core-interaction system precompiles with time-window gating; see `hyperliquid.md` |

Solidity language note: releases through 0.8.3x added no semantics that make old audits wrong, but transient-storage usage broadened after the keyword stabilized - grep `tload/tstore` for persistence-assumption bugs when porting across rollups whose fork levels differ `[verify exact versions on release notes]`.
