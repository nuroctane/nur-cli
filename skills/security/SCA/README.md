# SCA - smart-contract audit pack

First-party whitehat DeFi / smart-contract research pack for Nur.

Skill **names** stay kebab (`sc-research`, `hunting-x-linked-bounties`, …) so `/name` and NL triggers do not change. Runtime install still writes `~/.nur/skills/<name>/` (flat). This folder is the **repo** layout only.

Do **not** merge this pack into the 817 `cybersecurity` router. That pack's Crypto domain is cryptography, not DeFi. Route Solidity / Immunefi / DeFi through `sc-research`.

## Load one slice

| Ask | Skill |
|-----|--------|
| Start here / unknown protocol | `sc-research` |
| Paid Immunefi / X bounty writeup | `hunting-x-linked-bounties` (one table) |
| Historical class / named loss / zachxbt | `historical-smart-contract-vulns` (one reference) |
| Named-chain audit delta (Base, Robinhood Chain, Arbitrum, OP, Hyperliquid, ...) | `reviewing-major-chain-surfaces` (one chain file) |
| Current-year trends / OWASP mapping / tooling currency | `defi-security-trends-standards` (one reference) |
| Agentic-defense deception program / honeypots / attacker ID | `agentic-defense-game-theory` -> 3 companion skills (one slice) |
| Pre-deploy Foundry PASS/FAIL | `auditing-foundry-smart-contract-security` |
| Chain VM | matching `reviewing-*` playbook |

Raw ingest under `sc-research/library/_ingest/` is **not** a skill. Do not `skill(read)` it.

Lawful / in-scope only.

---

## License

**GNU General Public License v3.0 (or later)** — see [LICENSE](./LICENSE).

Meta CLI is free software: you may redistribute it and/or modify it under the
terms of the GPL as published by the Free Software Foundation, either version 3
of the License, or (at your option) any later version. It is distributed in the
hope that it will be useful, but **without any warranty**; without even the
implied warranty of merchantability or fitness for a particular purpose.
