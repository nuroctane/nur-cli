---
name: reviewing-nft-and-marketplace
description: "Review NFT and marketplace contracts: safeTransfer callbacks, royalty bypass, listing signature replay, payment token weirdness, and enumeration grief. Use for ERC-721/1155 markets and launchpads."
---

# Reviewing NFT and marketplace

In-scope. Callbacks are reentrancy (`reviewing-reentrancy-and-callbacks`). Signatures (`reviewing-signatures-permit-and-eip712`).

## Checklist

- `onERC721Received` / 1155 received during fill: can the buyer re-enter `buy` and get a second NFT?
- Royalty: ERC-2981 skipped on OTC / `transferFrom` bypass.
- Listing sig: nonce, expiry, chainId, cancellations.
- Payment in weird ERC-20 (`reviewing-token-standard-pitfalls`).
- Mint: public `mint` to attacker, merkle proof reuse, `tx.origin` allowlist.
- Enumeration (`tokenOfOwnerByIndex`) unbounded DoS.

Tests: attacker NFT that re-enters on receive; replay listing after cancel.

## Paid bounty analogs

| $ | Protocol | Pack | Writeup |
|---|----------|------|---------|
| $50k | BendDAO sewer-pass flash claim | T32 | [BendDAO](https://medium.com/@BendDAO/sewer-pass-flash-claim-vulnerability-9d2b0b1e09ef) |

BAYC / `setApprovalForAll` phishing is **ops** (`investigator-ops.md`), not this playbook.
