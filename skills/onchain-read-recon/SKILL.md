---
name: onchain-read-recon
description: "Read-only on-chain recon for verified Solidity: cast, Sourcify, storage layout, public transaction traces. Use before an in-scope audit or to reconstruct an already-mined incident. Never broadcast, never unlock wallets."
---

# On-chain read recon

Recon is **read-only**. Verified source, storage slots, and already-mined traces. No mempool watching for sandwiching, no attack broadcasts, no funded-key hosts.

If the user wants to "replay the hack on mainnet", refuse the broadcast and offer a local fork test instead.

## Allowed tools

- `cast` (Foundry): `call`, `storage`, `code`, `sig`, `4byte`, `age`, `receipt`, `logs`
- Sourcify / Etherscan-family verified source (match bytecode hash)
- `forge inspect` storage layout on local build of that verified source
- Tenderly / Phalcon / Openchain traces of **mined** txs
- Public RPCs (`eth_call`, `eth_getLogs`) 

## Not allowed

- `cast send` / `forge script --broadcast` of anything that moves victim funds
- Loading a plaintext mainnet key or mnemonic into the environment
- Running unverified bytecode in a debugger on a machine with unlocked wallets

## Steps

### 1. Pin the artifact

```bash
cast code <addr> --rpc-url $RPC
# confirm verified source (Sourcify / explorer) matches this bytecode
cast age <addr> --rpc-url $RPC
cast implementation <proxy> --rpc-url $RPC   # ERC-1967 if proxy
```

Record chain id, address, implementation, commit or explorer compiler settings.

### 2. Map storage

```bash
cast storage <addr> <slot> --rpc-url $RPC
forge inspect src/Foo.sol:Foo storage --pretty
```

Name the slots you will reason about (owner, totalSupply, packed balances). Do not brute-force private keys.

### 3. Trace a public tx (incident / known attack)

Only for txs already on-chain (rekt writeup, Immunefi published, user's own tx):

```bash
cast receipt 0x<tx> --rpc-url $RPC
cast run 0x<tx> --rpc-url $RPC     # local replay, no broadcast
```

Reconstruct: caller, callee, value, which storage slots changed, which external calls. Then write a **fork test** that asserts the same state transition.

DarkNavy `exploit-investigator` (if installed via the web3-skills pack) is this same job with a heavier loop. Same rule: public txs / authorized IR only.

### 4. Hand off

- Pre-deploy / contest: continue with `auditing-foundry-smart-contract-security`.
- Pricing surface: `reviewing-oracles-and-pricing`.
- Proxy: `reviewing-upgradeable-proxies`.
- Filing: `contest-and-bounty-reporting`.

## Expected output

A recon note: chain, addresses, verified-source match, proxy impl, interesting slots, and (if any) the public tx hash reconstructed. No calldata to broadcast.
