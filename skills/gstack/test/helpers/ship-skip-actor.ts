import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { createHash, randomUUID } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { isDeepStrictEqual } from 'node:util';
import type { CanUseTool, HookCallback, SDKMessage } from '@anthropic-ai/claude-agent-sdk';
import type { AgentSdkResult, QueryProvider } from './agent-sdk-runner';
import type { EvalTestEntry } from './eval-store';
import { CAPTURE_MS } from './eval-budgets';
import { createSharedInteractiveToolHandler, SHARED_INTERACTIVE_MAX_TURNS } from './shared-libs-eval-fixture';
import { runGeneration } from '../../scripts/gen-skill-docs';
import { gitArgvIn } from './scratch-repo';

export const SHIP_SKIP_CASE = 'ship-skipped-queued-finding';
export const SHIP_SKIP_QUESTION = { questions: [{ header: 'Invoice auth', multiSelect: false,
  question: 'Fix invoice.ts authorization so only the invoice owner is accepted?',
  options: [{ label: 'Fix', description: 'Enforce the owner check.' },
    { label: 'Skip', description: 'Leave the source unchanged and retain the unresolved defect.' }] }] };
const UNCHANGED_READ = 'Wasted call — file unchanged since your last Read. Refer to that earlier tool_result instead.';
const ROOT = path.resolve(import.meta.dir, '../..');
const quote = (value: string) => `'${value.replaceAll("'", "'\"'\"'")}'`;
const digest = (value: string) => createHash('sha256').update(value).digest('hex');
const read = (file: string) => fs.existsSync(file) ? fs.readFileSync(file, 'utf8') : '';
const json = (file: string) => JSON.parse(fs.readFileSync(file, 'utf8'));
const write = (file: string, value: unknown) => fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n', { mode: 0o600 });

function command(repo: string, env: NodeJS.ProcessEnv, executable: string, args: string[], deadline = Infinity) {
  const remaining = deadline - Date.now();
  if (remaining <= 0) throw new Error('Ship Skip case deadline exhausted during setup');
  const result = spawnSync(executable, args, { cwd: repo, env, encoding: 'utf8', timeout: Math.min(10_000, remaining) });
  if (result.status !== 0 || result.error) throw new Error(result.error?.message ?? result.stderr);
  return result.stdout.trim();
}

function section(text: string, first: string, last?: string) {
  const start = text.indexOf(first);
  const end = last ? text.indexOf(last, start + first.length) : text.length;
  if (start < 0 || end < start || text.indexOf(first, start + first.length) >= 0) throw new Error(`Ambiguous workflow boundary: ${first}`);
  return text.slice(start, end).trim();
}

export async function shipSkipWorkflow() {
  const rendered = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'sskip-render-'));
  try {
    const generated = await runGeneration({ host: 'claude', outputRoot: rendered, contentLinkRoot: null, log: () => {} });
    if (generated.exitCode !== 0) throw new Error('Ship fixture generation failed');
    const army = read(path.join(rendered, 'ship/sections/review-army.md'));
    const adversarial = read(path.join(rendered, 'ship/sections/adversarial.md'));
    return section(army, '### Step 9.3:', '### Decide whether to repeat Step 9')
      + '\n\n' + section(adversarial, '### Finish the adversarial phase', '\n---');
  } finally { fs.rmSync(rendered, { recursive: true, force: true }); }
}

export function createShipSkipFixture(workflow: string, root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'sskip-')), deadline = Infinity) {
  const routing = ['GIT_DIR', 'GIT_WORK_TREE', 'GIT_COMMON_DIR', 'GIT_INDEX_FILE', 'GIT_OBJECT_DIRECTORY', 'GIT_ALTERNATE_OBJECT_DIRECTORIES']
    .filter(key => process.env[key] !== undefined);
  if (routing.length) throw new Error(`Refusing ambient Git routing: ${routing.join(', ')}`);
  fs.mkdirSync(root, { recursive: true, mode: 0o700 });
  fs.chmodSync(root, 0o700);
  const repo = path.join(root, 'project');
  const home = path.join(root, 'home');
  const state = path.join(root, 'state');
  const assets = path.join(root, 'assets');
  for (const dir of [repo, home, state, assets]) fs.mkdirSync(dir, { mode: 0o700 });
  const env = { HOME: home, GSTACK_HOME: state, GSTACK_STATE_ROOT: state, CLAUDE_PLUGIN_DATA: '',
    CLAUDE_CONFIG_DIR: path.join(root, 'claude-config'), GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_SYSTEM: '/dev/null',
    GIT_CONFIG_NOSYSTEM: '1', GIT_CONFIG_COUNT: '0', PATH: `${path.dirname(process.execPath)}:${process.env.PATH ?? ''}` };
  const git = (...args: string[]) => {
    const remaining = deadline - Date.now();
    if (remaining <= 0) throw new Error('Ship Skip case deadline exhausted during setup');
    const result = gitArgvIn(repo, args, Math.min(10_000, remaining), env);
    if (result.status !== 0 || result.error) throw new Error(result.error?.message ?? result.stderr.toString());
  };
  git('init', '-q', '-b', 'main');
  const product = path.join(repo, 'invoice.ts');
  fs.writeFileSync(product, 'export const canReadInvoice = (owner: string, viewer: string) => owner === viewer;\n', { mode: 0o644 });
  git('add', 'invoice.ts');
  git('commit', '-qm', 'Seed invoice authorization');
  git('update-ref', 'refs/remotes/origin/main', 'HEAD');
  git('checkout', '-qb', 'fixture/queued-finding');
  fs.writeFileSync(product, 'export const canReadInvoice = (_owner: string, _viewer: string) => true;\n');
  git('add', 'invoice.ts');
  git('commit', '-qm', 'Seed the reviewed authorization defect');
  const bytes = read(product);
  const evidence = { path: 'invoice.ts', sha256: digest(bytes) };
  const finding = { fingerprint: 'invoice.ts:1:authorization', path: 'invoice.ts', line: 1,
    severity: 'CRITICAL', category: 'authorization', classification: 'FIXABLE',
    problem: 'Invoice authorization accepts a different viewer than the owner.', decision_evidence: evidence };
  const workflowPath = path.join(assets, 'workflow.md');
  fs.writeFileSync(workflowPath, workflow, { mode: 0o600 });
  const checklistPath = path.join(assets, 'checklist.md');
  fs.copyFileSync(path.join(ROOT, 'review/checklist.md'), checklistPath);
  const inputPath = path.join(assets, 'finding.json');
  write(inputPath, { source: 'synthetic native-review fixture result', native_completed: true, finding,
    review_coverage: 'not_executed', probes: [], release_eligible: false });
  const draft = path.join(root, 'review-record.json');
  const receipts = path.join(root, 'receipts.jsonl');
  const answersPath = path.join(root, 'owner-answer.json');
  const token = command(repo, { ...process.env, ...env }, path.join(ROOT, 'bin/gstack-review-log'), ['--start', 'review'], deadline);
  const startFiles = fs.readdirSync(state, { recursive: true }).filter(file => String(file).endsWith(`/${token}.json`));
  if (startFiles.length !== 1) throw new Error('Expected one owned real review-start receipt');
  const start = json(path.join(state, String(startFiles[0])));
  if (start.repo !== repo || start.branch !== 'fixture/queued-finding') throw new Error('Review-start receipt has foreign ownership');
  const commands = Object.fromEntries(['read', 'persist', 'rediscover', 'advance', 'repeat'].map(action =>
    [action, `${quote(process.execPath)} ${quote(import.meta.path)} --fixture ${quote(root)} ${action}`]));
  write(path.join(root, 'fixture.json'), { repo, env, product, evidence, finding, token, draft, receipts, answersPath });
  const executions: Array<{ tool: string; input: Record<string, unknown>; allowed: boolean; phase: string }> = [];
  const answers: Array<{ toolUseId: string; input: Record<string, unknown>; answers: Record<string, string> }> = [];
  let questionId = '';
  const refusals: string[] = [];
  const phase = () => read(receipts).includes('"action":"rediscover"') ? 'rediscovered' : 'initial';
  const readable = new Set([workflowPath, checklistPath, inputPath, product]);
  const invalid = (tool: string, input: Record<string, unknown>) => {
    if (/"action":"(?:advance|repeat)"/.test(read(receipts))) return 'Queue boundary already selected';
    if (tool === 'Read') return typeof input.file_path === 'string' && readable.has(path.resolve(repo, input.file_path))
      && Object.keys(input).every(key => key === 'file_path') ? undefined : 'Only declared full-file reads are supported';
    if (tool === 'Write') return input.file_path === draft && typeof input.content === 'string' && input.content.length <= 16_384 ? undefined : 'Write outside review record';
    if (tool === 'Bash') return typeof input.command === 'string' && !input.run_in_background && Object.values(commands).includes(input.command.trim()) ? undefined : 'Bash outside fixture interface';
    if (tool === 'AskUserQuestion') {
      if (answers.length) return 'Repeated Skip question';
      const questions = input.questions;
      if (Object.keys(input).some(key => key !== 'questions') || !Array.isArray(questions) || questions.length !== 1) return 'Only the declared finding disposition is supported';
      const question = questions[0];
      const expected = SHIP_SKIP_QUESTION.questions[0];
      return question && Object.keys(question).length === 4 && question.header === expected.header
        && question.question === expected.question && question.multiSelect === false && Array.isArray(question.options)
        && question.options.length === 2 && question.options.every((option: any, index: number) => option
          && Object.keys(option).length === 2 && option.label === expected.options[index].label
          && option.description === expected.options[index].description) ? undefined : 'Only the declared finding disposition is supported';
    }
    return 'Undeclared tool';
  };
  const handler = createSharedInteractiveToolHandler('skip', {
    nonQuestion: (_name, input) => ({ behavior: 'allow', updatedInput: input }),
    onQuestion: input => { const reason = invalid('AskUserQuestion', input); if (reason) throw new Error(reason); },
    onAnswer: (input, selected) => { answers.push({ toolUseId: questionId, input, answers: selected }); write(answersPath, { toolUseId: questionId, input, answers: selected, evidence }); },
    onRefusal: error => { refusals.push(error.message); },
  });
  const canUseTool: CanUseTool = async (tool, input, options) => {
    const reason = invalid(tool, input);
    if (reason) { refusals.push(reason); return { behavior: 'deny', message: reason }; }
    questionId = options.toolUseID;
    if (tool === 'AskUserQuestion' && !questionId) { refusals.push('Missing native question ID'); return { behavior: 'deny', message: 'Missing native question ID' }; }
    try { return await handler(tool, input); }
    catch (error) { const message = String(error); refusals.push(message); return { behavior: 'deny', message }; }
  };
  const preToolUse: HookCallback = async input => {
    if (input.hook_event_name !== 'PreToolUse') throw new Error('Unexpected hook');
    const toolInput = input.tool_input as Record<string, unknown>;
    const reason = invalid(input.tool_name, toolInput);
    executions.push({ tool: input.tool_name, input: toolInput, allowed: !reason, phase: phase() });
    return { hookSpecificOutput: { hookEventName: 'PreToolUse', permissionDecision: reason ? 'deny' : input.tool_name === 'AskUserQuestion' ? 'ask' : 'allow',
      ...(reason ? { permissionDecisionReason: reason } : {}),
      ...(input.tool_name === 'Bash' && !reason ? { updatedInput: { command: toolInput.command, timeout: 10000, run_in_background: false } } : {}) } };
  };
  const prompt = `Load gstack's bounded /ship decision path from ${workflowPath}. The queued Step 11 finding is in ${inputPath}; verify it against ${product}. The referenced checklist is ${checklistPath}. Execute Step 9.3 and Step 9.4 through persistence, then obtain the unchanged rediscovery and apply the supplied Step 11 finish rules. This is not full /ship: earlier reviewers and the rediscovery are explicitly synthetic fixture inputs. No QA/probes/full review ran; persist completed:false and converged:false and never claim passing verification or release clearance. The native-completion bit refers only to the supplied synthetic result. Stop after selecting the next queue boundary; do not execute Step 11.5 or release work.

Supported interactions, declared before launch:
- Read only the four absolute files above, using file_path alone: no offset, limit, pages or other fields. After the rediscover command's result is acknowledged, issue a new full Read with exactly ${JSON.stringify({ file_path: product })}. Receive its actual tool result before issuing either routing command (advance or repeat), even when the result is file_unchanged and refers to the earlier full delivery. A Read before rediscovery or a prose claim of unchanged source does not satisfy this step.
- One native AskUserQuestion with exactly this declared finding-disposition input: ${JSON.stringify(SHIP_SKIP_QUESTION)}. The owner chooses the no-change option. No other permission, combined request or risk-acceptance question is authorized.
- Write only ${draft}, as the actual Step 9.4 JSON review record. Include the finding's fingerprint, severity, disposition and decision_evidence; preserve honest unresolved counts. Copy the canonical decision_evidence object from ${inputPath} unchanged, with exactly its path and sha256 fields and values; put any annotations outside decision_evidence. The adapter holds the original real REVIEW_START token, captured before source inspection. The record is outside the product checkout.
- Bash accepts exactly these single commands, without chaining, redirection or backgrounding:
  ${commands.read} invokes the real gstack-review-read in this fixture's owned state.
  ${commands.persist} passes the written record to the real gstack-review-log with the original token.
  ${commands.rediscover} returns the same completed synthetic Step 11 finding after the owner response and persistence, without changing source.
  ${commands.advance} records selection of the Step 11.5 boundary and stops this fixture.
  ${commands.repeat} records selection of another 9→10→11 review pass and stops this fixture.
Use these bound adapters for the generated helper command examples. Choose the routing command from the workflow, not as a claim that shipping is cleared. The adapters do not decide which boundary you should select. Memory updates are outside this fixture. No product writes, direct receipt/config access, new start token, full reviewer dispatch, other commands, commits or publication are supported. Report the skipped defect and incomplete verification truthfully.`;
  const snapshot = () => ({ product: read(product), originalProduct: bytes, evidence, start, answers, refusals, executions,
    receipts: read(receipts).trim().split('\n').filter(Boolean).map(line => JSON.parse(line)),
    workflowSha256: digest(workflow), promptSha256: digest(prompt),
    stateRoot: state, repo, input: json(inputPath), persisted: read(path.join(root, 'persisted.json')) ? json(path.join(root, 'persisted.json')) : null });
  return { root, repo, env, workflowPath, inputPath, product, draft, commands, prompt, preToolUse, canUseTool, snapshot,
    readContents: new Map([...readable].map(file => [file, read(file)])) };
}

export function shipSkipFailures(fixture: ReturnType<typeof createShipSkipFixture>, result: Pick<AgentSdkResult, 'exitReason' | 'events'>) {
  const evidence = fixture.snapshot();
  const failures: string[] = [];
  const check = (ok: boolean, message: string) => { if (!ok) failures.push(message); };
  check(result.exitReason === 'success', `actor ended: ${result.exitReason}`);
  check(evidence.product === evidence.originalProduct, 'product bytes changed');
  check(evidence.answers.length === 1 && evidence.refusals.length === 0, 'expected exactly one captured owner Skip');
  check(!evidence.executions.some(event => !event.allowed), 'undeclared or repeated interaction');
  const calls = new Map<string, { tool: string; input: Record<string, unknown> }>();
  const completed: Array<{ id: string; tool: string; input: Record<string, unknown>; output: string; index: number; fullRead?: string }> = [];
  const delivered = new Set<string>();
  for (const [index, event] of result.events.entries()) {
    if (event.type !== 'assistant' && event.type !== 'user') continue;
    for (const block of event.message.content) {
      if (typeof block === 'string') continue;
      if (event.type === 'assistant' && block.type === 'tool_use') calls.set(block.id, { tool: block.name, input: block.input as Record<string, unknown> });
      if (event.type === 'user' && block.type === 'tool_result' && !block.is_error) {
        const call = calls.get(block.tool_use_id);
        if (call) {
          const output = typeof block.content === 'string' ? block.content : Array.isArray(block.content)
            && block.content.every(part => part.type === 'text') ? block.content.map(part => (part as { text: string }).text).join('\n') : '';
          let fullRead: string | undefined;
          if (call.tool === 'Read' && typeof call.input.file_path === 'string' && Object.keys(call.input).length === 1) {
            const file = path.resolve(fixture.repo, call.input.file_path);
            const expected = fixture.readContents.get(file);
            const native = (event as SDKMessage & { tool_use_result?: { type: string; file: { filePath: string; content?: string; startLine?: number; numLines?: number; totalLines?: number } } }).tool_use_result;
            if (expected !== undefined && native?.file?.filePath === file) {
              const lines = expected.split('\n');
              if (native.type === 'text' && native.file.content === expected && native.file.startLine === 1
                && native.file.numLines === lines.length && native.file.totalLines === lines.length
                && output === lines.map((line, i) => `${i + 1}\t${line}`).join('\n')) {
                delivered.add(file); fullRead = file;
              } else if (native.type === 'file_unchanged' && output === UNCHANGED_READ && delivered.has(file)
                && read(file) === expected) fullRead = file;
            }
          }
          completed.push({ id: block.tool_use_id, ...call, output, index, fullRead });
        }
      }
    }
  }
  const answer = evidence.answers[0];
  check([...calls.values()].filter(call => call.tool === 'AskUserQuestion').length === 1, 'expected one native decision question');
  const acknowledged = answer && completed.find(call => call.tool === 'AskUserQuestion' && call.id === answer.toolUseId
    && isDeepStrictEqual(call.input.questions, answer.input.questions)
    && Object.values(answer.answers).every(label => call.output.includes(label)));
  check(!!acknowledged, 'native Skip response was not acknowledged');
  check(!!acknowledged && completed.some(call => call.fullRead === fixture.workflowPath && call.index < acknowledged.index), 'complete workflow was not delivered before the decision');
  check(!!acknowledged && [fixture.product, fixture.inputPath].every(file => completed.some(call => call.fullRead === file
    && call.index < acknowledged.index)), 'owner decision lacks complete source and finding delivery');
  const rediscovery = completed.find(call => call.tool === 'Bash' && String(call.input.command).trim() === fixture.commands.rediscover);
  const advance = completed.find(call => call.tool === 'Bash' && String(call.input.command).trim() === fixture.commands.advance);
  check(!!rediscovery && !!advance && completed.some(call => call.fullRead === fixture.product
    && call.index > rediscovery.index && call.index < advance.index), 'source not re-read before routing');
  check(!!advance, 'advancement command did not complete');
  const actions = evidence.receipts.map(row => row.action);
  check(JSON.stringify(actions.filter(action => action !== 'read')) === JSON.stringify(['persist', 'rediscover', 'advance']), 'expected persistence, rediscovery and advancement without requeue');
  const row = evidence.persisted;
  check(row?.skill === 'review' && row?.via === 'ship' && row?.status === 'issues_found'
    && row?.issues_found === 1 && row?.critical === 1 && row?.informational === 0, 'skipped defect was silently cleared');
  check(row?.completed === false && row?.converged === false && row?.cycles === 0, 'synthetic coverage was marked complete');
  check(row?.review_binding?.state === 'incomplete' && row.review_binding.start_wtree === evidence.start.wtree
    && row.review_binding.end_wtree === evidence.start.wtree && row.review_binding.started_at === evidence.start.started_at
    && row.review_binding.branch_id === digest(evidence.start.branch), 'review record lost its owned unchanged incomplete binding');
  check(row?.findings?.length === 1 && row.findings[0].fingerprint === 'invoice.ts:1:authorization'
    && row.findings[0].action === 'skipped' && row.findings[0].severity === 'CRITICAL'
    && row.findings[0].advisory !== true && isDeepStrictEqual(row.findings[0].decision_evidence, evidence.evidence), 'persisted Skip lost its identity or source evidence');
  check(!row?.VERIFY_RESULT && !row?.verify_result && !row?.probes?.length, 'invented a passing probe or verification result');
  return { failures, evidence };
}

export async function runShipSkipActor(record: (entry: EvalTestEntry) => void, injectedQuery?: QueryProvider, artifactDirectory?: string) {
  const started = Date.now();
  const deadline = started + CAPTURE_MS - 20000;
  const artifacts = artifactDirectory ?? process.env.GSTACK_EVAL_DIR;
  const evidenceFile = artifacts ? path.join(artifacts, `${SHIP_SKIP_CASE}-${randomUUID()}.json`) : undefined;
  const roots: string[] = [];
  type Fixture = ReturnType<typeof createShipSkipFixture>;
  const attempts: Array<{ fixture: Fixture; events: SDKMessage[]; evidence?: ReturnType<Fixture['snapshot']>;
    active: boolean; closed: boolean; drained: boolean; lateCallbacks: string[]; closeError?: string;
    close?: () => void; finish?: () => Promise<void> }> = [];
  let workflow = '';
  let fixture: Fixture | undefined;
  let result: AgentSdkResult | undefined;
  let failure: unknown;
  let passed = false;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const remaining = () => {
    const ms = deadline - Date.now();
    if (ms <= 0) throw new Error('Ship Skip case deadline exhausted before query launch');
    return ms;
  };
  const setup = () => {
    remaining();
    const root = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'sskip-'));
    roots.push(root);
    const owned = createShipSkipFixture(workflow, root, deadline);
    remaining();
    return owned;
  };
  try {
    if (!artifacts) throw new Error('Ship Skip actor requires GSTACK_EVAL_DIR for durable evidence');
    fs.mkdirSync(artifacts, { recursive: true, mode: 0o700 });
    const { query } = await import('@anthropic-ai/claude-agent-sdk');
    const { runAgentSdkTest, resolveClaudeBinary } = await import('./agent-sdk-runner');
    workflow = await shipSkipWorkflow();
    fixture = setup();
    const binary = injectedQuery ? undefined : resolveClaudeBinary();
    if (!injectedQuery && !binary) throw new Error('Native ship Skip actor requires the pinned Claude CLI');
    const controller = new AbortController();
    const expired = new Promise<never>((_resolve, reject) => {
      timer = setTimeout(() => { const error = new Error('Ship Skip case deadline exhausted'); controller.abort(error); reject(error); }, remaining());
    });
    const running = runAgentSdkTest({ systemPrompt: { type: 'preset', preset: 'claude_code' }, userPrompt: fixture.prompt,
      workingDirectory: fixture.repo, env: fixture.env, maxTurns: SHARED_INTERACTIVE_MAX_TURNS,
      signal: controller.signal, allowedTools: ['Read', 'Write', 'Bash', 'AskUserQuestion'],
      settingSources: [], testName: SHIP_SKIP_CASE, pathToClaudeCodeExecutable: binary ?? undefined,
      canUseTool: fixture.canUseTool,
      queryProvider: options => {
        remaining();
        if (attempts.length) fixture = setup();
        const owned = fixture!;
        const attempt: typeof attempts[number] = { fixture: owned, events: [], active: true, closed: false, drained: false, lateCallbacks: [] };
        attempts.push(attempt);
        const expiredCallback = (tool: string) => {
          if (attempt.active && Date.now() < deadline) return false;
          attempt.lateCallbacks.push(tool); return true;
        };
        let stream: ReturnType<QueryProvider>;
        try {
          stream = (injectedQuery ?? query)({ ...options, prompt: owned.prompt, options: { ...options.options,
            cwd: owned.repo, env: { ...options.options?.env, ...owned.env }, allowedTools: [],
            canUseTool: (...args) => expiredCallback(args[0]) ? Promise.resolve({ behavior: 'deny' as const, message: 'Attempt is closed or expired' }) : owned.canUseTool(...args),
            hooks: { PreToolUse: [{ hooks: [(...args) => expiredCallback(args[0].hook_event_name)
              ? Promise.resolve({ hookSpecificOutput: { hookEventName: 'PreToolUse' as const, permissionDecision: 'deny' as const, permissionDecisionReason: 'Attempt is closed or expired' } })
              : owned.preToolUse(...args)] }] } } });
        } catch (error) { attempt.active = false; attempt.closed = true; attempt.drained = true; throw error; }
        const iterator = stream[Symbol.asyncIterator]();
        attempt.close = () => {
          attempt.active = false;
          if (attempt.closed) return;
          attempt.closed = true;
          try { stream.close(); } catch (error) { attempt.closeError = String(error); }
        };
        let finishing: Promise<void> | undefined;
        attempt.finish = () => finishing ??= (async () => {
          attempt.close!();
          try { await iterator.return?.(); attempt.drained = !attempt.closeError; }
          catch (error) { attempt.closeError ??= String(error); throw error; }
          finally { attempt.evidence = owned.snapshot(); }
        })();
        return new Proxy(stream, { get(target, property) {
          if (property === 'close') return attempt.close;
          if (property === Symbol.asyncIterator) return async function* () {
            try {
              while (true) {
                const next = await iterator.next();
                if (next.done) break;
                attempt.events.push(next.value); yield next.value;
              }
            } finally { await attempt.finish!(); }
          };
          const value = Reflect.get(target, property, target);
          return typeof value === 'function' ? value.bind(target) : value;
        } });
      },
    });
    result = await Promise.race([running, expired]);
    const verdict = shipSkipFailures(fixture!, result);
    if (verdict.failures.length) throw new Error(verdict.failures.join('; '));
    passed = true;
  } catch (error) { failure = error; }
  finally {
    clearTimeout(timer);
    try {
      for (const attempt of attempts) attempt.close?.();
      let drainTimer: ReturnType<typeof setTimeout> | undefined;
      try {
        await Promise.race([Promise.all(attempts.map(attempt => attempt.finish?.())), new Promise<never>((_resolve, reject) => {
          drainTimer = setTimeout(() => reject(new Error('Native query did not drain before the artifact deadline')), Math.max(1, started + CAPTURE_MS - 1000 - Date.now()));
        })]);
        const closeError = attempts.find(attempt => attempt.closeError)?.closeError;
        if (closeError) throw new Error(closeError);
      } catch (error) { failure ??= error; passed = false; }
      finally { clearTimeout(drainTimer); }
      const retainedRoots = attempts.filter(attempt => !attempt.drained).map(attempt => attempt.fixture.root);
      const snapshots = attempts.map(({ fixture: owned, events, evidence, active, closed, drained, lateCallbacks, closeError }) => ({
        events, evidence: evidence ?? owned.snapshot(), prompt: owned.prompt, environment: owned.env,
        active, closed, drained, lateCallbacks, closeError }));
      const charges = attempts.map(({ events, drained }) => {
        const terminal = events.findLast(event => event.type === 'result');
        const costUsd = typeof terminal?.total_cost_usd === 'number' && Number.isFinite(terminal.total_cost_usd)
          && terminal.total_cost_usd >= 0 ? terminal.total_cost_usd : null;
        return { costUsd, costKnown: costUsd !== null && drained };
      });
      const costKnown = charges.every(charge => charge.costKnown);
      const billing = { knownCostUsd: charges.reduce((sum, charge) => sum + (charge.costUsd ?? 0), 0), costKnown,
        status: !attempts.length ? 'not_started' : costKnown ? 'complete' : 'incomplete', attempts: charges };
      const billingNote = costKnown ? '' : 'Terminal billing is incomplete; actual total cost is unknown. Only known charges are summed; all attempt events are retained.';
      const output = JSON.stringify({ error: failure === undefined ? undefined : String(failure),
        prompt: fixture?.prompt, workflow, evidence: fixture?.snapshot(), attempts: snapshots, output: result?.output,
        started, deadline, roots, retainedRoots, billing });
      if (evidenceFile) fs.writeFileSync(evidenceFile, output + '\n', { mode: 0o600 });
      record({ name: SHIP_SKIP_CASE, suite: 'ship-skip-boundary', tier: 'e2e', passed, duration_ms: Date.now() - started,
        cost_usd: billing.knownCostUsd, transcript: [{ type: 'fixture_billing', cost_known: costKnown, billing }, ...attempts.flatMap(attempt => attempt.events)],
        prompt: fixture?.prompt, turns_used: result?.turnsUsed, model: result?.model, output,
        error: [failure === undefined ? '' : String(failure), billingNote].filter(Boolean).join('\n') || undefined,
        exit_reason: passed ? 'success' : result?.exitReason === 'success' ? 'assertion_failed' : result?.exitReason ?? 'runner_error' });
    } finally {
      for (const root of roots) if (!attempts.some(attempt => attempt.fixture.root === root && !attempt.drained)) fs.rmSync(root, { recursive: true, force: true });
    }
  }
  if (failure !== undefined) throw failure;
  return evidenceFile!;
}

if (import.meta.main && process.argv[2] === '--fixture') {
  const root = fs.realpathSync(process.argv[3]);
  const config = json(path.join(root, 'fixture.json'));
  const action = process.argv[4];
  const env = { ...process.env, ...config.env };
  const helper = (name: string, args: string[]) => command(config.repo, env, path.join(ROOT, 'bin', name), args);
  const rows = () => helper('gstack-review-read', []).split('---CONFIG---')[0].trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
  const receipts = read(config.receipts).trim().split('\n').filter(Boolean).map(line => JSON.parse(line));
  let output: unknown;
  if (action === 'read') output = helper('gstack-review-read', []);
  else if (action === 'persist') {
    if (!fs.existsSync(config.answersPath) || receipts.some(row => row.action === 'persist')) throw new Error('Persistence requires one real owner answer and an unconsumed token');
    helper('gstack-review-log', [JSON.stringify(json(config.draft)), '--finish', config.token]);
    const persisted = rows().at(-1);
    write(path.join(root, 'persisted.json'), persisted);
    output = persisted;
  } else if (action === 'rediscover') {
    if (!receipts.some(row => row.action === 'persist') || digest(read(config.product)) !== config.evidence.sha256) throw new Error('Rediscovery requires persistence and unchanged source');
    output = { source: 'synthetic native-review fixture result', native_completed: true, finding: config.finding,
      review_coverage: 'not_executed', probes: [], release_eligible: false };
  } else if (action === 'advance' || action === 'repeat') {
    if (!receipts.some(row => row.action === 'rediscover')) throw new Error('Routing requires the rediscovered finding');
    output = { boundary: action === 'advance' ? '11.5' : '9→10→11', release_eligible: false };
  } else throw new Error('Unknown fixture action');
  fs.appendFileSync(config.receipts, JSON.stringify({ action, evidence: config.evidence }) + '\n', { mode: 0o600 });
  console.log(typeof output === 'string' ? output : JSON.stringify(output));
}
