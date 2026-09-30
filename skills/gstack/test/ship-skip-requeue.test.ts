import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { ALL_HOST_CONFIGS } from '../hosts';
import { generateAdversarialStep, generateCrossReviewDedup, generateSharedCodeReuse } from '../scripts/resolvers/review';
import { HOST_PATHS } from '../scripts/resolvers/types';

const compact = (text: string) => text.replace(/\s+/g, ' ');
const review = compact(readFileSync(new URL('../ship/sections/review-army.md.tmpl', import.meta.url), 'utf8'));

describe.each(ALL_HOST_CONFIGS.map(({ name }) => name))('%s ship skip/requeue contract', host => {
  const ctx = { host, skillName: 'ship', tmplPath: '', paths: HOST_PATHS[host] };
  const dedup = compact(generateCrossReviewDedup(ctx));
  const adversarial = compact(generateAdversarialStep(ctx));
  const finish = adversarial.slice(adversarial.indexOf('### Finish the adversarial phase'));

  test('unchanged explicit skips are matched before the actionable queue drives a repeat', () => {
    const match = finish.indexOf("Apply Step 9.3's matching procedure");
    expect(match).toBeGreaterThan(-1);
    expect(match).toBeLessThan(finish.indexOf('2. **Fixes queued'));
    expect(finish).toContain('Only unmatched or reopened findings remain queued');
    expect(dedup).toContain('Only explicit `skipped` actions qualify');
    expect(dedup).toContain('If both history and the invocation action list lack decisions, classify normally');
    expect(dedup).toContain('Revalidated Skips suppress repeat questions and fixes');
    expect(dedup).toContain('Report the suppressed count once if nonzero');
    expect(dedup).toContain('reopen the finding; unrelated edits do not');
    const steps = ['1. **Validate severity.**', '2. **Read decisions.**',
      '3. **Match evidence.**', '4. **Match shared-code structurally.**', '5. **Apply dispositions.**'];
    const positions = steps.map(step => dedup.indexOf(step));
    expect(positions.every(position => position >= 0)).toBe(true);
    expect(positions).toEqual([...positions].sort((a, b) => a - b));
    expect(review).toContain('Classify only unmatched or reopened findings as AUTO-FIX or ASK');
    expect(review).toContain('after Step 9.3 matches all sources, including queued Steps 10–11 findings');
  });

  test('dedup and persistence include queued sources and preserve the explicit decision', () => {
    expect(dedup).toContain('exploratory QA and queued Steps 10–11 findings');
    expect(review).toContain('checklist, specialist, exploratory QA and queued Steps 10–11 records');
    expect(review).toContain('Save each explicit Skip immediately in the invocation action list');
    expect(review).toContain('preserve `advisory`, `evidence_paths` and `helper_target`');
  });

  test('working-tree changes and new evidence reopen matching identities', () => {
    expect(dedup).toContain('git diff --name-only <prior-review-commit>');
    expect(dedup).not.toContain('git diff --name-only <prior-review-commit> HEAD');
    expect(dedup).toContain('committed, staged, unstaged and non-ignored untracked source');
    expect(dedup).toContain('Changed inputs, proposal, behavior, risk or new evidence reopen the finding');
    expect(dedup).toContain('honoring later user decisions');
    expect(dedup).toContain('same fingerprint, advisory/defect kind and scope');
    expect(dedup).toContain('Compare supporting source and finding evidence with the saved decision');
    expect(dedup).toContain('as a shortlist, not proof');
  });

  test('fixed regressions and missing Skip proof are not suppressed', () => {
    expect(dedup).toContain('never `fixed`, `auto-fixed` or unanswered questions');
    expect(dedup).toContain('Missing proof or unknown comparisons require a fresh decision, not suppression');
    expect(finish).toContain('Keep scoped approvals');
    expect(dedup).toContain('Require the same fingerprint, advisory/defect kind and scope');
    expect(finish).toContain('Unvalidated historical Skips stay unmatched for the full Step 9 repeat below');
    expect(finish).toContain('never jump to 9.3 or mint a late REVIEW_START');
  });

  test('shared-code and advisory collisions retain stricter identity checks', () => {
    expect(dedup).toContain('they cannot suppress defects');
    expect(dedup).toContain('remove `advisory`, never downgrade severity');
    expect(dedup).toContain('Reject contradictory saved decisions');
    expect(dedup).toContain('requires re-reading all callers (including indirect callers) and the helper destination');
    expect(dedup).toContain('Missing metadata never permits ordinary line matching');
    expect(dedup).toContain('Prior-review reuse additionally requires the checker below; invocation decisions cannot replace it');
    const checker = compact(generateSharedCodeReuse(ctx));
    expect(checker).toContain('Only `reusable: true` permits suppression');
    expect(checker).toContain('False, command failure or unreadable output requires fresh source review');
    expect(checker).toContain('Do not supply your own snapshot, prior record or coverage');
  });

  test('skipped defects stay unresolved and required failures stay failed', () => {
    expect(dedup).toContain('not unresolved defects: retain them in counts, status and the final report');
    expect(dedup).toContain('Keep required-probe failures failed');
    expect(review).toContain('Skipping a fix is not risk acceptance or a passing probe');
    expect(review).toContain('Failed, blocked, inconclusive or not-run required probes mean false, never clean');
    expect(review).toContain('VERIFY_RESULT stays fail');
    expect(adversarial).toContain('retain the acknowledged findings and failed gate; do not report a clean review');
  });

  test('fresh review after edits, native coverage and cycle limits remain required', () => {
    expect(finish).toContain('**Required native review incomplete:** STOP');
    expect(finish).toContain('Insert Steps 9, 10 and 11 before the pending Step 11.5');
    expect(finish).toContain("never resets Step 9's three-cycle fix limit");
    expect(review).toContain('Set CYCLES to 0 on first entry only');
    expect(review).toContain('Increment CYCLES once if fixes were applied');
    expect(review).toContain('do not run a fourth fixing cycle');
    expect(finish).toContain('then continue to Step 11.5. Never jump directly to release preparation');
  });
});

test('ship invocation matching does not change standalone review routing', () => {
  const ctx = { host: 'claude' as const, skillName: 'review', tmplPath: '', paths: HOST_PATHS.claude };
  expect(generateCrossReviewDedup(ctx)).not.toContain('5. **Apply dispositions.**');
  expect(generateAdversarialStep(ctx)).not.toContain('### Finish the adversarial phase');
});
