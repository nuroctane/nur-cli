import { afterAll, expect } from 'bun:test';
import * as fs from 'node:fs';
import * as path from 'node:path';
import {
  createEvalCollector, describeIfSelected, finalizeEvalCollector, logCost, recordE2E,
  testConcurrentIfSelected,
} from './helpers/e2e-helpers';
import { getProjectEvalDir } from './helpers/eval-store';
import {
  createQaCallerFixture, QA_CALLER_CASES, QA_CALLER_TEST_MS, readCallerReceipt,
  retainQaCallerEvidence, runQaCaller, validateCallerEvidence,
  type QaCallerCase,
} from './helpers/qa-callers-fixture';
import type { SkillTestResult } from './helpers/session-runner';
import { readQACheckpointFiles } from './helpers/qa-checkpoint-evidence';

const collector = createEvalCollector('e2e-qa-callers');
let sequence = 0;

async function capture(caseId: QaCallerCase) {
  const run = process.env.EVALS_RUN_ID;
  if (!run || !/^[\w.-]+$/.test(run)) throw new Error('Caller captures require an explicit safe EVALS_RUN_ID for durable native evidence');
  const id = `${run}-${caseId}-${process.pid}-${++sequence}`;
  const fixture = createQaCallerFixture(caseId);
  let result: SkillTestResult | undefined;
  let passed = false;
  try {
    await fixture.observe();
    result = await runQaCaller(fixture, id);
    await fixture.close();
    const receipt = readCallerReceipt(fixture);
    const probes = fixture.probes();
    const requiredCharters = caseId === 'ship-exploratory-plan-checks' ? ['happy', 'adverse', 'plan:nine'] : ['happy', 'adverse'];
    const errors = validateCallerEvidence({
      caller: fixture.caller, result, probes, receipt,
      currentSnapshot: fixture.snapshot(), requiredCharters,
      mutations: fixture.mutationEvents, observerComplete: fixture.observation?.complete === true && fixture.observerErrors.length === 0,
      workflowCommands: fixture.workflowCommands,
      fixtureRoot: fixture.cwd,
      runtime: fixture.runtime,
      requireGuardedSmoke: true,
      requireCapturedEvidence: true,
      reportRoot: path.join(fixture.cwd, 'reports'),
      checkpointFiles: readQACheckpointFiles(path.join(fixture.cwd, 'reports')),
      reportMarkdown: fs.readFileSync(path.join(fixture.cwd, 'reports/review.md'), 'utf8'),
    });
    expect(errors).toEqual([]);
    expect(fs.readFileSync(path.join(fixture.cwd, 'reports/review.md'), 'utf8').trim().length).toBeGreaterThan(100);
    if (caseId === 'review-exploratory-small-cli') {
      const defect = probes.find(probe => probe.input === '0' && probe.status === 'fail');
      expect(defect).toBeDefined();
      expect(receipt.probes).toContain(defect!.id);
      expect(['fail', 'blocked']).toContain(receipt.status);
    } else if (caseId === 'ship-exploratory-unavailable') {
      const blocked = probes.find(probe => probe.status === 'blocked' && probe.exit !== 0);
      expect(blocked).toBeDefined();
      expect(receipt.probes).toContain(blocked!.id);
      expect(receipt.status).toBe('blocked');
      expect(receipt.remaining.length).toBeGreaterThan(0);
    } else {
      expect(receipt.status).toBe('pass');
      if (caseId === 'ship-exploratory-late-input') {
        expect(fixture.lateApplied).toBe(true);
        expect(new Set(probes.map(probe => probe.snapshot)).size).toBeGreaterThan(1);
      }
    }
    passed = true;
  } finally {
    await fixture.close();
    const artifacts = path.join(process.env.GSTACK_EVAL_DIR || getProjectEvalDir(), 'qa-callers', id);
    retainQaCallerEvidence(fixture, artifacts, result);
    if (result) {
      logCost(caseId, result);
      recordE2E(collector, caseId, 'Automatic parent exploratory QA', result, { passed });
    }
    fs.rmSync(fixture.root, { recursive: true, force: true });
    expect(fs.existsSync(path.join(artifacts, 'native-events.json'))).toBe(true);
    expect(fs.statSync(path.join(artifacts, 'native-probes.jsonl')).mode & 0o777).toBe(0o600);
  }
}

describeIfSelected('Automatic parent exploratory QA', [...QA_CALLER_CASES], () => {
  for (const caseId of QA_CALLER_CASES) {
    testConcurrentIfSelected(caseId, () => capture(caseId), QA_CALLER_TEST_MS);
  }
});

afterAll(() => finalizeEvalCollector(collector));
