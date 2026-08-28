# OP Mainnet and the Superchain (Unichain, Ink, World Chain, Soneium, ...)

OP Stack chains sharing Optimism's codebase and upgrade cadence: OP Mainnet, Unichain (Uniswap), Ink (Kraken), World Chain (Worldcoin), Soneium (Sony Block Solutions Labs), Base (own dossier), plus Mint/OPLive-class smaller chains. Review once per quirk set, deploy-awareness per chain.

## Stack & proofs

- OP Stack rollups; fault proofs have shipped progressively across members (OP Mainnet first, others trailing). Stage status per chain moves - check L2BEAT before citing a stage in a report.
- Superchain interop (native cross-chain calls within the family) rolled out through 2025-2026 `[verify per chain]`. New audit surface: **cross-Superchain message authenticity** - code that assumed "same bytes on my chain = same intent elsewhere" needs review against interop semantics; preinterop bridges stay the more common exploited surface.
- Shared security one-level-down is still one shared sequencer-set story per chain: single-operator censorship/reorder capability during upgrades remains in threat models until shared sequencing actually replaces it.

## Execution quirks

- Same fork-cadence caveats as Base (`base.md`): Holocene/Isthmus-class hardforks changed gas metering details; old deployments behave differently from new ones under identical bytecode.
- L1->L2 deposits and L2->L1 withdrawals both flow canonical messenger. Address aliasing on inbound messages (identical mechanism to Arbitrum's, offset differs: `0x0202...0202`) - same allowlist-bodge hunting grounds.
- Per-chain genesis/different deployed-standard addresses: never reuse "known" token or predeploy addresses between family members without checking that chain's registry. Wrong-address bugs are a config-review staple on multi-chain Superchain deploys.

## Canonical bridge & messaging

- Standard OP portal withdrawal window (~7 days historically, shortened where fault proofs active). CCTP V2 native routes for USDC spread across family members after 2025 - attestation/finality wait config is the hot checkpost-KelpDAO.
- Third-party messaging with DVN/security-module configs (LayerZero OFTs for wrapped assets across family members): count verifier quorums. One = exploit-shaped (KelpDAO rsETH ~$292M, April 18 2026, Lazarus-attributed).

## Oracles

- Chainlink + Pyth presence varies by member chain; Unichain carries Uniswap-native TWAP truth natively which most members lack. Sequencer uptime feed gating mandatory everywhere (OP Stack standard feed exists).

## Host incidents

- Sonne Finance empty-market liquidation (2024, OP Mainnet): compound-fork market created without proper initial collateral accounting - T14 class; any freshly-created lending market on a member chain deserves an initialization-phase test.
- DGLD bridge exploit via an OP Stack bridge deployment (Feb 23 2026) `[verify specifics]` - proof-validation on bridge copy-deploys again.
- Family-wide Balancer-class exposure Nov 2025 applied to all Superchain DeFi holding those vaults.

## Fork/test notes

- Foundry forks fine against each RPC; parameterize predeploy/token addresses per chain id. `optimism` npm/forge tooling (`foundryup` + `cast` Forge eth optimism guides) helps simulate deposit-tx flows locally via Anvil's op-geth-mode stacks where needed.
