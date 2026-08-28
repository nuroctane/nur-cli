# Tripwire telemetry (schema and plumbing)

All deception surfaces funnel into ONE event schema so the response ladder stays uniform across chains and layers.

## Canonical event schema

```
TripwireEvent {
  event_id        uuid v7 (time-ordered)
  timestamp_utc   iso8601 + source clock note
  layer           CONTRACT | RECON | INFRA | WALLET
  surface_id      manifest row (which lure)
  chain           chain id or "offchain"
  tx_hash / req_id  primary evidence pointer
  actor_address   what touched the lure (caller, wallet, source ip hash)
  marker_hit      watermark/channel id(s) observed
  behavior        enum: PROBE | FETCH | FUND | SIGN | CALL | SWEEP | SUBMIT
  depth           how far into the lure chain they got (1..n)
  calldata_hash   sha256 of full calldata/payload; raw stored separately
  self_reported   any model/harness/env strings volunteered
  confidence      SEPARATION_SCORE (see below)
  action_taken    OBSERVED | WATCHLISTED | ESCALATED | FROZEN | HANDED_OFF
  notes           free text, factual only
}
```

## Separation score (how innocent-proof was this touch?)

Score = f(reachability, sequence, self-report):

| Signal | Points |
|--------|--------|
| Touch required reading a non-public poison artifact | +3 |
| Multi-step chain completed (fetch -> verify -> call) | +3 |
| Attacker-shaped calldata (exploit-shaped sequence, not UI-shaped) | +2 |
| Self-reported env/model strings | +2 |
| Funding pattern consistent with attack infra (fresh wallet, bridge-hop) | +2 |
| Could plausibly be a legit user/keeper | -4 (hard veto) |

Ladder thresholds (defaults; your council can tune): score >= 6 -> watchlist + evidence prep; >= 10 -> escalate; any hard veto -> observe only, fix the lure's screening.

## Telemetry plumbing notes

- **EVM**: events from decoy contracts (index via your own watcher, not third parties - third-party indexers leak that you watch), plus mempool listeners for pre-inclusion probes; store calldata pre-image, not just hashes.
- **Solana**: program logs + events from decoy programs; account-subscribe on bait token accounts for first-touch.
- **Move/Cosmos/TON**: native event streams; normalize into the same schema.
- **Offchain**: web/CDN logs for decoy pages and canary URLs; DNS-level canaries (unique hostnames) are cheap and reliable; API-gate logs for self-report fields.
- **Retention**: append-only store, hashed event chains (each event includes prev hash) - you are building exhibits, and tamper-evidence starts now.
- **PII discipline**: addresses and infra only; no speculative doxxing fields; identities enter the record only via the attribution file with evidence links.

## What NOT to log/keep

- Nothing that would be illegal for you to collect (no intrusion into attacker systems to "get more").
- No copy of genuinely third-party user data that touched a lure by accident - purge on the hard-veto path.

## Testing the pipeline

Fire every lure yourself monthly (from a canary wallet you own): each should produce a complete TripwireEvent end-to-end. A lure whose telemetry silently broke is worse than no lure - it teaches the attacker your surfaces are unwatched.
