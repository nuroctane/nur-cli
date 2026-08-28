# Genuinely new attack surfaces (post-2024)

What 2025-2026 added that pre-2025 checklists do not contain. Each: the surface, why it exists, hunt seed, playbook pairing.

## 1. Delegated EOAs via EIP-7702

Any address can carry signed-in delegation code; drainer kits productized it within months of Pectra. Phishing-signature-as-root, uninitialized storage, nonce/replay, delegate-contract dependency bugs. Full doctrine: `reviewing-major-chain-surfaces/references/evm-fork-deltas.md`. Pair with `reviewing-signatures-permit-and-eip712` + `investigator-ops.md` for wallet-side incidents.

## 2. Verifier-quorum / security-module config in messaging layers

The bridge successor class: not message *code* but message *policy*. KelpDAO rsETH moved ~$292M through a 1-of-1 DVN config (Apr 2026). LayerZero DVN sets, CCIP allowed-offramps, CCTP attestation windows, Wormhole guardian-set assumptions - COUNT QUORUMS and diff-config per deployment chain. Hunt: wherever a config struct names verifiers/quorums, ask "what fraction is compromise?" Pair with `reviewing-bridges-and-messaging`.

## 3. Same-bytecode multi-chain blast radius

Balancer V2 drained on six chains from one math bug. Any repo deployed N-chains needs one test matrix over all targets (fork-level divergence + fee-model quirks change rounding paths). Hunt: constant tables/gas assumptions conditioned on chainid or silently divergent fork levels.

## 4. Governance-speed exploitability

Term Finance (~$8.5M, Aug 24 2026): full takeover in minutes. Auditable properties now include: vote-power acquisition latency (flash-vote still possible?), execution delay after quorum, pause-authority reachability, guardian multisig opsec. Pair with `reviewing-governance-and-timelocks`.

## 5. Shared-loss / insurance-run dynamics

JELLY on Hyperliquid showed mark-squeeze -> socialization -> insurance-bank-run as one design failure; HLP outflows post-incident are part of the vulnerability surface. Hunt: any protocol whose worst case mutates OTHER users' positions (ADL watersharing, socialized bad debt). Pair with perp/lending playbooks + `hyperliquid.md`.

## 6. RWA / tokenized-securities rails

Robinhood Chain-class tokenized-stock L2s: market-session-aware oracle freshness, issuer policy hooks (freeze/force-transfer), redemption custody boundary, equity-vs-crypto basis windows where perps/lending wrap stock tokens. Hunt: staleness params that assume crypto markets never close; transfer-hook authority maps. Pair with `robinhood-chain.md` + `reviewing-token-standard-pitfalls`.

## 7. Parallel-execution timing deltas

Monad-class optimistic parallel EVMs (and L1 BALs groundwork): serialized-devnet-passing assumptions race in production around same-block dependent txs. Hunt: read-then-write across contracts within block, relayer/indexer visibility assumptions. Pair with `alt-evm-l1s.md`.

## 8. AI-accelerated attacker ops

Drainer UX/AI-assisted phishing industrialization + Slither-MCP-class defender tooling arriving simultaneously. Consequence for reviews: social-engineering-resistant flow review (signature meaning clarity) is now in scope for ANY user-facing contract (`reviewing-frontend-and-ops-surfaces`).

## Not new, resurging (do not let juniors skip)

Reentrancy variants incl. read-only (SC08 dropped but alive), ERC-4626 inflation pairs (Sherlock rows keep paying), thin-market governance/oracle fusion (Mango lineage), proxy init-grabs now *cheaper* under 7702.
