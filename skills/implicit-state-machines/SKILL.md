---
name: implicit-state-machines
description: Find the implicit state machines in a codebase, the lifecycle and mode logic spread across flags, nullable fields and status strings instead of one explicit state type, and report their states, transitions and the bugs they allow. Use when asked to find implicit state machines, audit lifecycle or status logic, or explain why something gets stuck or reaches an impossible state.
---

# Implicit state machines

An implicit state machine is lifecycle or mode logic that no single type
owns. Its states live in several booleans or Options that only make sense
together (`busy` + `cancelling` + `pending`), in status strings compared
ad hoc, in phase fields mutated from many call sites, or in ordering rules
kept only by call order or a comment. Each one is a place where the code can
reach a combination nobody designed.

This is an audit. Read and report; change code only when the user asks.

## Find candidates

Search the whole codebase, not one module. Skip build output and vendored
code. Good leads:

- Two or more booleans or Options on one struct, record or component that
  are read together (`if a && !b`), or reset together.
- Fields named `status`, `state`, `phase`, `mode`, `stage`, `kind`, or
  timestamps like `started_at` / `completed_at` / `cancelled_at` whose
  presence encodes progress.
- String or integer statuses compared with `==` or `match` in more than
  one file.
- An enum that is mutated directly in many places even though a
  transition helper exists (code that bypasses `transition_to`).
- UI components with `busy`, `loading`, `error`, `notice` or `saving`
  flags.
- Async flows where an event, timer, retry or cancellation changes the
  same fields as the main path.
- Comments such as "must be called after", "only when", "never both".

Where the project already has an explicit machine, note it, and check
whether any code bypasses it.

## Trace each one

For every machine, write down:

1. **Name and location**: file plus type and member names (not only line
   numbers).
2. **States**: the states it really has, and the variables that encode
   each one.
3. **Transitions**: each transition and the place that performs it.
4. **Problems**, each labelled:
   - **CONFIRMED**: you traced a concrete sequence of events that reaches
     it. Write the sequence.
   - **POSSIBLE**: the code allows it, but you did not trace a trigger.

   Look for:
   - contradictory flag combinations;
   - stuck states with no exit;
   - transitions that skip a cleanup, an audit write or a notification;
   - missing guards (a transition allowed from a state it should not be);
   - races between concurrent events;
   - a reset in one path but not another;
   - duplicated status logic that can drift.
5. **Recommendation**: one line, such as "replace `busy`/`cancelling`
   with `enum TurnState { Idle, Running, Cancelling }`". Say whether it is
   a bug fix worth doing now or a refactor that can wait.

## Report

Rank the machines by risk, most risky first. Put the CONFIRMED problems at
the top, then POSSIBLE ones, then machines that are implicit but currently
correct. Keep each entry short: the reader needs the states, the bad
sequence and the fix, not a tour of the code.

When fixing one, make the state a single type the compiler checks (a sum
type, a tagged union, a state enum with data per variant), route every
transition through one function that rejects invalid source states, and
add a test for the sequence that used to break.
