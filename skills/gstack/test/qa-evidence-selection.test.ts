import { expect, test } from 'bun:test';
import { E2E_TOUCHFILES, selectTests } from './helpers/touchfiles';
import { QA_EVIDENCE_RUNTIME } from './helpers/qa-evidence-producer';
import { curateWindowsSafe } from '../scripts/test-free-shards';

const consumers = ['qa-functional-cli-report', 'qa-functional-webhook-report', 'qa-functional-cli-fix', 'qa-functional-webhook-fix',
  'review-exploratory-small-cli', 'ship-exploratory-small-cli', 'ship-exploratory-unavailable', 'ship-exploratory-plan-checks', 'ship-exploratory-late-input'];

test.each([...QA_EVIDENCE_RUNTIME, 'test/helpers/qa-evidence-producer.ts'])('%s selects every real production capture consumer', file => {
  const selected = selectTests([file], E2E_TOUCHFILES).selected;
  for (const consumer of consumers) expect(selected).toContain(consumer);
});

test.each(['bin/gstack-qa-evidence', 'lib/qa-evidence.ts'])('%s remains scoped to actual capture consumers', file => {
  expect(selectTests([file], E2E_TOUCHFILES).selected.sort()).toEqual([...consumers].sort());
});

test('Windows runs the actual capture/job tests and keeps Linux native observation coverage separate', () => {
  const selected = curateWindowsSafe(['test/qa-evidence.test.ts', 'test/qa-evidence-producer.test.ts']);
  expect(selected.safe).toEqual(['test/qa-evidence.test.ts']);
  expect(selected.excluded.map(entry => entry.file)).toEqual(['test/qa-evidence-producer.test.ts']);
});
