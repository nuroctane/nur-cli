/** Free checks for the bounded revalidation interface and native completion requirement. */
import { describe, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import {
  createSharedInteractiveToolHandler, createSharedLibsFixture, fixtureWrite, reviewPrompt, reviewRevalidationPrompt,
  SHARED_INTERACTIVE_MAX_TURNS, SHARED_LIBS_ROOT, type SharedLibsFixture,
} from './helpers/shared-libs-eval-fixture';
import { CAPTURE_MS, CAPTURE_LONG_MS } from './helpers/eval-budgets';
import { hasTrustedSharedLibsCheck } from './helpers/shared-libs-review-start-evidence';
import stageScope from './fixtures/shared-libs-lifecycle-r59-stage-scope-public.json';
import { seedPathReviewPrerequisites, checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt,
  type PathEligibilityFixture } from './helpers/shared-libs-path-fixture';
import * as fixtureHelpers from './helpers/shared-libs-eval-fixture';

const f = { root: '/fixture root', repo: '/fixture root/repo', state: '/fixture root/state',
  bin: '/fixture root/bin' } as SharedLibsFixture;
const instructions = path.join(f.root, 'review-lifecycle.md');
const input = path.join(f.root, 'current-advisory.jsonl');
const source = fs.readFileSync(path.join(import.meta.dir, 'skill-e2e-shared-libs.test.ts'), 'utf8');
const helper = fs.readFileSync(path.join(import.meta.dir, 'helpers/shared-libs-eval-fixture.ts'), 'utf8');
const native = JSON.parse(fs.readFileSync(path.join(import.meta.dir,
  'fixtures/shared-libs-revalidation-max-turns-public.json'), 'utf8'));
const pathsNative = JSON.parse(fs.readFileSync(path.join(import.meta.dir,
  'fixtures/shared-libs-paths-max-turns-public.json'), 'utf8'));
const pathsSource = fs.readFileSync(path.join(import.meta.dir, 'skill-e2e-shared-libs-paths.test.ts'), 'utf8');
const transpile = (text: string) => new Bun.Transpiler({ loader: 'ts' }).transformSync(text);

function pathCaptureAdapter(capture: (...args: any[]) => Promise<any>) {
  const start = pathsSource.indexOf('async function exerciseEligibility(');
  const end = pathsSource.indexOf('\ndescribeE2E(', start);
  expect(start).toBeGreaterThan(0);
  expect(end).toBeGreaterThan(start);
  const rows: { scenario: string; row: any }[] = [];
  const removed: string[] = [];
  const attempts: any[] = [];
  let ownedAttempt: any;
  const fixtures = new Map<string, SharedLibsFixture>();
  const prepared = new Map<string, PathEligibilityFixture>();
  const afterCompletion = () => { throw new Error('A non-success capture reached completion checks'); };
  const exercise = new Function('deps', `const { captures, preparePathEligibilityFixture, fs, path,
    reviewLifecycleInstructions, reviewPrompt, reviewRevalidationPrompt, runSharedInteractive, readRequests,
    toolCommandTrace, sourceReadTrace, fixtureGit, fixtureWorkingTree, reviewRecords, expect, CAPTURE_LONG_MS,
    hasTrustedSharedLibsCheck, SHARED_LIBS_ROOT, checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt } = deps;
    ${transpile(pathsSource.slice(start, end))} return exerciseEligibility;`)({
    captures: { runAttempt: async (name: string, kinds: string[], timeout: number, work: any) => {
      attempts.push({ name, kinds, timeout });
      ownedAttempt = { signal: new AbortController().signal, remainingMs: () => timeout,
        add: (scenario: string, row: any) => rows.push({ scenario, row }) };
      return work(ownedAttempt);
    } },
    preparePathEligibilityFixture: (kind: string) => {
      const fixture = createSharedLibsFixture(`prompt-${kind}`);
      fixtureWrite(fixture, 'src/retry-route.ts', 'authored caller');
      const value = { fixture, current: { evidence_paths: ['src/retry-route.ts'] },
        sourcePaths: ['src/retry-route.ts'], rawPaths: [], beforeTree: '', resumed: seedPathReviewPrerequisites(fixture) };
      prepared.set(kind, value);
      fixtures.set(kind, value.fixture);
      return value;
    },
    fs: { ...fs, rmSync: (root: string) => { removed.push(root); fs.rmSync(root, { recursive: true, force: true }); } }, path,
    reviewLifecycleInstructions: (fixture: SharedLibsFixture) => path.join(fixture.root, 'review-lifecycle.md'),
    reviewPrompt, reviewRevalidationPrompt, runSharedInteractive: capture, readRequests: () => [],
    toolCommandTrace: afterCompletion, sourceReadTrace: afterCompletion,
    fixtureGit: afterCompletion, fixtureWorkingTree: afterCompletion, reviewRecords: afterCompletion, expect, CAPTURE_LONG_MS,
    hasTrustedSharedLibsCheck, SHARED_LIBS_ROOT, checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt,
  });
  return { exercise, rows, removed, attempts, fixtures, prepared, get attempt() { return ownedAttempt; } };
}

describe('bounded shared-code revalidation prompt', () => {
  test('the actor scope replaces the captured R59 exploratory stage instead of adding another prerequisite', () => {
    const prompt = reviewPrompt(f, instructions, input, { actorCommand: 'cat /fixture/stage-output.json' });
    expect(prompt).toContain('replaces the entire Step 4.7 QA and Step 4.8 native adversarial stages');
    expect(prompt).toContain("Step 4's early QA selection/method-loading prerequisites");
    expect(prompt).toContain("Step 5.8's QA report requirement");
    const excluded = prompt.slice(prompt.indexOf('Do not perform QA scope/method asset loads'), prompt.indexOf('Existing tests'));
    for (const packet of stageScope.outside_component) {
      expect(packet.result.tool_use_id).toBe(packet.call.id);
      expect(packet.result.is_error).not.toBe(true);
      expect(packet.result.content.length).toBeGreaterThan(0);
      expect(excluded).toContain(packet.boundary);
    }
    expect(stageScope.post_fix_verification.call.input.command).toContain('bun test test/retry-after.test.ts');
    expect(stageScope.post_fix_verification.result.content).toContain('worker===lib true route===lib true');
    expect(prompt).toContain('Existing tests and caller/import checks needed to verify your source fixes still run');
    expect(prompt).toContain('do not restart exploratory QA or require QA artifacts');
    for (const retained of ['core/checklist', 'source/identity/snapshot checks', 'Fix-First decisions', 'approved source edits',
      're-review with a new REVIEW_START', 'zero-edit convergence', 'final persistence', 'Missing, failed, stale or wrong-state results require noncompletion',
      'no actual native coverage credit', 'Separate genuine QA/native evaluations remain required']) expect(prompt).toContain(retained);
    for (const other of [reviewPrompt(f, instructions, input), reviewRevalidationPrompt(f, instructions, input),
      reviewRevalidationPrompt(f, instructions, input, { input: '/fixture/resumed.json', checkCommand: 'check-prerequisites' })]) {
      expect(other).not.toContain('Do not perform QA scope/method asset loads');
    }
  });

  test('edit-capable replay declares fresh actor invocations instead of refreshing settled input', () => {
    const prompt = reviewPrompt(f, instructions, input, { actorCommand: 'cat /fixture/stage-output.json' });
    expect(prompt).toContain('explicitly declared SYNTHETIC prerequisite actor');
    for (const rule of ['each review pass', 'NEW synthetic result', 'exact current state and tool-use ID',
      'All prior receipts are preserved', 'Source-changing cycles invalidate earlier results',
      'invoke the actor again on the new zero-edit pass', "Never refresh an old receipt's hashes",
      'Missing, failed, stale or wrong-state results require noncompletion', 'cannot complete core/checklist review',
      'no actual native coverage credit']) expect(prompt).toContain(rule);
    expect(prompt).not.toContain('Required reviewer coverage for this scoped replay');
    expect(prompt).not.toContain('Do not edit target source');
  });

  test('resumed path scope supplies prerequisites without replacing completion or default caller instructions', () => {
    const resumed = { input: '/isolated/synthetic-prerequisites.json', checkCommand: 'fixture-prerequisite-check' };
    const original = reviewRevalidationPrompt(f, instructions, input);
    const prompt = reviewRevalidationPrompt(f, instructions, input, resumed);
    expect(original).toContain('Required reviewer coverage for this scoped replay is the core/checklist review plus the supplied completed maintainability result.');
    expect(prompt).not.toContain('Required reviewer coverage for this scoped replay');
    expect(prompt).toContain('SYNTHETIC settled Step 4.7 QA and Step 4.8 native adversarial');
    for (const requirement of ['not evidence that this model executed those stages', 'never actual native coverage credit',
      'Missing, failed, blocked, malformed or stale prerequisites require noncompletion', 'unchanged COMPLETED and CONVERGED rules',
      'Any source, branch, base, index or configuration change invalidates', 'do not regenerate them',
      'A finding that requires edits blocks this bounded replay', resumed.input, resumed.checkCommand]) expect(prompt).toContain(requirement);
    expect(prompt.slice(prompt.indexOf('Revalidation fixture execution contract:')))
      .toBe(original.slice(original.indexOf('Revalidation fixture execution contract:')));
    const production = fs.readFileSync(path.join(SHARED_LIBS_ROOT, 'review/SKILL.md.tmpl'), 'utf8');
    expect(production).toContain('Step 4.8 adversarial pass finish, and every required Step 4.7 probe passes.');
  });

  test('adds execution guidance after the complete shared prompt without supplying an answer or token', () => {
    const base = reviewPrompt(f, instructions, input);
    const prompt = reviewRevalidationPrompt(f, instructions, input);
    expect(prompt.slice(0, base.length)).toBe(base);
    const contract = prompt.slice(base.length);
    expect(contract).toContain(`${SHARED_INTERACTIVE_MAX_TURNS} assistant turns`);
    expect(contract).toContain(path.join(f.state, 'projects/fixture-shared-libs/.review-starts/<REVIEW_START>.json'));
    expect(contract).toContain('token actually returned by --start');
    expect(contract).toContain('separate, successful Read tool call or a single cat command');
    expect(contract).toContain('Verify its repo, branch, working tree and start time');
    expect(contract).toContain('Do not combine the record read with --start, the diff or other diagnostic commands');
    expect(contract).toContain('if the read fails, retry it before proceeding');
    expect(contract).toContain('Do not read the diff until step 2 verifies the start record');
    expect(contract.indexOf('Read that token\'s record')).toBeLessThan(contract.indexOf('Then read the diff in a subsequent call'));
    expect(contract).not.toMatch(/[a-f0-9]{8}(?:-[a-f0-9]{4}){3}-[a-f0-9]{12}/);
    expect(contract).toContain('Batch independent required source reads');
    expect(contract).toContain('Preserve every required evidence check and dependency');
    expect(contract).toContain('complete, untruncated read-back');
    expect(contract).toContain('same tool invocation');
    expect(contract).toContain('then return the final review summary');
    expect(contract).toContain('Failed persistence or verification remains a failure');
    expect(contract).toContain("Late source changes still require the workflow's normal re-review");
    expect(contract).not.toContain('choose Skip');
  });

  test('declares isolated receipt commands, direct reads and a separate supplied-finding disposition', () => {
    const prompt = reviewRevalidationPrompt(f, instructions, input);
    const commands = [...prompt.matchAll(/```bash\n([\s\S]*?)\n```/g)].map(match => match[1]);
    expect(commands).toHaveLength(3);
    expect(commands[0]).toBe(`'${SHARED_LIBS_ROOT}/bin/gstack-review-log' --start review`);
    expect(commands[1]).toBe(`'${SHARED_LIBS_ROOT}/bin/gstack-review-log' --check-shared-libs REVIEW_START <<'GSTACK_REVALIDATION_FINDING'\nCURRENT_FINDING_JSON\nGSTACK_REVALIDATION_FINDING`);
    expect(commands[2]).toBe(`'${SHARED_LIBS_ROOT}/bin/gstack-review-log' 'FINAL_REVIEW_JSON' --finish REVIEW_START && '${SHARED_LIBS_ROOT}/bin/gstack-review-read'`);
    for (const requirement of ['sole command', 'only the token', 'only one JSON value', 'literal path operands',
      'even when its body appeared in the diff', 'No path-variable loops', 'before the checker',
      'metadata in earlier calls', 'No preliminary commands', 'materially revised proposal is a separate finding',
      'unsupported or unfinished supplied finding stays blocked', 'without its actual explicit decision']) {
      expect(prompt).toContain(requirement);
    }
  });

  test('step 5 branches on the checker result: true suppresses without a new decision, false requires a fresh one', () => {
    const prompt = reviewRevalidationPrompt(f, instructions, input);
    const step5 = prompt.slice(prompt.indexOf('5. Act on the checker result'), prompt.indexOf('6. Complete final evidence'));
    expect(step5).toContain('reusable:true');
    expect(step5).toContain('the prior Skip carries forward');
    expect(step5).toContain('Ask no new decision question');
    expect(step5).toContain("exclude this advisory from the current pass's findings");
    expect(step5).toContain('Do not re-persist it as a current finding');
    expect(step5).toContain('reusable:false');
    expect(step5).toContain('Perform a fresh authored-source review and make an actual new decision');
    expect(step5).toContain('materially revised proposal is a separate finding');
    expect(step5).toContain('without its actual explicit decision');
    expect(step5).toContain('unsupported or unfinished supplied finding stays blocked');
    expect(step5).toContain('reusable:true whose independent authored/current-source verification does not hold');
    expect(step5).toContain('regardless of the checker result');
    expect(step5).toContain('never force a new Skip on invalid evidence');
    expect(step5.indexOf('reusable:true')).toBeLessThan(step5.indexOf('reusable:false'));
    expect(prompt).not.toContain("Record the supplied finding's disposition under its own");
  });

  test('the shared review prompt maps exact trusted asset roots and documented interfaces to avoid discovery', () => {
    const prompt = reviewPrompt(f, instructions, input);
    expect(prompt).toContain(`${SHARED_LIBS_ROOT}/review/checklist.md`);
    expect(prompt).toContain(`${SHARED_LIBS_ROOT}/review/sections/`);
    expect(prompt).toContain(`../qa/sections/<name>.md is ${SHARED_LIBS_ROOT}/qa/sections/<name>.md`);
    expect(prompt).toContain(`${SHARED_LIBS_ROOT}/bin`);
    expect(prompt).toContain(`${SHARED_LIBS_ROOT}/lib`);
    expect(prompt).toContain(f.bin);
    for (const iface of ['gstack-review-log --start review', '--check-shared-libs REVIEW_START',
      '--finish REVIEW_START', 'gstack-review-read']) expect(prompt).toContain(iface);
    for (const forbidden of ['do not rediscover it', 'enumerate the bin/lib/review/qa roots',
      'probe --help', 'read the fixture request logs']) expect(prompt).toContain(forbidden);
    expect(prompt).toContain('batch independent reads');
    expect(prompt).toContain('keep receipt-ordered commands separate');
    expect(prompt).toContain('capture the start token before reading the diff');
    expect(prompt).toContain('run --start, the checker and any declared stage-actor invocation each as its own sole command');
    expect(prompt).toContain('The only combined receipt call is the final persistence');
    expect(prompt).toContain('Still inspect the target repository source');
  });

  test('common guidance states the finish+read-back receipt contract directly, not by a dangling step 6 reference', () => {
    const lifecycle = reviewPrompt(f, instructions, input, { actorCommand: 'bun /fx/stage-actor.ts run' });
    const revalidation = reviewRevalidationPrompt(f, instructions, input);
    for (const prompt of [lifecycle, revalidation]) {
      expect(prompt).toContain('The only combined receipt call is the final persistence');
      expect(prompt).not.toContain('exactly as step 6 shows');
    }
    const commands = [...revalidation.matchAll(/```bash\n([\s\S]*?)\n```/g)].map(match => match[1]);
    expect(commands[2]).toBe(`'${SHARED_LIBS_ROOT}/bin/gstack-review-log' 'FINAL_REVIEW_JSON' --finish REVIEW_START && '${SHARED_LIBS_ROOT}/bin/gstack-review-read'`);
    expect(lifecycle).not.toContain('step 6');
  });

  test('the actual revalidation capture uses the wrapper and preserves the skip actor', async () => {
    const scenario = source.slice(source.indexOf("test('shared-libs-review-revalidation'"));
    const marker = "'shared-libs-review-revalidation', async () => {";
    const start = scenario.indexOf(marker) + marker.length;
    const end = scenario.indexOf('}, result => {', start);
    expect(start).toBeGreaterThan(marker.length);
    expect(end).toBeGreaterThan(start);
    const result = { exitReason: 'success' };
    const resumed = { input: '/fixture root/resumed-review-prerequisites.json', checkCommand: 'fixture-prerequisite-check' };
    const calls: any[] = [];
    const callback = transpile(`async function invokeCapture() { ${scenario.slice(start, end)} }`);
    const attempt = { signal: new AbortController().signal, remainingMs: () => CAPTURE_LONG_MS, add() {} };
    const invoke = new Function('deps', `const { f, instructions, input, resumed, attempt, reviewRevalidationPrompt, runSharedInteractive } = deps;
      let questions = []; ${callback} return invokeCapture;`)({
      f, instructions, input, resumed, attempt, reviewRevalidationPrompt,
      runSharedInteractive: async (...args: any[]) => { calls.push(args); return { result, questions: [] }; },
    });
    expect(await invoke()).toBe(result);
    expect(calls).toEqual([[f, 'shared-libs-review-revalidation', reviewRevalidationPrompt(f, instructions, input, resumed), 'skip', { attempt, prerequisiteSource: 'synthetic-fixture-input' }]]);
    const lifecycle = source.slice(source.indexOf("test('shared-libs-review-lifecycle'"), source.indexOf("test('shared-libs-review-revalidation'"));
    expect(lifecycle).toContain('reviewPrompt(f, instructions, input, stageActor)');
    expect(lifecycle).not.toContain('reviewRevalidationPrompt(');
  });

  test('all four registered revalidation variants supply prerequisites only after their state changes', async () => {
    const contexts = new Map<string, any>(), labels = new Map<string, string>(), rows: any[] = [];
    let registered: () => Promise<void>;
    const record = source.slice(source.indexOf('async function recordCapture('), source.indexOf('\nfunction assertReadOnly('));
    const registration = source.slice(source.indexOf("  test('shared-libs-review-revalidation'"), source.lastIndexOf('\n});'));
    new Function('deps', `const { test, captures, fs, path, expect, CAPTURE_LONG_MS,
      createSharedLibsFixture, seedReviewSources, fixtureWrite, installNormalizingFilter, seedSkippedAdvisory,
      fixtureWorkingTree, fixtureGit, reviewLifecycleInstructions, seedPathReviewPrerequisites, checkPathReviewPrerequisites,
      reviewRevalidationPrompt, runSharedInteractive } = deps; ${transpile(record + registration)}`)({
      ...fixtureHelpers, fs, path, expect, CAPTURE_LONG_MS,
      test: (name: string, body: () => Promise<void>, timeout: number) => {
        expect(name).toBe('shared-libs-review-revalidation'); expect(timeout).toBe(CAPTURE_LONG_MS); registered = body;
      },
      captures: { runAttempt: async (_name: string, cases: string[], _timeout: number, work: any) => {
        expect(cases).toEqual(['unchanged', 'secondary', 'branch', 'filtered']);
        return work({ signal: new AbortController().signal, remainingMs: () => _timeout,
          add: (scenario: string, row: any) => rows.push({ scenario, row }) });
      } },
      createSharedLibsFixture: (label: string) => { const f = fixtureHelpers.createSharedLibsFixture(label); labels.set(f.root, label); return f; },
      seedPathReviewPrerequisites: (f: SharedLibsFixture) => {
        const resumed = seedPathReviewPrerequisites(f), checked = checkPathReviewPrerequisites(f, resumed.input);
        expect(checked.settled).toBe(true);
        const label = labels.get(f.root)!;
        expect(checked.context.binding.branch).toBe(label === 'revalidate-branch' ? 'feature-a' : 'feature/a');
        const caller = fs.readFileSync(path.join(f.repo, 'src/retry-route.ts'), 'utf8');
        if (label === 'revalidate-secondary') expect(caller).toContain('Caller integration changed after the skipped review');
        if (label === 'revalidate-filtered') expect(caller).toContain('RAW-ONLY changed caller bytes after the skipped review');
        contexts.set(f.root, { resumed, checked });
        return resumed;
      }, checkPathReviewPrerequisites,
      runSharedInteractive: async (f: SharedLibsFixture, name: string, prompt: string, choice: string, options: any) => {
        expect(name).toBe('shared-libs-review-revalidation'); expect(choice).toBe('skip');
        expect(options).toEqual({ attempt: expect.objectContaining({ add: expect.any(Function) }), prerequisiteSource: 'synthetic-fixture-input' });
        const supplied = contexts.get(f.root);
        expect(checkPathReviewPrerequisites(f, supplied.resumed.input)).toEqual(supplied.checked);
        expect(prompt).toBe(reviewRevalidationPrompt(f, path.join(f.root, 'review-lifecycle.md'), path.join(f.root, 'current-advisory.jsonl'), supplied.resumed));
        throw new Error('Free revalidation boundary reached');
      },
    });
    await expect(registered!()).rejects.toThrow('Free revalidation boundary reached');
    expect(contexts.size).toBe(4);
    expect(rows).toHaveLength(4);
    expect(rows.every(({ row }) => row.passed === false)).toBe(true);
    for (const root of contexts.keys()) expect(fs.existsSync(root)).toBe(false);
  }, 30_000);

  test('the registered verify enforces true reuse (no question, no current advisory) versus false fresh decision', async () => {
    const record = source.slice(source.indexOf('async function recordCapture('), source.indexOf('\nfunction assertReadOnly('));
    const registration = source.slice(source.indexOf("  test('shared-libs-review-revalidation'"), source.lastIndexOf('\n});'));
    const labels = new Map<string, string>();
    const stubTrue = () => true;
    let mutateUnchangedQuestion = false;
    let rows: any[] = [];
    const persistFinal = (fx: SharedLibsFixture, findings: any[]) => {
      const log = path.join(SHARED_LIBS_ROOT, 'bin/gstack-review-log');
      const env = { ...process.env, ...fx.env, PATH: process.env.PATH, GSTACK_HOME: fx.state };
      const token = execFileSync(log, ['--start', 'review'], { cwd: fx.repo, env, encoding: 'utf8', timeout: 30_000 }).trim();
      execFileSync(log, [JSON.stringify({ skill: 'review', timestamp: new Date().toISOString(),
        status: 'clean', issues_found: 0, critical: 0, informational: 0, quality_score: 10,
        findings, completed: true, converged: true, cycles: 0 }), '--finish', token],
      { cwd: fx.repo, env, encoding: 'utf8', timeout: 30_000 });
      return token;
    };
    const build = () => new Function('deps', `const { test, captures, fs, path, expect, CAPTURE_LONG_MS, createHash, SHARED_LIBS_ROOT,
      createSharedLibsFixture, seedReviewSources, fixtureWrite, installNormalizingFilter, seedSkippedAdvisory,
      fixtureWorkingTree, fixtureGit, reviewLifecycleInstructions, seedPathReviewPrerequisites, checkPathReviewPrerequisites,
      reviewRevalidationPrompt, runSharedInteractive, toolCommandTrace, reviewRecords,
      hasTrustedSharedLibsCheck, hasTrustedReviewStartRead, hasPathReviewPrerequisiteReceipt } = deps; ${transpile(record + registration)}`)({
      ...fixtureHelpers, fs, path, expect, CAPTURE_LONG_MS, createHash, SHARED_LIBS_ROOT,
      hasTrustedSharedLibsCheck: stubTrue, hasTrustedReviewStartRead: stubTrue, hasPathReviewPrerequisiteReceipt: stubTrue,
      checkPathReviewPrerequisites, seedPathReviewPrerequisites,
      test: (_name: string, body: () => Promise<void>) => { registered = body; },
      captures: { runAttempt: async (_name: string, _cases: string[], _timeout: number, work: any) =>
        work({ signal: new AbortController().signal, remainingMs: () => _timeout,
          add: (scenario: string, row: any) => rows.push({ scenario, row }) }) },
      createSharedLibsFixture: (label: string) => { const fx = fixtureHelpers.createSharedLibsFixture(label); labels.set(fx.root, label); return fx; },
      runSharedInteractive: async (fx: SharedLibsFixture) => {
        const label = labels.get(fx.root)!;
        const advisory = fixtureHelpers.reviewRecords(fx).find((r: any) => r.skill === 'review').findings[0];
        const token = persistFinal(fx, label === 'revalidate-unchanged' ? [] : [{ ...advisory, action: 'skipped' }]);
        const result = { exitReason: 'success', events: [], toolCalls: [
          { tool: 'Bash', input: { command: `'${path.join(SHARED_LIBS_ROOT, 'bin/gstack-review-log')}' --check-shared-libs ${token}` } },
          { tool: 'Bash', input: { command: `'${path.join(SHARED_LIBS_ROOT, 'bin/gstack-review-read')}'` } },
          { tool: 'Read', input: { file_path: path.join(fx.repo, 'src/retry-route.ts') } },
          { tool: 'Read', input: { file_path: path.join(fx.repo, 'lib/retry-after.ts') } },
        ] };
        const questions = label === 'revalidate-unchanged'
          ? (mutateUnchangedQuestion ? [{ q: 'reconfirm prior skip?' }] : [])
          : [{ q: 'reuse the helper here?' }];
        return { result, questions };
      },
    });
    let registered: () => Promise<void>;
    build();
    await registered!();
    expect(rows).toHaveLength(4);
    expect(rows.every(({ row }) => row.passed === true)).toBe(true);

    rows = [];
    mutateUnchangedQuestion = true;
    build();
    await expect(registered!()).rejects.toThrow();
    expect(rows.find(({ scenario }) => scenario === 'unchanged').row.passed).toBe(false);
  }, 60_000);

  test('the actual interactive runner uses the declared existing limit without changing clocks or actor', async () => {
    const start = helper.indexOf('export async function runSharedInteractive(');
    expect(start).toBeGreaterThan(0);
    let body = helper.slice(start).replace('export async function', 'async function');
    for (const [module, replacement] of [
      ['./agent-sdk-runner', 'deps.sdk'], ['@anthropic-ai/claude-agent-sdk', 'deps.provider'], ['./eval-budgets', 'deps.budgets'],
    ]) {
      expect(body).toContain(`await import('${module}')`);
      body = body.replace(`await import('${module}')`, replacement);
    }
    let observed: any;
    let shimCalls = 0;
    const invoke = new Function('deps', `const { fs, path, SHARED_LIBS_ROOT, SHARED_INTERACTIVE_MAX_TURNS,
      createSharedInteractiveToolHandler, installSourceShims, readRequests } = deps;
      ${transpile(body)} return runSharedInteractive;`)({
      fs, path, SHARED_LIBS_ROOT, SHARED_INTERACTIVE_MAX_TURNS, createSharedInteractiveToolHandler,
      installSourceShims: (fixture: SharedLibsFixture) => { expect(fixture).toBe(f); shimCalls++; },
      readRequests: () => [], budgets: { CAPTURE_MS },
      sdk: { runAgentSdkTest: async (options: any) => { observed = options; return { exitReason: 'success' }; },
        passThroughNonAskUserQuestion: (_name: string, value: unknown) => ({ behavior: 'allow', updatedInput: value }),
        resolveClaudeBinary: () => '/fixture/claude' },
      provider: { query: () => { throw new Error('No provider may run in this free adapter'); } },
    });
    const attempt = { signal: new AbortController().signal, remainingMs: () => CAPTURE_LONG_MS, add() {} };
    const plain = await invoke(f, 'shared-libs-review-revalidation', 'prompt', 'skip', { attempt });
    expect(shimCalls).toBe(1);
    expect(plain.result.fixturePrerequisiteSource).toBeUndefined();
    expect(observed.maxTurns).toBe(SHARED_INTERACTIVE_MAX_TURNS);
    expect(observed.maxTurns).toBe(30);
    expect(observed.maxRetries).toBe(0);
    expect(observed.signal).toBe(attempt.signal);
    expect(observed.model).toBeUndefined();
    expect(observed.allowedTools).toContain('AskUserQuestion');
    expect(CAPTURE_MS).toBe(300_000);
    expect(CAPTURE_LONG_MS).toBe(600_000);
    const supplied = await invoke(f, 'shared-libs-review-revalidation', 'prompt', 'skip', { attempt, prerequisiteSource: 'synthetic-fixture-input' });
    expect(supplied.result.fixturePrerequisiteSource).toBe('synthetic-fixture-input');
  });

  test('actual recordCapture rejects native max-turns even after verified CURRENT persistence', async () => {
    const persisted = JSON.parse('{' + native.events.at(-1).message.content[0].content);
    expect(persisted.completed).toBe(true);
    expect(persisted.converged).toBe(true);
    expect(persisted.review_binding.state).toBe('verified');
    expect(persisted.review_freshness.status).toBe('CURRENT');
    expect(native.terminal.subtype).toBe('error_max_turns');
    const start = source.indexOf('async function recordCapture(');
    const end = source.indexOf('\nfunction assertReadOnly(', start);
    expect(start).toBeGreaterThan(0);
    expect(end).toBeGreaterThan(start);
    const recordCapture = new Function('expect', `${transpile(source.slice(start, end))} return recordCapture;`)(expect);
    const rows: any[] = [];
    let verified = false;
    const result = { exitReason: native.terminal.subtype, turnsUsed: native.terminal.num_turns,
      durationMs: native.terminal.duration_ms, events: [...native.events, native.terminal],
      toolCalls: native.events.filter((event: any) => event.type === 'assistant')
        .flatMap((event: any) => event.message.content.map((block: any) => ({ tool: block.name, input: block.input }))) };
    await expect(recordCapture({ add: (scenario: string, row: any) => rows.push({ scenario, row }) }, 'secondary',
      'shared-libs-review-revalidation', async () => result, () => { verified = true; })).rejects.toThrow();
    expect(verified).toBe(false);
    expect(rows).toHaveLength(1);
    expect(rows[0].row.passed).toBe(false);
    expect(rows[0].row.exit_reason).toBe('error_max_turns');
    expect(rows[0].row.turns_used).toBe(31);
    expect(rows[0].row.transcript).toContainEqual(native.terminal);
    expect(rows[0].row.transcript).toContainEqual(native.events.at(-1));
  });

  test.each([
    ['shared-libs-review-path-eligibility', ['symlinks', 'submodule', 'ignored']],
    ['shared-libs-review-index-flags', ['assume-unchanged', 'skip-worktree']],
    ['shared-libs-review-prior-coverage', ['legacy', 'removed-filter']],
  ] as const)('the actual %s callback supplies the existing bounded interface to every scenario', async (name, kinds) => {
    const calls: any[][] = [];
    const adapter = pathCaptureAdapter(async (...args: any[]) => {
      calls.push(args);
      throw new Error('Free capture boundary reached');
    });
    await expect(adapter.exercise(name, [...kinds])).rejects.toThrow('Free capture boundary reached');
    expect(adapter.attempts).toEqual([{ name, kinds: [...kinds], timeout: CAPTURE_LONG_MS }]);
    expect(calls).toHaveLength(kinds.length);
    for (const kind of kinds) {
      const fixture = adapter.fixtures.get(kind)!;
      const expected = reviewRevalidationPrompt(fixture, path.join(fixture.root, 'review-lifecycle.md'),
        path.join(fixture.root, 'current-advisory.jsonl'), adapter.prepared.get(kind)!.resumed)
        + '\nAll named caller sources are first-party authored runtime code. Inspect them directly, including any Git/path boundary, before deciding whether the previous review decision can be reused. The fixture contains no generated caller sources.';
      expect(calls.find(call => call[0] === fixture)).toEqual([fixture, name, expected, 'skip', { attempt: adapter.attempt }]);
      expect(adapter.rows.find(row => row.scenario === kind)?.row.passed).toBe(false);
      expect(adapter.removed).toContain(fixture.root);
    }
  });

  test('the actual path callback rejects max-turns after a correctly answered native Skip', async () => {
    const question = pathsNative.events[0].message.content[0];
    const answer = pathsNative.events[1].message.content[0];
    expect(question.name).toBe('AskUserQuestion');
    expect(answer.tool_use_id).toBe(question.id);
    expect(answer.is_error).not.toBe(true);
    expect(answer.content).toContain('="Skip".');
    expect(pathsNative.terminal).toMatchObject({ subtype: 'error_max_turns', is_error: true, num_turns: 31 });
    const questions: any[] = [];
    const answers: any[] = [];
    const adapter = pathCaptureAdapter(async (_fixture, _name, _prompt, choose) => {
      const permission = createSharedInteractiveToolHandler(choose, {
        nonQuestion: () => { throw new Error('Only the captured native question is expected'); },
        onQuestion: input => { questions.push(input); },
        onAnswer: (_input, values) => { answers.push(values); },
      });
      const decision = await permission(question.name, question.input);
      expect(decision).toEqual({ behavior: 'allow', updatedInput: { ...question.input,
        answers: { [question.input.questions[0].question]: 'Skip' } } });
      return { questions, result: { exitReason: pathsNative.terminal.subtype,
        turnsUsed: pathsNative.terminal.num_turns, durationMs: pathsNative.terminal.duration_ms,
        costUsd: pathsNative.terminal.total_cost_usd, costKnown: true,
        events: [...pathsNative.events, pathsNative.terminal],
        toolCalls: [{ tool: question.name, input: question.input }], output: '' } };
    });
    await expect(adapter.exercise('shared-libs-review-index-flags', ['skip-worktree'])).rejects.toThrow('error_max_turns');
    expect(answers).toEqual([{ [question.input.questions[0].question]: 'Skip' }]);
    expect(adapter.rows).toHaveLength(1);
    expect(adapter.rows[0]).toMatchObject({ scenario: 'skip-worktree', row: {
      passed: false, exit_reason: 'error_max_turns', turns_used: 31,
    } });
    expect(adapter.rows[0].row.transcript).toContainEqual(pathsNative.terminal);
    expect(adapter.rows[0].row.transcript).toContainEqual(pathsNative.events[1]);
    expect(adapter.removed).toEqual([adapter.fixtures.get('skip-worktree')!.root]);
  });
});
