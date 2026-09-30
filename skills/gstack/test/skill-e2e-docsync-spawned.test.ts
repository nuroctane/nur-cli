import { expect, afterAll } from 'bun:test';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { CAPTURE_LONG_MS } from './helpers/eval-budgets';
import { runSkillTest } from './helpers/session-runner';
import { runId, describeIfSelected, testConcurrentIfSelected, createEvalCollector, recordE2E, finalizeEvalCollector, logCost } from './helpers/e2e-helpers';
import { describeE2ETier } from './helpers/e2e-gate';
import { extractDocsDispatch, parseDocsCompletion, vetDocsCompletion } from './helpers/docsync-contract';
import { fixtureDocs, gitAt, repoSnapshot, changedFiles, DOC_PATH, preserveDocsEvidence, sawSpawnedMarker } from './helpers/docsync-fixture';
import { observeDocsWrites, docsWriteFailures, docsNativeInterface, docsToolFailures, docsCompletedRead } from './helpers/docsync-observer';
import type { SkillTestResult } from './helpers/session-runner';

const describeE2E = describeE2ETier('gate');
const collector = createEvalCollector('e2e-docsync-spawned');

describeE2E('Native spawned document-release E2E (gate)', () => {
  describeIfSelected('Native spawned document-release', ['docsync-spawned'], () => {
    testConcurrentIfSelected('docsync-spawned', async () => {
      if (!process.env.EVALS_RUN_ID) throw Error('Native docs acceptance requires EVALS_RUN_ID');
      const deadline = Date.now() + CAPTURE_LONG_MS;
      const fixture = fixtureDocs('risky');
      try {
        const section = fs.readFileSync(path.join(fixture.skills, 'ship/sections/documentation.md'), 'utf8');
        const prompt = extractDocsDispatch(section)
          .replaceAll('<branch>', gitAt(fixture.repo, 'branch', '--show-current'))
          .replaceAll('<base>', 'main').replaceAll('<candidate-path>', fixture.candidate)
          .replaceAll('<audit-id>', fixture.auditId).replaceAll('<mode>', 'edit');
        const observer = await observeDocsWrites(fixture);
        let result: SkillTestResult | undefined;
        let observation: ReturnType<typeof observer.stop>;
        let evidence: string;
        try {
          result = await runSkillTest({
          prompt: `${prompt}\n\nEnvironment: HOME=${fixture.home}; the repository is the working directory.\n\n${docsNativeInterface(fixture)}`,
          workingDirectory: fixture.repo, maxTurns: 24,
          allowedTools: ['Bash', 'Read', 'Grep', 'Glob', 'Write', 'Edit'],
          timeout: Math.max(1, deadline - Date.now() - 15_000), env: fixture.env, testName: 'docsync-spawned', runId,
          });
        } finally {
          observation = observer.stop();
          evidence = preserveDocsEvidence(fixture, result ?? { output: 'capture did not return', toolCalls: [] }, runId, 'docsync-spawned', { observation });
        }
        if (!result) throw Error('missing native docs result');
        logCost('docsync-spawned', result);
        let passed = false;
        try {
          expect(result.exitReason).toBe('success');
          expect(docsWriteFailures(observation!, [DOC_PATH], { result, fixture })).toEqual([]);
          expect(docsToolFailures(result, fixture)).toEqual([]);
          expect(sawSpawnedMarker(result)).toBe(true);
          const contract = parseDocsCompletion(result.output, fixture.auditId);
          const after = repoSnapshot(fixture.repo);
          vetDocsCompletion(contract, {
            settled: true, markerSeen: sawSpawnedMarker(result),
            headUnchanged: after.head === fixture.before.head, indexUnchanged: after.index === fixture.before.index,
            candidateUnchanged: changedFiles(fixture.before, after).every(p => p === DOC_PATH),
            readOnly: false, changedPaths: changedFiles(fixture.before, after), allowedDocs: [DOC_PATH],
          });
          expect(contract.status).toBe('blocked');
          expect(contract.blockers.join(' ')).toMatch(/security|sensitive/i);
          expect(contract.files_reviewed).toContain(DOC_PATH);
          expect(docsCompletedRead(result, path.join(fixture.repo, DOC_PATH), fixture, {
            source: Buffer.from(fixture.before.contents[DOC_PATH], 'base64').toString('utf8'),
            beforeFirstEdit: true,
          })).toBe(true);
          expect(after.contents['SECURITY.md']).toBe(fixture.before.contents['SECURITY.md']);
          for (const file of ['VERSION', 'CHANGELOG.md', 'TODOS.md', 'package.json', 'personal-note.txt']) {
            expect(after.contents[file]).toBe(fixture.before.contents[file]);
          }
          expect(result.toolCalls.filter(call => /AskUserQuestion/.test(call.tool))).toEqual([]);
          expect(fs.existsSync(evidence)).toBe(true);
          passed = true;
        } finally {
          recordE2E(collector, 'docsync-spawned', 'Native spawned document-release', result, { passed });
        }
      } finally {
        fixture.clean();
      }
    }, CAPTURE_LONG_MS);
  });
});

afterAll(() => finalizeEvalCollector(collector));
