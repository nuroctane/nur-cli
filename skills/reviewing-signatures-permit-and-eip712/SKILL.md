---
name: reviewing-signatures-permit-and-eip712
description: "Review EIP-712, permit, and custom signatures: replay, chainId, nonce, deadline, ecrecover address(0), malleability, encodePacked collisions. Use for permits, listings, votes, and bridge attestations."
---

# Reviewing signatures, permit, and EIP-712

In-scope. Local tests with `vm.sign`. Never reuse a user's live signature.

## Domain

- `EIP712` domain: name, version, **chainId**, verifyingContract.
- Forked/replayed on another chain if chainId missing (SWC-121).

## Message

- Nonce increments; replay reverts.
- Deadline checked (`block.timestamp`).
- `ecrecover` result != `address(0)` (SWC-122).
- EIP-2098 compact vs 65-byte; s-value malleability (SWC-117). Prefer OZ `ECDSA.recover`.

## encodePacked

Two dynamic types in `abi.encodePacked` can collide (SWC-133). Use `abi.encode` for typed data.

## Permit

- ERC-20 permit vs DAI-style permit (allowed vs value).
- Signature used as unlimited allowance forever is a product finding.

## Listings / votes / mints

Same rules. A marketplace listing without nonce is infinite replay (`reviewing-nft-and-marketplace`).

Tests: `test_RevertWhen_Replay`, `test_RevertWhen_WrongChain`, `test_RevertWhen_Expired`, `test_RevertWhen_ZeroRecover`.
