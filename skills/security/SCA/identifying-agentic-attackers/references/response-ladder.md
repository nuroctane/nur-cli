# The response ladder (what happens when something fires)

Pre-committed, graduated, mechanical. Write your protocol's version of this table, get council sign-off, drill it twice a year. Column "who" assumes: W = on-call watcher, H = deception program owner (human), C = security council / counsel.

## The ladder

| Rung | Trigger (separation score / event class) | Automated action | Human action | Who |
|------|------------------------------------------|------------------|--------------|-----|
| 0 | Any tripwire event | Log to append-only store; hash-chain; enrich (funding graph) | None | W |
| 1 | Score < 6 (ambiguous, e.g. crawler noise) | Nothing beyond logging | Weekly review queue | W |
| 2 | Score 6-9, CONTRACT/RECON layer | Watchlist address in your own protocol views; begin evidence pack | Confirm lure integrity; refresh if burned | W + H |
| 3 | Score >= 10, or multi-artifact correlation | Same + open incident channel; prep handoff targets | Assess: probe vs rehearsal vs live-op | H |
| 4 | Live-attack signature on a DECOY (their real tx hitting bait) | Guardian records; decoy reverts politely; evidence pack opens | Handoff prep; decide on public deterrence signal | H + C |
| 5 | Live-attack signature on PROD path (real exposure) | Protocol pause per ToS/code; sweep protocol-owned funds to guardian; issuer freeze requests (OPS15 interplay) - pre-authorized | Declare incident; SEAL 911; safe-harbor posture (`battlechain-safe-harbor-whitehat`); full handoff | H + C, exec informed |
| 6 | Confirmed theft in progress or completed | Freeze interplay everywhere pre-authorized; evidence preservation freeze (legal hold) | Law enforcement filing; negotiations ONLY via counsel; public comms per plan | C + exec |

## Drills (run quarterly, 1 hour)

1. Fire one canary end-to-end; time from touch to enriched TripwireEvent.
2. Tabletop rung 5: who can pause, how fast, what is the freeze-call list (issuer desks, exchange contacts, SEAL)?
3. Manifest audit: any lure without a watcher, any marker without a row?

## Timing targets (why the ladder is mechanical)

- Probe-to-watchlist: minutes (automation), not days.
- Rung-5 pause decision: pre-authorized means zero debate; the debate already happened at council.
- Issuer freeze requests: hours matter - the Drift/CCTP 2026 episode showed unfreeze-freeze windows are short and move billions.
- Public deterrence signal: after evidence is packaged, never before (never teach while the operator is still active against you).

## Interaction with safe harbor and bounties

- If the "attacker" turns out to be a whitehat operating in scope (it happens), pivot immediately: bounty payout + postmortem credit. Your decoy just paid for itself in talent identification.
- Safe-harbor frameworks protect defined defender/whitehat actions during live incidents - know the boundaries before rung 5, not during it (`battlechain-safe-harbor-whitehat`).
- If an agent operator later claims "research": the evidence pack decides. Separation scores, self-reports, and funding patterns are the arbiter, not their claim.

## Failure modes to design against

| Failure | Consequence | Mitigation |
|---------|-------------|------------|
| Lure fires, nobody notices for weeks | Free recon for attacker | Monthly canary self-fire test |
| Over-response to a probe | Teaches attacker your map; burns credibility | Score-gated rungs; observe-first defaults |
| Manifest drift (lures not in manifest) | Unattributable signals, unowned risk | Quarterly audit; CI check on decoy deploy scripts |
| Public over-sharing after a catch | Collapses ambiguity asset | Deterrence-signal template; comms review |
| Ops-layer touch triaged as routine decoy hit | Bybit-class miss | OPS-layer events bypass rungs 1-2 by default |
