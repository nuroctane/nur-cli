---
name: prior-art-first
description: Look for maintained, clearly licensed open-source implementations before building unfamiliar functionality from scratch. Use when a task needs a feature, algorithm, protocol, parser or integration you have not built before, or before adding a dependency.
---

# Prior art first

When a feature is unfamiliar, do not start writing it from zero. Someone has usually solved it,
hit its edge cases and fixed them. Find that work, compare it, and adapt the proven design.
This saves the rewrite you would otherwise do after discovering the same edge cases yourself.

## When this applies

- A parser, protocol client, file format, algorithm, sync or retry scheme, auth flow,
  rate limiter, diff or merge routine, or any integration with an outside service.
- Before adding a dependency (compare it with the alternatives first).
- Not for small glue code, or for patterns the repository already has: reuse those.

## Steps

1. **Search the repository first.** It may already have this, or a helper that does half of it.
2. **Search for implementations.** `gh search repos "<the job in plain words>" --sort stars
   --limit 10`, `gh search code "<distinctive API or term>" --limit 20`, plus `web_search` for
   docs, specs and write-ups. Search by what the code does, not by a library name you remember.
3. **Qualify each candidate** before reading it closely:
   - License: `gh api repos/OWNER/REPO --jq .license.spdx_id`. Prefer MIT, Apache-2.0, BSD,
     ISC, MPL-2.0. Treat GPL/AGPL/SSPL, "NOASSERTION" and no license as do-not-copy.
   - Activity: last push, open-to-closed issue ratio, release cadence
     (`gh api repos/OWNER/REPO --jq '.pushed_at, .open_issues_count'`).
   - Known problems: `gh search issues "<feature> bug" --repo OWNER/REPO`.
4. **Compare two or three** on approach, dependencies, size, platform support and the issues
   users hit. Write the comparison down in a few lines before choosing.
5. **Decide** one of: depend on it, adapt its design in your own code, or write it yourself
   because nothing fits (say why).
6. **Respect provenance.** Copy code only when its license is compatible and its origin is
   clear, keep its notice, and record the source (repo and commit) in a comment or NOTICE file.
   Never paste code of unknown origin, from incompatible licenses, or from a model's memory of
   someone else's repository.

## Report

Before implementing, tell the user in two or three sentences what you found, what you chose and
why, including the license. Then build.

Source: adapted from a rule shared by @YohoYoung6810 ("when a feature is unfamiliar, search
actively maintained, clearly licensed GitHub projects first; compare approaches, dependencies and
known issues; prefer proven designs; never copy incompatible or unclear-provenance code").
