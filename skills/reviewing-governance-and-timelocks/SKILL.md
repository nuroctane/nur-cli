---
name: reviewing-governance-and-timelocks
description: "Review Governor, timelock, and upgrade-via-governance: delay bypass, proposal hijack, snapshot vs flashloan votes, queue/execute hash mismatch. Use for OZ Governor, custom DAOs, and admin multisigs."
---

# Reviewing governance and timelocks

In-scope. Do not grief a live DAO.

## Timelock

- `queue` hash = target + value + data + salt + eta.
- `execute` only after delay; cancelled ops cannot execute.
- Admin of the timelock vs proposer vs executor roles. A hot EOA executor is a finding if the docs claim a delay.

## Governor

- Vote snapshot must be **before** the proposal (flashloan votes).
- Quorum and threshold math; 0 supply edge.
- `relay` / `updateTimelock` / `setProposalThreshold` access.
- Proposal hijack: if description is not hashed into the id the way OZ does, two payloads can collide.

## Upgrade path

Governance that can `upgradeTo` is a crown jewel. Combine with `reviewing-upgradeable-proxies`.

## Short delay vs TVL

A 1-hour delay on a $100M vault is a product finding; say so as Medium/QA unless the program forbids governance design comments.

Tests: `test_RevertWhen_ExecuteBeforeDelay`, `test_RevertWhen_FlashloanVotes` (snapshot).
