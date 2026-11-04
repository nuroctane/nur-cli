---
name: regression-guard
description: Guard a landed fix against regression by analyzing its diff for test-coverage gaps. Use after a bug fix lands, or when asked whether a fix is covered, what tests it needs, or how the bug could have been caught earlier.
---

# Regression guard

A fix without a test for its cause will regress silently. Work from the
landed diff, not from the bug report's memory of it.

1. **Read the diff.** `git diff` or `git show` for the fix commit. List
   every behavior the diff changed, including edge cases it touched
   indirectly (bounds, empty inputs, error paths, ordering).
2. **Map coverage.** For each changed behavior, find the test that covers
   it. A test that passes with the fix and fails without it is coverage;
   a test that passes either way is not. Name the uncovered behaviors
   explicitly.
3. **Generate the missing tests.** Write a unit or regression test per
   uncovered behavior, smallest first. Each test names the behavior, the
   input, and the expected output, and fails without the fix.
4. **Audit how the bug could have been caught earlier.** Ask, per
   uncovered behavior: what test, check, or review step would have caught
   it before it shipped? Record the answer as a concrete addition (a
   test, a lint rule, a checklist item), not as a general lesson.
5. **Report.** Ranked list: covered behaviors, uncovered behaviors with
   their new tests, and the earlier-catch additions. Change nothing
   beyond the tests the user picks, unless the user asked for the full
   guard pass.
