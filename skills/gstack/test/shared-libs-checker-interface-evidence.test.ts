import { afterAll, beforeAll, describe, expect, test } from 'bun:test';
import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { createHash } from 'node:crypto';
import { sharedLibsFingerprint } from '../lib/review-evidence';
import { hasTrustedSharedLibsCheck, hasTrustedReviewStartRead } from './helpers/shared-libs-review-start-evidence';
import {
  createSharedInteractiveToolHandler, createSharedLibsFixture, fixtureGit, fixtureWorkingTree, fixtureWrite, installNormalizingFilter,
  reviewLifecycleInstructions, reviewRevalidationPrompt, reviewRecords, seedReviewSources, seedSkippedAdvisory,
  SHARED_LIBS_ROOT, shellQuote, toolCommandTrace,
  type SharedLibsFixture,
} from './helpers/shared-libs-eval-fixture';
import {
  preparePathEligibilityFixture, seedPathReviewPrerequisites, checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt, type PathEligibilityFixture,
} from './helpers/shared-libs-path-fixture';
import { CAPTURE_LONG_MS } from './helpers/eval-budgets';
import capturedChecker from './fixtures/shared-libs-index-flags-r59-checker-public.json';

const helper = path.join(SHARED_LIBS_ROOT, 'bin/gstack-review-log');
const fixtures: SharedLibsFixture[] = [];
const captures = new Map<string, any>();
const source = fs.readFileSync(path.join(import.meta.dir, 'skill-e2e-shared-libs.test.ts'), 'utf8');
const pathSource = fs.readFileSync(path.join(import.meta.dir, 'skill-e2e-shared-libs-paths.test.ts'), 'utf8');
const pathKinds = ['symlinks', 'submodule', 'ignored', 'legacy', 'assume-unchanged', 'skip-worktree', 'removed-filter'] as const;

async function capture(change: string, prepared?: PathEligibilityFixture, declared = false) {
  const f = prepared?.fixture ?? createSharedLibsFixture(`checker-${change}`);
  fixtures.push(f);
  let current = prepared?.current;
  if (!current) {
    seedReviewSources(f);
    fixtureWrite(f, 'src/retry-worker.ts', fs.readFileSync(path.join(f.repo, 'src/retry-worker.ts'), 'utf8')
      .replace('const unusedRetryDiagnostic = "unused";\n', ''));
    if (change === 'filtered') installNormalizingFilter(f);
    const { action: _action, ...finding } = await seedSkippedAdvisory(f);
    current = finding;
    if (change === 'branch') fixtureGit(f, 'checkout', '-b', 'feature-a');
    if (change === 'secondary') fixtureWrite(f, 'src/retry-route.ts',
      fs.readFileSync(path.join(f.repo, 'src/retry-route.ts'), 'utf8') + '// Changed caller\n');
    if (change === 'filtered') fixtureWrite(f, 'src/retry-route.ts',
      fs.readFileSync(path.join(f.repo, 'src/retry-route.ts'), 'utf8') + '// RAW-ONLY changed caller\n');
  }
  const events: any[] = [];
  const invoke = (command: string) => {
    const id = `call-${events.length}`;
    events.push({ type: 'assistant', message: { content: [{ type: 'tool_use', id, name: 'Bash', input: { command } }] } });
    const output = execFileSync('bash', ['-c', command], { cwd: f.repo, env: { ...process.env, ...f.env },
      encoding: 'utf8', timeout: 30_000 });
    events.push({ type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: id, content: output }] } });
    return output.trim();
  };
  const instructionFile = reviewLifecycleInstructions(f);
  const instructions = fs.readFileSync(instructionFile, 'utf8');
  const resumed = prepared?.resumed ?? seedPathReviewPrerequisites(f);
  const prerequisites = checkPathReviewPrerequisites(f, resumed.input);
  const protocol = declared ? [...reviewRevalidationPrompt(f, instructionFile, path.join(f.root, 'current-advisory.jsonl'), resumed)
    .matchAll(/```bash\n([\s\S]*?)\n```/g)].map(match => match[1]) : [];
  if (declared) expect(protocol).toHaveLength(3);
  const startCommand = instructions.match(/```bash\n(DIFF_BASE=\$[\s\S]*?)\n```/)?.[1];
  expect(startCommand).toBeDefined();
  const token = invoke(declared ? protocol[0] : startCommand!).split('\n')[0];
  if (declared) invoke('git diff origin/main');
  for (const file of current.evidence_paths) invoke(`cat ${shellQuote(file)}`);
  const checkAt = events.length;
  const receipt = JSON.parse(invoke(declared ? protocol[1].replace('REVIEW_START', token).replace('CURRENT_FINDING_JSON', JSON.stringify(current))
    : `${shellQuote(helper)} --check-shared-libs ${token} <<'FINDING'\n${JSON.stringify(current)}\nFINDING`));
  const questions: any[] = [];
  if (declared && !receipt.reusable) {
    const input = { questions: [{ question: 'Revalidate this supplied authored-source advisory?',
      options: [{ label: 'Fix', description: 'Apply the extraction.' },
        { label: 'Skip', description: 'Keep the source and index unchanged; record this advisory decision.' }] }] };
    const callback = createSharedInteractiveToolHandler('skip', {
      nonQuestion: () => { throw new Error('No non-question tool is allowed by this free actor'); },
      onQuestion: value => { questions.push(value); }, onAnswer: () => {},
    });
    const id = `decision-${events.length}`;
    events.push({ type: 'assistant', message: { content: [{ type: 'tool_use', id, name: 'AskUserQuestion', input }] } });
    const answer = await callback('AskUserQuestion', input);
    expect(answer.updatedInput.answers).toEqual({ [input.questions[0].question]: 'Skip' });
    events.push({ type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: id,
      content: JSON.stringify(answer.updatedInput.answers) }] } });
  }
  {
    const settled = JSON.parse(invoke(resumed.checkCommand));
    expect(settled).toEqual(checkPathReviewPrerequisites(f, resumed.input));
    expect(settled).toMatchObject({ synthetic: true, native_coverage: false, settled: true, current: true });
  }
  const finishAt = events.length;
  const record = JSON.stringify({ skill: 'review', status: 'clean', issues_found: 0,
    completed: true, converged: true, findings: change === 'unchanged' ? [] : [{ ...current, action: 'skipped' }] });
  invoke(declared ? protocol[2].replace("'FINAL_REVIEW_JSON'", shellQuote(record)).replace('REVIEW_START', token)
    : `${shellQuote(helper)} ${shellQuote(record)} --finish ${token} && ${shellQuote(path.join(SHARED_LIBS_ROOT, 'bin/gstack-review-read'))}`);
  const expected = { helper, repo: f.repo, state: f.state, slug: 'fixture-shared-libs',
    directory: path.join(f.state, 'projects/fixture-shared-libs/.review-starts'),
    branch: fixtureGit(f, 'symbolic-ref', '--quiet', '--short', 'HEAD'), wtree: fixtureWorkingTree(f),
    startedAt: receipt.review_start.started_at, finding: current, reusable: change === 'unchanged',
    coveredPaths: receipt.snapshot.covered_paths };
  return { f, current, token, receipt, events, checkAt, finishAt, expected, prepared, resumed, prerequisites, ...(declared ? { questions } : {}) };
}

beforeAll(async () => {
  for (const change of ['unchanged', 'secondary', 'branch', 'filtered']) captures.set(change, await capture(change));
  for (const kind of pathKinds) captures.set(`path-${kind}`, await capture(`path-${kind}`, preparePathEligibilityFixture(kind)));
  for (const change of ['unchanged', 'secondary', 'branch', 'filtered']) captures.set(`declared-${change}`, await capture(change, undefined, true));
  for (const kind of pathKinds) captures.set(`declared-path-${kind}`, await capture(`path-${kind}`, preparePathEligibilityFixture(kind), true));
}, 120_000);
afterAll(() => { for (const fixture of fixtures) fs.rmSync(fixture.root, { recursive: true, force: true }); });

function replay(change = 'unchanged') { return structuredClone(captures.get(change)); }
function call(run: any, at = run.checkAt) { return run.events[at].message.content[0]; }
function result(run: any, at = run.checkAt) { return run.events[at + 1].message.content[0]; }
function alterReceipt(run: any, change: (receipt: any) => void) {
  const receipt = JSON.parse(result(run).content);
  change(receipt);
  result(run).content = JSON.stringify(receipt);
}

function pathCallback(run: any, overrides: Record<string, any> = {}) {
  const start = pathSource.indexOf('async function exerciseEligibility(');
  const end = pathSource.indexOf('\ndescribeE2E(', start);
  const readStart = pathSource.indexOf('function sourceReadTrace(');
  expect(start).toBeGreaterThan(readStart);
  expect(end).toBeGreaterThan(start);
  const rows: any[] = [];
  const native = { events: run.events, exitReason: 'success', output: '', toolCalls: run.events
    .filter((event: any) => event.type === 'assistant')
    .flatMap((event: any) => event.message.content.filter((block: any) => block.type === 'tool_use')
      .map((block: any) => ({ tool: block.name, input: block.input }))) };
  const exercise = new Function('deps', new Bun.Transpiler({ loader: 'ts' }).transformSync(`const {
    captures, preparePathEligibilityFixture, fs, path, reviewLifecycleInstructions, reviewRevalidationPrompt,
    runSharedInteractive, readRequests, toolCommandTrace, fixtureGit, fixtureWorkingTree, reviewRecords, expect,
    CAPTURE_LONG_MS, hasTrustedSharedLibsCheck, SHARED_LIBS_ROOT,
    checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt } = deps;
    ${pathSource.slice(readStart, end)} return exerciseEligibility;`))({
    captures: { runAttempt: async (_name: string, _kinds: string[], _timeout: number, work: any) =>
      work({ add: (scenario: string, row: any) => rows.push({ scenario, row }) }) },
    preparePathEligibilityFixture: () => run.prepared,
    fs: { ...fs, rmSync: (directory: string) => { expect(directory).toBe(run.f.root); } }, path,
    reviewLifecycleInstructions, reviewRevalidationPrompt,
    runSharedInteractive: async (fixture: any, _name: string, prompt: string, choice: string) => {
      expect(choice).toBe('skip');
      expect(fixture).toBe(run.prepared.fixture);
      expect(prompt).toContain(reviewRevalidationPrompt(fixture, path.join(fixture.root, 'review-lifecycle.md'),
        path.join(fixture.root, 'current-advisory.jsonl'), run.prepared.resumed));
      return { result: native, questions: run.questions ?? [{}] };
    },
    readRequests: () => [], toolCommandTrace, fixtureGit, fixtureWorkingTree, reviewRecords, expect,
    CAPTURE_LONG_MS, hasTrustedSharedLibsCheck, SHARED_LIBS_ROOT,
    checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt, ...overrides,
  });
  return { rows, invoke: () => exercise('shared-libs-review-path-eligibility', [run.kind]) };
}

function capturedPathRun(index: number) {
  const captured = structuredClone(capturedChecker.attempts[index]);
  const events: any[] = captured.events;
  const blocks = events.flatMap(event => event.message.content);
  const calls = blocks.filter(block => block.type === 'tool_use');
  const returned = (id: string) => {
    const result = blocks.find(block => block.type === 'tool_result' && block.tool_use_id === id);
    expect(result).toBeDefined();
    expect(result.is_error).not.toBe(true);
    expect(typeof result.content).toBe('string');
    return result.content as string;
  };
  const files = new Map<string, string>(calls.filter(call => call.name === 'Read')
    .map(call => [call.input.file_path, returned(call.id).replace(/^\d+\t/gm, '')]));
  const repo = captured.repo, root = path.posix.dirname(repo), state = path.posix.join(root, 'state');
  const input = path.posix.join(root, 'resumed-review-prerequisites.json');
  const supplied = path.posix.join(root, 'current-advisory.jsonl');
  const current = JSON.parse(files.get(supplied)!);
  const prerequisiteCall = calls.find(call => call.input.command?.includes('--check-review-prerequisites'));
  const prerequisites = JSON.parse(returned(prerequisiteCall.id));
  expect(prerequisites).toMatchObject({ settled: true, current: true, context: { binding: { root, repo, state } } });
  expect(prerequisites.context).toEqual(JSON.parse(files.get(input)!));
  const finish = calls.find(call => call.input.command?.includes('--finish'));
  const records = returned(finish.id).split('\n').filter(line => line.startsWith('{')).map(line => JSON.parse(line));
  const f = { root, repo, state } as SharedLibsFixture;
  const run = { f, kind: 'assume-unchanged', events, questions: calls.filter(call => call.name === 'AskUserQuestion').map(call => call.input),
    prepared: { fixture: f, current, sourcePaths: ['src/retry-route.ts'], beforeTree: prerequisites.context.binding.wtree,
      resumed: { input, checkCommand: prerequisiteCall.input.command } } };
  expect(captured.exit_reason).toBe('success');
  expect(run.questions).toHaveLength(1);
  for (const call of calls.filter(call => call.name === 'AskUserQuestion')) {
    for (const question of call.input.questions) {
      expect(returned(call.id)).toContain(`"${question.question}"="Skip"`);
      expect(question.options.some((option: any) => option.label === 'Skip')).toBe(true);
    }
  }
  const overrides = {
    path: path.posix, SHARED_LIBS_ROOT: '/workspace/gstack',
    fs: {
      readFileSync: (file: string) => { expect(files.has(file)).toBe(true); return files.get(file); },
      realpathSync: (file: string) => file,
      writeFileSync: (file: string, contents: string) => {
        expect(file).toBe(supplied);
        expect(JSON.parse(contents)).toEqual(current);
      },
      rmSync: (directory: string) => { expect(directory).toBe(root); },
    },
    reviewLifecycleInstructions: () => path.posix.join(root, 'review-lifecycle.md'),
    checkPathReviewPrerequisites: (fixture: SharedLibsFixture, file: string) => {
      expect(fixture).toBe(f); expect(file).toBe(input); return prerequisites;
    },
    fixtureGit: (fixture: SharedLibsFixture, ...args: string[]) => {
      expect(fixture).toBe(f); expect(args).toEqual(['symbolic-ref', '--quiet', '--short', 'HEAD']);
      return prerequisites.context.binding.branch;
    },
    fixtureWorkingTree: (fixture: SharedLibsFixture) => {
      expect(fixture).toBe(f); return prerequisites.context.binding.wtree;
    },
    reviewRecords: (fixture: SharedLibsFixture) => { expect(fixture).toBe(f); return structuredClone(records); },
  };
  return { run, overrides };
}

describe('native executable shared-code checker evidence', () => {
  test.each([0, 1])('the complete R59 public attempt %i satisfies the actual callback without a manual fingerprint call', async index => {
    const { run, overrides } = capturedPathRun(index);
    const commands = run.events.flatMap(event => event.message.content)
      .filter(block => block.type === 'tool_use' && block.name === 'Bash').map(block => block.input.command).join('\n');
    expect(commands).not.toContain('sharedLibsFingerprint');
    let checks = 0;
    const adapter = pathCallback(run, { ...overrides, hasTrustedSharedLibsCheck: (events: unknown[], expected: any) => {
      checks++;
      expect(events).toBe(run.events);
      expect(expected.finding).toEqual(run.prepared.current);
      expect(expected.reusable).toBe(false);
      expect(expected.coveredPaths).toEqual(['src/retry-worker.ts', 'lib/retry-after.ts']);
      return hasTrustedSharedLibsCheck(events, expected);
    } });
    await adapter.invoke();
    expect(checks).toBe(1);
    expect(adapter.rows).toMatchObject([{ scenario: 'assume-unchanged', row: { passed: true } }]);
    const refused = pathCallback(run, { ...overrides, hasTrustedSharedLibsCheck: () => false });
    await expect(refused.invoke()).rejects.toThrow();
    expect(refused.rows[0].row.passed).toBe(false);
  });

  test.each([0, 1].flatMap(index => ['missing', 'failed', 'unpaired', 'fingerprint', 'binding', 'extra coverage'].map(invalid => [index, invalid] as const)))(
    'the R59 public attempt %i callback rejects a %s checker receipt', async (index, invalid) => {
      const { run, overrides } = capturedPathRun(index);
      const blocks = run.events.flatMap(event => event.message.content);
      const check = blocks.find(block => block.type === 'tool_use' && block.input.command?.includes('--check-shared-libs'));
      const receipt = blocks.find(block => block.type === 'tool_result' && block.tool_use_id === check.id);
      if (invalid === 'missing') receipt.content = '';
      else if (invalid === 'failed') receipt.is_error = true;
      else if (invalid === 'unpaired') receipt.tool_use_id = 'unpaired-checker';
      else {
        const value = JSON.parse(receipt.content);
        if (invalid === 'fingerprint') value.fingerprint = `shared-libs:${'a'.repeat(64)}`;
        if (invalid === 'binding') value.review_start.started_at = 'foreign';
        if (invalid === 'extra coverage') value.snapshot.covered_paths.push('src/retry-route.ts');
        receipt.content = JSON.stringify(value);
      }
      let checks = 0;
      const adapter = pathCallback(run, { ...overrides, hasTrustedSharedLibsCheck: (events: unknown[], expected: any) => {
        checks++;
        return hasTrustedSharedLibsCheck(events, expected);
      } });
      await expect(adapter.invoke()).rejects.toThrow();
      expect(checks).toBe(1);
      expect(adapter.rows[0].row.passed).toBe(false);
    });

  test('the actual path callback consumes the current synthetic prerequisite result without claiming native coverage', async () => {
    const run = replay('declared-path-assume-unchanged');
    run.kind = 'assume-unchanged';
    let verified = 0;
    const adapter = pathCallback(run, { hasPathReviewPrerequisiteReceipt: (events: any[], command: string, expected: any) => {
      verified++;
      expect(command).toBe(run.prepared.resumed.checkCommand);
      expect(expected).toMatchObject({ synthetic: true, native_coverage: false, settled: true, current: true });
      expect(expected.context.binding.repo).toBe(run.f.repo);
      expect(expected.context.binding.state).toBe(run.f.state);
      expect(expected.context.binding.index).toMatch(/^h /m);
      return hasPathReviewPrerequisiteReceipt(events, command, expected);
    } });
    await adapter.invoke();
    expect(verified).toBe(1);
    expect(adapter.rows[0].row.passed).toBe(true);
    expect(adapter.rows[0].row.transcript[0]).toMatchObject({
      prerequisite_source: 'synthetic-fixture-input', prerequisite_native_coverage: false,
    });
    const refused = pathCallback(run, { hasPathReviewPrerequisiteReceipt: () => false });
    await expect(refused.invoke()).rejects.toThrow('consume current synthetic prerequisites');
    expect(refused.rows[0].row.passed).toBe(false);
  });

  test.each(['missing', 'failed', 'unpaired', 'assistant-only', 'caption', 'false', 'foreign state', 'after finish'])(
    'the registered path callback rejects %s prerequisite receipts even with a completed record', async invalid => {
      const run = replay('declared-path-ignored');
      run.kind = 'ignored';
      const at = run.events.findIndex((event: any) => event.type === 'assistant'
        && event.message.content[0].input?.command === run.prepared.resumed.checkCommand);
      expect(at).toBeGreaterThan(run.checkAt);
      if (invalid === 'missing') run.events.splice(at, 2);
      if (invalid === 'failed') result(run, at).is_error = true;
      if (invalid === 'unpaired') result(run, at).tool_use_id = 'another-call';
      if (invalid === 'assistant-only') run.events[at + 1].type = 'assistant';
      if (invalid === 'caption') call(run, at).input = { command: 'true', description: run.prepared.resumed.checkCommand };
      if (invalid === 'false' || invalid === 'foreign state') {
        const receipt = JSON.parse(result(run, at).content);
        if (invalid === 'false') receipt.settled = false;
        else receipt.context.binding.state = '/another-fixture/state';
        result(run, at).content = JSON.stringify(receipt);
      }
      if (invalid === 'after finish') run.events.push(...run.events.splice(at, 2));
      const adapter = pathCallback(run);
      await expect(adapter.invoke()).rejects.toThrow('consume current synthetic prerequisites');
      expect(adapter.rows[0].row.passed).toBe(false);
    });

  test.each(['missing file', 'missing QA', 'empty probes', 'failed probe', 'missing native', 'failed native',
    'blocked native', 'foreign state', 'branch', 'base', 'index flag', 'raw hidden source', 'configuration', 'structured required'])(
    'synthetic prerequisites cannot settle with %s', async invalid => {
      const prepared = preparePathEligibilityFixture('assume-unchanged'), f = prepared.fixture;
      try {
        const original = checkPathReviewPrerequisites(f, prepared.resumed.input);
        expect(original.settled).toBe(true);
        const context = structuredClone(original.context);
        if (invalid === 'missing QA') delete context.qa;
        if (invalid === 'empty probes') context.qa.required_probes = [];
        if (invalid === 'failed probe') context.qa.required_probes[0].status = 'failed';
        if (invalid === 'missing native') delete context.native_adversarial;
        if (invalid === 'failed native' || invalid === 'blocked native') context.native_adversarial.status = invalid.split(' ')[0];
        if (invalid === 'foreign state') context.binding.state = '/another-fixture/state';
        if (invalid === 'structured required') context.structured_review.required = true;
        fs.writeFileSync(prepared.resumed.input, JSON.stringify(context));
        if (invalid === 'missing file') fs.unlinkSync(prepared.resumed.input);
        if (invalid === 'branch') fixtureGit(f, 'checkout', '-b', 'another-branch');
        if (invalid === 'base') fixtureGit(f, 'update-ref', 'refs/remotes/origin/main', 'HEAD~1');
        if (invalid === 'index flag') fixtureGit(f, 'update-index', '--no-assume-unchanged', 'src/retry-route.ts');
        if (invalid === 'raw hidden source') {
          fs.appendFileSync(path.join(f.repo, 'src/retry-route.ts'), '\n// Changed after the synthetic QA result\n');
          expect(fixtureWorkingTree(f)).toBe(original.context.binding.wtree);
        }
        if (invalid === 'configuration') fixtureGit(f, 'config', 'core.ignorecase', 'true');
        const checked = checkPathReviewPrerequisites(f, prepared.resumed.input);
        expect(checked.settled).toBe(false);
        expect(JSON.parse(execFileSync('bash', ['-c', prepared.resumed.checkCommand], {
          cwd: f.repo, env: { ...process.env, ...f.env }, encoding: 'utf8', timeout: 30_000,
        }))).toEqual(checked);
        const run = { f, prepared, kind: 'assume-unchanged', events: [] };
        const adapter = pathCallback(run);
        await expect(adapter.invoke()).rejects.toThrow('fixture prerequisites must be settled before capture');
      } finally { fs.rmSync(f.root, { recursive: true, force: true }); }
    });

  test.each(['raw source', 'supplied context'])('the registered callback rejects late changes to %s', async changed => {
    const run = replay('declared-path-assume-unchanged');
    run.kind = 'assume-unchanged';
    const file = changed === 'raw source' ? path.join(run.f.repo, 'src/retry-route.ts') : run.prepared.resumed.input;
    const before = fs.readFileSync(file, 'utf8');
    const adapter = pathCallback(run, { runSharedInteractive: async () => {
      fs.writeFileSync(file, before + '\n');
      return { questions: run.questions, result: { exitReason: 'success', output: '', events: run.events,
        toolCalls: run.events.filter((event: any) => event.type === 'assistant')
          .map((event: any) => ({ tool: event.message.content[0].name, input: event.message.content[0].input })) } };
    } });
    try {
      await expect(adapter.invoke()).rejects.toThrow('prerequisite state must remain unchanged');
      expect(adapter.rows[0].row.passed).toBe(false);
    } finally { fs.writeFileSync(file, before); }
  });

  test.each(['unchanged', 'secondary', 'branch', 'filtered'])('real %s checker receipt supplies the mechanical proof without manual trace words', change => {
    const run = replay(change);
    expect(run.receipt.reusable).toBe(change === 'unchanged');
    expect(hasTrustedSharedLibsCheck(run.events, run.expected)).toBe(true);
    expect(JSON.stringify(run.events)).not.toMatch(/check-attr|ls-files|canReuseSharedLibsAdvisory|sharedLibsFingerprint/);
    expect(hasTrustedReviewStartRead(run.events, run.expected)).toBe(false);
  });

  test.each(['literal cd', 'literal token assignment', 'native text blocks', 'native Read callers'])('preserves the bounded %s interface', form => {
    const run = replay();
    if (form === 'literal cd') call(run).input.command = `cd ${shellQuote(run.f.repo)} && ${call(run).input.command}`;
    if (form === 'literal token assignment') call(run).input.command = `TOKEN=${run.token}; `
      + call(run).input.command.replace(run.token, '"$TOKEN"');
    if (form === 'native text blocks') result(run).content = [{ type: 'text', text: result(run).content }];
    if (form === 'native Read callers') {
      for (let at = 2; at <= run.current.evidence_paths.length * 2; at += 2) {
        call(run, at).name = 'Read';
        call(run, at).input = { file_path: run.current.evidence_paths[at / 2 - 1] };
      }
    }
    expect(hasTrustedSharedLibsCheck(run.events, run.expected)).toBe(true);
  });

  test('the generated Step 3 start precedes diff output without losing the printed token', () => {
    const run = replay();
    expect(call(run, 0).input.command).toContain('DIFF_BASE=$(git merge-base origin/main HEAD)');
    expect(call(run, 0).input.command).toContain('git diff "$DIFF_BASE"');
    expect(result(run, 0).content).toStartWith(run.token + '\n');
    expect(result(run, 0).content).toContain('diff --git');
    expect(hasTrustedSharedLibsCheck(run.events, run.expected)).toBe(true);
  });

  test.each(['direct', 'assigned'])('retains %s start invocation proof', form => {
    const run = replay();
    call(run, 0).input.command = form === 'direct' ? `${shellQuote(helper)} --start review`
      : `REVIEW_START=$(${shellQuote(helper)} --start review); echo "$REVIEW_START"`;
    result(run, 0).content = run.token + '\n';
    expect(hasTrustedSharedLibsCheck(run.events, run.expected)).toBe(true);
  });

  test.each(['repo', 'branch', 'wtree', 'started_at', 'skill'])('rejects a mismatched start %s', field => {
    const run = replay();
    alterReceipt(run, receipt => { receipt.review_start[field] = 'foreign'; });
    expect(hasTrustedSharedLibsCheck(run.events, run.expected)).toBe(false);
  });

  test.each(['fingerprint', 'wtree', 'branch_id', 'partial coverage', 'extra coverage', 'duplicate coverage',
    'false', 'string true', 'missing decision', 'missing snapshot', 'missing start'])('rejects invalid %s', kind => {
    const run = replay();
    alterReceipt(run, receipt => {
      if (kind === 'fingerprint') receipt.fingerprint = 'shared-libs:' + 'a'.repeat(64);
      if (kind === 'wtree') receipt.snapshot.wtree = 'a'.repeat(40);
      if (kind === 'branch_id') receipt.snapshot.branch_id = createHash('sha256').update('feature-a').digest('hex');
      if (kind === 'partial coverage') receipt.snapshot.covered_paths.pop();
      if (kind === 'extra coverage') receipt.snapshot.covered_paths.push('src/unread.ts');
      if (kind === 'duplicate coverage') receipt.snapshot.covered_paths.push(receipt.snapshot.covered_paths[0]);
      if (kind === 'false') receipt.reusable = false;
      if (kind === 'string true') receipt.reusable = 'true';
      if (kind === 'missing decision') delete receipt.reusable;
      if (kind === 'missing snapshot') delete receipt.snapshot;
      if (kind === 'missing start') delete receipt.review_start;
    });
    expect(hasTrustedSharedLibsCheck(run.events, run.expected)).toBe(false);
  });

  test('even matching partial expectation and result cannot prove reusable coverage', () => {
    const run = replay();
    run.expected.coveredPaths.pop();
    alterReceipt(run, receipt => { receipt.snapshot.covered_paths = run.expected.coveredPaths; });
    expect(hasTrustedSharedLibsCheck(run.events, run.expected)).toBe(false);
  });

  test.each(['missing result', 'error result', 'unpaired result', 'assistant result', 'private result', 'caption',
    'echo', 'inert heredoc', 'quoted invocation', 'conditional invocation', 'forged appended result', 'foreign helper',
    'wrong token', 'wrong finish token', 'wrong start token', 'unknown token variable', 'rebound token variable',
    'foreign cwd', 'foreign state', 'source caption', 'missing caller read', 'errored caller read',
    'echo start', 'echo finish', 'inert batched start', 'forged base command', 'token only in diff',
    'private events', 'check before start', 'read after check',
    'receipt after finish', 'missing finish', 'failed finish'])('rejects %s', kind => {
    const run = replay();
    if (kind === 'missing result') run.events.splice(run.checkAt + 1, 1);
    if (kind === 'error result') result(run).is_error = true;
    if (kind === 'unpaired result') result(run).tool_use_id = 'foreign';
    if (kind === 'assistant result') run.events[run.checkAt + 1].type = 'assistant';
    if (kind === 'private result') run.events[run.checkAt + 1] = { type: 'fixture_result', content: result(run).content };
    if (kind === 'caption') call(run).input = { command: 'true', description: call(run).input.command };
    if (kind === 'echo') call(run).input.command = `echo ${shellQuote(result(run).content)}`;
    if (kind === 'inert heredoc') call(run).input.command = `cat <<'SOURCE'\n${call(run).input.command}\nSOURCE`;
    if (kind === 'quoted invocation') call(run).input.command = `printf '%s' ${shellQuote(call(run).input.command)}`;
    if (kind === 'conditional invocation') call(run).input.command = `false && ${call(run).input.command}`;
    if (kind === 'forged appended result') call(run).input.command += `\necho ${shellQuote(result(run).content)}`;
    if (kind === 'foreign helper') call(run).input.command = call(run).input.command.replace(helper, '/foreign/gstack-review-log');
    if (kind === 'wrong token') call(run).input.command = call(run).input.command.replace(run.token, 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa');
    if (kind === 'wrong finish token') call(run, run.finishAt).input.command = call(run, run.finishAt).input.command.replace(run.token, 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa');
    if (kind === 'wrong start token') result(run, 0).content = 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa';
    if (kind === 'unknown token variable') call(run).input.command = call(run).input.command.replace(run.token, '"$UNKNOWN"');
    if (kind === 'rebound token variable') call(run).input.command = `TOKEN=${run.token}; TOKEN=aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa; `
      + call(run).input.command.replace(run.token, '"$TOKEN"');
    if (kind === 'foreign cwd') call(run).input.command = `cd /foreign; ${call(run).input.command}`;
    if (kind === 'foreign state') call(run).input.command = `GSTACK_HOME=/foreign; ${call(run).input.command}`;
    if (kind === 'source caption') call(run, 2).input = { command: 'true', description: 'cat src/retry-worker.ts' };
    if (kind === 'missing caller read') run.events.splice(2, 2);
    if (kind === 'errored caller read') result(run, 2).is_error = true;
    if (kind === 'echo start') call(run, 0).input.command = `echo ${shellQuote(call(run, 0).input.command)}`;
    if (kind === 'echo finish') call(run, run.finishAt).input.command = `echo ${shellQuote(call(run, run.finishAt).input.command)}`;
    if (kind === 'inert batched start') call(run, 0).input.command = `cat <<'SOURCE'\n${call(run, 0).input.command}\nSOURCE`;
    if (kind === 'forged base command') call(run, 0).input.command = call(run, 0).input.command.replace('git merge-base origin/main HEAD', 'echo fake-base');
    if (kind === 'token only in diff') result(run, 0).content = `diff --git a/file b/file\n+${run.token}\n`;
    if (kind === 'private events') run.events = [{ type: 'fixture_events', events: run.events }];
    if (kind === 'check before start') run.events = [...run.events.slice(run.checkAt, run.checkAt + 2), ...run.events.slice(0, run.checkAt), ...run.events.slice(run.finishAt)];
    if (kind === 'read after check') run.events = [...run.events.slice(0, 2), ...run.events.slice(4, run.finishAt), ...run.events.slice(2, 4), ...run.events.slice(run.finishAt)];
    if (kind === 'receipt after finish') run.events = [...run.events.slice(0, run.checkAt + 1), ...run.events.slice(run.finishAt), run.events[run.checkAt + 1]];
    if (kind === 'missing finish') run.events.splice(run.finishAt);
    if (kind === 'failed finish') result(run, run.finishAt).is_error = true;
    expect(hasTrustedSharedLibsCheck(run.events, run.expected)).toBe(false);
  });

  test.each(['unchanged', 'secondary', 'branch', 'filtered'])('the registered native %s acceptance callback consumes the checker result and coverage', change => {
    const scenario = source.slice(source.indexOf("test('shared-libs-review-revalidation'"));
    const marker = '}, result => {';
    const start = scenario.indexOf(marker) + marker.length;
    const body = scenario.slice(start, scenario.indexOf('\n        });', start));
    const verify = new Function('deps', 'result', new Bun.Transpiler({ loader: 'ts' }).transformSync(`const {
      change, questions, expect, toolCommandTrace, reviewRecords, createHash, path, f, current,
      fixtureGit, fixtureWorkingTree, hasTrustedReviewStartRead, hasTrustedSharedLibsCheck, SHARED_LIBS_ROOT,
      resumed, prerequisites, checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt
    } = deps; ${body}`));
    const run = replay(change);
    const native = { events: run.events, toolCalls: run.events.filter((event: any) => event.type === 'assistant')
      .map((event: any) => ({ tool: event.message.content[0].name, input: event.message.content[0].input })) };
    let checks = 0;
    const deps = { change, questions: change === 'unchanged' ? [] : [{}], expect, toolCommandTrace,
      reviewRecords, createHash, path, f: run.f, current: run.current, fixtureGit, fixtureWorkingTree,
      hasTrustedReviewStartRead, SHARED_LIBS_ROOT, resumed: run.resumed, prerequisites: run.prerequisites,
      checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt, hasTrustedSharedLibsCheck: (events: unknown[], expected: any) => {
        checks++;
        expect(events).toBe(run.events);
        expect(expected).toEqual(run.expected);
        return hasTrustedSharedLibsCheck(events, expected);
      } };
    verify(deps, native);
    expect(checks).toBe(1);
    expect(() => verify({ ...deps, hasTrustedSharedLibsCheck: () => false }, native)).toThrow();
    const partial = structuredClone(native);
    partial.events[run.checkAt + 1].message.content[0].content = JSON.stringify({ ...run.receipt,
      snapshot: { ...run.receipt.snapshot, covered_paths: [] } });
    expect(() => verify({ ...deps, hasTrustedSharedLibsCheck }, partial)).toThrow();
    for (const invalid of ['false', 'missing', 'error', 'caption', 'unread caller']) {
      const changed = structuredClone(native);
      const block = changed.events[run.checkAt + 1].message.content[0];
      if (invalid === 'false') block.content = JSON.stringify({ ...run.receipt, reusable: !run.expected.reusable });
      if (invalid === 'missing') changed.events.splice(run.checkAt + 1, 1);
      if (invalid === 'error') block.is_error = true;
      if (invalid === 'caption') changed.events[run.checkAt].message.content[0].input = { command: 'true', description: call(run).input.command };
      if (invalid === 'unread caller') changed.events.splice(2, 2);
      expect(() => verify({ ...deps, hasTrustedSharedLibsCheck }, changed)).toThrow();
    }
    expect(() => verify({ ...deps, questions: change === 'unchanged' ? [{}] : [] }, native)).toThrow();
    expect(() => verify({ ...deps, reviewRecords: () => [] }, native)).toThrow();
  });

  test.each(pathKinds)('the actual %s callback consumes false proof and enforces final excluded coverage', async kind => {
    const run = replay(`path-${kind}`);
    run.kind = kind;
    expect(run.receipt.reusable).toBe(false);
    let checks = 0;
    const adapter = pathCallback(run, { hasTrustedSharedLibsCheck: (events: unknown[], expected: any) => {
      checks++;
      expect(events).toBe(run.events);
      expect(expected).toEqual(run.expected);
      return hasTrustedSharedLibsCheck(events, expected);
    } });
    await adapter.invoke();
    expect(checks).toBe(1);
    expect(adapter.rows).toMatchObject([{ scenario: kind, row: { passed: true } }]);
    const refused = pathCallback(run, { hasTrustedSharedLibsCheck: () => false });
    await expect(refused.invoke()).rejects.toThrow();
    expect(refused.rows).toMatchObject([{ scenario: kind, row: { passed: false } }]);
    if (kind !== 'legacy' && kind !== 'removed-filter') {
      const forged = pathCallback(run, { hasTrustedSharedLibsCheck: () => true,
        reviewRecords: (fixture: SharedLibsFixture) => {
          const records = reviewRecords(fixture);
          records.at(-1).findings[0].snapshot_covered_paths = [...run.current.evidence_paths];
          return records;
        } });
      await expect(forged.invoke()).rejects.toThrow();
      expect(forged.rows).toMatchObject([{ scenario: kind, row: { passed: false } }]);
    }
  });

  test.each(['true', 'error', 'missing result', 'partial coverage', 'caption', 'unread caller', 'wrong tree', 'late receipt'])(
    'the actual symlink callback rejects %s instead of waiving path eligibility', async invalid => {
      const run = replay('path-symlinks');
      run.kind = 'symlinks';
      if (invalid === 'true') alterReceipt(run, receipt => { receipt.reusable = true; });
      if (invalid === 'error') result(run).is_error = true;
      if (invalid === 'missing result') run.events.splice(run.checkAt + 1, 1);
      if (invalid === 'partial coverage') alterReceipt(run, receipt => { receipt.snapshot.covered_paths = []; });
      if (invalid === 'caption') call(run).input = { command: 'true', description: call(run).input.command };
      if (invalid === 'unread caller') run.events.splice(4, 2);
      if (invalid === 'wrong tree') alterReceipt(run, receipt => { receipt.snapshot.wtree = 'a'.repeat(40); });
      if (invalid === 'late receipt') run.events = [...run.events.slice(0, run.checkAt + 1), ...run.events.slice(run.finishAt), run.events[run.checkAt + 1]];
      const adapter = pathCallback(run);
      await expect(adapter.invoke()).rejects.toThrow();
      expect(adapter.rows).toMatchObject([{ scenario: 'symlinks', row: { passed: false } }]);
    });

  test.each(['unchanged', 'secondary', 'branch', 'filtered'])('declared receipt commands satisfy the real %s revalidation callback', change => {
    const run = replay(`declared-${change}`);
    expect(result(run, 0).content.trim()).toBe(run.token);
    expect(JSON.parse(result(run).content)).toEqual(run.receipt);
    expect(call(run, run.finishAt).input.command).toContain(`--finish ${run.token} && '${SHARED_LIBS_ROOT}/bin/gstack-review-read'`);
    expect(run.questions).toHaveLength(change === 'unchanged' ? 0 : 1);
    const scenario = source.slice(source.indexOf("test('shared-libs-review-revalidation'"));
    const marker = '}, result => {';
    const start = scenario.indexOf(marker) + marker.length;
    const body = scenario.slice(start, scenario.indexOf('\n        });', start));
    const verify = new Function('deps', 'result', new Bun.Transpiler({ loader: 'ts' }).transformSync(`const {
      change, questions, expect, toolCommandTrace, reviewRecords, createHash, path, f, current,
      fixtureGit, fixtureWorkingTree, hasTrustedReviewStartRead, hasTrustedSharedLibsCheck, SHARED_LIBS_ROOT,
      resumed, prerequisites, checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt
    } = deps; ${body}`));
    const native = { events: run.events, toolCalls: run.events.filter((event: any) => event.type === 'assistant')
      .map((event: any) => ({ tool: event.message.content[0].name, input: event.message.content[0].input })) };
    let checks = 0;
    verify({ change, questions: run.questions, expect, toolCommandTrace, reviewRecords, createHash, path,
      f: run.f, current: run.current, fixtureGit, fixtureWorkingTree, hasTrustedReviewStartRead, SHARED_LIBS_ROOT,
      resumed: run.resumed, prerequisites: run.prerequisites, checkPathReviewPrerequisites, hasPathReviewPrerequisiteReceipt,
      hasTrustedSharedLibsCheck: (events: unknown[], expected: any) => {
        checks++;
        expect(events).toBe(run.events);
        expect(expected).toEqual(run.expected);
        return hasTrustedSharedLibsCheck(events, expected);
      } }, native);
    expect(checks).toBe(1);
  });

  test.each(pathKinds)('declared receipt commands satisfy the real %s path callback after an actual actor decision', async kind => {
    const run = replay(`declared-path-${kind}`);
    run.kind = kind;
    expect(result(run, 0).content.trim()).toBe(run.token);
    expect(JSON.parse(result(run).content)).toEqual(run.receipt);
    expect(run.receipt.reusable).toBe(false);
    expect(run.questions).toHaveLength(1);
    let checks = 0;
    const adapter = pathCallback(run, { hasTrustedSharedLibsCheck: (events: unknown[], expected: any) => {
      checks++;
      expect(expected).toEqual(run.expected);
      return hasTrustedSharedLibsCheck(events, expected);
    } });
    await adapter.invoke();
    expect(checks).toBe(1);
    expect(adapter.rows).toMatchObject([{ scenario: kind, row: { passed: true } }]);
  });

  test.each(['revised-only identity', 'unsupported finding', 'unfinished review', 'missing explicit decision'])(
    'canonical transport cannot waive %s in the real ignored-path callback', async failure => {
      const run = replay('declared-path-ignored');
      run.kind = 'ignored';
      if (failure === 'missing explicit decision') run.questions = [];
      const adapter = pathCallback(run, { reviewRecords: (fixture: SharedLibsFixture) => {
        const records = reviewRecords(fixture);
        const final = records.at(-1);
        if (failure === 'revised-only identity') {
          const revised = { ...final.findings[0], evidence_paths: [
            'src/retry-worker.ts', 'src/scheduler.ts', 'src/retry-route.ts', 'lib/retry-after.ts',
          ] };
          revised.fingerprint = sharedLibsFingerprint(revised);
          revised.snapshot_covered_paths = [...revised.evidence_paths];
          expect(revised.fingerprint).not.toBe(run.current.fingerprint);
          final.findings = [revised];
        }
        if (failure === 'unsupported finding') final.findings = [];
        if (failure === 'unfinished review') final.completed = false;
        return records;
      } });
      await expect(adapter.invoke()).rejects.toThrow();
      expect(adapter.rows).toMatchObject([{ scenario: 'ignored', row: { passed: false } }]);
    });

  test.each(['start batching', 'mixed checker stdout', 'finish metadata prelude', 'loop-only caller reads'])(
    'the unchanged detector still rejects %s after protocol declaration', shape => {
      const run = replay('declared-unchanged');
      if (shape === 'start batching') call(run, 0).input.command = `echo start; ${call(run, 0).input.command}; git diff origin/main`;
      if (shape === 'mixed checker stdout') result(run).content = `checker receipt:\n${result(run).content}\nexit=0`;
      if (shape === 'finish metadata prelude') call(run, run.finishAt).input.command = `TS=$(date -u); ${call(run, run.finishAt).input.command}`;
      if (shape === 'loop-only caller reads') {
        for (const event of run.events) for (const block of event.message.content) {
          if (block.type === 'tool_use' && block.name === 'Bash' && block.input.command.startsWith('cat ')) {
            block.input.command = `for f in ${block.input.command.slice(4)}; do cat "$f"; done`;
          }
        }
      }
      expect(hasTrustedSharedLibsCheck(run.events, run.expected)).toBe(false);
    });
});
