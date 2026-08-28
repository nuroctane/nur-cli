# Decoy primitives (functions, storage, guardian automation)

The buildable atoms. Sketches are illustrative - run them through your normal review pipeline like any prod change.

## 1. Decoy functions (I, C, S)

An admin-shaped function in a **separate decoy contract** (never prod logic). Calling it records and politely reverts:

```solidity
contract OpsUtilitiesV2 { // decoy, address "leaks" via watermarked doc only
    address internal immutable GUARDIAN; // real automation, not shown here

    function emergencyWithdraw(address token, uint256 amount) external {
        emit TripwireTouched(msg.sender, "opsUtilities.emergencyWithdraw",
            keccak256(abi.encode(token, amount)), block.number);
        // inert: records and reverts; no state change, no value movement
        revert("paused during migration");
    }
    event TripwireTouched(address indexed caller, string surface, bytes32 probe, uint256 blockNumber);
}
```

Why it works: the agent plans an admin-path exploit; the first probe is a signed, timestamped identification event, and the polite revert keeps the agent in the loop (it will retry variants - each one more evidence, more of its budget). Effect: the exploit hypothesis dies after paying for several probes.

Placement rules: address published ONLY through poison channels (`poisoning-agent-recon-surfaces`); never in docs, ABIs you publish, or explorer-verified prod source.

## 2. Tripwire storage slots (I, S)

Reserve decoy slots that look meaningful (e.g., a fake `pendingAdmin`, a fake `upgradePath`) in contracts you control. Any write (onlyOwner-gated prod code never writes them; they exist so an attacker's "take admin via storage slot X" plan lands somewhere observable when they bribe/patch via unusual paths).

- In decoy impls behind sacrificial proxies: give the decoy impl a *wrong* storage layout that maps "theft" writes onto the tripwire region. Effect: **P** - their working mainnet-fork exploit writes into a slot that emits `TripwireTouched` live.
- In real proxies: never alter ERC-1967 semantics. Bait lives in decoy deployments, not in treasury-carrying proxies.

## 3. Proxy-collision bait (I, P) - T12/T13 theater, done safely

Publish a decoy UUPS "implementation" (uninitialized, funds-free) whose `initialize` records its caller and mints them a worthless "admin role". Agents running the classic uninitialized-proxy grab against your surface self-identify and gain nothing. Effect: screening at its purest - only someone running takeover tooling calls it.

## 4. Guardian automation (commitment device)

Tripwire events must trigger pre-authorized, deterministic responses. Pattern:

```solidity
// offchain watcher -> guardian (multisig + automation) decisions pre-agreed:
// T1 probe on decoy          -> log, watchlist address, no prod action
// T2 probe + funding spike   -> watchlist + exchange/SEAL heads-up
// T3 live attack signature   -> protocol pause (per ToS/code), sweep-to-guardian
//                                of protocol-owned funds, incident channel opens
```

Credible because pre-committed (no mid-attack discretion to exploit). Note the ladder's restraint: most touches earn *observation*, not response - overreacting to probes teaches attackers your tripwire map (a side-channel on your decoy set). Fire prod-level defense only on live-attack signatures.

## 5. Decoy "feature" modules (C, P) - novel shapes beat textbook traps

Pattern-detectors flag classic honeypot bytecode. Protocol-plausible decoys survive:

- A "rewards-boost" side module whose rates are bait-value only (never backed).
- A decoy "arb router" for a market that does not exist.
- A "migration helper" contract that is 90% real logic and 10% bait - the real-looking part makes it crawl-worthy; the bait part is the tripwire.

Rule of thumb: the decoy should appear in a security review as "dead code, weird but harmless" - not as "obvious honeypot".

## 6. Timing traps (C)

Decoy functions whose success requires an impossible-on-live condition (a state combo only reachable on a fork, see `anti-simulation-fork-traps.md`). The agent's sim says yes; live says revert after gas. Repeat until the operator's optimizer gives up on that plan family.

## 7. Canary strings and numbers (I)

Unique constants embedded in each decoy (magic values, "checksum" strings). When they surface in attacker tooling, dumps, or a rival protocol's exploit attempt, they prove which artifact leaked - the same mechanism as watermarked docs, but onchain. Keep a manifest mapping constant -> distribution channel (`poisoning-agent-recon-surfaces` `references/watermarking-channel-ids.md`).

## Anti-patterns

- Decoys inside treasury contracts (one bug = real loss).
- Decoys that harm/brick legit integrators (violates screening invariant; you become the protocol that rugged a keeper).
- Publicly tweeting your decoy addresses (collapses ambiguity; only disclose when a tripwire FIRES and you want the deterrence signal).
- Decoys without an owner watching telemetry (free recon for the attacker).
