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

## Paid bounty analogs

Full `$` table: `hunting-x-linked-bounties` `references/paid-payouts.md`.

| $ | Protocol | Pack | Whitehat |
|---|----------|------|----------|
| $6M | Aurora ExitToNear / infinite spend | T18 / O01 | pwning.eth |
| ~$2.2M | Polygon MRC20 no balance check | T18 | Leon Spacewalker |
| ~$2M | Polygon Plasma double-spend | T17 | Gerhard Wagner |
| **$1M** | Scroll message spoof | T19 | WhiteHatMage |
| $200k | Interlay | T19 | pwning.eth |
| $50k | Wormhole (Marco); Axelar halt | T19 / C01 | Marco Nunes |
| High | Across V3 | T19 | zachobront, deadrosesxyz |
| $100k | Story Network | T19 | WhiteHatMage, Jiri123 |
| $5k | O3; LayerZero Trust low | T18 / T20 | Trust |

KelpDAO 2026 single-verifier **loss** is T20 (`named-losses.md`), not a bounty. Nomad trusted-root is a hack analog (`case-cards.md` 7.7).
