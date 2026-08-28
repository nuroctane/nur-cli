# Hyperliquid (HyperCore + HyperEVM)

The highest-volume perp DEX venue through 2025-2026. Two distinct worlds auditors must not conflate:

- **HyperCore** - the exchange/perps engine itself: a closed-source orderbook engine with its own BFT validator consensus, NOT an EVM. Code is protocol-defined, not public contracts. You audit *behavior* (marking, liquidations, socialization rules), not source.
- **HyperEVM** - an EVM execution environment launched Feb 2025 sharing state/risk plumbing with HyperCore. Contracts here are normal Solidity with unusual precompile-mediated side effects.

## The defining incidents

- **JELLY squeeze / "validator put" (Mar 26 2025)**: attacker opened a leveraged short on the JELLY memecoin perp, self-liquidated into HLP (the shared market-making/insurance vault), then pumped spot ~429% so forced settlement ballooned HLP exposure ($10-13M projected, potentially vault-draining). Response: validators delisted JELLY and force-settled at team-chosen prices within minutes. Sources: [Oak Research postmortem](https://oakresearch.io/en/analyses/investigations/hyperliquid-jelly-attack-context-vulnerability-team-solution), [Halborn explainer](https://www.halborn.com/blog/post/explained-the-hyperliquid-hack-march-2025), [Blockworks analysis](https://blockworks.com/news/hyperliquid-decentralization-validator-put). Audit translation: thin-market ADL/socialization paths and oracle-mark feedback loops are code-expressible risk; the fix that deployed was *governance intervention*, which is itself a solvency assumption your review should surface explicitly.
- HLP TVL fell from ~$540M to ~$150M weeks after - insurance funds can be priced OUT by users post-incident; model run-on-insurance dynamics for any shared-loss design.
- Validator set criticism (small count, high operator concentration) persisted as the standing decentralization objection `[verify current count]`.

## HyperEVM specifics (where contract bugs live)

- **HyperCore <-> HyperEVM read/write precompiles**: system-address-mediated writes gated during partial blocks. Contracts assuming atomic cross-environment effects get TOCTOU'd around block boundaries. Read upstream docs on write/read action availability windows before reviewing anything bridging balances between Core and EVM.
- No public mempool; ordering effects flow from the closed engine's tx stream, so MEV reasoning imported from Ethereum does NOT transfer.
- Native HYPE has no ERC-20 wrapper canonical form everywhere - spend/permit math differs by token standard actually used per deployment.
- Oracles: spot marks derive from the Core engine; EVM-side price consumers depend on those internal marks + whatever push/precompile mirror they consume - freshness and manipulation windows must be reasoned against Core data, not AMM pools.

## Ecosystem DeFi on HyperEVM

Perp-adjacent lending protocols (HypurrFi/Kinetiq/Felix-class) suffered small-to-mid oracle/market-config exploits mid-2025 (`[verify individual cases]` before citing). Class pattern: protocol misconfiguring perp-market rates or using manipulable internal marks -> repeated T05/T14 findings.

## Review checklist

1. Any asset moving Core<->EVM: identify the exact window when writes apply; test flows straddling boundaries.
2. Insurance/socialization exposure: does user loss waterfall silently feed HLP or similar? Model worst-case mark moves per parameter blocks (JELLY math).
3. Governance/backstop assumptions stated in docs but enforced by validator vote = document as trust dependency.
4. Cross-engine liquidation latency: positions whose collateral lives in one world and liability in the other.

## Non-goals

Do not try to "audit" Core itself from outside; recon trades via `onchain-read-recon` equivalents (Hyperliquid explorer/API), watch official incident reports, and pin findings to observable behavior, not assumed internals.
