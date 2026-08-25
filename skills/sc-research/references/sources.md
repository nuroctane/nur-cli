# Public researcher sources (X + GitHub)

Catalog assembled for Nur's `sc-research` pack. Treat as **sources**, not people to impersonate. Skills we bake are original Nur playbooks; these are the canons they map onto.

## Agent SKILL.md kits (the thing people ship on X)

| Repo | What it is | X / author |
|------|------------|------------|
| [DarkNavySecurity/web3-skills](https://github.com/DarkNavySecurity/web3-skills) | `contract-auditor` (DFS mapping + hunt agents), `client-auditor` (node/P2P/consensus), `exploit-investigator` (public tx reconstruction) | [@DarkNavyOrg](https://x.com/DarkNavyOrg) [@Defi_Nerd_sec](https://x.com/Defi_Nerd_sec) - posted the open-source preview on X |
| [dudesahn/dark-navy-web3-skills](https://github.com/dudesahn/dark-navy-web3-skills) | Community mirror of the above | - |
| [Cyfrin/solskill](https://github.com/Cyfrin/solskill) | Production Solidity + BattleChain whitehat deploy / Safe Harbor | [@PatrickAlphaC](https://x.com/PatrickAlphaC) [@CyfrinAudits](https://x.com/CyfrinAudits) |
| [0xinit/cryptoskills](https://github.com/0xinit/cryptoskills) | Slither, Echidna, Foundry, ~95 protocol primers | cryptoskills.dev |
| [shuvonsec/claude-bug-bounty](https://github.com/shuvonsec/claude-bug-bounty) `skills/web3-audit` | 10 DeFi bug classes + Immunefi-shaped reports | - |
| [mariano-aguero/solidity-security-audit-skill](https://github.com/mariano-aguero/solidity-security-audit-skill) | Firm-style audit methodology | - |
| [zpano/solidity-audit](https://github.com/zpano/solidity-audit) | Orchestrator + specialized audit agents | - |
| [yolodolo42/solidity-audit-skill](https://github.com/yolodolo42/solidity-audit-skill) | Claude skill + MCP build/test/audit tools | - |
| [Wizbisy/solidity-auditor](https://github.com/Wizbisy/solidity-auditor) | Three-phase Solidity auditor | - |
| QuillAudits QuillShield Claude skills | Intent-driven audit (not pattern-only) | [@QuillAudits_AI](https://x.com/QuillAudits_AI) |
| [mukul975/Anthropic-Cybersecurity-Skills](https://github.com/mukul975/Anthropic-Cybersecurity-Skills) | Source of Nur's two existing Solidity skills | already wired in `packs.rs` |

Marketplace indexes that are **not** DeFi-native yet (opening for Nur): [skills.sh](https://skills.sh), ClawHub, `travisvn/awesome-claude-skills`, `VoltAgent/awesome-claude-skills`, BuilderIO/skills.

## Research canons (convert; do not wrap exploits)

| Source | Why it is a skill |
|--------|-------------------|
| [Trail of Bits building-secure-contracts](https://github.com/crytic/building-secure-contracts) | Token, proxy, upgrade, invariant patterns |
| [SWC Registry](https://swcregistry.io/) | Weakness IDs the Foundry skill already maps |
| [Solodit](https://solodit.xyz/) | Historical findings by protocol type |
| [DeFiHackLabs](https://github.com/SunWeb3Sec/DeFiHackLabs) | Post-mortems as **regression tests**, not attack runbooks |
| Cyfrin Updraft + Aderyn docs | Matches the Aderyn layer |
| Foundry Book invariant/fuzz chapters | Handler + ghost variable pattern |
| Secureum, RareSkills, Damn Vulnerable DeFi, Ethernaut, Paradigm CTF | Researcher gym |
| Immunefi / Cantina / Sherlock / Code4rena / Hats docs | Scope, severity, report format |
| [EEA EthTrust Security Levels](https://entethalliance.org/specs/ethtrust-sl/) | Living Solidity vulnerability spec (SWC successor) |
| [SCSVS](https://github.com/ComposableSecurity/SCSVS) | Smart Contract Security Verification Standard |
| OpenZeppelin Contracts + upgrades docs | ERC-4626, Governor, UUPS, SafeERC20 |
| Chainlink data-feeds + L2 sequencer docs | Staleness / uptime |
| Uniswap v2/v3/v4 core | AMM/CL callback model |
| [Damn Vulnerable DeFi](https://github.com/theredguild/damn-vulnerable-defi) | Gym (Foundry) |
| [Ethernaut](https://ethernaut.openzeppelin.com/) | Gym |
| [Halmos](https://github.com/a16z/halmos) | Symbolic Foundry tests |
| Certora / Kontrol / Wake / Semgrep ToB rules | Formal + extra static |

## X accounts the field learns from

**Independent researchers / wardens**
`@samczsun` `@pcaversaccio` `@pashovkrum` `@bytes032` `@cmichelio` `@0xRajeev` `@0xNazgul` `@0xJuancito` `@officer_cia` `@MuditGupta_` `@trustindistro` `@transmissions11` `@00xSEV` `@0xBarz` `@Defi_Nerd_sec` `@naeaexeth` `@anchabadze` `@PeterSRWeb3` `@QuillAudits_AI` `@0xvivekd`

**Firms / platforms**
`@trailofbits` `@CyfrinAudits` `@PatrickAlphaC` `@OpenZeppelin` `@spearbit` `@cantinaxyz` `@sherlockdefi` `@code4rena` `@immunefi` `@hatsfinance` `@AckeeBlockchain` `@sigma_prime` `@Zellic_io` `@ottersec` `@runtimeverification` `@CertoraInc` `@BlockSecTeam` `@SlowMist_Team` `@rektnews` `@DarkNavyOrg` `@QuillAudits_AI`

**Tooling**
`@crytic_` (Slither / Echidna / building-secure-contracts) · Cyfrin/Aderyn · Foundry · Halmos (a16z crypto eng)

## Search queries (re-run on X when hunting new kits)

```
SKILL.md smart contract audit
"claude skill" (foundry OR slither OR aderyn OR invariant)
"agent skill" (Immunefi OR Code4rena OR Spearbit)
from:pcaversaccio (audit OR invariant OR proxy)
from:DarkNavyOrg skills
from:Defi_Nerd_sec skills
"solodit" (vault OR oracle OR reentrancy)
```
