---
name: reviewing-bridges-and-messaging
description: "Review token bridges and cross-chain messages: replay, decoder, finality, aliasing, amount inflation, and unauthorized mint. Use for lock/mint, burn/mint, LayerZero, Optimism messenger, zk bridges."
---

# Reviewing bridges and messaging

Bridge bugs are usually Critical. In-scope source only. Never test by moving real funds across chains.

## Conservation

Minted/unlocked on the destination cannot exceed locked/burned on the source (plus documented fees). Prove with ghosts on both sides in a local dual-chain mock, not production.

## Message id

- Each message processed once (`done[id]`, nonce, nullifier).
- Replay on the same chain and **across forks** / reorgs if finality is short.
- Out-of-order: if allowed, state machine must not skip a spend.

## Decoder / recipient

- Amount and recipient cannot be inflated by padding, `abi.encodePacked` collision (SWC-133), or leftover calldata.
- Address aliasing (L1->L2) vs `msg.sender` checks.
- Token mapping: unmapped token should revert, not mint a spoofed asset.

## Caller

- `lzReceive` / `onMessage` / `xDomainMessageSender` authenticated as the real messenger/endpoint.
- Config (trusted remotes, merkle roots) is admin-gated and two-step where claimed.

## Finality

Assuming instant L1 inclusion on an optimistic rollup is a finding if the window is still open.

Next: `reviewing-l2-sequencer-and-finality`, `reviewing-signatures-permit-and-eip712` if messages are signed.
