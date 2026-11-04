---
name: debug
description: Debug a failing behavior deterministically instead of guessing at fixes. Use when a bug, crash, failing test, or wrong output is reported, or when asked to find why something broke.
---

# Debug

A fix that is not tied to a reproduction is a guess. Work the seven
phases in order, and do not skip to the fix.

1. **Symptom capture.** Write the exact symptom: what was run, what was
   expected, what happened instead, with the command, the output, and the
   file:line where it surfaced. A symptom without a command is not
   captured yet.
2. **Git archaeology.** Find when the behavior changed: `git log` on the
   files involved, `git diff` against the last known good state, and
   `sgrep` for the behavior's name. If nothing changed in the code, the
   change is in input, environment, or a dependency: check those next.
3. **Reproduce with a targeted test.** Write the smallest reproduction
   that fails: a failing test, a command, or a script that prints the
   wrong output. Run it and confirm it fails for the same reason as the
   symptom. If the reproduction does not fail, the symptom is not
   understood yet: go back to phase 1.
4. **Log inspection.** Read the logs around the failure, not just the
   last line. Note what happened immediately before the failure, and
   what did not happen that should have.
5. **Hypothesis.** State one hypothesis at a time, with the check that
   would prove or disprove it. Rank by evidence, not by familiarity.
   Test the check, record the result, and move on. Do not stack fixes
   for several hypotheses at once: stacked fixes hide which one worked.
6. **Fix and green confirmation.** Fix the single confirmed cause, then
   run the reproduction from phase 3 and confirm it passes, plus the
   surrounding test suite for the area. A fix that passes its own test
   and breaks its neighbours is not a fix.
7. **Regression test and commit.** Leave a regression test that fails
   without the fix and passes with it. Commit only what the fix and its
   test need, with a message naming the symptom and the cause.

## Rules

- Never edit code before phase 3 has a failing reproduction, unless the
  user asked for a speculative fix and said so.
- Never claim a fix from a single passing run when the failure was
  intermittent: run the reproduction three times, or name the flakiness
  and what would make it deterministic.
- Credentials in a reproduction go through the `secret` tool and `$NAME`
  only, never pasted into a test, a log, or a file.
