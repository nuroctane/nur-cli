# Solana / SVM

EVM playbooks will miss these. Load `reviewing-solana-programs` for the hunt loop. Firms: OtterSec, Neodyme, Neodyme "Solana security" classes, Coral/Anchor docs.

## Threat model (one sentence)

A Solana instruction is a list of **accounts + data**. If the program does not check **owner, discriminator, signer, expected address, canonical bump, and callee program id**, the runtime will still execute.

## Classes (S01-S10)

### S01 Missing signer

- **Wrongly trusted:** `AccountInfo` with the admin pubkey is the admin.
- **Seeds:** `Signer`, `is_signer`, `#[account(signer)]`, `authority`.
- **Local test idea:** Anchor: invoke with the right pubkey in the account list but `signer: false`. Must fail.
- **Analog:** Neodyme class-1; many drainers.

### S02 Account confusion / fake account

- **Wrongly trusted:** data that deserializes is the real mint / vault / obligation.
- **Seeds:** `owner == program_id`, 8-byte discriminator, `has_one`, `address =`.
- **Local test idea:** pass a same-layout account owned by the attacker program.
- **Analog:** **Cashio** fake collateral; Port Finance obligation logic ($630k bounty - withdraw without full debt).

### S03 Arbitrary CPI

- **Wrongly trusted:** `invoke` to a user-supplied program id.
- **Seeds:** `CpiContext`, `invoke`, `program` account unconstrained.
- **Local test idea:** CPI to a mock program that returns success and writes attacker state. Real program id must be hardcoded or in a PDA config you verified.

### S04 PDA non-canonical bump

- **Wrongly trusted:** any bump that finds *a* PDA.
- **Seeds:** `find_program_address`, stored `bump`, `seeds =`, `bump =`.
- **Local test idea:** create a PDA with a non-canonical bump if the program allows; it must not authenticate as the canonical authority.

### S05 Sysvar spoof

- **Wrongly trusted:** "instructions sysvar" account is `Sysvar1nstructions...`.
- **Seeds:** `load_instruction_at`, `instructions::ID`, `sysvar`.
- **Local test idea:** pass a fake account with crafted instruction bytes. Must revert unless key == real sysvar.
- **Analog:** **Wormhole 2022 ~$326M**. This is the SVM cousin of T12 (the $10M bounty was the *other* Wormhole bug: uninitialized EVM proxy). Do not mix them.

### S06 Close / reinit

- **Wrongly trusted:** `lamports == 0` means gone forever.
- **Seeds:** `close =`, `CLOSE_ACCOUNT`, zeroing data before close.
- **Local test idea:** close, then the attacker reopens the same PDA with hostile data before the victim's next ix.

### S07 Token-2022 transfer hook

- **Wrongly trusted:** transfer amount in the token program cannot be rewritten by a hook.
- **Seeds:** transfer hook program, `TransferFee`, confidential transfer.
- **Local test idea:** hook that tries to CPI back into the vault mid-transfer (reentrancy analog).

### S08 CPI signer seeds

- **Wrongly trusted:** `invoke_signed` seeds match the PDA you think.
- **Local test idea:** wrong seed list still signs a different PDA.

### S09 Tick / CL math

- **Analog:** **Raydium $505k** tick manipulation (Immunefi).
- **Local test idea:** ghost concentrated-liquidity invariant after swap; attacker-controlled tick arrays cannot skip net liquidity.

### S10 Oracle + governance + ops

- **Analog:** **Mango 2022** (oracle + thin book + gov); **Drift 2026 ~$285M** (TRM: long social engineering + gov/oracle - **not** a missing `Signer` in one ix).
- **Review:** admin keys, oracle session, insurance fund, keeper bots. If the bug is a stolen key, say **ops**. Do not invent a program bug.

## Other SVM historical names

| Name | Class | Notes |
|------|-------|-------|
| Slope | ops | wallet mnemonic; not a program |
| Crema | S09-ish | fee / tick |
| Wormhole portal (EVM side) | T12 | $10M bounty |
| Wormhole (Solana side 2022 hack) | S05 | $326M loss |
| marginfi flash loan | S03/T09-adj | Asymmetric.re contained |
| Kamino | | $1.5M Immunefi ceiling (program) |

## Grep / ripgrep (Anchor)

```
invoke\(|invoke_signed\(|CpiContext
is_signer|Signer
owner
find_program_address|create_program_address
load_instruction_at|instructions::
close\s*=
token_2022|transfer_hook
unchecked_account|AccountInfo
```

Skip `target/`, `docs/`. After a hit: who can pass this account, what owner is required, what happens if CPI returns Ok with hostile leftover accounts.

## What not to do

No mainnet drain scripts. No copying Wormhole exploit transactions from a funded key. Fork tests: `solana-test-validator` / LiteSVM / Anchor localnet only.
