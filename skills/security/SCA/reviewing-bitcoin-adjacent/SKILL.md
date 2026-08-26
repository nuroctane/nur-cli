---
name: reviewing-bitcoin-adjacent
description: "Review Bitcoin Script, Lightning HTLCs, Stacks Clarity, BitVM/Babylon challenge games, and EVM-on-Bitcoin sidechains. Use when the VM is not Solidity. Lawful/in-scope only."
---

# Reviewing Bitcoin-adjacent systems

In-scope scripts / Clarity / challenge-game spec only. No mainnet sweeps, no live channel griefing.

Historical: `historical-smart-contract-vulns` `references/bitcoin-and-other-vms.md` (Stacks Immunefi DoS, VeChain if EVM-like, exchange ops vs Script).

## Bitcoin Script

Policy (miniscript / tapscript tree) equals the actual leaves (`B01`). Sighash covers the intended outputs.

## Lightning

HTLC hash, amount, and timeout bind both sides (`B02`). Revoked state: watchtower assumptions documented.

## Stacks

Clarity DoS / unbounded maps (`B03`). Clarinet tests.

## BitVM / Babylon

Honest party can challenge in the documented window (`B04`). Missing challenge path is the finding.

## Sidechains

RSK / similar: EVM catalog **plus** federation/bridge `T20`. Liquid: Elements + federation ops.

## Output

`(path or leaf, wrongly trusted, B0x, local test harness)`. If the bug is an exchange key, label **ops** and stop.

## Paid bounty analogs

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| ~$76k | Stacks Clarity DoS | B03 | Catchme |
| $15k | Sovryn (Rootstock) | T01 / B01 | gandu_whitehat |

AlexLab Stacks losses are **hacks** (`bitcoin-and-other-vms.md`). DMM Bitcoin $304M is custodial ops.
