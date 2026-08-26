# Researcher gym (always as Foundry tests)

Do not skip the gym. Tools without taste produce detector spam.

## Order

1. **Ethernaut** (OpenZeppelin) - visibility, fallback, token, reentrancy, king, elevator, privacy, recovery, denial, shop, dex, puzzle, motorbike (proxy), double entry.
2. **Damn Vulnerable DeFi** (v4, Foundry) - flash loans, oracles, puppet, unstoppable, naive receiver, side entrance, the rewarder, selfie, compromised, free rider, backdoor, climber (timelock), wallet mining, ABI smuggling, withdrawal, compact.
3. **Cyfrin Updraft** - Solidity, Foundry, security course, MEV, assembly. X: [@PatrickAlphaC](https://x.com/PatrickAlphaC)
4. **Secureum** epochs + RACE quizzes.
5. **RareSkills** articles (proxy, 1155, 712, gas golf, security).
6. **Paradigm CTF** / **EthernautDAO** / **QuillCTF** / **Mr. Clue** style challenges when you want adversarial puzzles.
7. **DeFiHackLabs** - convert a **public** post-mortem into a Foundry regression. Playbook: `reconstructing-public-postmortems`.

## How to train (non-negotiable)

- Every challenge becomes a Foundry test in a personal repo.
- After solving, write the **invariant** that would have caught it.
- Map the challenge to an SWC id **and** a Solodit analog.
- Do not copy public exploit repos onto a machine with funded keys.

## Reading list (free, public)

- Trail of Bits [building-secure-contracts](https://github.com/crytic/building-secure-contracts)
- Foundry Book: fuzz + invariant chapters
- OpenZeppelin Contracts docs (especially upgrades, Governor, ERC-4626)
- Chainlink data-feeds docs (staleness, L2 sequencer)
- Uniswap v2/v3/v4 whitepapers + core reviews
- samczsun blog / [The Dark Forest](https://www.paradigm.xyz/2020/08/ethereum-is-a-dark-forest) (MEV context)
- pcaversaccio writings on proxies and EVM footguns

Playbook: `researcher-gym-and-curriculum`
