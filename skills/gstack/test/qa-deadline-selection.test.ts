import { expect, test } from 'bun:test';
import { E2E_TOUCHFILES, selectTests } from './helpers/touchfiles';

test.each(['bin/gstack-qa-deadline', 'lib/qa-deadline.ts', 'lib/claude-code-windows-job.ts'])('%s selects all bounded QA consumers', file => {
  const selected = selectTests([file], E2E_TOUCHFILES).selected;
  for (const id of ['review-exploratory-small-cli', 'ship-exploratory-small-cli', 'ship-exploratory-unavailable',
    'ship-exploratory-plan-checks', 'ship-exploratory-late-input', 'qa-quick', 'qa-only-no-fix',
    'qa-fix-loop', 'qa-functional-cli-report', 'qa-functional-webhook-report',
    'qa-functional-cli-fix', 'qa-functional-webhook-fix']) expect(selected).toContain(id);
  for (const id of ['qa-b6-static', 'qa-b7-spa',
    'qa-b8-checkout']) expect(selected).not.toContain(id);
});
