# The agentic attack loop and where to intercept it

The modern attacker against a DeFi protocol is frequently an agent loop (LLM + tooling + wallets) or an operator running several. Defenses keyed to human-attacker tempo are mistimed. This file maps the loop, the observations it consumes, and the interception point per stage.

## The loop, stage by stage

| Stage | What the agent does | Observations consumed | Defender lever | Effect |
|-------|--------------------|-----------------------|----------------|--------|
| 1. Tasking | Operator sets target + goal; agent plans | Public loss stats, bounty pages, TVL ranks | Publish bounty scope precisely; keep loss-surface docs accurate so planning targets the hardened path (they will anyway - make it cost) | C |
| 2. Recon | Reads verified source, docs, governance forum, explorer labels, ABI dumps, fork tooling, search results for "<protocol> exploit", "<protocol> admin key", "<protocol> audit pdf" | Search + crawl layer | **Be where they land**: watermarked decoy docs, bait search results, labeled decoy contracts - see `poisoning-agent-recon-surfaces` | I, P |
| 3. Planning | Builds exploit hypothesis, fetches ABIs, addresses, slot layouts | Everything recon returned, incl. your poison | Ship wrong-but-plausible plans: decoy impl storage layouts, decoy governance queue docs | P |
| 4. Simulation | Forks mainnet (Anvil/foundry/devnets), dry-runs, static-analyzes | Fork behavior, view functions, gas | Anti-simulation divergence: fork sees a profitable path that mainnet denies - `deploying-contract-honeypots` `references/anti-simulation-fork-traps.md` | P, C |
| 5. Funding | Bridges/funds an attack wallet, sometimes via mixers/rented keys | Your decoy "profit pool", bait wallet | Bait wallets + watermarked canary transfers: first touch of bait funds identifies the wallet - `bait-funds-and-canaries.md` | I |
| 6. Execution | Sends txs (bundles, private routes, bribes) | Live chain state | Tripwire functions fire guardian automation; sim-vs-live divergence burns their bribe | I, C |
| 7. Exfil/launder | Moves proceeds through bridges/mixers/exchanges | Chain analytics | Pre-authorized freeze interplay (CCTP/issuer lists), analytics watchlists, evidence pack - `identifying-agentic-attackers` | I, D |

## Landing-surface inventory (do this for YOUR protocol)

An agent's recon is only as good as its search. Enumerate what a search for your protocol returns and rank by "what would I want if I were the agent":

1. Verified source on block explorers (comments included - agents read them).
2. Your docs site, including "internal-looking" paths, robots.txt, sitemaps.
3. GitHub: your org, forks, issues, any leaked-looking files.
4. Explorer labels: contract names, proxy labels, token tags.
5. Governance forum / snapshot / timelock UIs (plan stage).
6. Subgraphs/APIs/indexers exposing your state in convenient shapes.
7. Third-party "protocol intel" aggregators and audit-PDF mirrors.

Each row is a legitimate place to be present with a watermarked decoy - and each row is ALSO where your real security docs must be excellent, because some visitors are human auditors and whitehats. Deception is an overlay on good faith surfaces, not a replacement.

## Case study: the benchmark-solutions pattern (2026, public)

A defender ran solution-lookup pages for AI-benchmark exploit tasks (CyberGym/ARVO ids) indexed for exactly the queries agents make. Design properties worth copying:

- **No self-labeling**: pages looked like the content agents want. No "honeypot" banner. The moment a lure self-identifies, EV math flips and screening dies.
- **Identification embedded in retrieval**: the "solution" API asked callers to self-report model id + harness; unrecognized values were served a fallback anyway. The lure learned about its biter while serving it.
- **Depth beyond page views**: reported "bites" included agents performing additional submission actions - i.e., the decoy captured *behavioral* signal, not just traffic.
- **Case hygiene**: agents never learned a case id existed; the defender controlled all correlation.
- **Community humor as signal**: the replies ("retrieve the solution by sending Bitcoin", "session expired, resend payment") joke about drainer aesthetics - useful as *deterrence memes*, and a bright line: funny copy is fine; actually taking an attacker's funds is outside the lawful gate (their funds identification ≠ your license to take them).

Copy the pattern, not the page: for a DeFi protocol the equivalent is a "leaked ops runbook" doc that asks a verify question, a decoy admin endpoint that logs calldata before reverting politely, a bait wallet whose first spender gets watchlisted.

## Interception economics

Intercept early: a poisoned recon observation costs the attacker minutes and corrupts every downstream stage. Intercept late (execution) and you pay in real risk. The plays in the companion skills are ordered stage-early where possible: recon poisoning first, contract tripwires second, telemetry everywhere.
