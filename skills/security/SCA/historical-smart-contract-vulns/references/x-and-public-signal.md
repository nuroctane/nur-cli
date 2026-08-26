# X and public signal (pointers, not primary sources)

X is how the field **hears** about writeups. A tweet is not a case card until the Immunefi blog, protocol postmortem, or firm post agrees.

**Packaged X bounty intel (load these, not the ingest file):**

| Slice | Skill |
|-------|--------|
| Every paid row + playbook | `hunting-x-linked-bounties` `references/paid-payouts.md` |
| 2024-2026 X table | `hunting-x-linked-bounties` `references/x-linked-2024-2026.md` |
| Handle -> skill | `hunting-x-linked-bounties` `references/researcher-index.md` |

This file keeps handles, search queries, and gaps. Confirm `$` and class on the linked article.

## Handles worth following (already in `sc-research/references/sources.md`)

**Platforms:** `@immunefi` `@rektnews` `@code4rena` `@sherlockdefi` `@cantinaxyz` `@hatsfinance` `@spearbit`

**Firms:** `@trailofbits` `@ottersec` `@Zellic_io` `@BlockSecTeam` `@SlowMist_Team` `@CyfrinAudits` `@OpenZeppelin` `@AckeeBlockchain` `@sigma_prime` `@CertoraInc` `@DarkNavyOrg`

**Ops / phishing (not Solidity; catalog: `investigator-ops.md`):** `@zachxbt` `@tayvano_`

**Wardens / writers (examples, not endorsements):** `@samczsun` `@pcaversaccio` `@pashovkrum` `@bytes032` `@cmichelio` `@0xRajeev` `@officer_cia` `@0xriptide` `@satya0x` `@PwningEth` `@saurik` `@trust__90` `@zachobront` `@kankodu` `@WhiteHatMage` `@marcotnunes`

## X-linked payouts that already have writeups

These rows started as X/Mirror and were confirmed on a blog. Full `$` + playbook: `hunting-x-linked-bounties` `paid-payouts.md`.

| Pointer | Protocol | Confirm on |
|---------|----------|------------|
| satya0x / Immunefi | Wormhole $10M | [Immunefi](https://medium.com/immunefi/wormhole-uninitialized-proxy-bugfix-review-90250c41a43a) |
| PwningEth | Aurora $6M, Moonbeam/Frontier | Immunefi + [pwning.mirror.xyz](https://pwning.mirror.xyz/okyEG4lahAuR81IMabYL5aUdvAsZ8cRCbYBXh8RHFuE) |
| saurik | Optimism $2M | [Immunefi](https://medium.com/immunefi/optimism-infinite-money-duplication-bugfix-review-daa6597146a0) |
| 0xriptide | Arbitrum inbox; LEVEL OOS | [medium](https://medium.com/@0xriptide/hackers-in-arbitrums-inbox-ca23272641a2) |
| GothicShanon | Balancer $1M rounding | [Immunefi](https://medium.com/immunefi/balancer-rounding-error-bugfix-review-cbf69482ee3d) |
| WhiteHatMage | Scroll $1M; Story $100k | [Scroll forum](https://forum.scroll.io/t/report-scroll-mainnet-emergency-upgrade-on-2025-04-25/666), [Story](https://www.story.foundation/blog/story-network-postmortem) |
| riproprip | Raydium $505k | [Immunefi](https://medium.com/immunefi/raydium-tick-manipulation-bugfix-review-c6aae4527ed6) |
| usmannk / Catchme | Sei $2M+ / $75k | [usmannkhan.com](https://usmannkhan.com/bug%20reports/2024/06/17/sei-bug-report.html) |
| ChainLight | zkSync Era $50k | [medium](https://medium.com/chainlight/uncovering-a-zk-evm-soundness-bug-in-zksync-era-f3bc1b2a66d8) |
| LonelySloth / Ehsan | zkSync Lite $200k | [Immunefi](https://medium.com/immunefi/zksync-insufficient-proof-verification-bugfix-review-dcd57944d0e2) |
| F4lt | Sui shutdown $50k | [Immunefi](https://immunefi.com/blog/bug-fix-reviews/sui-network-shutdown/) |
| Marco Nunes | Axelar halt; Wormhole high | [marcotnunes.com](https://marcotnunes.com/axelar-network-cross-chain-halt-vulnerability/) |
| kankodu | Silo $100k; Vesu; Balancer V2 | X + Vesu docs |
| zachobront + deadrosesxyz | Across V3 | [mirror](https://mirror.xyz/0x9D6b7f5e8d1b9dFea8dDD29c0DbD81687e721601/mrt70ckjaZymv9keUy_TzHVIzjBOQr-Hx_KI1ydFeoQ) |
| Catchme | Stacks DoS | [Immunefi](https://medium.com/immunefi/stacks-dos-bugfix-review-dc0f2a75b276) |
| Lastc0de | Acala halt | [Immunefi](https://immunefi.com/blog/all/acala-block-production-shutdown-bug-fix-review/) |
| nnez | VeChain VTHO | [Immunefi](https://immunefi.com/blog/all/vechainthor-vtho-accrual-bypass-bug-fix-review/) |
| f4lc0n | Injective $50k | [x](https://x.com/al_f4lc0n/status/2033110168045568434) - confirm Immunefi |
| jayjonah.eth | Evmos $150k | [medium](https://medium.com/@jjordanjjordan/150-000-evmos-vulnerability-through-reading-documentation-d26328590a7a) |
| Trust Security | Fluidity, LayerZero series, RAI unpaid | [trust-security.xyz](https://www.trust-security.xyz) |
| Zellic | Astar | [zellic.io](https://www.zellic.io/blog/finding-a-critical-vulnerability-in-astar/) |

## Search queries (re-run; do not scrape behind auth)

```
from:immunefi (payout OR "bugfix review" OR paid)
from:rektnews
"bugfix review" Immunefi
from:ottersec (CPI OR signer OR PDA)
from:BlockSecTeam (postmortem OR analysis)
from:SlowMist_Team
from:trailofbits (advisory OR vulnerability)
```

Nur does not log into X for you. If a Firecrawl/X scrape is available in-session, dump hits into a working note, then **promote** only rows that get a writeup URL into `notable-bounties.md` style cards.

## 2024-2026 X pointers

Canonical table with Playbook column: `hunting-x-linked-bounties/references/x-linked-2024-2026.md`. Do not load `skills/security/SCA/sc-research/library/_ingest/grok-x-bounties.md` into a review.

Sherlock 2024 judging still pays **ERC-4626 inflation** (Napier, Burve, Notional Exponent). OpenZeppelin 2024 Uniswap v4 periphery: 1 Critical + 1 High (resolved). Immunefi Alchemix V3 comp: liquidation fee overpay when collateral is gone.

## Gaps (honest)

- Private Immunefi reports stay private. This library cannot list them.
- 2026 dollar figures for KelpDAO / Drift / June multi-bridge differ across Immunefi, TRM, PeckShield, altfins. Prefer official postmortems.
- Many GregoAI / similar X threads lack a matching Immunefi blog at scrape time. Treat as **unconfirmed**.
- Hats / Sherlock on-chain payouts are messy to sum; use the contest report, not a screenshot.
- NFT/game chains and Hyperliquid custom VMs are under-indexed here on purpose until a public writeup exists.

## Rule

If the only source is an X screenshot of a dashboard, it does **not** go into a contest report. If X links to Immunefi/Mirror/forum, cite **that**.
