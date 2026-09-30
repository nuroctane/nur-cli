import { afterAll, test } from 'bun:test';
import { CAPTURE_MS } from './helpers/eval-budgets';
import { describeE2ETier, e2eTierEnabled } from './helpers/e2e-gate';
import { EvalCollector } from './helpers/eval-store';
import { runShipSkipActor } from './helpers/ship-skip-actor';

const describeE2E = describeE2ETier('gate');
const collector = e2eTierEnabled('gate') ? new EvalCollector('e2e', undefined, 'ship-skip-boundary') : null;
describeE2E('/ship unchanged queued Skip decision', () => {
  test('ship-skipped-queued-finding', async () => {
    await runShipSkipActor(entry => collector!.addTest(entry));
  }, CAPTURE_MS);
});
afterAll(async () => { await collector?.finalize(); });
