# Git forensics classes (seven analyses)

Run against the in-scope branch (HEAD). Interpretation is the audit work; the git invocations are one-liners. All classes from x-ray-class methodology.

## The seven sections

| # | Section | What it answers | How |
|---|---------|-----------------|-----|
| 1 | repo_shape | Is there usable history at all? A single-commit `squashed_import` has NO evolutionary signal - say so and move on | `git log --oneline \| wc -l`, branch topology |
| 2 | fix_candidates | Which past commits look like security fixes? Each fix-shaped commit marks the class of bug this codebase already produced once | Subject/body regex: `fix`, `security`, `revert`, `cve`, `audit`, `overflow`, `reentrancy`, `patch`, `hotfix`; score by touched-path danger |
| 3 | dangerous_area_changes | Is value-handling code a churn hotspot? High churn + value logic = softest area | `git log --numstat` over in-scope paths, weight by value-flow files |
| 4 | late_changes | Code modified near the end of history (pre-announcement/deploy) is the temporal risk proxy - late = less reviewed, less tested | Last-N commits diff concentration by file |
| 5 | forked_deps | Copied external code carries its lineage's known-attack history; fingerprint vendor code, then map to that lineage's exploit classes | Grep license headers/paths (`lib/`, `vendor/`), diff against upstream tags where available |
| 6 | tech_debt | TODO/FIXME/HACK density by area; commented-out guards; version-pinned libs with known advisories | Grep + `package`/`remappings` review |
| 7 | dev_patterns | Team habits that predict bugs: force-push rewriting history, giant squash merges, "quick fix" chains touching the same file repeatedly | `git log` patterns, reflog if available |

## Fix-candidate scoring (what makes one worth reporting)

A commit scores toward report-worthiness when it: touches value-moving code (2), fixes a class in this pack's taxonomy (2, name the class id), reverts a recent feature (1), was deployed hot (followed by deploy/upgrade commit, 1). Score >= 5: report as a risk area with the historical bug class it evidence - the SAME class is the first thing to hunt in current code (`plays` thinking: past bug -> grep every sibling).

## Interpretation rules

1. **History is a map, not a finding.** Fix-shaped commits direct attention; the finding still needs the four-gate treatment (`auditing-multi-pass-review-lenses`).
2. **Late-change review beats average review.** Weight lenses 1-2-5 on late-touched files.
3. **Forked-dep lineage table**: for each vendored lib, record upstream, version, known incidents (search `historical-smart-contract-vulns` case cards), and whether this codebase patched or froze the vulnerable pattern.
4. **Anomaly honesty**: squashed/no-history repos get one line in the report - absence of evidence about process is itself process evidence.

## Feeding the pack

- Fix candidates + hotspots -> priority order for `auditing-multi-pass-review-lenses` passes.
- Forked-dep table -> `static-analysis-slither-aderyn-semgrep-wake` detector expectations.
- Churn + test-signal counts -> `auditing-foundry-smart-contract-security` coverage gaps.
