# Plays per ops/phishing/key-theft vector (OPS01-20)

The ops layer is where most real money is lost (Bybit-class). Agents are increasingly the operators of drainer infrastructure and phishing fleets, so deception applies here too - but ONLY on defender-owned surfaces. Class ids per taxonomy; deep catalog in `historical-smart-contract-vulns` `references/investigator-ops.md`.

| ID | Agent hunts | Decoy play | Tripwire signal | Effect | Exemplar |
|----|-------------|-----------|-----------------|--------|----------|
| OPS01 | Safe/multisig UI lies | Your own signing infra: decoy "convenience" signing endpoint on a lookalike domain you own that records any submitted calldata | POST to decoy signer with real-looking calldata | I,D | Bybit $1.5B |
| OPS02 | Drainer-as-a-service integration | Defender-run decoy "drainer kit" repo (clearly fake keys, no working malware - it is a watched maze of docs + bait wallets) | Kit user's wallet first-touching bait | I,C | Inferno/Pink/Angel |
| OPS03 | Permit/Permit2 phishing targets | Canary approvals: bait wallets with watermarked permits published via poison channels | Permit consumed on bait wallet | I | tayvano_ campaigns |
| OPS04 | WalletConnect session hijack | Honeypot dapp (yours) that looks like a juicy victim frontend; session requests recorded | WC session to honeypot dapp | I,S | fake-connect family |
| OPS05 | Address poisoning (dust to your ops wallets) | Taint-monitoring: your ops wallets never interact with lookalike-address dust; alerts fire on it instead | Lookalike-address dust arrivals | I | poison-dust class |
| OPS06 | Support impersonation of YOUR team | Decoy "admin support" channel on a lookalike domain that records impostor scripts | Impostor engaging decoy channel | I,D | Genesis creditor 2024 |
| OPS07 | Fake meeting/link drops to your staff | Canaries in your own staff-facing docs; unusual link fetches from non-corp ranges alert | Canary fetch from attacker infra | I | fake-Zoom class |
| OPS08 | Password-manager/secret dumps | Honeypot secrets in decoy vaults/CI secrets with canary values; any use alerts | Canary secret used anywhere | I | LastPass fallout class |
| OPS09 | Blind-signing hardware flows | Decoy "firmware update" artifacts on your infra that record fetch+execute-attempts (inert files) | Fetch of decoy firmware from attacker infra | I | blind-sign class |
| OPS10 | Exchange/bridge key compromise | Canary API keys in decoy custody dashboards (read-only fake views) | Canary key used at any endpoint | I,D | Ronin/Harmony/Phemex |
| OPS11 | Rug/shill insider intel | n/a for defenders of honest protocols; do NOT run fake-rug entrapment on retail - out of scope/lawful gate | - | - | rug families |
| OPS12 | SIM attacks on your staff | Canaries in your domain-registration/telecom docs; registrar lock alerts | Canary touching + lock-change attempts | I | SIM families |
| OPS13 | NFT phishing (setApprovalForAll) | Decoy "NFT tool" dapp on your domain that records approval attempts to it | setApprovalForAll to decoy contract | I,S | BAYC 2021 |
| OPS14 | Leftover approvals on YOUR contracts | Same as `bait-funds-and-canaries.md` approved-spender traps | Drainer bot pulling from decoy allowance | I | leftover-approve class |
| OPS15 | Issuer freeze/CCTP lag exploits | Nothing to decoy; this is IR posture (pre-authorized freeze interplay). Deception adjacent: decoy "treasury routing" docs naming bait addresses | Bait-address funding observed | I | Circle Files; Drift CCTP |
| OPS16 | Custodian contractor insiders | Canary data per-vendor: each vendor's view of your ops docs contains a unique watermark | Watermark surfaces outside vendor | I,S | USMS 2026 |
| OPS17 | Signer-machine malware | Decoy "operational notes" files on signer machines that alert if exfiltrated/read externally | Canary file fetch off-machine | I | Radiant II class |
| OPS18 | Frontend JS supply chain | Your own CDN: integrity-locked, plus decoy JS endpoints that alert on tamper-probes | Fetch patterns hitting decoy endpoints | I,D | Bybit Sygnia |
| OPS19 | Adapter/chainlist poisoning | Decoy RPC endpoints (you control) with honeypot responses; watch for scanner abuse | RPC abuse patterns from attacker infra | I | adapter-poison class |
| OPS20 | Dust/noise recon of your wallets | Watchlists on ops-wallet dust with per-source marking | Anomalous dust sources clustering | I | dust-recon class |

## Rules specific to the ops layer

1. **Never weaponize real malware** - the decoy "drainer kit" is documentation + bait wallets + logs, no functioning payload. The lawful gate is absolute here; this row is the most abuse-prone idea in the whole family, so its decoy must be inert by construction.
2. Your own staff are the most common accidental tripwire-triggers: brief security-relevant staff that decoy artifacts exist (not where all of them are).
3. Ops-layer bait ages fast; refresh quarterly with the trends skill.
4. Every OPS decoy touch is ALSO potentially a real incident (Bybit-class events start as probes) - the response ladder (`identifying-agentic-attackers`) treats ops-layer touches with higher urgency than contract-layer probes.
