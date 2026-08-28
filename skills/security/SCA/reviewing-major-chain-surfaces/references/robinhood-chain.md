# Robinhood Chain

The flagship Arbitrum Orbit RaaS launch of 2026: Robinhood's own L2 carrying tokenized U.S. stocks/ETFs ("Stock Tokens") plus AI-assisted trading features. Live on public mainnet early July 2026 after a testnet week processing 4M+ txs ([Arbitrum blog](https://blog.arbitrum.io/robinhood-chain-mainnet/); [robinhood.com/us/en/chain](https://robinhood.com/us/en/chain/)).

## Stack & security model

- Built with Arbitrum's platform stack (Orbit RaaS family), settling up through Arbitrum to Ethereum inheritance. 100ms block cadence claimed at launch.
- Sequenced/operated by Robinhood - single-operator chain. Everything from this dossier's Orbit playbook applies, plus: one party controls inclusion AND ordering AND pausing. Assume sequencer-level freeze/reorder capability in your threat model at all times (not "futureMEV", current capability).
- Compliance rails matter more than usual: transfer restrictions/hook-driven securities compliance on Stock Tokens mean token behavior depends on issuer-controlled policy contracts. Auditing RWA tokens here = auditing the *policy engine* (who flips what flag, timelocked?) more than arithmetic. Read `reviewing-token-standard-pitfalls` hooks section first, then come back.

## What breaks specifically on RWA stock-token rails

Settlement and market hours are off-chain truth:

- Price oracles reference equity markets closed nights/weekends/holidays - staleness params must model market sessions, not just seconds-counting. A "price" 20 minutes old is expected Sunday; the same age Tuesday mid-session is a finding.
- Corporate actions (dividends, splits, halts) mutate underlying value outside any EVM event; guard logic must distinguish "market closed" from "feed broke".
- Redemption/off-ramp path through the issuer is the real custody boundary: mint/burn authority, bank rails behind it, and which signatures move reserves. That is `reviewing-frontend-and-ops-surfaces` + `investigator-ops.md` territory combined with contract review - the Bybit-class loss mode is operational, not Solidity.
- 24/7 crypto DEX venues listing these tokens trade against equities truth mismatch -> protocol designs bridging the two (perps on stock tokens, lending markets using them as collateral) carry basis-risk/exploit windows exactly where feeds disagree. Grep margin engines for how they mark during non-market hours.

## Bridge/messaging

- Canonical = Orbit native gateway up through Arbitrum One; third-party messaging (LayerZero/CCTP routes) appears once secondary assets arrive. Post-KelpDAO (single-DVN config, April 2026), ANY security-module config on these bridges deserves specific scrutiny: quorum-of-one verifier sets are the exact failure shape that lost ~$292M (`evm-fork-deltas.md`; `hunting-x-linked-bounties` rows).

## Oracles

- Equity price distribution is Robinhood-controlled at launch; third-party ecosystem apps consuming those feeds via endpoints inherit an unauditable trust root. Treat any external app quoting "RH-mark price" like a private oracle: verify attestation/signature scheme before trusting updates.

## Review checklist (RWA-L2 specific)

1. Who can pause transfers, force-freeze an address, or upgrade policy? Timelock? Multisig quorum? Single key?
2. Are epoch/cutoff boundaries (market close) handled atomically? State mutations striding session boundaries?
3. Redemptions: burning order vs wire release - can burn happen while fiat leg fails, or double-pay?
4. Governance keys on the contract set vs the company's ops perimeter - map them both.
5. Any second-layer app (lending/perp) treating stock tokens like crypto collateral - weekend marking math tested?

## Status caveats

Fresh chain; no public exploit history as of 2026-08 refresh. The interesting bugs will be in the interplay layers above, not the sequencer. Re-check official docs whenever product scope changes, since every new asset class moved onto the chain widens the oracle and policy surfaces.

Sources: [Arbitrum blog](https://blog.arbitrum.io/robinhood-chain-mainnet/), [Arbitrum Foundation builders block](https://blog.arbitrum.foundation/builders-block-021-robinhood-chain-mainnet-live-on-arbitrum-founder-house-london-winners/), [Goldsky data support](https://goldsky.com/blog/robinhood-chain-data-live-on-goldsky), [Cobo launch summary](https://www.cobo.com/agentic-wallet/news/robinhood-chain-mainnet-launches-with-tokenized-stocks-and-ai-trading-in).
