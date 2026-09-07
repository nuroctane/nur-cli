# Reference model and differential testing

## Building the reference model

1. **Scope to the core**: the paths that move value or accounting. Everything else (admin, periphery, events) is out of the model.
2. **Spec-faithful, not code-faithful**: the model implements what the DOCS promise. Where docs are silent, model the safest sane interpretation and record the assumption - that assumption list is itself audit output.
3. **Dumb and straight**: no gas optimization, no assembly, no inheritance webs, no custom errors - reads like a tutorial. Divergences must be semantically interesting, never syntactic.
4. **Same interfaces as the target** where practical, so test harnesses can point at either implementation.

## Differential testing patterns

### 1. Same-input diffing

Run identical call sequences against model and target; assert equal observable outcomes (balances, supplies, rates) within documented tolerance (rounding deltas). Divergence candidates: fee-on-transfer handling, rounding direction, edge amounts (0, 1, max).

### 2. Property mirroring

Each `property_` test from `discovering-protocol-properties` runs against BOTH implementations. Properties that hold on the model but fail on the target are your strongest findings - the model is the proof of concept by construction.

### 3. Fork mirrors (prior version exists)

If the protocol forked a known implementation (Compound-fork, Uniswap-fork): mirror each upstream function against the fork's version. Every intentional divergence needs a documented reason (docs or PR); undocumented divergences are where fork bugs live (the classic: forked the code, not the fix).

### 4. Spec property extraction

Spec sentences with numbers ("1% fee", "7-day timelock", "cap 10M") become literal assertions on the target. Cheap, mechanical, and the highest signal-per-line testing there is - spec drift is everywhere.

### 5. Deploy fixtures

Tests run against deployment-realistic state (real constructor args, real proxy wiring, real token addresses on a fork) - not `new Contract()` idealism. Deployment drift (wrong owner, uninitialized steps skipped) is a finding family that pure unit tests never see.

## Interpreting divergences

| Observation | Verdict path |
|-------------|--------------|
| Model holds, target fails | Finding candidate -> judging gates |
| Model fails, target "fails differently" | Model encoding bug OR undocumented intended behavior - check docs/PRs, then re-judge |
| Both hold | Coverage gained; move to next property |
| Both fail | Your shared assumption is wrong - usually a spec misreading worth reporting as design ambiguity |

## Cost control

Model the core only; diff on shared inputs; keep tolerances documented. A model twice the size of the target is a second audit, not a tool.
