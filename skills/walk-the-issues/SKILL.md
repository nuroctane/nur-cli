---
name: walk-the-issues
description: Groom and clear an open-issue backlog, one issue at a time, through branch, research, implementation, test, and pull request. Use when asked to clear, groom, triage, or work through open GitHub issues.
---

# Walk the issues

Work the backlog in a loop, one issue at a time, and never let a
half-finished issue block the next one.

1. **Groom.** List all open issues. Close duplicates and stale issues
   only where the user or the repo's policy says to; otherwise rank by
   the user's priority, then by dependency order. Record the ranked list
   before starting any implementation.
2. **Per issue, in order:**
   - Branch (or worktree) per issue, named for the issue, so one
     issue's work never contaminates the next.
   - Research: read the issue, the code it points to, and the relevant
     tests. Use `sgrep` and `read_file` before writing anything.
   - Implement the smallest change that closes the issue, following the
     repo's existing conventions.
   - Test: run the reproduction or the new test, plus the surrounding
     suite. A fix without a passing test is not done.
   - Pull request: one PR per issue, naming the issue it closes, what
     changed, and how it was tested.
3. **Loop and report.** After each issue, report it closed (or why it
   could not be closed, with the blocker named) and move to the next.
   At the end, report the closed list, the blocked list with blockers,
   and the issues deliberately left open.

## Rules

- One issue, one branch, one PR. A PR that closes three issues at once
  cannot be reviewed or reverted cleanly.
- Never force-push, never rewrite another contributor's branch, and
  never close an issue the work did not actually close. A blocked issue
  stays open with its blocker recorded.
- Credentials for issue tooling go through the `secret` tool and `$NAME`
  only, never pasted into a PR, a comment, or a commit message.
