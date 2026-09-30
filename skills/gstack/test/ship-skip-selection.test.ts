import { expect, test } from 'bun:test';
import { E2E_TIERS, E2E_TOUCHFILES, LLM_JUDGE_TOUCHFILES, selectTests } from './helpers/touchfiles';
import { computePaidCaseSelection } from '../scripts/test-paid-shards';

const id = 'ship-skipped-queued-finding';

test.each([
  'scripts/resolvers/review.ts', 'scripts/resolvers/index.ts',
  'ship/sections/review-army.md.tmpl', 'ship/sections/adversarial.md.tmpl',
  'ship/sections/manifest.json', 'bin/gstack-review-log', 'bin/gstack-review-read',
  'bin/gstack-slug', 'bin/gstack-config', 'bin/gstack-brain-enqueue',
  'lib/review-evidence.ts', 'test/helpers/ship-skip-actor.ts', 'test/helpers/scratch-repo.ts',
  'test/skill-e2e-ship-skip.test.ts', '.github/docker/Dockerfile.ci',
])('%s selects the native queued-Skip regression', file => {
  expect(selectTests([file], E2E_TOUCHFILES).selected).toContain(id);
  expect(E2E_TIERS[id]).toBe('gate');
});

test('native-only fixture files do not select quality judges', () => {
  expect(selectTests(['test/helpers/ship-skip-actor.ts', 'test/ship-skip-actor.test.ts',
    'test/skill-e2e-ship-skip.test.ts', 'test/helpers/scratch-repo.ts'], LLM_JUDGE_TOUCHFILES).selected).toEqual([]);
  expect(selectTests(['qa-only/SKILL.md.tmpl'], E2E_TOUCHFILES).selected).not.toContain(id);
});

test('scratch identity helper selects only queued-Skip and preserves its fast-profile deferral', () => {
  const changedFiles = ['test/helpers/scratch-repo.ts'];
  expect(selectTests(changedFiles, E2E_TOUCHFILES).selected).toEqual([id]);
  const full = computePaidCaseSelection({ profile: 'full', env: {}, changedFiles });
  expect(full.selection).toEqual({ e2e: [id], judges: [] });
  const pr = computePaidCaseSelection({ profile: 'pr', env: {}, changedFiles });
  expect(pr.selection).toEqual({ e2e: [], judges: [] });
  expect(pr.coverage?.mode).toBe('pr');
  expect(pr.coverage?.deferred).toEqual([{
    id, tier: 'gate', reason: 'Broad gate census/release coverage; outside the fast PR profile',
  }]);
});

test.each(['test/helpers/qa-checkpoint-evidence.ts', 'test/fixtures/qa-only-charter-public.json'])
  ('%s selects the native QA-only preparation gate', file => {
    expect(selectTests([file], E2E_TOUCHFILES).selected).toContain('qa-only-no-fix');
  });
