# Corpus calibration (real findings data)

Calibrate severity calls and dedup instincts against real contest-finding corpora instead of vibes. Primary public corpus: Zaevlad/audit-findings-dataset on Hugging Face (contest findings with Code4rena-style and Sherlock-style provenance).

## Dataset schema (what you can learn from it)

| Field | Content | Calibration use |
|-------|---------|-----------------|
| `bug_title` | Finding title | Title-style norms: mechanism + consequence |
| `bug_desc` | Description with code snippets | How real findings localize the root cause |
| `bug_poc` | PoC or "no poc" | Reality check: many published findings shipped WITHOUT executable PoCs - your bar can be higher |
| `bug_rec` | Remediation | Fix-style norms; feeds the fix-preservation gate |
| `bug_sev` / `bug_sev_raw` | Normalized + original severity | Severity drift between platforms - calibrate before cross-platform comparisons |
| `bug_weight` | Numeric weight 0-1 | Continuous severity rather than 4 buckets; useful for ranking within a severity |
| `bug_full` | Long-form narrative (root cause, trigger, impact, fix) | Report-writing style reference |

File names encode provenance (`2023-05-maia_H-04_chunk_1.json` = contest, protocol, finding id). Submitter text identifies Sherlock-style rows. License is non-standard - consult the dataset card before redistribution; fine for internal calibration.

## Calibration exercises (do these quarterly or when judging feels off)

1. **Severity drift audit**: pull 30 findings across severities; judge them blind with `auditing-multi-pass-review-lenses` `references/finding-standard-and-judgement` gates; compare. Systematic offsets (you call M what contests call H) are exactly the calibration you need before a contest.
2. **Family frequency**: tally root-cause families across a slice; check against `defi-security-trends-standards` trend tables - contest families and loss families DIFFER (contests over-index rounding/access; losses over-index logic/oracle).
3. **PoC-rate reality check**: measure what fraction of published H/M findings include executable PoCs; set your personal bar above the median.
4. **Dedup drills**: take 20 same-protocol findings; practice group_key assignment; compare against platform dedup decisions where visible.

## Severity matrix cross-check

Platform alignment: Immunefi's severity matrix (critical/high/medium/low by impact class) governs bug-bounty reports; contest platforms (C4/Sherlock) use their own judged scales; the Plamen-class tooling aligned its matrix to Immunefi v2.x `[verify current version]`. When writing reports: use the TARGET PLATFORM's scale, and state which scale you used - never mix scales in one report.

## Related pack files

- `triaging-and-deduping-findings` SKILL.md (detector-noise pipeline - this file is the data-driven supplement)
- `auditing-multi-pass-review-lenses` `references/dedup-and-completeness.md` (group_key mechanics)
- `contest-and-bounty-reporting` (platform-specific report formats)
