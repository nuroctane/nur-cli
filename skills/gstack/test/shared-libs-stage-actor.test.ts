import { afterEach, expect, test } from 'bun:test';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import * as fixtures from './helpers/shared-libs-eval-fixture';
import { createLifecyclePrerequisiteActor } from './helpers/shared-libs-path-fixture';
import { sharedLibsFingerprint } from '../lib/review-evidence';
import { CAPTURE_MS, CAPTURE_LONG_MS } from './helpers/eval-budgets';
import { E2E_TOUCHFILES, GLOBAL_TOUCHFILES, selectTests } from './helpers/touchfiles';
import stageScope from './fixtures/shared-libs-lifecycle-r59-stage-scope-public.json';

const roots: string[] = [];
afterEach(() => { for (const root of roots.splice(0)) fs.rmSync(root, { recursive: true, force: true }); });
const source = fs.readFileSync(path.join(import.meta.dir, 'skill-e2e-shared-libs.test.ts'), 'utf8');
const helper = fs.readFileSync(path.join(import.meta.dir, 'helpers/shared-libs-eval-fixture.ts'), 'utf8');
const transpile = (text: string) => new Bun.Transpiler({ loader: 'ts' }).transformSync(text);
const record = source.slice(source.indexOf('async function recordCapture('), source.indexOf('\nfunction assertReadOnly('));
const registration = source.slice(source.indexOf("  test('shared-libs-review-lifecycle'"), source.indexOf("  test('shared-libs-review-revalidation'"));

function runner(f: fixtures.SharedLibsFixture, mode: string, observations: any[]) {
  let body = helper.slice(helper.indexOf('export async function runSharedInteractive(')).replace('export async function', 'async function');
  for (const [module, replacement] of [['./agent-sdk-runner', 'deps.sdk'], ['@anthropic-ai/claude-agent-sdk', 'deps.provider'], ['./eval-budgets', 'deps.budgets']]) {
    body = body.replace(`await import('${module}')`, replacement);
  }
  let session: any;
  const query = (args: any) => ({ async *[Symbol.asyncIterator]() {
    expect(args.options.hooks).toBe(session.stageActor.hooks);
    const events: any[] = [];
    const hooks = args.options.hooks;
    let count = 0;
    const hook = async (kind: 'PreToolUse' | 'PostToolUse', tool: string, input: any, id: string) => {
      for (const matcher of hooks[kind] ?? []) for (const callback of matcher.hooks) {
        const result = await callback({ hook_event_name: kind, tool_name: tool, tool_input: input,
          tool_use_id: id, cwd: f.repo, session_id: 'free-lifecycle', transcript_path: path.join(f.root, 'public.jsonl'),
          ...(kind === 'PostToolUse' ? { tool_response: {} } : {}) }, mode === 'optional hook id' ? undefined : id, { signal: new AbortController().signal });
        expect(result.hookSpecificOutput?.permissionDecision).not.toBe('deny');
      }
    };
    const invoke = async (command: string, isStage = false) => {
      const id = `free-${++count}`, input = { command };
      events.push({ type: 'assistant', message: { content: [{ type: 'tool_use', name: 'Bash', id, input }] } });
      if (!(isStage && mode === 'no-call')) await hook('PreToolUse', 'Bash', input, id);
      const text = isStage && mode === 'no-call' ? JSON.stringify({ synthetic: true, native_coverage: false, settled: true })
        : execFileSync('bash', ['-c', command], { cwd: f.repo, env: { ...process.env, ...f.env }, encoding: 'utf8', timeout: 30_000 }).trim();
      await hook('PostToolUse', 'Bash', input, id);
      events.push({ type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: id, content: text }] } });
      return text;
    };
    const edit = async (relative: string, text: string) => {
      const id = `free-${++count}`, input = { file_path: path.join(f.repo, relative), content: text };
      events.push({ type: 'assistant', message: { content: [{ type: 'tool_use', name: 'Write', id, input }] } });
      await hook('PreToolUse', 'Write', input, id);
      fixtures.fixtureWrite(f, relative, text);
      await hook('PostToolUse', 'Write', input, id);
      events.push({ type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: id, content: 'written' }] } });
    };
    const actor = session.stageActor;
    await invoke(`cat ${fixtures.shellQuote(path.join(fixtures.SHARED_LIBS_ROOT, 'review/checklist.md'))} src/retry-worker.ts src/retry-route.ts lib/retry-after.ts`);
    const initial = mode === 'missing' ? undefined : await invoke(actor.actorCommand, true);
    const question = mode === 'post-fix verification' ? structuredClone(stageScope.decision.call.input) : { questions: [{ question: 'Apply the supported shared-code advisory?', options: [
      { label: 'Fix as recommended', description: 'Replace both inline parsers with the existing helper.' },
      { label: 'Skip', description: 'Keep both inline parsers unchanged.' },
    ] }] };
    const answer = await session.canUseTool('AskUserQuestion', question);
    const selected = answer.updatedInput.answers[question.questions[0].question];
    const approved = /Fix as recommended/.test(selected);
    const worker = fs.readFileSync(path.join(f.repo, 'src/retry-worker.ts'), 'utf8').replace('const unusedRetryDiagnostic = "unused";\n', '');
    await edit('src/retry-worker.ts', approved ? "export { retrySeconds } from '../lib/retry-after';\n" : worker);
    if (approved) await edit('src/retry-route.ts', "export { retrySeconds } from '../lib/retry-after';\n");
    let verification: string | undefined;
    if (mode === 'post-fix verification') {
      if (approved) expect(stageScope.decision.result.content).toContain(`"${question.questions[0].question}"="${selected}"`);
      verification = await invoke(stageScope.post_fix_verification.call.input.command);
      expect(verification).toContain('1 pass');
      expect(verification).toContain('0 fail');
      expect(verification).toContain(`worker===lib ${approved} route===lib ${approved} sample 42 5`);
    }
    const token = await invoke(`${fixtures.shellQuote(path.join(fixtures.SHARED_LIBS_ROOT, 'bin/gstack-review-log'))} --start review`);
    await invoke('git diff origin/main && cat src/retry-worker.ts src/retry-route.ts lib/retry-after.ts');
    const finding = { severity: 'INFORMATIONAL', confidence: 9, advisory: true, category: 'shared-libs',
      path: 'src/retry-worker.ts', line: 1, summary: 'Use the established retry contract',
      evidence_paths: ['src/retry-worker.ts', 'src/retry-route.ts', 'lib/retry-after.ts'],
      helper_target: { path: 'lib/retry-after.ts', symbol: 'retrySeconds' }, action: approved ? 'fixed' : 'skipped' };
    const fingerprint = await invoke(`bun -e 'const { sharedLibsFingerprint } = await import(process.argv[1]); console.log(sharedLibsFingerprint(JSON.parse(await Bun.stdin.text())));' ${fixtures.shellQuote(path.join(fixtures.SHARED_LIBS_ROOT, 'lib/review-evidence.ts'))} <<'FINDING'\n${JSON.stringify(finding)}\nFINDING`);
    let current: string | undefined;
    if (mode === 'failed') await edit('src/retry-route.ts', '');
    if (!['missing', 'stale', 'relabeled old'].includes(mode)) current = await invoke(actor.actorCommand, true);
    if (mode === 'relabeled old') {
      const receipt = JSON.parse(initial!);
      const id = `free-${++count}`;
      receipt.id = 'relabeled-old-result'; receipt.tool_use_id = id; receipt.generation++;
      events.push({ type: 'assistant', message: { content: [{ type: 'tool_use', name: 'Bash', id, input: { command: actor.actorCommand } }] } },
        { type: 'user', message: { content: [{ type: 'tool_result', tool_use_id: id, content: JSON.stringify(receipt) }] } });
    }
    if (mode === 'late edit') await edit('src/retry-route.ts', fs.readFileSync(path.join(f.repo, 'src/retry-route.ts'), 'utf8') + '\n// late edit\n');
    if (mode === 'edited then restored') {
      const before = fs.readFileSync(path.join(f.repo, 'src/retry-route.ts'), 'utf8');
      await edit('src/retry-route.ts', before + '\n// temporary edit\n');
      await edit('src/retry-route.ts', before);
    }
    const record = { skill: 'review', status: 'clean', issues_found: 0, critical: 0, informational: 0,
      completed: true, converged: true, findings: [{ ...finding, fingerprint }] };
    await invoke(`${fixtures.shellQuote(path.join(fixtures.SHARED_LIBS_ROOT, 'bin/gstack-review-log'))} ${fixtures.shellQuote(JSON.stringify(record))} --finish ${token} && ${fixtures.shellQuote(path.join(fixtures.SHARED_LIBS_ROOT, 'bin/gstack-review-read'))}`);
    const stageResults = events.filter(event => event.type === 'user' && event.message.content[0].content === current);
    const result = stageResults.at(-1)?.message.content[0];
    if (mode === 'missing result') events.splice(events.indexOf(stageResults.at(-1)), 1);
    if (mode === 'failed result') result.is_error = true;
    if (mode === 'foreign' || mode === 'failed QA' || mode === 'missing native') {
      const receipt = JSON.parse(result.content);
      if (mode === 'foreign') receipt.binding.state = '/foreign/state';
      if (mode === 'failed QA') receipt.qa.required_probes[0].status = 'failed';
      if (mode === 'missing native') delete receipt.native_adversarial;
      result.content = JSON.stringify(receipt);
    }
    if (mode === 'late result') events.push(...events.splice(events.indexOf(stageResults.at(-1)), 1));
    if (mode === 'rewritten history') {
      const first = JSON.parse(initial!);
      fs.writeFileSync(path.join(f.root, 'synthetic-stage-receipts', `${first.id}.json`), current!);
    }
    const receipts = fs.readdirSync(path.join(f.root, 'synthetic-stage-receipts')).filter(file => file !== 'current.json');
    if (mode === 'valid' || mode === 'optional hook id') {
      expect(receipts).toHaveLength(2);
      const first = JSON.parse(initial!), last = JSON.parse(current!);
      expect(first.id).not.toBe(last.id);
      expect(first.binding).not.toEqual(last.binding);
      expect(fs.readFileSync(path.join(f.root, 'synthetic-stage-receipts', `${first.id}.json`), 'utf8')).toBe(initial);
      expect(last).toMatchObject({ synthetic: true, native_coverage: false, settled: true });
    }
    observations.push({ f, initial, current, events, receipts, verification });
    for (const event of events) yield event;
    yield { type: 'result', subtype: 'success', total_cost_usd: 0 };
  } });
  return new Function('deps', `const { fs, path, SHARED_LIBS_ROOT, SHARED_INTERACTIVE_MAX_TURNS,
    createSharedInteractiveToolHandler, installSourceShims, readRequests } = deps;
    ${transpile(body)} return async (...args) => { deps.session(args[4].stageActor); return runSharedInteractive(...args); };`)({
    fs, path, SHARED_LIBS_ROOT: f.root, SHARED_INTERACTIVE_MAX_TURNS: fixtures.SHARED_INTERACTIVE_MAX_TURNS,
    createSharedInteractiveToolHandler: fixtures.createSharedInteractiveToolHandler, installSourceShims: fixtures.installSourceShims,
    readRequests: fixtures.readRequests, budgets: { CAPTURE_MS }, session: (stageActor: any) => { session = { stageActor }; },
    sdk: { resolveClaudeBinary: () => '/free/claude', passThroughNonAskUserQuestion: (_name: string, input: any) => ({ behavior: 'allow', updatedInput: input }),
      runAgentSdkTest: async (options: any) => {
        session.canUseTool = options.canUseTool;
        expect(options.maxTurns).toBe(30); expect(options.maxRetries).toBe(0);
        const events: any[] = [];
        for await (const event of options.queryProvider({ prompt: options.userPrompt, options: { abortController: new AbortController() } })) events.push(event);
        return { exitReason: 'success', events, output: 'Synthetic fixture-stage inputs, not actual QA/adversarial coverage.',
          toolCalls: events.filter(event => event.type === 'assistant').flatMap(event => event.message.content
            .filter((block: any) => block.type === 'tool_use').map((block: any) => ({ tool: block.name, input: block.input }))) };
      } }, provider: { query },
  });
}

function lifecycle(mode: string) {
  const rows: any[] = [], observations: any[] = [];
  let registered: () => Promise<void>;
  new Function('deps', `const { test, captures, fs, path, expect, CAPTURE_LONG_MS, createHash, sharedLibsFingerprint,
    createSharedLibsFixture, seedReviewSources, reviewLifecycleInstructions, specialistFixture, createLifecyclePrerequisiteActor,
    reviewPrompt, runSharedInteractive, reviewRecords, toolCommandTrace } = deps;
    ${transpile(record + '\n' + registration)}`)({
    ...fixtures, createHash, sharedLibsFingerprint, path, expect, CAPTURE_LONG_MS, createLifecyclePrerequisiteActor,
    fs: { ...fs, rmSync: (root: string) => { expect(roots).toContain(root); } },
    test: (name: string, body: () => Promise<void>, timeout: number) => {
      expect(name).toBe('shared-libs-review-lifecycle'); expect(timeout).toBe(CAPTURE_LONG_MS); registered = body;
    },
    captures: { runAttempt: async (_name: string, cases: string[], _timeout: number, work: any) => {
      expect(cases).toEqual(['skip', 'approve']); return work({ signal: new AbortController().signal,
        remainingMs: () => CAPTURE_LONG_MS, add: (scenario: string, row: any) => rows.push({ scenario, row }) });
    } },
    createSharedLibsFixture: (name: string) => { const f = fixtures.createSharedLibsFixture(name); roots.push(f.root); return f; },
    runSharedInteractive: async (f: fixtures.SharedLibsFixture, name: string, prompt: string, choose: string, options: any) => {
      expect(prompt).toBe(fixtures.reviewPrompt(f, path.join(f.root, 'review-lifecycle.md'), path.join(f.root, 'specialist-input.jsonl'), options.stageActor));
      if (mode === 'post-fix verification') {
        expect(prompt).toContain('replaces the entire Step 4.7 QA and Step 4.8 native adversarial stages');
        expect(prompt).toContain('Existing tests and caller/import checks needed to verify your source fixes still run');
      }
      return runner(f, mode, observations)(f, name, prompt, choose, options);
    },
  });
  return { invoke: () => registered!(), rows, observations };
}

test('the registered lifecycle callbacks retain captured post-fix verification without exploratory QA', async () => {
  const adapter = lifecycle('post-fix verification');
  await adapter.invoke();
  expect(adapter.rows).toHaveLength(2);
  for (const { row } of adapter.rows) {
    expect(row.passed).toBe(true);
    expect(row.transcript.at(-1)).toMatchObject({ prerequisite_source: 'synthetic-fixture-stage-actor', prerequisite_native_coverage: false });
    expect(row.transcript.at(-1).prerequisite_receipts).toHaveLength(2);
  }
  for (const observation of adapter.observations) {
    const calls = observation.events.filter((event: any) => event.type === 'assistant')
      .flatMap((event: any) => event.message.content);
    const commands = calls.filter((call: any) => call.name === 'Bash').map((call: any) => call.input.command);
    expect(commands).toContain(stageScope.post_fix_verification.call.input.command);
    expect(commands.join('\n')).toContain('review/checklist.md');
    expect(commands.join('\n')).toContain('git diff origin/main');
    expect(commands.join('\n')).toContain('sharedLibsFingerprint');
    expect(commands.join('\n')).toContain('--finish');
    expect(commands.filter((command: string) => command.includes('synthetic-stage-receipts/current.json'))).toHaveLength(2);
    expect(JSON.stringify(calls)).not.toMatch(/qa\/sections\/|qa-reports|exploration-\d+\.json|charter\.md|functional-report\.md|command -v aside/);
    expect(observation.verification).toContain('0 fail');
  }
}, 30_000);

test.each(['valid', 'optional hook id'])('both registered lifecycle cases invoke fresh synthetic stages after actual approved edits (%s)', async mode => {
  const adapter = lifecycle(mode);
  await adapter.invoke();
  expect(adapter.rows).toHaveLength(2);
  expect(adapter.observations).toHaveLength(2);
  for (const { row } of adapter.rows) {
    expect(row.passed).toBe(true);
    expect(row.transcript.at(-1)).toMatchObject({ prerequisite_source: 'synthetic-fixture-stage-actor', prerequisite_native_coverage: false });
    expect(row.transcript.at(-1).prerequisite_receipts).toHaveLength(2);
    expect(row.transcript.at(-1).prerequisite_receipts.every((receipt: any) => receipt.synthetic && receipt.native_coverage === false)).toBe(true);
  }
}, 30_000);

test.each(['missing', 'no-call', 'stale', 'relabeled old', 'late edit', 'edited then restored', 'failed',
  'missing result', 'failed result', 'foreign', 'failed QA', 'missing native', 'late result', 'rewritten history'])(
  'the registered lifecycle callbacks reject %s despite completed final records', async mode => {
    const adapter = lifecycle(mode);
    await expect(adapter.invoke()).rejects.toThrow('consume a current invoked synthetic stage result');
    expect(adapter.rows).toHaveLength(2);
    for (const { row } of adapter.rows) {
      expect(row.passed).toBe(false);
      expect(row.transcript.at(-1)).toMatchObject({ prerequisite_source: 'synthetic-fixture-stage-actor', prerequisite_native_coverage: false });
    }
  }, 30_000);

test('the actor helper selects both existing neighboring native bodies', () => {
  for (const file of ['test/helpers/shared-libs-path-fixture.ts']) {
    const selected = selectTests([file], E2E_TOUCHFILES, GLOBAL_TOUCHFILES).selected;
    expect(selected).toContain('shared-libs-review-lifecycle');
    expect(selected).toContain('shared-libs-review-revalidation');
  }
});

test('the captured lifecycle scope packet selects only its native lifecycle consumer', () => {
  const selected = selectTests(['test/fixtures/shared-libs-lifecycle-r59-stage-scope-public.json'], E2E_TOUCHFILES, GLOBAL_TOUCHFILES);
  expect(selected.reason).toBe('diff');
  expect(selected.selected).toEqual(['shared-libs-review-lifecycle']);
});

test('the registered hook refuses foreign state and inconsistent tool identities without issuing a result', async () => {
  const f = fixtures.createSharedLibsFixture('actor-boundary');
  roots.push(f.root);
  fixtures.seedReviewSources(f);
  const actor = createLifecyclePrerequisiteActor(f);
  for (const [cwd, suppliedId] of [[f.state, 'request'], [f.repo, 'foreign-request']]) {
    const output = await actor.hooks.PreToolUse[0].hooks[0]({ hook_event_name: 'PreToolUse', tool_name: 'Bash',
      tool_input: { command: actor.actorCommand }, tool_use_id: 'request', cwd,
      session_id: 'free-boundary', transcript_path: path.join(f.root, 'public.jsonl') }, suppliedId,
    { signal: new AbortController().signal });
    expect(output).toMatchObject({ hookSpecificOutput: { permissionDecision: 'deny' } });
    expect(fs.readdirSync(path.join(f.root, 'synthetic-stage-receipts'))).toEqual([]);
  }
  expect(actor.verify([])).toBe(false);
});
