# Grep seeds (in-scope source only)

Search production Solidity (`src/`, `contracts/`). Skip `lib/`, `node_modules/`, `out/`, `test/` unless the user named those files.

These are **review seeds**, not exploit recipes.

## Auth / upgrade

```
onlyOwner|onlyRole|onlyAdmin|require\(msg\.sender
tx\.origin
initialize\(|reinitializer|upgradeTo|_authorizeUpgrade
selfdestruct|delegatecall
```

## Value / accounting

```
transfer\(|transferFrom\(|safeTransfer|call\{value
totalAssets|convertToShares|convertToAssets
mint\(|burn\(|_mint|_burn
balanceOf|totalSupply
unchecked
```

## Price

```
getReserves|slot0|latestAnswer|latestRoundData|consult\(|observe\(
twap|oracle|getPrice|peek\(
```

## Callbacks / reentrancy

```
onERC721Received|onERC1155Received|tokensReceived
uniswapV2Call|uniswapV3SwapCallback|uniswapV3MintCallback
receive\(|fallback\(
nonReentrant
```

## Signatures

```
ecrecover|ECDSA|permit\(|_hashTypedDataV4|nonce
abi\.encodePacked
```

## Tokens / weird ERC-20

```
approve\(|increaseAllowance|fee|rebase|decimals\(
safeTransferFrom
```

## Bridges / messaging

```
lzReceive|_lzReceive|onMessage|relay|processMessage
xDomainMessageSender|messenger
```

## Governance

```
queue\(|execute\(|timelock|propose\(|castVote
```

## Solana / Anchor (in-scope `programs/`)

```
invoke\(|invoke_signed\(|CpiContext
is_signer|Signer
find_program_address|create_program_address
load_instruction_at|instructions::
unchecked_account
transfer_hook|token_2022
```

## Move

```
borrow_global|share_object|public entry
has key|TreasuryCap|AdminCap
```

## Cosmos / CosmWasm

```
MsgTransfer|ibc-hooks|submessage|reply
x/authz|AnteHandler|ics23
```

## Cairo

```
u256_div|felt252|u256
```

After grep: open the function, ask who can call it, what it trusts, what it calls, what invariant it can break. Then a **local** test (Foundry / Anchor / Move / cw-multi-test). Never jump from a grep hit to a mainnet tx.

If the incident is Bybit / a drainer / Permit2 phishing / stolen keys, stop grepping Solidity and open `historical-smart-contract-vulns/references/investigator-ops.md`. If the user named a **paid Immunefi/X writeup**, load `hunting-x-linked-bounties` and grep the Playbook column, not this whole file.
