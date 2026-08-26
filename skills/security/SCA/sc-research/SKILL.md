---
name: sc-research
description: "Router for whitehat smart-contract security research. Use for DeFi audits, Immunefi/Code4rena/Cantina/Sherlock contests, invariants, oracles, proxies, bridges, lending, AMMs, vaults, and responsible disclosure. Lawful/in-scope only. Never load the whole pack."
---

# Smart-contract security research (whitehat)

Nur's DeFi researcher pack. **Authorized targets only**: your Foundry project, a contest repo, or a published bug-bounty program whose scope you have read this session.

Do **not** load every playbook. Pick one. Offensive steps are Foundry tests you own or have written permission to run - never live drain scripts, never unpublished 0-days against production, never unlocked funded wallets.

The 817 cybersecurity pack's "Crypto" domain is cryptography, not DeFi. Use this router instead.

## How to use

1. Confirm scope (contest README / Immunefi assets / your `src/`). Out of scope = stop. Read `references/disclosure.md`.
2. Optional: `references/grep-patterns.md` then `references/bug-classes.md` to pick a class. Paid Immunefi/X analog: **one** slice of `hunting-x-linked-bounties`. Historical incident or non-EVM VM: **one** slice of `historical-smart-contract-vulns` (never the whole library).
3. `skill(action=read, name=<kebab-name>)` for **one** playbook below.
4. PoCs are **local Foundry tests**. No broadcast of attack txs.
5. File via `contest-and-bounty-reporting`.

Companion spine: `auditing-foundry-smart-contract-security` is the pre-deploy PASS/FAIL gate (Slither + Aderyn + optional Mythril + fuzz/invariants + key hygiene). Any high/critical static finding, failing test, leaked key, or thin coverage on value-moving code = **FAIL**.

## Playbooks (load one)

### Process

| User ask | Skill |
|----------|--------|
| pre-deploy Foundry audit / Slither / Aderyn / forge test | `auditing-foundry-smart-contract-security` |
| triage detector spam | `triaging-and-deduping-findings` |
| Immunefi / C4 / Sherlock / Cantina writeup | `contest-and-bounty-reporting` |
| Immunefi/X paid bounty / "what class paid" / satya0x / saurik payout | `hunting-x-linked-bounties` (one reference file) |
| learn / Ethernaut / DVD / Updraft | `researcher-gym-and-curriculum` |
| public incident -> regression test | `reconstructing-public-postmortems` |
| BattleChain / Safe Harbor | `battlechain-safe-harbor-whitehat` |
| older Slither+Mythril-only | `analyzing-ethereum-smart-contract-vulnerabilities` (legacy) |

### Protocol classes

| User ask | Skill |
|----------|--------|
| historical bugs / SWC / Solodit | `reviewing-with-solodit-and-swc` |
| bounty/hack library / what class was Wormhole | `historical-smart-contract-vulns` (one reference file) |
| paid Immunefi table / X bounty intel | `hunting-x-linked-bounties` then the Playbook column |
| zachxbt / tayvano_ / was this even a contract bug | `historical-smart-contract-vulns` `references/investigator-ops.md` |
| Solana / Anchor / SVM | `reviewing-solana-programs` |
| Aptos / Sui / Move | `reviewing-move-modules` |
| Cosmos / IBC / CosmWasm | `reviewing-cosmos-and-ibc` |
| Cairo / Starknet | `reviewing-cairo-and-starknet` |
| Bitcoin Script / Lightning / Stacks / BitVM | `reviewing-bitcoin-adjacent` |
| wallet UI / Safe module / Permit2 phishing / Bybit-class | `reviewing-frontend-and-ops-surfaces` then `investigator-ops.md` |
| ERC-4626 / vault / inflation | `reviewing-erc4626-and-vaults` |
| AMM / CL / Uniswap callback | `reviewing-amm-and-cl-pools` |
| lending / liquidation / CDP | `reviewing-lending-and-liquidations` |
| bridge / LayerZero / messenger | `reviewing-bridges-and-messaging` |
| governor / timelock | `reviewing-governance-and-timelocks` |
| ERC-20/721/1155 weird tokens | `reviewing-token-standard-pitfalls` |
| permit / EIP-712 / ecrecover | `reviewing-signatures-permit-and-eip712` |
| reentrancy / hooks / callbacks | `reviewing-reentrancy-and-callbacks` |
| onlyOwner / tx.origin / initialize | `reviewing-access-control-and-auth` |
| minOut / deadline / sandwich analysis | `reviewing-mev-ordering-and-slippage` |
| L2 sequencer / alias / finality | `reviewing-l2-sequencer-and-finality` |
| NFT marketplace | `reviewing-nft-and-marketplace` |
| two functions, one tx | `reviewing-cross-function-and-composer` |
| Chainlink / TWAP / spot | `reviewing-oracles-and-pricing` |
| UUPS / storage layout | `reviewing-upgradeable-proxies` |

### Tools

| User ask | Skill |
|----------|--------|
| invariant handler + ghosts | `writing-foundry-invariant-handlers` |
| Echidna / Medusa | `fuzzing-with-echidna-and-medusa` |
| Slither / Aderyn / Semgrep / Wake | `static-analysis-slither-aderyn-semgrep-wake` |
| Halmos / Certora / Kontrol | `formal-verification-halmos-certora-kontrol` |
| cast / Sourcify / public tx | `onchain-read-recon` |
| node / Firedancer / rippled | `auditing-blockchain-clients` |

## Fat references (read as needed)

| File | Contents |
|------|----------|
| `references/disclosure.md` | Lawful-use gate |
| `references/sources.md` | X accounts + GitHub skill kits |
| `references/swc-map.md` | SWC-100..136 + living replacements |
| `references/bug-classes.md` | 20 EVM DeFi classes + pointer to 50+ taxonomy |
| `references/tools-matrix.md` | Static / symbolic / fuzz / recon |
| `references/platforms.md` | Immunefi, C4, Sherlock, Cantina, Hats, BattleChain |
| `references/gym.md` | Curriculum |
| `references/tob-bsc.md` | Trail of Bits building-secure-contracts map |
| `references/grep-patterns.md` | In-scope grep seeds |
| `references/protocol-invariants.md` | Conservation laws by type |

## External kits (ecosystem ensure + on demand)

| Kit | Notes |
|-----|--------|
| [DarkNavySecurity/web3-skills](https://github.com/DarkNavySecurity/web3-skills) | `/contract-auditor`, `/client-auditor`, `/exploit-investigator` (public txs / IR only). X: [@DarkNavyOrg](https://x.com/DarkNavyOrg) [@Defi_Nerd_sec](https://x.com/Defi_Nerd_sec) |
| [Cyfrin/solskill](https://github.com/Cyfrin/solskill) | `/solidity`, BattleChain. X: [@PatrickAlphaC](https://x.com/PatrickAlphaC) |
| [0xinit/cryptoskills](https://github.com/0xinit/cryptoskills) | Slither/Echidna/protocol primers (install on demand; repo may 404) |
| [shuvonsec/claude-bug-bounty](https://github.com/shuvonsec/claude-bug-bounty) `web3-audit` | 10-class checklist, not an exploit cookbook |
| QuillShield skills | [@QuillAudits_AI](https://x.com/QuillAudits_AI) |
| mariano-aguero / zpano / yolodolo42 / Wizbisy Solidity auditors | Firm-style SKILL.md on GitHub |

Do not merge those into the 817 SOC blob. Progressive disclosure.

## Researcher path

1. Gym (`researcher-gym-and-curriculum`).
2. Handler invariants on every value-moving contract.
3. Solodit + SWC + bug-classes on every review. Paid Immunefi/X analog: one `hunting-x-linked-bounties` slice. Non-EVM, ops-vs-code, or named incident: one `historical-smart-contract-vulns` slice.
4. In-scope C4/Cantina/Sherlock, then Immunefi.
5. Report: impact, root cause, local Foundry test, fix.
