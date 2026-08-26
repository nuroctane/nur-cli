---
name: formal-verification-halmos-certora-kontrol
description: "Formal/symbolic properties on Solidity with Halmos, solc SMTChecker, Certora CVL, and Kontrol. Use when fuzzing is not enough for a conservation law. Slow path, like Mythril."
---

# Formal verification (Halmos, Certora, Kontrol, SMTChecker)

In-scope. Start with Foundry invariants. Promote the law to a prover when the user asks or when the property is small and huge-value.

## Halmos (a16z)

Foundry tests that use `vm.assume` / symbolic `uint256`. Run `halmos --function check_`. Good for arithmetic and access control. Bounded loops.

## SMTChecker

`solc --model-checker-engine chc` or `pragma` annotations. Fast on small contracts; explodes on complex DeFi.

## Certora CVL

Specs in `.spec` files. Use when the project already has Certora CI. Do not invent a full CVL suite unprompted.

## Kontrol (Runtime Verification)

Foundry + K. Use when the repo already has `kontrol` proofs.

## Mythril

Still in the Foundry skill. Timeout 300s on the highest-value contract only.

Output: a property that **proved**, a counterexample (turn into a Foundry test), or "bound exhausted" (not a pass).
