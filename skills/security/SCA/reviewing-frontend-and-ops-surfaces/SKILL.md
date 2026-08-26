---
name: reviewing-frontend-and-ops-surfaces
description: "Review wallets, Safe modules, Permit2, EIP-712 UIs, and signer runbooks for phishing-class bugs (Bybit Safe UI, drainers, leftover approvals). Use when the loss class is ops/frontend, not vault math. Lawful/in-scope only."
---

# Reviewing frontend and ops surfaces

In-scope **product** source only: wallet UI, Safe module, Permit2 integration, dapp frontend, signer docs. Tests are local (Foundry `vm.sign`, wallet e2e). Never paste drainer kits. Never phish a live user.

Load `historical-smart-contract-vulns` `references/investigator-ops.md` for analog names (Bybit, WazirX, Radiant malware, Inferno/Pink/Angel, Permit2). One slice.

If the target is a vault/AMM/lending pool with no signer UI in scope, stop and use the matching protocol playbook instead.

## Conservation

What the signer **is shown** (simulation, EIP-712 fields, hardware screen) is what they **authorize**. Approvals cannot outlive the documented intent. A Safe hash computed on an airgapped machine matches the hosted UI.

## Checklist (label each row OPS0x)

1. **Safe / multisig UI (OPS01, OPS18).** Decoded `to`, `value`, `data`, operation match the human copy. Independent hash (Safe Transaction Service vs local). Subresource integrity on the wallet JS you ship.
2. **EIP-712 domain (OPS03).** `name`, `version`, `chainId`, `verifyingContract` equal the UI string. Missing `chainId` is a finding.
3. **Permit / Permit2 (OPS03, OPS14).** Spender is the documented router. Amount is not silent `uint160.max` unless the copy says unlimited. `PermitBatch` lists every token the UI listed. Revoke / `lockdown` path exists.
4. **ERC-20 approve / NFT `setApprovalForAll` (OPS13, OPS14).** Default cap; operator allowlist; leftover allowance after the flow ends.
5. **WalletConnect / injected (OPS04).** Session bound to origin; no silent typed-data on route change; disconnect is real.
6. **Clear-sign / hardware (OPS09).** Device screen is parsed, not "blind hex". Official support URLs only in copy.
7. **Signer malware assumption (OPS17).** Runbook: second machine simulates the same Safe tx. Host Electron app is not the source of truth.
8. **Seed / 7702 (OPS09).** No flow asks for a seed or "dev mode" to connect. `connect_but_seed_prompt` class is a product-critical finding.

## Output

Table: `(surface, what UI shows, what is signed, OPS id, local test idea)`. Next: `contest-and-bounty-reporting` or a wallet-security ticket. Do not file Bybit/WazirX/Radiant-malware as `reviewing-erc4626-and-vaults`.

## Public analogs (ops, not Immunefi Solidity payouts)

These are **not** in `hunting-x-linked-bounties`. They live in `investigator-ops.md`: Bybit Safe UI / JS (OPS01/18), WazirX, Radiant malware vs Radiant math, Inferno/Pink/Angel drainers, Permit2 phishing, leftover approvals, Circle Files / Drift CCTP freeze lag.

If the user pasted an X bounty writeup (Wormhole, Aurora, Scroll, Raydium), you are in the **wrong** skill. Load `hunting-x-linked-bounties`.
