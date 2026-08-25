---
name: reviewing-l2-sequencer-and-finality
description: "Review L2 assumptions: sequencer downtime, Chainlink sequencer uptime, address aliasing, delayed inbox, custom gas tokens, and optimistic finality windows. Use for OP/Arbitrum/Base/zk and any protocol that reads L1 state from L2."
---

# Reviewing L2 sequencer and finality

In-scope L2 deployments. Combine with `reviewing-oracles-and-pricing` and `reviewing-bridges-and-messaging`.

## Sequencer

- Chainlink L2 sequencer uptime feed + grace period after restart. Missing = stale prices during downtime (paid class).
- Protocol pause when sequencer is down if it liquidates.

## Aliasing / callers

- Optimism `l1Block` / aliasing: L1 sender != raw `msg.sender` on L2.
- Arbitrum retryable tickets: aliased aliased aliased. Confirm the exact alias the messenger uses.

## Finality

- Optimistic: challenge window. A bridge that credits on sequencer receipt without waiting is a bridge finding.
- zk: proof lag vs instant UI.

## Custom gas token / fee token

Accounting that assumes ETH is the gas token breaks on Orbit/OP-custom.

Tests: mock sequencer down (`updatedAt` stale) and assert liquidations revert.
