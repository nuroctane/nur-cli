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

After grep: open the function, ask who can call it, what it trusts, what it calls, what invariant it can break. Then a Foundry test. Never jump from a grep hit to a mainnet tx.
