import { expect, test } from 'bun:test';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  reviewLifecycleInstructions, SHARED_LIBS_ROOT, type SharedLibsFixture,
} from './helpers/shared-libs-eval-fixture';

test('the native shared-code fixture consumes the relocated persistence contract', () => {
  const root = mkdtempSync(join(tmpdir(), 'review-flow-'));
  try {
    const text = readFileSync(reviewLifecycleInstructions({ root } as SharedLibsFixture), 'utf8');
    const start = text.indexOf('## Step 5.8: Persist Eng Review result');
    const command = text.indexOf('/bin/gstack-review-log', start);
    expect(start).toBeGreaterThan(0);
    expect(command).toBeGreaterThan(start);
    for (const field of ['REVIEW_START', 'COMPLETED', 'CONVERGED', 'CYCLES', 'snapshot_covered_paths']) {
      expect(text.slice(start, command)).toContain(field);
    }
    expect(text.match(/## Step 5\.8: Persist Eng Review result/g)).toHaveLength(1);
    expect(text).toContain('--finish REVIEW_START');
    expect(text).toContain(`${SHARED_LIBS_ROOT}/review/sections/shared-code-reuse.md`);
    expect(text).not.toContain('### Before persisting Eng Review (Step 5.8)');
    expect(readFileSync(join(SHARED_LIBS_ROOT, 'review/sections/shared-code-reuse.md'), 'utf8'))
      .toContain('canReuseSharedLibsAdvisory');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
