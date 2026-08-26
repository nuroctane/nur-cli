# Whitehat disclosure and lawful use

This pack is for **defensive research**: pre-deploy review, in-scope contests, and published bug-bounty programs.

## Allowed

- Code you own or a client has hired you to review.
- Contest / audit-competition repos with a public scope file (Code4rena, Sherlock, Cantina, Hats, Immunefi audit comps).
- Immunefi (or similar) programs whose **assets-in-scope** page you have read this session.
- Read-only reconstruction of **already-mined** public transactions and published post-mortems.
- Local Foundry tests, fork tests against public RPC, and static analysis on in-scope source.

## Not allowed

- Targeting a live protocol with no published program, or anything marked out of scope.
- Writing drain scripts, exploit payloads, or "how to steal from X" runbooks.
- Broadcasting attack transactions, sandwiching users, or griefing mainnet/testnet.
- Running untrusted bytecode on a host with unlocked / plaintext funded keys.
- Social engineering of protocol teams, or leaking an undisclosed finding except through the program's official channel.

## Disclosure order

1. Confirm in-scope (asset, chain, commit hash, known issues).
2. Reproduce in a **local** Foundry test (or fork test that does not broadcast).
3. File through the program portal (Immunefi / contest finding form). Do not tweet first.
4. If there is no program and you still found a bug in software you do not own: stop and tell the user to use the project's published security contact. Do not "helpfully" drop a PoC in public.

## Severity language

Use the program's own scale (Immunefi, C4, Sherlock). If none: impact (funds lost/locked, auth bypass, grief) x likelihood. Critical/High blocks a deploy recommendation.
